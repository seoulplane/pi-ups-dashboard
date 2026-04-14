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

1. Build and copy binary:

```bash
cargo build --release
sudo mkdir -p /opt/pi-ups-dashboard
sudo cp target/release/pi-ups-dashboard /opt/pi-ups-dashboard/pi-ups-dashboard
sudo cp -R deploy /opt/pi-ups-dashboard/
```

2. Install and start service:

```bash
cd /opt/pi-ups-dashboard
chmod +x deploy/systemd/install-service.sh
./deploy/systemd/install-service.sh
```

Service unit file: `deploy/systemd/pi-ups-dashboard.service`
