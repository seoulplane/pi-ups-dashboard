#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="pi-ups-dashboard.service"
REPO_DIR="/opt/repos/pi-ups-dashboard"
DEPLOY_DIR="/opt/pi-ups-dashboard"
BIN_SRC="$REPO_DIR/target/release/pi-ups-dashboard"
BIN_DST="$DEPLOY_DIR/pi-ups-dashboard"

if [[ ! -f "$BIN_SRC" ]]; then
  echo "Expected binary at $BIN_SRC"
  echo "Build with: cd $REPO_DIR && cargo build --release"
  exit 1
fi

sudo mkdir -p "$DEPLOY_DIR"
sudo cp "$BIN_SRC" "$BIN_DST"
sudo cp -R "$REPO_DIR/deploy" "$DEPLOY_DIR/"

sudo cp "$DEPLOY_DIR/deploy/systemd/$SERVICE_NAME" "/etc/systemd/system/$SERVICE_NAME"
sudo systemctl daemon-reload
sudo systemctl enable "$SERVICE_NAME"
sudo systemctl restart "$SERVICE_NAME"
sudo systemctl status "$SERVICE_NAME" --no-pager
