#!/usr/bin/env bash
set -euo pipefail

GITHUB_REPO="${GITHUB_REPO:-seoulplane/pi-ups-dashboard}"
DEPLOY_DIR="${DEPLOY_DIR:-/opt/pi-ups-dashboard}"
BACKUP_ROOT="${BACKUP_ROOT:-/opt/pi-ups-dashboard-backups}"
STATE_DIR="${STATE_DIR:-/var/lib/pi-ups-dashboard-updater}"
SERVICE_NAME="${SERVICE_NAME:-pi-ups-dashboard.service}"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:8080/api/dashboard}"
HEALTH_RETRIES="${HEALTH_RETRIES:-12}"
HEALTH_DELAY_SECONDS="${HEALTH_DELAY_SECONDS:-2}"

as_root() {
  if [[ "${EUID}" -eq 0 ]]; then
    "$@"
  else
    sudo "$@"
  fi
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing dependency: $1"
    exit 1
  fi
}

asset_for_arch() {
  case "$(uname -m)" in
    aarch64|arm64)
      echo "pi-ups-dashboard-linux-aarch64-musl"
      ;;
    armv7l|armv6l|armhf)
      echo "pi-ups-dashboard-linux-armv7-musleabihf"
      ;;
    *)
      return 1
      ;;
  esac
}

extract_tag_name() {
  sed -n 's/.*"tag_name": "\([^"]*\)".*/\1/p' "$1" | head -n1
}

extract_asset_url() {
  local file="$1"
  local asset="$2"
  grep -o "https://[^"]*${asset}[^"]*" "$file" | head -n1
}

service_user_group() {
  local user group
  user="$(as_root systemctl show -p User --value "$SERVICE_NAME" 2>/dev/null || true)"
  group="$(as_root systemctl show -p Group --value "$SERVICE_NAME" 2>/dev/null || true)"

  if [[ -z "$user" ]]; then
    user="${SERVICE_USER:-${SUDO_USER:-$(id -un)}}"
  fi

  if [[ -z "$group" ]]; then
    group="${SERVICE_GROUP:-$(id -gn "$user")}"
  fi

  echo "$user:$group"
}

health_check() {
  local try=1
  while [[ "$try" -le "$HEALTH_RETRIES" ]]; do
    if curl -fsS "$HEALTH_URL" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$HEALTH_DELAY_SECONDS"
    try=$((try + 1))
  done
  return 1
}

rollback() {
  local backup_dir="$1"

  echo "Deployment failed. Restoring previous version from $backup_dir"
  as_root mkdir -p "$DEPLOY_DIR"
  as_root rm -rf "$DEPLOY_DIR/static" "$DEPLOY_DIR/deploy"

  if [[ -f "$backup_dir/pi-ups-dashboard" ]]; then
    as_root install -m 0755 "$backup_dir/pi-ups-dashboard" "$DEPLOY_DIR/pi-ups-dashboard"
  fi

  if [[ -d "$backup_dir/static" ]]; then
    as_root cp -R "$backup_dir/static" "$DEPLOY_DIR/static"
  fi

  if [[ -d "$backup_dir/deploy" ]]; then
    as_root cp -R "$backup_dir/deploy" "$DEPLOY_DIR/deploy"
  fi

  if [[ -f "$backup_dir/$SERVICE_NAME" ]]; then
    as_root cp "$backup_dir/$SERVICE_NAME" "/etc/systemd/system/$SERVICE_NAME"
  fi

  as_root systemctl daemon-reload
  as_root systemctl restart "$SERVICE_NAME"
  as_root systemctl status "$SERVICE_NAME" --no-pager || true
}

require_cmd curl
require_cmd sed
require_cmd tar

asset_binary="$(asset_for_arch)" || {
  echo "Unsupported architecture: $(uname -m)"
  exit 1
}

api_url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

release_json="$tmp_dir/release.json"
curl -fsSL "$api_url" -o "$release_json"

latest_tag="$(extract_tag_name "$release_json")"
if [[ -z "$latest_tag" ]]; then
  echo "Unable to determine latest release tag from GitHub API"
  exit 1
fi

as_root mkdir -p "$STATE_DIR"
current_tag_file="$STATE_DIR/current-tag"
current_tag=""
if as_root test -f "$current_tag_file"; then
  current_tag="$(as_root cat "$current_tag_file")"
fi

if [[ "$latest_tag" == "$current_tag" ]]; then
  echo "Already up-to-date at $latest_tag"
  exit 0
fi

binary_url="$(extract_asset_url "$release_json" "$asset_binary")"
static_url="$(extract_asset_url "$release_json" "static.tar.gz")"
deploy_url="$(extract_asset_url "$release_json" "deploy.tar.gz")"

if [[ -z "$binary_url" || -z "$static_url" || -z "$deploy_url" ]]; then
  echo "Missing one or more required assets in release $latest_tag"
  exit 1
fi

echo "Downloading release assets for $latest_tag"
curl -fsSL "$binary_url" -o "$tmp_dir/pi-ups-dashboard"
curl -fsSL "$static_url" -o "$tmp_dir/static.tar.gz"
curl -fsSL "$deploy_url" -o "$tmp_dir/deploy.tar.gz"

tar -xzf "$tmp_dir/static.tar.gz" -C "$tmp_dir"
tar -xzf "$tmp_dir/deploy.tar.gz" -C "$tmp_dir"

if [[ ! -d "$tmp_dir/static" || ! -d "$tmp_dir/deploy" ]]; then
  echo "Release assets did not contain static/ and deploy/ directories"
  exit 1
fi

as_root mkdir -p "$BACKUP_ROOT"
backup_dir="$BACKUP_ROOT/$(date +%Y%m%d-%H%M%S)-${latest_tag}"
as_root mkdir -p "$backup_dir"

if as_root test -d "$DEPLOY_DIR"; then
  as_root cp -a "$DEPLOY_DIR/." "$backup_dir/"
fi
if as_root test -f "/etc/systemd/system/$SERVICE_NAME"; then
  as_root cp "/etc/systemd/system/$SERVICE_NAME" "$backup_dir/$SERVICE_NAME"
fi

user_group="$(service_user_group)"
service_user="${user_group%%:*}"
service_group="${user_group##*:}"

if ! id -u "$service_user" >/dev/null 2>&1; then
  echo "Service user does not exist: $service_user"
  exit 1
fi

echo "Deploying $latest_tag"
as_root mkdir -p "$DEPLOY_DIR"
as_root install -m 0755 "$tmp_dir/pi-ups-dashboard" "$DEPLOY_DIR/pi-ups-dashboard"
as_root rm -rf "$DEPLOY_DIR/static" "$DEPLOY_DIR/deploy"
as_root cp -R "$tmp_dir/static" "$DEPLOY_DIR/static"
as_root cp -R "$tmp_dir/deploy" "$DEPLOY_DIR/deploy"

unit_tmp="$tmp_dir/$SERVICE_NAME"
sed -e "s/^User=.*/User=$service_user/" -e "s/^Group=.*/Group=$service_group/" "$DEPLOY_DIR/deploy/systemd/$SERVICE_NAME" > "$unit_tmp"
as_root cp "$unit_tmp" "/etc/systemd/system/$SERVICE_NAME"

as_root systemctl daemon-reload
as_root systemctl enable "$SERVICE_NAME"
as_root systemctl restart "$SERVICE_NAME"

if ! health_check; then
  rollback "$backup_dir"
  exit 1
fi

echo "$latest_tag" | as_root tee "$current_tag_file" >/dev/null
as_root systemctl status "$SERVICE_NAME" --no-pager

echo "Updated successfully to $latest_tag"
