use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::Utc;
use serde::Serialize;
use sysinfo::Disks;
use sysinfo::Networks;
use sysinfo::System;
use tokio::sync::Mutex;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    telemetry: Arc<Mutex<TelemetryState>>,
}

struct TelemetryState {
    previous_rx: u64,
    previous_tx: u64,
    previous_sample: Instant,
    initialized: bool,
}

#[derive(Serialize)]
struct DashboardResponse {
    updated_at: String,
    stale: bool,
    status: String,
    system: SystemStats,
    network: NetworkStats,
    ups: UpsStats,
}

#[derive(Serialize)]
struct SystemStats {
    cpu_percent: u8,
    cpu_used_cores: f32,
    cpu_total_cores: u16,
    cpu_used_percent: f32,
    cpu_total_percent: f32,
    ram_percent: u8,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
    storage_percent: u8,
    storage_used_bytes: u64,
    storage_total_bytes: u64,
    temperature_c: f32,
}

#[derive(Serialize)]
struct NetworkStats {
    download_bytes_per_sec: f64,
    upload_bytes_per_sec: f64,
}

#[derive(Serialize)]
struct UpsStats {
    status: String,
    battery_percent: f32,
    load_percent: f32,
    line_voltage: f32,
    runtime_minutes: i32,
    last_transfer: String,
    source: String,
}

#[tokio::main]
async fn main() {
    let state = AppState {
        telemetry: Arc::new(Mutex::new(TelemetryState {
            previous_rx: 0,
            previous_tx: 0,
            previous_sample: Instant::now(),
            initialized: false,
        })),
    };

    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind server");

    println!("Dashboard running at http://127.0.0.1:8080");

    axum::serve(listener, app)
        .await
        .expect("server failed");
}

fn build_app(state: AppState) -> Router {
    let api = Router::new().route("/dashboard", get(get_dashboard));

    Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new("static"))
        .with_state(state)
}

async fn get_dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let mut system = System::new_all();
    system.refresh_all();

    let cpu_total_cores = system.cpus().len().max(1) as u16;
    let cpu_usage = system.global_cpu_info().cpu_usage().clamp(0.0, 100.0);
    let cpu_used_cores = (cpu_total_cores as f32) * (cpu_usage / 100.0);
    let cpu_percent = cpu_usage.round() as u8;

    let total_memory_bytes = (system.total_memory() as u64).saturating_mul(1024);
    let used_memory_bytes = (system.used_memory() as u64).saturating_mul(1024);
    let total_memory = total_memory_bytes as f64;
    let used_memory = used_memory_bytes as f64;
    let ram_percent = if total_memory > 0.0 {
        ((used_memory / total_memory) * 100.0).round().clamp(0.0, 100.0) as u8
    } else {
        0
    };

    let mut storage_percent = 0_u8;
    let mut storage_used_bytes = 0_u64;
    let mut storage_total_bytes = 0_u64;
    let disks = Disks::new_with_refreshed_list();
    for disk in disks.list() {
        if disk.mount_point().to_string_lossy() == "/" {
            storage_total_bytes = disk.total_space();
            let avail_bytes = disk.available_space();
            storage_used_bytes = storage_total_bytes.saturating_sub(avail_bytes);

            let total = storage_total_bytes as f64;
            let avail = avail_bytes as f64;
            if total > 0.0 {
                storage_percent = (((total - avail) / total) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8;
            }
            break;
        }
    }

    let temperature_c = read_pi_temperature_c().unwrap_or(0.0);

    let (download_bps, upload_bps) = compute_network_rates(&state).await;

    let ups = collect_ups_stats().unwrap_or_else(|| UpsStats {
        status: "UNKNOWN".to_string(),
        battery_percent: 0.0,
        load_percent: 0.0,
        line_voltage: 0.0,
        runtime_minutes: 0,
        last_transfer: "Unavailable".to_string(),
        source: "fallback".to_string(),
    });

    let status = derive_global_status(temperature_c, &ups.status, &ups.battery_percent);

    let payload = DashboardResponse {
        updated_at: Utc::now().to_rfc3339(),
        stale: false,
        status,
        system: SystemStats {
            cpu_percent,
            cpu_used_cores,
            cpu_total_cores,
            cpu_used_percent: cpu_usage,
            cpu_total_percent: 100.0,
            ram_percent,
            ram_used_bytes: used_memory_bytes,
            ram_total_bytes: total_memory_bytes,
            storage_percent,
            storage_used_bytes,
            storage_total_bytes,
            temperature_c,
        },
        network: NetworkStats {
            download_bytes_per_sec: download_bps,
            upload_bytes_per_sec: upload_bps,
        },
        ups,
    };

    Json(payload)
}

async fn compute_network_rates(state: &AppState) -> (f64, f64) {
    let mut networks = Networks::new_with_refreshed_list();
    networks.refresh();

    let mut total_rx = 0_u64;
    let mut total_tx = 0_u64;

    for (_, data) in &networks {
        total_rx += data.total_received();
        total_tx += data.total_transmitted();
    }

    let mut lock = state.telemetry.lock().await;
    let now = Instant::now();

    if !lock.initialized {
        lock.previous_rx = total_rx;
        lock.previous_tx = total_tx;
        lock.previous_sample = now;
        lock.initialized = true;
        return (0.0, 0.0);
    }

    let elapsed = now.duration_since(lock.previous_sample).as_secs_f64().max(1.0);
    let rx_delta = total_rx.saturating_sub(lock.previous_rx) as f64;
    let tx_delta = total_tx.saturating_sub(lock.previous_tx) as f64;

    lock.previous_rx = total_rx;
    lock.previous_tx = total_tx;
    lock.previous_sample = now;

    (rx_delta / elapsed, tx_delta / elapsed)
}

fn read_pi_temperature_c() -> Option<f32> {
    let raw = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?;
    let milli = raw.trim().parse::<f32>().ok()?;
    Some((milli / 1000.0 * 10.0).round() / 10.0)
}

fn collect_ups_stats() -> Option<UpsStats> {
    let output = Command::new("apcaccess").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    Some(parse_apcaccess_text(&text))
}

fn parse_apcaccess_text(text: &str) -> UpsStats {
    let mut status = "UNKNOWN".to_string();
    let mut battery_percent = 0.0;
    let mut load_percent = 0.0;
    let mut line_voltage = 0.0;
    let mut runtime_minutes = 0;
    let mut last_transfer = "NONE".to_string();

    for line in text.lines() {
        let mut parts = line.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();

        match key {
            "STATUS" => status = value.to_string(),
            "BCHARGE" => battery_percent = parse_leading_float(value),
            "LOADPCT" => load_percent = parse_leading_float(value),
            "LINEV" => line_voltage = parse_leading_float(value),
            "TIMELEFT" => runtime_minutes = parse_leading_float(value).round() as i32,
            "LASTXFER" => last_transfer = value.to_string(),
            _ => {}
        }
    }

    UpsStats {
        status,
        battery_percent,
        load_percent,
        line_voltage,
        runtime_minutes,
        last_transfer,
        source: "apcupsd".to_string(),
    }
}

fn parse_leading_float(input: &str) -> f32 {
    let token = input.split_whitespace().next().unwrap_or("0");
    match token.parse::<f32>() {
        Ok(value) if value.is_finite() => value,
        _ => 0.0,
    }
}

fn derive_global_status(temp_c: f32, ups_status: &str, battery_percent: &f32) -> String {
    if ups_status != "ONLINE" || *battery_percent < 20.0 || temp_c >= 70.0 {
        "Critical".to_string()
    } else if *battery_percent < 40.0 || temp_c >= 60.0 {
        "Warning".to_string()
    } else {
        "Healthy".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use tower::ServiceExt;

    const APCACCESS_NOTIONAL: &str = include_str!("../tests/data/apcaccess_notional.txt");
    const APCACCESS_ONBATT_NOTIONAL: &str =
        include_str!("../tests/data/apcaccess_onbatt_notional.txt");
    const APCACCESS_MALFORMED_NOTIONAL: &str =
        include_str!("../tests/data/apcaccess_malformed_notional.txt");

    #[test]
    fn parse_apcaccess_notional_data_file() {
        let stats = parse_apcaccess_text(APCACCESS_NOTIONAL);

        assert_eq!(stats.status, "ONLINE");
        assert!((stats.battery_percent - 96.5).abs() < 0.01);
        assert!((stats.load_percent - 22.0).abs() < 0.01);
        assert!((stats.line_voltage - 121.4).abs() < 0.01);
        assert_eq!(stats.runtime_minutes, 47);
        assert_eq!(stats.last_transfer, "No transfers since turnon");
        assert_eq!(stats.source, "apcupsd");
    }

    #[test]
    fn derive_status_from_notional_thresholds() {
        assert_eq!(derive_global_status(52.0, "ONLINE", &96.5), "Healthy");
        assert_eq!(derive_global_status(61.0, "ONLINE", &50.0), "Warning");
        assert_eq!(derive_global_status(72.0, "ONLINE", &50.0), "Critical");
        assert_eq!(derive_global_status(50.0, "ONBATT", &50.0), "Critical");
    }

    #[test]
    fn parse_apcaccess_onbatt_notional_data_file() {
        let stats = parse_apcaccess_text(APCACCESS_ONBATT_NOTIONAL);

        assert_eq!(stats.status, "ONBATT");
        assert!((stats.battery_percent - 18.2).abs() < 0.01);
        assert!((stats.load_percent - 41.0).abs() < 0.01);
        assert!((stats.line_voltage - 0.0).abs() < 0.01);
        assert_eq!(stats.runtime_minutes, 6);
        assert_eq!(stats.last_transfer, "Automatic or explicit self test");
    }

    #[test]
    fn parse_apcaccess_malformed_values_fallback_to_zero() {
        let stats = parse_apcaccess_text(APCACCESS_MALFORMED_NOTIONAL);

        assert_eq!(stats.status, "ONLINE");
        assert_eq!(stats.battery_percent, 0.0);
        assert_eq!(stats.load_percent, 0.0);
        assert_eq!(stats.line_voltage, 0.0);
        assert_eq!(stats.runtime_minutes, 0);
        assert_eq!(stats.last_transfer, "Communication lost and restored");
    }

    #[test]
    fn parse_apcaccess_missing_fields_uses_defaults() {
        let stats = parse_apcaccess_text("STATUS   : ONLINE\n");

        assert_eq!(stats.status, "ONLINE");
        assert_eq!(stats.battery_percent, 0.0);
        assert_eq!(stats.load_percent, 0.0);
        assert_eq!(stats.line_voltage, 0.0);
        assert_eq!(stats.runtime_minutes, 0);
        assert_eq!(stats.last_transfer, "NONE");
    }

    #[test]
    fn parse_leading_float_handles_units_and_bad_values() {
        assert!((parse_leading_float("121.4 Volts") - 121.4).abs() < 0.01);
        assert!((parse_leading_float("18.2 Percent") - 18.2).abs() < 0.01);
        assert_eq!(parse_leading_float("???"), 0.0);
        assert_eq!(parse_leading_float(""), 0.0);
    }

    #[test]
    fn derive_status_battery_threshold_boundaries() {
        assert_eq!(derive_global_status(50.0, "ONLINE", &39.9), "Warning");
        assert_eq!(derive_global_status(50.0, "ONLINE", &19.9), "Critical");
        assert_eq!(derive_global_status(59.9, "ONLINE", &40.0), "Healthy");
    }

    fn test_state() -> AppState {
        AppState {
            telemetry: Arc::new(Mutex::new(TelemetryState {
                previous_rx: 0,
                previous_tx: 0,
                previous_sample: Instant::now(),
                initialized: false,
            })),
        }
    }

    #[tokio::test]
    async fn integration_api_dashboard_returns_expected_json_shape() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("body should be readable");
        let parsed: Value = serde_json::from_slice(&body).expect("valid JSON payload");

        assert!(parsed.get("updated_at").is_some());
        assert!(parsed.get("status").is_some());
        assert!(parsed.get("system").is_some());
        assert!(parsed.get("network").is_some());
        assert!(parsed.get("ups").is_some());

        let system = parsed.get("system").expect("system object");
        let network = parsed.get("network").expect("network object");
        let ups = parsed.get("ups").expect("ups object");

        assert!(system.get("cpu_percent").is_some());
        assert!(system.get("cpu_used_cores").is_some());
        assert!(system.get("cpu_total_cores").is_some());
        assert!(system.get("cpu_used_percent").is_some());
        assert!(system.get("cpu_total_percent").is_some());
        assert!(system.get("ram_percent").is_some());
        assert!(system.get("ram_used_bytes").is_some());
        assert!(system.get("ram_total_bytes").is_some());
        assert!(system.get("storage_percent").is_some());
        assert!(system.get("storage_used_bytes").is_some());
        assert!(system.get("storage_total_bytes").is_some());
        assert!(system.get("temperature_c").is_some());

        assert!(network.get("download_bytes_per_sec").is_some());
        assert!(network.get("upload_bytes_per_sec").is_some());

        assert!(ups.get("status").is_some());
        assert!(ups.get("battery_percent").is_some());
        assert!(ups.get("load_percent").is_some());
        assert!(ups.get("line_voltage").is_some());
        assert!(ups.get("runtime_minutes").is_some());
        assert!(ups.get("last_transfer").is_some());
        assert!(ups.get("source").is_some());
    }

    #[tokio::test]
    async fn integration_static_root_serves_html_page() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("body should be readable");
        let html = String::from_utf8(body.to_vec()).expect("response should be utf-8 HTML");

        assert!(html.contains("Pi UPS Dashboard"));
    }
}
