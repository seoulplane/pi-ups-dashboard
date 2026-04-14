#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="${REPO_DIR:-/opt/repos/pi-ups-dashboard}"
DEPLOY_DIR="${DEPLOY_DIR:-/opt/pi-ups-dashboard}"
SERVICE_NAME="${SERVICE_NAME:-pi-ups-dashboard.service}"
SERVICE_USER="${SERVICE_USER:-${SUDO_USER:-$(id -un)}}"
SERVICE_GROUP="${SERVICE_GROUP:-$(id -gn "$SERVICE_USER")}"

BIN_DST="$DEPLOY_DIR/pi-ups-dashboard"
WEB_SRC="$REPO_DIR/static"
WEB_DST="$DEPLOY_DIR/static"
DEPLOY_SRC="$REPO_DIR/deploy"

if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
  echo "Service user does not exist: $SERVICE_USER"
  exit 1
fi

choose_default_binary() {
  case "$(uname -m)" in
    aarch64|arm64)
      echo "$REPO_DIR/binaries/pi-ups-dashboard-linux-aarch64-musl"
      ;;
    armv7l|armv6l|armhf)
      echo "$REPO_DIR/binaries/pi-ups-dashboard-linux-armv7-musleabihf"
      ;;
    *)
      return 1
      ;;
  esac
}

BIN_SRC="${1:-}"
if [[ -z "$BIN_SRC" ]]; then
  if [[ -f "$REPO_DIR/target/release/pi-ups-dashboard" ]]; then
    BIN_SRC="$REPO_DIR/target/release/pi-ups-dashboard"
  elif ! BIN_SRC="$(choose_default_binary)"; then
    echo "Unsupported architecture: $(uname -m)"
    echo "Pass an explicit binary path as the first argument."
    exit 1
  fi
fi

if [[ ! -f "$BIN_SRC" ]]; then
  echo "Binary not found: $BIN_SRC"
  exit 1
fi

if [[ ! -d "$WEB_SRC" ]]; then
  echo "Expected web assets at $WEB_SRC"
  exit 1
fi

if [[ ! -d "$DEPLOY_SRC" ]]; then
  echo "Expected deploy files at $DEPLOY_SRC"
  exit 1
fi

echo "Deploying binary: $BIN_SRC"
sudo mkdir -p "$DEPLOY_DIR"
sudo install -m 0755 "$BIN_SRC" "$BIN_DST"

echo "Syncing web pages from $WEB_SRC to $WEB_DST"
if command -v rsync >/dev/null 2>&1; then
  sudo mkdir -p "$WEB_DST"
  sudo rsync -a --delete "$WEB_SRC/" "$WEB_DST/"
else
  sudo rm -rf "$WEB_DST"
  sudo cp -R "$WEB_SRC" "$WEB_DST"
fi

echo "Updating deploy metadata"
sudo rm -rf "$DEPLOY_DIR/deploy"
sudo cp -R "$DEPLOY_SRC" "$DEPLOY_DIR/deploy"

echo "Installing and restarting service"
UNIT_SRC="$DEPLOY_DIR/deploy/systemd/$SERVICE_NAME"
UNIT_TMP="$(mktemp)"
sed -e "s/^User=.*/User=$SERVICE_USER/" -e "s/^Group=.*/Group=$SERVICE_GROUP/" "$UNIT_SRC" > "$UNIT_TMP"
sudo cp "$UNIT_TMP" "/etc/systemd/system/$SERVICE_NAME"
rm -f "$UNIT_TMP"

sudo systemctl daemon-reload
sudo systemctl enable "$SERVICE_NAME"
sudo systemctl restart "$SERVICE_NAME"

if ! sudo systemctl is-active --quiet "$SERVICE_NAME"; then
  echo "Service failed to start. Recent logs:"
  sudo journalctl -u "$SERVICE_NAME" -n 50 --no-pager || true
  exit 1
fi

sudo systemctl status "$SERVICE_NAME" --no-pager

echo "Done. Binary and web pages updated."
