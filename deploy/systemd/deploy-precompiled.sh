#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="/opt/repos/pi-ups-dashboard"
DEPLOY_DIR="/opt/pi-ups-dashboard"
SERVICE_NAME="pi-ups-dashboard.service"
STATIC_SRC="$REPO_DIR/static"
DEPLOY_SRC="$REPO_DIR/deploy"
BIN_DST="$DEPLOY_DIR/pi-ups-dashboard"

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

if [[ ! -d "$REPO_DIR" ]]; then
  echo "Expected repository at $REPO_DIR"
  exit 1
fi

if [[ ! -d "$STATIC_SRC" ]]; then
  echo "Expected static assets at $STATIC_SRC"
  exit 1
fi

if [[ ! -d "$DEPLOY_SRC" ]]; then
  echo "Expected deploy files at $DEPLOY_SRC"
  exit 1
fi

BIN_SRC="${1:-}"
if [[ -z "$BIN_SRC" ]]; then
  if ! BIN_SRC="$(choose_default_binary)"; then
    echo "Unsupported architecture: $(uname -m)"
    echo "Pass a binary path explicitly, for example:"
    echo "  ./deploy/systemd/deploy-precompiled.sh $REPO_DIR/binaries/pi-ups-dashboard-linux-aarch64-musl"
    exit 1
  fi
fi

if [[ ! -f "$BIN_SRC" ]]; then
  echo "Binary not found: $BIN_SRC"
  exit 1
fi

echo "Deploying binary from $BIN_SRC"
sudo mkdir -p "$DEPLOY_DIR"
sudo install -m 0755 "$BIN_SRC" "$BIN_DST"
sudo rm -rf "$DEPLOY_DIR/static" "$DEPLOY_DIR/deploy"
sudo cp -R "$STATIC_SRC" "$DEPLOY_DIR/static"
sudo cp -R "$DEPLOY_SRC" "$DEPLOY_DIR/deploy"

echo "Installing and starting systemd service"
sudo cp "$DEPLOY_DIR/deploy/systemd/$SERVICE_NAME" "/etc/systemd/system/$SERVICE_NAME"
sudo systemctl daemon-reload
sudo systemctl enable --now "$SERVICE_NAME"
sudo systemctl status "$SERVICE_NAME" --no-pager

echo "Done. Dashboard should be available on http://<pi-ip>:8080"
