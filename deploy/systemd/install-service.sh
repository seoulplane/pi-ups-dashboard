#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="pi-ups-dashboard.service"
PROJECT_DIR="/opt/pi-ups-dashboard"
BIN_PATH="$PROJECT_DIR/pi-ups-dashboard"

if [[ ! -f "$BIN_PATH" ]]; then
  echo "Expected binary at $BIN_PATH"
  echo "Build with: cargo build --release && cp target/release/pi-ups-dashboard $BIN_PATH"
  exit 1
fi

sudo cp "$PROJECT_DIR/deploy/systemd/$SERVICE_NAME" "/etc/systemd/system/$SERVICE_NAME"
sudo systemctl daemon-reload
sudo systemctl enable "$SERVICE_NAME"
sudo systemctl restart "$SERVICE_NAME"
sudo systemctl status "$SERVICE_NAME" --no-pager
