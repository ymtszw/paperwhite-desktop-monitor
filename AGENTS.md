# AGENTS.md

Instructions for coding agents and contributors working on this repository (`paperwhite-desktop-monitor`).
User-facing docs live in [README.md](README.md). Keep README focused on end users and put development details and architectural invariants here.

---

## 1. Project Overview

A high-contrast, zero-overflow E-ink dashboard server written in Rust for Windows, dedicated to repurposing a **Kindle Paperwhite 3** (PW3 Manga Model 32GB, FW 5.15.1.1) as an external hardware monitor for Windows PC and Synology NAS (`DS223j`).

| File / Module | Role |
|---|---|
| `src/main.rs` | Server entrypoint, Axum HTTP server (`:28080`), LAN firewall middleware, Windows PDH thermal zone reader, NVML GPU metrics, WMI hardware fan reader (`root\LibreHardwareMonitor`), async NAS polling (`:8789`), embedded static assets. |
| `static/index.html` | Single-file legacy WebKit-compatible dashboard. 3x3 table-cell grid, responsive sparklines, high-contrast typography, screen rotator (0°/90°/180°/270°), auto-reload on server restart, fullscreen PC sleep card. |
| `deny.toml` | `cargo-deny` configuration enforcing license compliance, bans on malicious/banned crates, and security advisories. |
| `logs/server.log` | Persistent log file combined with Windows Application Event Log (`[KindleDesktopMonitor]`). |

---

## 2. Strict Architectural Invariants

1. **Zero Python Constraint**:
   - Under no circumstances introduce Python scripts, pip packages, or Python wrappers. The backend must remain strictly pure native Rust on Windows, and Go on Synology NAS (`fnz/nas`).
2. **Completely Headless Operation on Windows**:
   - Compiled with `#![windows_subsystem = "windows"]`. No console window may ever appear on the user's desktop during startup or continuous background execution.
   - All logging is directed to `logs/server.log` and Windows Application Event Log via `RegisterEventSourceW`/`ReportEventW`.
3. **Kindle Paperwhite 3 (Legacy WebKit) Display Invariants**:
   - Screen resolution: 1072x1268 (Portrait), safe landscape viewport: **1226 x 1064 px** (height deducted by 42px top chrome).
   - **Zero Scrolling Policy**: Telemetry must continuously report `sTop=0, sLeft=0`. Any vertical or horizontal overflow is treated as a breaking bug.
   - **No Unsupported Unicode / Emojis**: Kindle PW3 built-in fonts lack emojis and exotic symbols (they render as `▯`). Only use standard ASCII and proven symbols (`↓`, `↑`, `⟳`, `☾`, `%`, `°C`).
   - **No CSS Viewport Bug Traps**: Legacy WebKit miscalculates relative percentage heights inside `display: table-cell`. All inner chart bars and sparklines must use explicit integer pixel bounding (`maxBarPx = 50`).
4. **Sub-System Visual Differentiation**:
   - **PC Tiles (Rows 1 & 2: CPU, GPU, RAM, NETWORK, STORAGE)**: Clean white cards with crisp 3px solid black border, black text, and thin solid divider lines (`solid #000`).
   - **Room Climate Tile (Row 2, Cell 5: ROOM CLIMATE)**: Inverted black pill badge header (`[ ROOM CLIMATE ]`), source tag (`SWITCHBOT`), and **dotted divider lines** (`dotted #000`), displaying ambient room temperature, humidity, battery, and dynamic sparkline history.
   - **NAS Tiles (Row 3, Cells 6 & 7: NAS SYS, NAS VOL / NW)**: Branded with a solid **inverted black header banner** (`background: #000; color: #fff;`) spanning edge-to-edge, and **dashed divider lines** (`border-top: 2px dashed #000;`), creating instant visual distinction on monochrome E-ink displays.
   - **System Controller Tile (Row 3, Cell 8: SYSTEM)**: Screen rotator and motherboard system temperature card.

---

## 3. Environment & Toolchain Setup

Windows 11 (x86_64-pc-windows-gnu / x86_64-pc-windows-msvc), Rust 2024 edition.

```powershell
# Quality tools (must be installed globally in ~/.cargo/bin)
cargo install cargo-binstall --locked
cargo binstall -y cargo-machete cargo-audit cargo-deny
rustup component add rustfmt clippy
```

---

## 4. Quality Gate (Run Before Every Commit)

Every commit must pass the full verification gate with **zero warnings and zero errors**:

```powershell
cargo fmt --check; cargo clippy --all-targets --all-features; cargo machete; cargo audit; cargo deny check; cargo test
```

- **`cargo fmt --check`**: Enforces canonical code style.
- **`cargo clippy --all-targets --all-features`**: Enforces strict Rust idiomatic patterns. Must maintain zero warnings.
- **`cargo machete`**: Detects and eliminates unused dependencies in `Cargo.toml`.
- **`cargo audit`**: Checks for RustSec CVE security advisories.
- **`cargo deny check`**: Validates licenses, crate bans, and sources via `deny.toml`.
- **`cargo test`**: Runs unit test suite.

---

## 5. Process Lifecycle & Daemon Management

Because the binary uses `#![windows_subsystem = "windows"]`, launching it in standard shells requires careful handling:

1. **Rebuilding the binary (`os error 5` mitigation)**:
   - When the server is already running, Windows locks `target\release\paperwhite-desktop-monitor.exe`.
   - Always terminate the existing process before compiling:
     ```powershell
     Stop-Process -Name paperwhite-desktop-monitor -Force -ErrorAction SilentlyContinue
     cargo build --release
     ```
2. **Spawning Detached Background Daemon**:
   - Processes started directly from an agent's terminal shell or child job can be terminated when the parent session finishes.
   - To spawn a permanently detached root process, use WMI:
     ```powershell
     Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
       CommandLine = "$PWD\target\release\paperwhite-desktop-monitor.exe";
       CurrentDirectory = "$PWD"
     }
     ```
3. **Auto-Reload on Client**:
   - `SystemStatus.server_instance` stores the server boot timestamp. When the server restarts with a new instance ID, the Kindle web client detects the change on its next poll and automatically triggers `window.location.reload(true)` to pick up new HTML/CSS assets without user intervention.

---

## 6. Hardware Sensor Architecture

- **CPU Temperature**:
  - Read via native Windows PDH performance counter: `\Thermal Zone Information(\_TZ.TZ00)\Temperature` (converted from Kelvin to Celsius). Falls back to `sysinfo::Components`.
- **GPU Metrics (Usage, VRAM, Temperature, Fan RPM / %)**:
  - Read via native NVML (`nvml-wrapper` crate loading `nvml.dll`). Retrieves live fan speed directly in RPM via `dev.fan_speed_rpm(0)` (falling back to percentage via `dev.fan_speed(0)`). Operates in user mode with zero elevation issues.
- **Motherboard / Case Fan Sensors (MSI PRO Z790-A)**:
  - The PC's motherboard uses a Nuvoton NCT6687D Super I/O chip. Direct port I/O access (ports 0x2E/0x2F) requires a Ring 0 kernel driver.
  - While MSI Center uses `NTIOLib_X64.sys` internally, its driver device (`\\.\NTIOLib_CC_COMM`) and named pipe (`\\.\pipe\MSI_CC_Engine`) enforce strict ACLs (Administrator/SYSTEM only) and do NOT expose a public WMI provider or REST API.
  - The server queries `root\LibreHardwareMonitor` and `root\OpenHardwareMonitor` via WMI (`wmi` crate) using `SELECT Name, Value FROM Sensor WHERE SensorType = 'Fan'`.
  - When LibreHardwareMonitor or FanControl runs with Administrator privileges and WMI enabled, all fan speeds (`CPU Fan`, `Pump Fan`, `System Fan 1..N`) are dynamically queried.
  - If WMI is standby, the GPU fan (RPM) remains fully active, and the CPU/SYS fan panels gracefully show standby states without breaking layout.
- **Synology NAS Telemetry**:
  - Sampled asynchronously every 2 seconds from `http://192.168.0.12:8789/api/stats` (`fnz-nas` daemon on DS223j).
  - Retrieves `cpuPct`, `memPct`, `diskPct` (`/volume1`), `tempC`, `netInKb`, `netOutKb`, and `systemLoad1`.
