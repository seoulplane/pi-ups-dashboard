#!/usr/bin/env bash
set -euo pipefail

SERVICE_FILE="pi-ups-dashboard-update.service"
TIMER_FILE="pi-ups-dashboard-update.timer"
PROJECT_DIR="/opt/repos/pi-ups-dashboard"

if [[ ! -f "$PROJECT_DIR/deploy/systemd/$SERVICE_FILE" ]]; then
  echo "Missing $PROJECT_DIR/deploy/systemd/$SERVICE_FILE"
  exit 1
fi

if [[ ! -f "$PROJECT_DIR/deploy/systemd/$TIMER_FILE" ]]; then
  echo "Missing $PROJECT_DIR/deploy/systemd/$TIMER_FILE"
  exit 1
fi

if [[ ! -x "$PROJECT_DIR/deploy/systemd/update-from-release.sh" ]]; then
  echo "Expected executable script at $PROJECT_DIR/deploy/systemd/update-from-release.sh"
  echo "Run: chmod +x $PROJECT_DIR/deploy/systemd/update-from-release.sh"
  exit 1
fi

sudo cp "$PROJECT_DIR/deploy/systemd/$SERVICE_FILE" "/etc/systemd/system/$SERVICE_FILE"
sudo cp "$PROJECT_DIR/deploy/systemd/$TIMER_FILE" "/etc/systemd/system/$TIMER_FILE"

sudo systemctl daemon-reload
sudo systemctl enable --now "$TIMER_FILE"
sudo systemctl list-timers "$TIMER_FILE" --no-pager

echo "Update timer installed."
