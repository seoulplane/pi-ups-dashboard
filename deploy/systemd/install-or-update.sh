#!/usr/bin/env bash
set -euo pipefail

REPO_URL="https://github.com/seoulplane/pi-ups-dashboard.git"
REPO_DIR="/opt/repos/pi-ups-dashboard"
DEPLOY_DIR="/opt/pi-ups-dashboard"
SERVICE_NAME="pi-ups-dashboard.service"
BRANCH="${1:-main}"

if ! command -v git >/dev/null 2>&1; then
  echo "Missing dependency: git"
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Missing dependency: cargo"
  echo "Install Rust first: curl https://sh.rustup.rs -sSf | sh -s -- -y"
  exit 1
fi

# Ensure /opt/repos exists and is writable by the invoking user.
if [[ ! -d /opt/repos ]]; then
  sudo mkdir -p /opt/repos
fi

if [[ ! -w /opt/repos ]]; then
  sudo chown "$(id -un)":"$(id -gn)" /opt/repos
fi

if [[ -d "$REPO_DIR/.git" ]]; then
  echo "Updating existing repository at $REPO_DIR"
  git -C "$REPO_DIR" fetch --prune origin
  git -C "$REPO_DIR" checkout "$BRANCH"
  git -C "$REPO_DIR" pull --ff-only origin "$BRANCH"
else
  echo "Cloning repository to $REPO_DIR"
  git clone --branch "$BRANCH" "$REPO_URL" "$REPO_DIR"
fi

echo "Building release binary"
cargo build --release --manifest-path "$REPO_DIR/Cargo.toml"

BIN_SRC="$REPO_DIR/target/release/pi-ups-dashboard"
BIN_DST="$DEPLOY_DIR/pi-ups-dashboard"
STATIC_SRC="$REPO_DIR/static"
STATIC_DST="$DEPLOY_DIR/static"

if [[ ! -f "$BIN_SRC" ]]; then
  echo "Build did not produce $BIN_SRC"
  exit 1
fi

if [[ ! -d "$STATIC_SRC" ]]; then
  echo "Expected static assets at $STATIC_SRC"
  exit 1
fi

echo "Deploying to $DEPLOY_DIR"
sudo mkdir -p "$DEPLOY_DIR"
sudo install -m 0755 "$BIN_SRC" "$BIN_DST"
sudo rm -rf "$STATIC_DST"
sudo cp -R "$STATIC_SRC" "$STATIC_DST"
sudo rm -rf "$DEPLOY_DIR/deploy"
sudo cp -R "$REPO_DIR/deploy" "$DEPLOY_DIR/"

echo "Installing and starting systemd service"
sudo cp "$DEPLOY_DIR/deploy/systemd/$SERVICE_NAME" "/etc/systemd/system/$SERVICE_NAME"
sudo systemctl daemon-reload
sudo systemctl enable --now "$SERVICE_NAME"
sudo systemctl status "$SERVICE_NAME" --no-pager

echo "Done. Dashboard should be available on http://<pi-ip>:8080"
