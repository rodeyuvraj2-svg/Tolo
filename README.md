# ⏳ Tolo — Windows Screen Time & Digital Wellbeing

[![GitHub Release](https://img.shields.io/github/v/release/rodeyuvraj2-svg/Tolo?color=2563eb&label=Latest%20Version&style=flat-square)](https://github.com/rodeyuvraj2-svg/Tolo/releases/latest)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%7C%2011%20(64--bit)-0078d4?style=flat-square&logo=windows)](https://github.com/rodeyuvraj2-svg/Tolo/releases/latest)
[![Built with Rust](https://img.shields.io/badge/Built%20With-Rust-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=flat-square)](LICENSE)
[![Privacy](https://img.shields.io/badge/Privacy-100%25%20Local%20%26%20Private-success?style=flat-square)](https://github.com/rodeyuvraj2-svg/Tolo)

> A modern, ultra-lightweight, 100% private screen time and digital wellbeing desktop application for Windows. Written in native **Rust** with low CPU usage, background tracking, and zero telemetry or subscriptions.

---

## ⬇️ Quick Download & Install (For Users)

Getting started with Tolo takes less than 30 seconds. No complex installers or dependencies needed!

### 1. Download Tolo
Click the button below to download the latest standalone binary:

👉 **[Download tolo.exe (Latest Release)](https://github.com/rodeyuvraj2-svg/Tolo/releases/latest/download/tolo.exe)**  
*(Alternative: [Download as ZIP](https://github.com/rodeyuvraj2-svg/Tolo/releases/latest/download/tolo-windows-x64.zip) or browse [All Releases](https://github.com/rodeyuvraj2-svg/Tolo/releases))*

### 2. Run the Application
1. Move `tolo.exe` to any folder of your choice (e.g. `Desktop`, `Documents`, or `Program Files`).
2. Double-click `tolo.exe` to launch.

> [!NOTE]
> **Windows SmartScreen Note:**  
> Because Tolo is an open-source tool, Windows Defender SmartScreen might say *"Windows protected your PC"*.  
> Simply click **More info** → **Run anyway**.

### 3. Optional: Add a Desktop / Taskbar Shortcut
* **Pin to Taskbar:** While Tolo is open, right-click the Tolo icon on your taskbar and click **Pin to taskbar**.
* **Run on Startup:** Inside Tolo, go to **Settings** and toggle **Start on Windows Login** to automatically track your daily productivity whenever you boot your PC.

---

## 💡 How It Works

* **Silent Background Tracking:** When you click the **`X`** button, Tolo closes from your screen and taskbar, but **stays running silently in the background** (in your Windows System Tray next to the clock). It tracks your active screen time, idle time, and app pickups with almost 0% CPU impact.
* **Restore Window:** Whenever you want to view your analytics, click the **Tolo icon in the System Tray** (bottom-right near the clock) or click your desktop/taskbar shortcut.
* **Completely Quit:** Right-click the **Tolo tray icon** (near the clock) and click **❌ Exit Tolo**.

---

## ✨ Features

* **🔒 100% Private & Offline:** All your data is stored locally on your PC in SQLite. No tracking servers, no cloud sync, no analytics, no subscriptions.
* **⚡ Ultra Fast & Lightweight:** Built in native Rust using native Win32 APIs and WebView2. Consumes minimal RAM and less than 0.1% CPU.
* **📊 Live App Usage & Pickups:** Displays real Windows application icons, tracking exact active duration, idle time, total app openings, and first/most opened apps.
* **⏱️ Daily Limits & Downtime:** Configure custom app limits, daily screen time goals, and scheduled downtime with gentle desktop toast reminders.
* **📈 Rich Analytics:** View comprehensive daily, 7-day weekly, and 30-day productivity trends and hourly bar charts.

---

## 🛠️ Building From Source (For Developers)

If you prefer to compile Tolo from source code yourself:

### Prerequisites
* Windows 10 / 11 (64-bit)
* [Rust & Cargo](https://rustup.rs/) (msvc toolchain)
* WebView2 Runtime (pre-installed on Windows 10 and 11)

### Build Steps
```powershell
# 1. Clone the repository
git clone https://github.com/rodeyuvraj2-svg/Tolo.git
cd Tolo

# 2. Compile an optimized release build
cargo build --release

# 3. Run the binary
.\target\release\tolo.exe
```

---

## 📁 Project Structure

```text
Tolo/
├── Cargo.toml            # Package configuration
├── build.rs              # Windows resource compiler configuration
├── tolo.ico              # Multi-size Windows application icon
├── README.md             # Architecture & download documentation
├── .github/workflows/    # Automated CI/CD release workflow
├── ui/
│   ├── index.html        # Embedded desktop UI with Tolo branding & logo
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

## 📄 License
This project is licensed under the [MIT License](LICENSE).
