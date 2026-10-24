use axum::{
    extract::{ConnectInfo, Request},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Disks, System};
use tokio::sync::RwLock;

// Embedded dashboard HTML
const INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Debug, Clone, Serialize, Default)]
pub struct DiskInfo {
    pub name: String,
    pub total_gb: f64,
    pub free_gb: f64,
    pub used_gb: f64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemStatus {
    pub server_instance: u64, // Unique timestamp for auto-reload detection
    pub hostname: String,
    pub os: String,
    pub uptime_secs: u64,
    pub uptime_str: String,
    pub uptime_days: u64,
    pub cpu_usage: f32, // 30s smoothed moving average
    pub cpu_cores: usize,
    pub mem_total_gb: f64,
    pub mem_used_gb: f64,
    pub mem_percent: f64, // 30s smoothed moving average
    pub disks: Vec<DiskInfo>,
    pub server_ip: String,
    pub server_port: u16,
}

/// Check if the connecting IP is a loopback or private LAN address
fn is_local_or_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            ipv4.is_loopback()
                || octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ((ipv6.segments()[0] & 0xffc0) == 0xfe80)
                || ((ipv6.segments()[0] & 0xfe00) == 0xfc00)
        }
    }
}

/// Format seconds into readable days/hours
fn format_uptime(secs: u64) -> (String, u64) {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    if days > 0 {
        (format!("{}日 {}時間", days, hours), days)
    } else {
        (format!("{}時間", hours), 0)
    }
}

/// Middleware to enforce Local / Private LAN access only
async fn lan_access_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if !is_local_or_lan(addr.ip()) {
        eprintln!("[BLOCKED] Access denied from non-LAN IP: {}", addr);
        return (
            StatusCode::FORBIDDEN,
            "403 Forbidden: Access is restricted to local LAN only.",
        )
            .into_response();
    }
    next.run(req).await
}

/// Serve dashboard index page
async fn serve_index() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        ),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        )],
        Html(INDEX_HTML),
    )
}

/// Serve status API endpoint
async fn serve_status(
    axum::extract::State(state): axum::extract::State<Arc<RwLock<SystemStatus>>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if let (Some(win_h), Some(scr_h)) = (params.get("winH"), params.get("scrH")) {
        println!(
            "[Kindle Telemetry] win: {}x{}, docClient: {}x{}, scroll: {}x{}, sTop={}, sLeft={}, rot={}",
            params.get("winW").unwrap_or(&"-".to_string()),
            win_h,
            params.get("docW").unwrap_or(&"-".to_string()),
            params.get("docH").unwrap_or(&"-".to_string()),
            params.get("scrW").unwrap_or(&"-".to_string()),
            scr_h,
            params.get("sTop").unwrap_or(&"0".to_string()),
            params.get("sLeft").unwrap_or(&"0".to_string()),
            params.get("rot").unwrap_or(&"-".to_string()),
        );
    }
    let status = state.read().await.clone();
    (
        [(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate"),
        )],
        Json(status),
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(28080);

    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let instance_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    println!("======================================================");
    println!("  Kindle Paperwhite Desktop Monitor Server (Rust)");
    println!("======================================================");
    println!("* Instance ID:           {}", instance_id);
    println!("* Moving Average:        Enabled (30s window)");
    println!("* Auto-Reload on Update: Enabled");
    println!("* Server Port:           {}", port);
    println!("* PC LAN IP:             {}", local_ip);
    println!(">> Kindle URL:           http://{}:{}/", local_ip, port);
    println!("======================================================");

    let shared_status = Arc::new(RwLock::new(SystemStatus {
        server_instance: instance_id,
        server_ip: local_ip.clone(),
        server_port: port,
        ..Default::default()
    }));

    // Background monitoring task (samples every 1s, computes 30s moving average)
    let status_clone = Arc::clone(&shared_status);
    let local_ip_clone = local_ip.clone();
    tokio::spawn(async move {
        let mut sys = System::new_all();
        let mut disks = Disks::new_with_refreshed_list();
        let hostname = System::host_name().unwrap_or_else(|| "PC".to_string());
        let os_str = format!(
            "{} {}",
            System::name().unwrap_or_default(),
            System::os_version().unwrap_or_default()
        )
        .trim()
        .to_string();

        let cpu_cores = sys.cpus().len();

        const WINDOW_SIZE: usize = 30;
        let mut cpu_history: VecDeque<f32> = VecDeque::with_capacity(WINDOW_SIZE);
        let mut mem_history: VecDeque<f64> = VecDeque::with_capacity(WINDOW_SIZE);

        loop {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            disks.refresh(true);

            let instant_cpu = sys.global_cpu_usage();
            cpu_history.push_back(instant_cpu);
            if cpu_history.len() > WINDOW_SIZE {
                cpu_history.pop_front();
            }
            let cpu_avg = cpu_history.iter().sum::<f32>() / cpu_history.len() as f32;

            let total_mem_bytes = sys.total_memory();
            let used_mem_bytes = sys.used_memory();
            let total_mem_gb = total_mem_bytes as f64 / 1_073_741_824.0;
            let used_mem_gb = used_mem_bytes as f64 / 1_073_741_824.0;
            let instant_mem_pct = if total_mem_bytes > 0 {
                (used_mem_bytes as f64 / total_mem_bytes as f64) * 100.0
            } else {
                0.0
            };

            mem_history.push_back(instant_mem_pct);
            if mem_history.len() > WINDOW_SIZE {
                mem_history.pop_front();
            }
            let mem_avg = mem_history.iter().sum::<f64>() / mem_history.len() as f64;

            let uptime_secs = System::uptime();
            let (uptime_str, uptime_days) = format_uptime(uptime_secs);

            // Collect Disks
            let mut disk_list = Vec::new();
            for disk in disks.list() {
                let total = disk.total_space();
                if total == 0 {
                    continue;
                }
                let free = disk.available_space();
                let used = total.saturating_sub(free);
                let total_gb = total as f64 / 1_073_741_824.0;
                let free_gb = free as f64 / 1_073_741_824.0;
                let used_gb = used as f64 / 1_073_741_824.0;
                let usage_pct = (used as f64 / total as f64) * 100.0;
                let name = disk.mount_point().to_string_lossy().to_string();

                disk_list.push(DiskInfo {
                    name,
                    total_gb,
                    free_gb,
                    used_gb,
                    usage_percent: usage_pct,
                });
            }

            // Write into shared state
            {
                let mut lock = status_clone.write().await;
                lock.server_instance = instance_id;
                lock.hostname = hostname.clone();
                lock.os = os_str.clone();
                lock.uptime_secs = uptime_secs;
                lock.uptime_str = uptime_str;
                lock.uptime_days = uptime_days;
                lock.cpu_usage = cpu_avg;
                lock.cpu_cores = cpu_cores;
                lock.mem_total_gb = total_mem_gb;
                lock.mem_used_gb = used_mem_gb;
                lock.mem_percent = mem_avg;
                lock.disks = disk_list;
                lock.server_ip = local_ip_clone.clone();
                lock.server_port = port;
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Build Axum Router
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(serve_status))
        .layer(middleware::from_fn(lan_access_middleware))
        .with_state(shared_status);

    let bind_addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind TCP listener on {}: {}", bind_addr, e);
            std::process::exit(1);
        }
    };

    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        eprintln!("Server error: {}", e);
    }
}
