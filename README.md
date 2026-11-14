# Tolo — Native Windows Screen Time & Digital Wellbeing in Rust

A high-performance, private, 100% local Windows screen time and digital wellbeing desktop application written in **Rust**. Built with pure native Win32 windowing, low-power background tracking, system tray integration, and zero external subscriptions or telemetry.

---

## 🎨 Branding & Identity

* **Name:** **Tolo**
* **Identity:** Modern Apple/Fluent squircle style featuring an hourglass silhouette forming the letter **T** with electric blue, deep violet, and warm amber accents.
* **Icon Assets:**
  * Multi-resolution Windows Icon: `tolo.ico` (256x256, 128x128, 64x64, 48x48, 32x32, 16x16)
  * In-App Embedded SVG/PNG Data: `ui/logo_base64.txt`

---

## 🔍 Architecture Breakdown

* **Language & Core:** Rust (`rustc 1.84+`, MSVC x64)
* **Desktop Windowing:** `tao 0.37` + `wry 0.57` (WebView2 in-process)
* **Subsystem:** Native Windows GUI mode (`#![windows_subsystem = "windows"]`) — no black console window
* **Tracking Engine:** Low-overhead Win32 API (`GetForegroundWindow`, `GetWindowThreadProcessId`, `QueryFullProcessImageNameW`, `GetLastInputInfo`)
* **Live Database:** Embedded SQLite (`rusqlite 0.32` with WAL mode)
* **System Tray & Toast:** Win32 Shell Notify Icon (`Shell_NotifyIconW`), action center toasts, and right-click context menu
* **Windows Autostart:** Direct registry persistence in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Tolo`

---

## 📁 Project Structure

```text
Tolo/
├── Cargo.toml            # Package configuration (name = "tolo")
├── Cargo.lock            # Dependency lockfile
├── build.rs              # Windows resource compiler configuration
├── tolo.ico              # Multi-size Windows application icon
├── README.md             # Architecture & documentation
├── ui/
│   ├── index.html        # Embedded desktop UI with Tolo branding & logo
│   ├── logo_base64.txt   # Inlined logo Data URI
│   └── icons/            # Embedded fallback icon base64 payloads
└── src/
    ├── main.rs           # Single-instance mutex enforcement & entrypoint
    ├── gui.rs            # Tao native desktop window + Wry WebView2 + background API server
    ├── tray.rs           # Win32 system tray message loop & native notifications
    ├── autostart.rs      # Windows Registry startup toggle (HKCU Run key)
    ├── tracker.rs        # Low-power background active window & idle monitoring thread
    ├── db.rs             # SQLite database layer with all schema tables & analytics queries
    ├── models.rs         # Data models & entity structs
    ├── icon_extractor.rs # Win32 icon extraction (extracts real .exe icons)
    └── windows_api.rs    # Safe Win32 API wrappers
```

---

## 🚀 Building & Running Tolo

### Prerequisites
* Windows 10 / 11 (64-bit)
* [Rust](https://rustup.rs/) (msvc toolchain)
* WebView2 Runtime (pre-installed on Windows 10/11)

### Build from source
```powershell
cargo build --release
```
The compiled standalone binary will be located at `target\release\tolo.exe`.

### Run
```powershell
cargo run --release
```

---

## ✨ Features Unlocked

* **100% Native Desktop Window:** Tao native Win32 window with smooth WebView2 rendering.
* **System Tray Integration:** Minimizes to the notification area near the taskbar clock when you close the window (`X`). Double-click tray icon to restore.
* **Full Privacy & Zero Telemetry:** All database records stay 100% on your local machine in `screentime.sqlite3`.
* **Pro Features Unlocked:** Full limits, downtime schedules, hourly distribution graphs, real extracted executable icons, and historical analytics without any paywalls or license restrictions.
