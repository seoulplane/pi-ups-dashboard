use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::Utc;
use serde::Serialize;
use sysinfo::Disks;
use sysinfo::Networks;
use sysinfo::System;
use sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;
use tokio::process::Command;
use tokio::sync::{broadcast, RwLock};
use tokio::time;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const APCACCESS_TIMEOUT: Duration = Duration::from_secs(3);
const BROADCAST_CAPACITY: usize = 16;
const STALE_AFTER: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct AppState {
    snapshot: Arc<RwLock<DashboardResponse>>,
    tx: broadcast::Sender<DashboardResponse>,
}

#[derive(Clone, Serialize)]
struct DashboardResponse {
    updated_at: String,
    stale: bool,
    status: String,
    system: SystemStats,
    network: NetworkStats,
    ups: UpsStats,
}

#[derive(Clone, Serialize)]
struct SystemStats {
    cpu_percent: u8,
    cpu_active_cores: u16,
    cpu_used_cores: f32,
    cpu_total_cores: u16,
    cpu_used_percent: f32,
    cpu_total_percent: f32,
    cpu_cores_percent: Vec<u8>,
    ram_percent: u8,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
    storage_percent: u8,
    storage_used_bytes: u64,
    storage_total_bytes: u64,
    temperature_c: f32,
}

#[derive(Clone, Serialize)]
struct NetworkStats {
    download_bytes_per_sec: f64,
    upload_bytes_per_sec: f64,
}

#[derive(Clone, Serialize)]
struct UpsStats {
    status: String,
    battery_percent: f32,
    load_percent: f32,
    line_voltage: f32,
    runtime_minutes: i32,
    last_transfer: String,
    source: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
    let snapshot = Arc::new(RwLock::new(empty_snapshot()));

    tokio::spawn(sampler_task(snapshot.clone(), tx.clone()));

    let state = AppState {
        snapshot,
        tx,
    };

    let static_dir = resolve_static_dir();
    let app = build_app(state, static_dir.clone());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind server");

    println!(
        "Dashboard running at http://127.0.0.1:8080 (static: {})",
        static_dir.display()
    );

    axum::serve(listener, app).await.expect("server failed");
}

fn resolve_static_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("static");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("static")
}

fn build_app(state: AppState, static_dir: PathBuf) -> Router {
    let api = Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/dashboard/stream", get(get_dashboard_stream));

    let static_service = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=300"),
        ))
        .service(ServeDir::new(static_dir));

    Router::new()
        .nest("/api", api)
        .fallback_service(static_service)
        .layer(CompressionLayer::new())
        .with_state(state)
}

async fn get_dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let snap = state.snapshot.read().await.clone();
    Json(snap)
}

async fn get_dashboard_stream(State(state): State<AppState>) -> impl IntoResponse {
    let initial = state.snapshot.read().await.clone();
    let rx = state.tx.subscribe();

    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(
            Event::default()
                .json_data(&initial)
                .unwrap_or_else(|_| Event::default()),
        );

        let mut tail = BroadcastStream::new(rx);
        while let Some(Ok(snap)) = tail.next().await {
            yield Ok(
                Event::default()
                    .json_data(&snap)
                    .unwrap_or_else(|_| Event::default()),
            );
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn sampler_task(
    snapshot: Arc<RwLock<DashboardResponse>>,
    tx: broadcast::Sender<DashboardResponse>,
) {
    let mut system = System::new_all();
    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();

    system.refresh_cpu();
    time::sleep(MINIMUM_CPU_UPDATE_INTERVAL).await;

    let mut prev_rx: u64 = 0;
    let mut prev_tx: u64 = 0;
    let mut prev_sample = Instant::now();
    let mut initialized = false;

    loop {
        system.refresh_cpu();
        system.refresh_memory();
        disks.refresh();
        networks.refresh();

        let cpu_total_cores = system.cpus().len().max(1) as u16;
        let cpu_usage = system.global_cpu_info().cpu_usage().clamp(0.0, 100.0);
        let cpu_cores_percent: Vec<u8> = system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage().clamp(0.0, 100.0).round() as u8)
            .collect();
        let cpu_active_cores = cpu_cores_percent.iter().filter(|&&p| p >= 1).count() as u16;
        let cpu_used_cores = (cpu_total_cores as f32) * (cpu_usage / 100.0);
        let cpu_percent = cpu_usage.round() as u8;

        let total_memory_bytes = system.total_memory();
        let used_memory_bytes = system.used_memory();
        let ram_percent = if total_memory_bytes > 0 {
            ((used_memory_bytes as f64 / total_memory_bytes as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8
        } else {
            0
        };

        let (storage_percent, storage_used_bytes, storage_total_bytes) = read_root_disk(&disks);
        let temperature_c = read_pi_temperature_c().unwrap_or(0.0);

        let mut total_rx = 0_u64;
        let mut total_tx = 0_u64;
        for (name, data) in &networks {
            if is_virtual_interface(name) {
                continue;
            }
            total_rx += data.total_received();
            total_tx += data.total_transmitted();
        }

        let now = Instant::now();
        let (download_bps, upload_bps) = if initialized {
            let elapsed = now.duration_since(prev_sample).as_secs_f64().max(0.001);
            (
                total_rx.saturating_sub(prev_rx) as f64 / elapsed,
                total_tx.saturating_sub(prev_tx) as f64 / elapsed,
            )
        } else {
            (0.0, 0.0)
        };
        prev_rx = total_rx;
        prev_tx = total_tx;
        prev_sample = now;
        initialized = true;

        let ups = collect_ups_stats().await.unwrap_or_else(fallback_ups);
        let status = derive_global_status(temperature_c, &ups.status, &ups.battery_percent);

        let payload = DashboardResponse {
            updated_at: Utc::now().to_rfc3339(),
            stale: false,
            status,
            system: SystemStats {
                cpu_percent,
                cpu_active_cores,
                cpu_used_cores,
                cpu_total_cores,
                cpu_used_percent: cpu_usage,
                cpu_total_percent: 100.0,
                cpu_cores_percent,
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

        {
            let mut guard = snapshot.write().await;
            *guard = payload.clone();
        }
        let _ = tx.send(payload);

        time::sleep(SAMPLE_INTERVAL).await;
    }
}

fn is_virtual_interface(name: &str) -> bool {
    name == "lo"
        || name.starts_with("docker")
        || name.starts_with("br-")
        || name.starts_with("veth")
        || name.starts_with("tailscale")
        || name.starts_with("wg")
}

fn read_root_disk(disks: &Disks) -> (u8, u64, u64) {
    for disk in disks.list() {
        if disk.mount_point().to_string_lossy() == "/" {
            let total = disk.total_space();
            let avail = disk.available_space();
            let used = total.saturating_sub(avail);
            let pct = if total > 0 {
                ((used as f64 / total as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8
            } else {
                0
            };
            return (pct, used, total);
        }
    }
    (0, 0, 0)
}

fn read_pi_temperature_c() -> Option<f32> {
    let raw = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?;
    let milli = raw.trim().parse::<f32>().ok()?;
    Some((milli / 1000.0 * 10.0).round() / 10.0)
}

async fn collect_ups_stats() -> Option<UpsStats> {
    let output = time::timeout(APCACCESS_TIMEOUT, Command::new("apcaccess").output())
        .await
        .ok()?
        .ok()?;

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

fn fallback_ups() -> UpsStats {
    UpsStats {
        status: "UNKNOWN".to_string(),
        battery_percent: 0.0,
        load_percent: 0.0,
        line_voltage: 0.0,
        runtime_minutes: 0,
        last_transfer: "Unavailable".to_string(),
        source: "fallback".to_string(),
    }
}

fn empty_snapshot() -> DashboardResponse {
    DashboardResponse {
        updated_at: Utc::now().to_rfc3339(),
        stale: true,
        status: "Healthy".to_string(),
        system: SystemStats {
            cpu_percent: 0,
            cpu_active_cores: 0,
            cpu_used_cores: 0.0,
            cpu_total_cores: 1,
            cpu_used_percent: 0.0,
            cpu_total_percent: 100.0,
            cpu_cores_percent: Vec::new(),
            ram_percent: 0,
            ram_used_bytes: 0,
            ram_total_bytes: 0,
            storage_percent: 0,
            storage_used_bytes: 0,
            storage_total_bytes: 0,
            temperature_c: 0.0,
        },
        network: NetworkStats {
            download_bytes_per_sec: 0.0,
            upload_bytes_per_sec: 0.0,
        },
        ups: fallback_ups(),
    }
}

#[allow(dead_code)]
fn is_stale(updated_at: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(updated_at) {
        Ok(ts) => {
            let elapsed = Utc::now().signed_duration_since(ts.with_timezone(&Utc));
            elapsed.to_std().map(|d| d > STALE_AFTER).unwrap_or(false)
        }
        Err(_) => true,
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

    #[test]
    fn is_virtual_interface_detects_loopback_and_bridges() {
        assert!(is_virtual_interface("lo"));
        assert!(is_virtual_interface("docker0"));
        assert!(is_virtual_interface("br-abc123"));
        assert!(is_virtual_interface("veth9f1"));
        assert!(!is_virtual_interface("eth0"));
        assert!(!is_virtual_interface("wlan0"));
    }

    fn test_state() -> AppState {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        AppState {
            snapshot: Arc::new(RwLock::new(empty_snapshot())),
            tx,
        }
    }

    #[tokio::test]
    async fn integration_api_dashboard_returns_expected_json_shape() {
        let app = build_app(test_state(), PathBuf::from("static"));

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
        assert!(system.get("cpu_active_cores").is_some());
        assert!(system.get("cpu_used_cores").is_some());
        assert!(system.get("cpu_total_cores").is_some());
        assert!(system.get("cpu_used_percent").is_some());
        assert!(system.get("cpu_total_percent").is_some());
        assert!(system.get("cpu_cores_percent").is_some());
        let cores = system
            .get("cpu_cores_percent")
            .and_then(Value::as_array)
            .expect("cpu_cores_percent should be an array");
        for v in cores {
            let pct = v.as_u64().expect("each core percent is numeric");
            assert!(pct <= 100);
        }
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

        let cpu_total_cores = system
            .get("cpu_total_cores")
            .and_then(Value::as_u64)
            .expect("cpu_total_cores should be numeric");
        let cpu_active_cores = system
            .get("cpu_active_cores")
            .and_then(Value::as_u64)
            .expect("cpu_active_cores should be numeric");
        let cpu_used_cores = system
            .get("cpu_used_cores")
            .and_then(Value::as_f64)
            .expect("cpu_used_cores should be numeric");
        let ram_total_bytes = system
            .get("ram_total_bytes")
            .and_then(Value::as_u64)
            .expect("ram_total_bytes should be numeric");
        let ram_used_bytes = system
            .get("ram_used_bytes")
            .and_then(Value::as_u64)
            .expect("ram_used_bytes should be numeric");
        let storage_total_bytes = system
            .get("storage_total_bytes")
            .and_then(Value::as_u64)
            .expect("storage_total_bytes should be numeric");
        let storage_used_bytes = system
            .get("storage_used_bytes")
            .and_then(Value::as_u64)
            .expect("storage_used_bytes should be numeric");

        assert!(cpu_total_cores >= 1);
        assert!(cpu_active_cores <= cpu_total_cores);
        assert!(cpu_used_cores >= 0.0);
        assert!(cpu_used_cores <= cpu_total_cores as f64 + 0.01);
        assert!(ram_total_bytes >= ram_used_bytes);
        assert!(storage_total_bytes >= storage_used_bytes);
    }

    #[tokio::test]
    async fn integration_static_root_serves_html_page() {
        let app = build_app(test_state(), PathBuf::from("static"));

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
        assert!(html.contains("id=\"cpu-detail\""));
        assert!(html.contains("id=\"ram-detail\""));
        assert!(html.contains("id=\"storage-detail\""));
        assert!(html.contains("id=\"temp-f\""));
        assert!(!html.contains("PI UPS Dashboard"));
    }
}
