# Pi Zero 2W + APC UPS Dashboard

Lightweight Rust web dashboard that shows:
- Raspberry Pi stats: CPU, RAM, storage, temperature, network upload/download rate
- APC UPS stats from `apcaccess`
- In-memory trend history (last 20 samples) for temperature and network throughput

## Run

```bash
cargo run
```

Open http://127.0.0.1:8080

The server binds to `0.0.0.0:8080`, so anyone on your network can view it via `http://<pi-ip>:8080`.

## Precompiled binaries

Cross-compiled Linux binaries are placed in `binaries/`:

- `binaries/pi-ups-dashboard-linux-aarch64-musl` (for 64-bit Raspberry Pi OS)
- `binaries/pi-ups-dashboard-linux-armv7-musleabihf` (for 32-bit Raspberry Pi OS)

Build them from macOS (Apple Silicon) with Docker + `cross`:

```bash
CROSS_CONTAINER_OPTS='--platform=linux/amd64' cross build --release --target aarch64-unknown-linux-musl
CROSS_CONTAINER_OPTS='--platform=linux/amd64' cross build --release --target armv7-unknown-linux-musleabihf
mkdir -p binaries
cp target/aarch64-unknown-linux-musl/release/pi-ups-dashboard binaries/pi-ups-dashboard-linux-aarch64-musl
cp target/armv7-unknown-linux-musleabihf/release/pi-ups-dashboard binaries/pi-ups-dashboard-linux-armv7-musleabihf
```

## GitHub Actions

This repo includes three workflows:

- `.github/workflows/ci.yml`: runs tests and release compile on PRs and pushes to `main`
- `.github/workflows/build-binaries.yml`: cross-compiles Pi binaries on push to `main` (and manual runs) and uploads them as workflow artifacts
- `.github/workflows/release-bundle.yml`: on version tags (`v*`) creates a GitHub Release with deploy assets (`binary + static + deploy`)

Recommended deploy flow for a Pi on a private network is pull-based:

1. Pi checks for updates (cron/systemd timer)
2. Pi runs `git pull --ff-only`
3. Pi runs one of the deploy scripts in `deploy/systemd/`

This is safer than giving GitHub Actions direct SSH access into your Pi and works well behind NAT/firewalls.

Better than pulling source on every update: pull release assets.

- Build and publish artifacts in GitHub Actions (`release-bundle.yml`)
- Pi checks latest GitHub Release and downloads only deployment assets
- Pi deploys binary/static/deploy assets, restarts service, and runs a health check
- On failure, Pi rolls back to the previous deployed version

Updater files:

- `deploy/systemd/update-from-release.sh`
- `deploy/systemd/pi-ups-dashboard-update.service`
- `deploy/systemd/pi-ups-dashboard-update.timer`
- `deploy/systemd/install-update-timer.sh`
- `deploy/systemd/pi-ups-dashboard-updater.env.example`

Install the updater timer on Pi:

```bash
cd /opt/repos/pi-ups-dashboard
chmod +x deploy/systemd/update-from-release.sh deploy/systemd/install-update-timer.sh
./deploy/systemd/install-update-timer.sh
```

Run an immediate update check:

```bash
sudo systemctl start pi-ups-dashboard-update.service
sudo systemctl status pi-ups-dashboard-update.service --no-pager
```

Optional: set update channel/pinning controls:

```bash
sudo cp /opt/repos/pi-ups-dashboard/deploy/systemd/pi-ups-dashboard-updater.env.example /etc/default/pi-ups-dashboard-updater
sudo nano /etc/default/pi-ups-dashboard-updater
sudo systemctl daemon-reload
sudo systemctl restart pi-ups-dashboard-update.timer
```

Useful options in `/etc/default/pi-ups-dashboard-updater`:

- `RELEASE_TAG_PATTERN='^v[0-9]+\.[0-9]+\.[0-9]+$'` for stable-only SemVer tags
- `PINNED_TAG=v1.2.3` to lock to a specific release

## Notes

- Frontend auto-refreshes every 30 seconds.
- If `apcaccess` is unavailable, the UPS panel shows fallback values and a warning state.
- Temperature is read from `/sys/class/thermal/thermal_zone0/temp` when available.

## Tests

Run backend tests:

```bash
cargo test
```

The parser tests use a notional fixture file at `tests/data/apcaccess_notional.txt`.
Additional fixtures cover UPS battery discharge and malformed inputs:
- `tests/data/apcaccess_onbatt_notional.txt`
- `tests/data/apcaccess_malformed_notional.txt`

Integration-style tests also validate:
- `GET /api/dashboard` returns a valid JSON shape with expected top-level and nested fields.
- `GET /` serves the dashboard HTML page.

## systemd autostart

### One-command install/update (recommended)

This script will clone or pull the repo, build, deploy to `/opt/pi-ups-dashboard`, and enable/start the service.

By default it runs the service as the current shell user (or `SUDO_USER`). Override explicitly if needed:

```bash
SERVICE_USER=whoami SERVICE_GROUP=whoami ./deploy/systemd/install-or-update.sh
```

```bash
chmod +x deploy/systemd/install-or-update.sh
./deploy/systemd/install-or-update.sh
```

Optional: deploy a branch other than `main`:

```bash
./deploy/systemd/install-or-update.sh <branch>
```

### Option 2: deploy precompiled binary (no Cargo needed on Pi)

This script deploys a binary from `binaries/`, copies static/deploy files, and enables/starts the service.

By default it runs the service as the current shell user (or `SUDO_USER`). Override explicitly if needed:

```bash
SERVICE_USER=whoami SERVICE_GROUP=whoami ./deploy/systemd/deploy-precompiled.sh
```

```bash
chmod +x deploy/systemd/deploy-precompiled.sh
./deploy/systemd/deploy-precompiled.sh
```

It auto-selects a binary based on CPU architecture. You can also pass a binary path explicitly:

```bash
./deploy/systemd/deploy-precompiled.sh /opt/repos/pi-ups-dashboard/binaries/pi-ups-dashboard-linux-aarch64-musl
```

1. Keep the git repo under `/opt/repos/pi-ups-dashboard` and build:

```bash
sudo mkdir -p /opt/repos
cd /opt/repos
git clone https://github.com/seoulplane/pi-ups-dashboard.git
cd pi-ups-dashboard
cargo build --release
```

2. Deploy to `/opt/pi-ups-dashboard` and start service:

```bash
cd /opt/repos/pi-ups-dashboard
chmod +x deploy/systemd/install-service.sh
./deploy/systemd/install-service.sh
```

The installer copies:
- binary: `/opt/repos/pi-ups-dashboard/target/release/pi-ups-dashboard` -> `/opt/pi-ups-dashboard/pi-ups-dashboard`
- static assets: `/opt/repos/pi-ups-dashboard/static` -> `/opt/pi-ups-dashboard/static`
- service files: `/opt/repos/pi-ups-dashboard/deploy` -> `/opt/pi-ups-dashboard/deploy`

Service unit file: `deploy/systemd/pi-ups-dashboard.service`
