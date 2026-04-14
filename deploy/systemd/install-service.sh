#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="pi-ups-dashboard.service"
REPO_DIR="/opt/repos/pi-ups-dashboard"
DEPLOY_DIR="/opt/pi-ups-dashboard"
BIN_SRC="$REPO_DIR/target/release/pi-ups-dashboard"
BIN_DST="$DEPLOY_DIR/pi-ups-dashboard"
STATIC_SRC="$REPO_DIR/static"
STATIC_DST="$DEPLOY_DIR/static"
SERVICE_USER="${SERVICE_USER:-${SUDO_USER:-$(id -un)}}"

if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
  echo "Service user does not exist: $SERVICE_USER"
  exit 1
fi

SERVICE_GROUP="${SERVICE_GROUP:-$(id -gn "$SERVICE_USER")}"

if [[ ! -f "$BIN_SRC" ]]; then
  echo "Expected binary at $BIN_SRC"
  echo "Build with: cd $REPO_DIR && cargo build --release"
  exit 1
fi

if [[ ! -d "$STATIC_SRC" ]]; then
  echo "Expected static assets at $STATIC_SRC"
  exit 1
fi

sudo mkdir -p "$DEPLOY_DIR"
sudo cp "$BIN_SRC" "$BIN_DST"
sudo rm -rf "$STATIC_DST"
sudo cp -R "$STATIC_SRC" "$STATIC_DST"
sudo cp -R "$REPO_DIR/deploy" "$DEPLOY_DIR/"

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
