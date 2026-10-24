#![windows_subsystem = "windows"]

use axum::{
    Router,
    extract::{ConnectInfo, Request},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Components, Disks, System};
use tokio::sync::RwLock;

#[cfg(windows)]
mod win_thermal {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    #[repr(C)]
    struct PdhFmtCounterValue {
        c_status: u32,
        _pad: u32,
        double_value: f64,
    }

    #[link(name = "pdh")]
    unsafe extern "system" {
        fn PdhOpenQueryW(
            szDataSource: *const u16,
            dwUserData: usize,
            phQuery: *mut *mut std::ffi::c_void,
        ) -> i32;

        fn PdhAddCounterW(
            hQuery: *mut std::ffi::c_void,
            szFullCounterPath: *const u16,
            dwUserData: usize,
            phCounter: *mut *mut std::ffi::c_void,
        ) -> i32;

        fn PdhAddEnglishCounterW(
            hQuery: *mut std::ffi::c_void,
            szFullCounterPath: *const u16,
            dwUserData: usize,
            phCounter: *mut *mut std::ffi::c_void,
        ) -> i32;

        fn PdhCollectQueryData(hQuery: *mut std::ffi::c_void) -> i32;

        fn PdhGetFormattedCounterValue(
            hCounter: *mut std::ffi::c_void,
            dwFormat: u32,
            lpdwType: *mut u32,
            pValue: *mut PdhFmtCounterValue,
        ) -> i32;

        fn PdhCloseQuery(hQuery: *mut std::ffi::c_void) -> i32;
    }

    const PDH_FMT_DOUBLE: u32 = 0x00000200;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub struct ThermalReader {
        query: *mut std::ffi::c_void,
        counter: *mut std::ffi::c_void,
    }

    unsafe impl Send for ThermalReader {}

    impl ThermalReader {
        pub fn new() -> Option<Self> {
            unsafe {
                let mut query = null_mut();
                let q_res = PdhOpenQueryW(null_mut(), 0, &mut query);
                if q_res != 0 {
                    crate::app_error(&format!("PdhOpenQueryW failed: 0x{:X}", q_res));
                    return None;
                }

                let candidates = [
                    r"\Thermal Zone Information(\_TZ.TZ00)\Temperature",
                    r"\Thermal Zone Information(\_tz.tz00)\Temperature",
                    r"\Thermal Zone Information(*)\Temperature",
                ];

                let mut counter = null_mut();
                let mut added = false;

                for c_path in &candidates {
                    let path = to_wide(c_path);
                    if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) == 0 {
                        crate::app_log(&format!("PdhAddEnglishCounterW succeeded for {}", c_path));
                        added = true;
                        break;
                    }
                    if PdhAddCounterW(query, path.as_ptr(), 0, &mut counter) == 0 {
                        crate::app_log(&format!("PdhAddCounterW succeeded for {}", c_path));
                        added = true;
                        break;
                    }
                }

                if !added {
                    crate::app_error("Failed to add thermal counter to PDH query");
                    PdhCloseQuery(query);
                    return None;
                }

                let _ = PdhCollectQueryData(query);
                Some(Self { query, counter })
            }
        }

        pub fn read_celsius(&self) -> Option<u32> {
            unsafe {
                if PdhCollectQueryData(self.query) == 0 {
                    let mut val = PdhFmtCounterValue {
                        c_status: 0,
                        _pad: 0,
                        double_value: 0.0,
                    };
                    let mut val_type = 0;
                    if PdhGetFormattedCounterValue(
                        self.counter,
                        PDH_FMT_DOUBLE,
                        &mut val_type,
                        &mut val,
                    ) == 0
                    {
                        let kelvin = val.double_value;
                        if kelvin > 200.0 && kelvin < 400.0 {
                            let c = kelvin - 273.15;
                            return Some(c.round() as u32);
                        }
                    }
                }
                None
            }
        }
    }

    impl Drop for ThermalReader {
        fn drop(&mut self) {
            unsafe {
                if !self.query.is_null() {
                    PdhCloseQuery(self.query);
                }
            }
        }
    }
}

#[cfg(windows)]
mod win_eventlog {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegisterEventSourceW(
            lpUNCServerName: *const u16,
            lpSourceName: *const u16,
        ) -> *mut std::ffi::c_void;

        fn ReportEventW(
            hEventLog: *mut std::ffi::c_void,
            wType: u16,
            wCategory: u16,
            dwEventID: u32,
            lpUserSid: *mut std::ffi::c_void,
            wNumStrings: u16,
            dwDataSize: u32,
            lpStrings: *const *const u16,
            lpRawData: *mut std::ffi::c_void,
        ) -> i32;

        fn DeregisterEventSource(hEventLog: *mut std::ffi::c_void) -> i32;
    }

    const EVENTLOG_INFORMATION_TYPE: u16 = 4;
    const EVENTLOG_ERROR_TYPE: u16 = 1;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn write_event(msg: &str, is_error: bool) {
        unsafe {
            let source = to_wide("Application");
            let handle = RegisterEventSourceW(null_mut(), source.as_ptr());
            if !handle.is_null() {
                let formatted = format!("[KindleDesktopMonitor] {}", msg);
                let wide_msg = to_wide(&formatted);
                let strings = [wide_msg.as_ptr()];
                let event_type = if is_error {
                    EVENTLOG_ERROR_TYPE
                } else {
                    EVENTLOG_INFORMATION_TYPE
                };

                ReportEventW(
                    handle,
                    event_type,
                    0,
                    1000,
                    null_mut(),
                    1,
                    0,
                    strings.as_ptr(),
                    null_mut(),
                );

                DeregisterEventSource(handle);
            }
        }
    }
}

// Global logger combining file and Windows Event Log
static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn app_log(msg: &str) {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] {}\n", now, msg);

    if let Ok(_guard) = LOG_LOCK.lock() {
        // Write to logs/server.log
        let _ = std::fs::create_dir_all("logs");
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("logs/server.log")
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    #[cfg(windows)]
    win_eventlog::write_event(msg, false);
}

pub fn app_error(msg: &str) {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] [ERROR] {}\n", now, msg);

    if let Ok(_guard) = LOG_LOCK.lock() {
        let _ = std::fs::create_dir_all("logs");
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("logs/server.log")
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    #[cfg(windows)]
    win_eventlog::write_event(msg, true);
}

#[cfg(windows)]
mod win_fans {
    use super::FanInfo;
    use serde::Deserialize;
    use wmi::WMIConnection;

    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "PascalCase")]
    struct WmiSensor {
        name: String,
        value: f32,
    }

    pub fn query_fans() -> (Option<u32>, Vec<FanInfo>) {
        let mut cpu_fan: Option<u32> = None;
        let mut case_fans: Vec<FanInfo> = Vec::new();

        let namespaces = ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"];
        for ns in namespaces {
            if let Ok(wmi_con) = WMIConnection::with_namespace_path(ns) {
                let query = "SELECT Name, Value FROM Sensor WHERE SensorType = 'Fan'";
                if let Ok(sensors) = wmi_con.raw_query::<WmiSensor>(query)
                    && !sensors.is_empty()
                {
                    for s in sensors {
                        let rpm = s.value.round() as u32;
                        let lower = s.name.to_lowercase();
                        if lower.contains("cpu") {
                            if cpu_fan.is_none() || (cpu_fan == Some(0) && rpm > 0) {
                                cpu_fan = Some(rpm);
                            }
                        } else if !lower.contains("gpu") {
                            case_fans.push(FanInfo { name: s.name, rpm });
                        }
                    }
                    case_fans.sort_by_key(|f| f.name.to_uppercase());
                    return (cpu_fan, case_fans);
                }
            }
        }

        (cpu_fan, case_fans)
    }
}

// Embedded dashboard HTML
const INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FanInfo {
    pub name: String,
    pub rpm: u32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DiskInfo {
    pub name: String,
    pub total_gb: f64,
    pub free_gb: f64,
    pub used_gb: f64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NasStats {
    #[serde(rename = "cpuPct")]
    pub cpu_pct: f64,
    #[serde(rename = "systemLoad1")]
    pub system_load1: f64,
    #[serde(rename = "memTotalGb")]
    pub mem_total_gb: f64,
    #[serde(rename = "memUsedGb")]
    pub mem_used_gb: f64,
    #[serde(rename = "memPct")]
    pub mem_pct: f64,
    #[serde(rename = "diskTotalGb")]
    pub disk_total_gb: f64,
    #[serde(rename = "diskUsedGb")]
    pub disk_used_gb: f64,
    #[serde(rename = "diskFreeGb")]
    pub disk_free_gb: f64,
    #[serde(rename = "diskPct")]
    pub disk_pct: f64,
    #[serde(rename = "netInKb")]
    pub net_in_kb: f64,
    #[serde(rename = "netOutKb")]
    pub net_out_kb: f64,
    #[serde(rename = "tempC")]
    pub temp_c: f64,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub cpu_history: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemStatus {
    pub server_instance: u64, // Unique timestamp for auto-reload detection
    pub hostname: String,
    pub os: String,
    pub server_ip: String,
    pub server_port: u16,
    pub sys_temp: Option<u32>,

    // Synology NAS (DS223j)
    pub nas: Option<NasStats>,

    // CPU
    pub cpu_usage: f32, // smoothed %
    pub cpu_cores: usize,
    pub cpu_temp: Option<u32>,
    pub cpu_history: Vec<f32>, // 20 points (0..100)
    pub cpu_fan: Option<u32>,

    // GPU
    pub gpu_usage: f32, // smoothed %
    pub gpu_name: String,
    pub vram_used_gb: f64,
    pub vram_total_gb: f64,
    pub gpu_temp: u32,
    pub gpu_history: Vec<f32>, // 20 points (0..100)
    pub gpu_fan: Option<u32>,

    // RAM
    pub mem_total_gb: f64,
    pub mem_used_gb: f64,
    pub mem_percent: f64,      // smoothed %
    pub mem_history: Vec<f64>, // 20 points (0..100)

    // Network
    pub net_in_kb: f64,  // smoothed KB/s
    pub net_out_kb: f64, // smoothed KB/s
    pub net_in_history: Vec<f64>,
    pub net_out_history: Vec<f64>,

    // All Non-removable Disks (sorted A-Z)
    pub disks: Vec<DiskInfo>,

    // Case / System Fans (Multiple headers)
    pub fans: Vec<FanInfo>,
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

/// Middleware to enforce Local / Private LAN access only
async fn lan_access_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if !is_local_or_lan(addr.ip()) {
        app_error(&format!(
            "[BLOCKED] Access denied from non-LAN IP: {}",
            addr
        ));
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
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache, no-store, must-revalidate"),
            ),
        ],
        Html(INDEX_HTML),
    )
}

/// Serve recent logs
async fn serve_logs() -> impl IntoResponse {
    let content = std::fs::read_to_string("logs/server.log")
        .unwrap_or_else(|_| "No logs recorded yet.".to_string());
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, no-cache, must-revalidate"),
            ),
        ],
        content,
    )
}

/// Serve status API endpoint
async fn serve_status(
    axum::extract::State(state): axum::extract::State<Arc<RwLock<SystemStatus>>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if let (Some(win_h), Some(scr_h)) = (params.get("winH"), params.get("scrH")) {
        app_log(&format!(
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
        ));
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

    app_log(&format!(
        "Server started. Instance ID: {}, Port: {}, LAN IP: {}",
        instance_id, port, local_ip
    ));

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
        let mut networks = sysinfo::Networks::new_with_refreshed_list();
        let mut components = Components::new_with_refreshed_list();
        let nvml = nvml_wrapper::Nvml::init().ok();
        #[cfg(windows)]
        let thermal_reader = win_thermal::ThermalReader::new();

        let hostname = System::host_name().unwrap_or_else(|| "PC".to_string());
        let os_str = format!(
            "{} {}",
            System::name().unwrap_or_default(),
            System::os_version().unwrap_or_default()
        )
        .trim()
        .to_string();

        let cpu_cores = sys.cpus().len();

        let gpu_name = if let Some(ref n) = nvml {
            n.device_by_index(0)
                .and_then(|d| d.name())
                .unwrap_or_else(|_| "NVIDIA GPU".to_string())
        } else {
            "GPU".to_string()
        };

        const SMOOTH_WINDOW: usize = 15;
        const HIST_LEN: usize = 20;

        let mut cpu_smooth: VecDeque<f32> = VecDeque::with_capacity(SMOOTH_WINDOW);
        let mut mem_smooth: VecDeque<f64> = VecDeque::with_capacity(SMOOTH_WINDOW);
        let mut gpu_smooth: VecDeque<f32> = VecDeque::with_capacity(SMOOTH_WINDOW);
        let mut net_in_smooth: VecDeque<f64> = VecDeque::with_capacity(SMOOTH_WINDOW);
        let mut net_out_smooth: VecDeque<f64> = VecDeque::with_capacity(SMOOTH_WINDOW);

        let mut cpu_hist: VecDeque<f32> = VecDeque::from(vec![0.0; HIST_LEN]);
        let mut mem_hist: VecDeque<f64> = VecDeque::from(vec![0.0; HIST_LEN]);
        let mut gpu_hist: VecDeque<f32> = VecDeque::from(vec![0.0; HIST_LEN]);
        let mut net_in_hist: VecDeque<f64> = VecDeque::from(vec![0.0; HIST_LEN]);
        let mut net_out_hist: VecDeque<f64> = VecDeque::from(vec![0.0; HIST_LEN]);
        let mut nas_cpu_hist: VecDeque<f64> = VecDeque::from(vec![0.0; HIST_LEN]);

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
            .unwrap_or_default();
        let mut last_nas_stats: Option<NasStats> = None;
        let mut last_cpu_fan: Option<u32> = None;
        let mut last_case_fans: Vec<FanInfo> = Vec::new();

        let mut second_counter: u64 = 0;

        loop {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            disks.refresh(true);
            networks.refresh(true);
            components.refresh(true);

            // CPU / Thermal Zone Temperature (PDH native on Windows)
            #[cfg(windows)]
            let cpu_temp: Option<u32> = thermal_reader
                .as_ref()
                .and_then(|t| t.read_celsius())
                .or_else(|| {
                    components
                        .iter()
                        .find_map(|c| c.temperature().map(|t| t.round() as u32))
                });
            #[cfg(not(windows))]
            let cpu_temp: Option<u32> = components
                .iter()
                .find_map(|c| c.temperature().map(|t| t.round() as u32));

            // 1. CPU
            let instant_cpu = sys.global_cpu_usage();
            cpu_smooth.push_back(instant_cpu);
            if cpu_smooth.len() > SMOOTH_WINDOW {
                cpu_smooth.pop_front();
            }
            let cpu_avg = cpu_smooth.iter().sum::<f32>() / cpu_smooth.len() as f32;

            // 2. RAM
            let total_mem_bytes = sys.total_memory();
            let used_mem_bytes = sys.used_memory();
            let total_mem_gb = total_mem_bytes as f64 / 1_073_741_824.0;
            let used_mem_gb = used_mem_bytes as f64 / 1_073_741_824.0;
            let instant_mem_pct = if total_mem_bytes > 0 {
                (used_mem_bytes as f64 / total_mem_bytes as f64) * 100.0
            } else {
                0.0
            };
            mem_smooth.push_back(instant_mem_pct);
            if mem_smooth.len() > SMOOTH_WINDOW {
                mem_smooth.pop_front();
            }
            let mem_avg = mem_smooth.iter().sum::<f64>() / mem_smooth.len() as f64;

            // 3. GPU
            let mut instant_gpu = 0.0f32;
            let mut vram_used_gb = 0.0f64;
            let mut vram_total_gb = 0.0f64;
            let mut gpu_temp = 0u32;
            let mut gpu_fan: Option<u32> = None;
            if let Some(ref n) = nvml
                && let Ok(dev) = n.device_by_index(0)
            {
                if let Ok(util) = dev.utilization_rates() {
                    instant_gpu = util.gpu as f32;
                }
                if let Ok(mem) = dev.memory_info() {
                    vram_used_gb = mem.used as f64 / 1_073_741_824.0;
                    vram_total_gb = mem.total as f64 / 1_073_741_824.0;
                }
                if let Ok(temp) =
                    dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                {
                    gpu_temp = temp;
                }
                if let Ok(fan) = dev.fan_speed(0) {
                    gpu_fan = Some(fan);
                }
            }
            gpu_smooth.push_back(instant_gpu);
            if gpu_smooth.len() > SMOOTH_WINDOW {
                gpu_smooth.pop_front();
            }
            let gpu_avg = gpu_smooth.iter().sum::<f32>() / gpu_smooth.len() as f32;

            // 4. Network
            let mut total_rx_bytes = 0u64;
            let mut total_tx_bytes = 0u64;
            for (iface, data) in &networks {
                if !iface.to_lowercase().contains("loopback") {
                    total_rx_bytes += data.received();
                    total_tx_bytes += data.transmitted();
                }
            }
            let instant_rx_kb = total_rx_bytes as f64 / 1024.0;
            let instant_tx_kb = total_tx_bytes as f64 / 1024.0;
            net_in_smooth.push_back(instant_rx_kb);
            if net_in_smooth.len() > SMOOTH_WINDOW {
                net_in_smooth.pop_front();
            }
            let net_in_avg = net_in_smooth.iter().sum::<f64>() / net_in_smooth.len() as f64;

            net_out_smooth.push_back(instant_tx_kb);
            if net_out_smooth.len() > SMOOTH_WINDOW {
                net_out_smooth.pop_front();
            }
            let net_out_avg = net_out_smooth.iter().sum::<f64>() / net_out_smooth.len() as f64;

            // History & NAS sampling every 2 seconds
            second_counter += 1;
            if second_counter.is_multiple_of(2) {
                cpu_hist.push_back(cpu_avg);
                if cpu_hist.len() > HIST_LEN {
                    cpu_hist.pop_front();
                }

                mem_hist.push_back(mem_avg);
                if mem_hist.len() > HIST_LEN {
                    mem_hist.pop_front();
                }

                gpu_hist.push_back(gpu_avg);
                if gpu_hist.len() > HIST_LEN {
                    gpu_hist.pop_front();
                }

                net_in_hist.push_back(net_in_avg);
                if net_in_hist.len() > HIST_LEN {
                    net_in_hist.pop_front();
                }

                net_out_hist.push_back(net_out_avg);
                if net_out_hist.len() > HIST_LEN {
                    net_out_hist.pop_front();
                }

                // Query NAS stats
                let client = http_client.clone();
                let resp = client
                    .get("http://192.168.0.12:8789/api/stats")
                    .send()
                    .await;
                if let Ok(res) = resp {
                    if let Ok(mut stats) = res.json::<NasStats>().await {
                        stats.online = true;
                        nas_cpu_hist.push_back(stats.cpu_pct);
                        if nas_cpu_hist.len() > HIST_LEN {
                            nas_cpu_hist.pop_front();
                        }
                        stats.cpu_history = nas_cpu_hist.iter().copied().collect();
                        last_nas_stats = Some(stats);
                    }
                } else if let Some(ref mut st) = last_nas_stats {
                    st.online = false;
                }

                // Query Hardware Fan sensors via WMI (LibreHardwareMonitor / OpenHardwareMonitor)
                #[cfg(windows)]
                {
                    if let Ok((c_fan, c_fans)) =
                        tokio::task::spawn_blocking(win_fans::query_fans).await
                    {
                        if c_fan.is_some() {
                            last_cpu_fan = c_fan;
                        }
                        if !c_fans.is_empty() {
                            last_case_fans = c_fans;
                        }
                    }
                }
            }

            // 5. Disks (all non-removable, sorted by name A-Z)
            let mut disk_list = Vec::new();
            for disk in disks.list() {
                if disk.is_removable() {
                    continue;
                }
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
            disk_list.sort_by_key(|a| a.name.to_uppercase());

            // Write into shared state
            {
                let mut lock = status_clone.write().await;
                lock.server_instance = instance_id;
                lock.hostname = hostname.clone();
                lock.os = os_str.clone();
                lock.server_ip = local_ip_clone.clone();
                lock.server_port = port;
                lock.sys_temp = cpu_temp;

                lock.cpu_usage = cpu_avg;
                lock.cpu_cores = cpu_cores;
                lock.cpu_temp = cpu_temp;
                lock.cpu_history = cpu_hist.iter().copied().collect();
                lock.cpu_fan = last_cpu_fan;

                lock.gpu_usage = gpu_avg;
                lock.gpu_name = gpu_name.clone();
                lock.vram_used_gb = vram_used_gb;
                lock.vram_total_gb = vram_total_gb;
                lock.gpu_temp = gpu_temp;
                lock.gpu_history = gpu_hist.iter().copied().collect();
                lock.gpu_fan = gpu_fan;

                lock.mem_total_gb = total_mem_gb;
                lock.mem_used_gb = used_mem_gb;
                lock.mem_percent = mem_avg;
                lock.mem_history = mem_hist.iter().copied().collect();

                lock.net_in_kb = net_in_avg;
                lock.net_out_kb = net_out_avg;
                lock.net_in_history = net_in_hist.iter().copied().collect();
                lock.net_out_history = net_out_hist.iter().copied().collect();

                lock.disks = disk_list;
                lock.nas = last_nas_stats.clone();
                lock.fans = last_case_fans.clone();
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Build Axum Router
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(serve_status))
        .route("/api/logs", get(serve_logs))
        .layer(middleware::from_fn(lan_access_middleware))
        .with_state(shared_status);

    let bind_addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            app_error(&format!(
                "Failed to bind TCP listener on {}: {}",
                bind_addr, e
            ));
            std::process::exit(1);
        }
    };

    app_log(&format!(
        "Server listening on {} (LAN: {})",
        bind_addr, local_ip
    ));

    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        app_error(&format!("Server error: {}", e));
    }
}
