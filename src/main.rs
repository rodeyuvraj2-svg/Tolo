#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod db;
mod gui;
mod icon_extractor;
mod models;
mod tracker;
mod tray;
mod windows_api;

use std::sync::Arc;
use db::Database;
use gui::GuiService;
use tracker::TrackerService;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write("panic_log.txt", format!("PANIC: {:?}", info));
    }));
    // 0. Auto-register Start Menu shortcut for new users on first launch
    autostart::ensure_shortcuts();

    // Set modern Windows Application User Model ID for taskbar grouping and branding
    unsafe {
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let _ = SetCurrentProcessExplicitAppUserModelID(windows::core::w!("YuvrajRode.Tolo.App.1.0"));
    }

    // 1. Single Instance Mutex enforcement
    let mutex_name = windows::core::w!("Local\\Tolo_Rust_App_SingleInstance_Mutex");
    let _h_mutex = unsafe {
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::Win32::Foundation::GetLastError;
        let h = CreateMutexW(None, true, mutex_name);
        if GetLastError() == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
            use windows::Win32::UI::WindowsAndMessaging::{
                AllowSetForegroundWindow, FindWindowW, PostMessageW, SetForegroundWindow, ShowWindow,
                ASFW_ANY, SW_RESTORE, SW_SHOW, WM_COMMAND,
            };
            use windows::Win32::Foundation::{LPARAM, WPARAM};

            let _ = AllowSetForegroundWindow(ASFW_ANY);

            // 1. Try sending ID_TRAY_OPEN (1001) to ToloTray window
            let tray_hwnd = FindWindowW(windows::core::w!("ToloTrayWindowClass"), windows::core::w!("ToloTray")).unwrap_or_default();
            if !tray_hwnd.0.is_null() {
                let _ = PostMessageW(Some(tray_hwnd), WM_COMMAND, WPARAM(1001), LPARAM(0));
                return Ok(());
            }

            // 2. Try bringing existing window to front directly
            let mut hwnd = FindWindowW(None, windows::core::w!("Tolo")).unwrap_or_default();
            if hwnd.0.is_null() {
                hwnd = FindWindowW(None, windows::core::w!("Tolo — Screen Time & Digital Wellbeing")).unwrap_or_default();
            }
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
                return Ok(());
            }

            // 3. Mutex exists but NO window or tray responded:
            // A headless zombie process is holding the mutex. Terminate it so the app can start!
            let current_pid = std::process::id();
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/FI", &format!("PID ne {}", current_pid), "/IM", "tolo.exe"])
                    .creation_flags(0x08000000)
                    .output();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
        h
    };

    // 2. Resolve database path with canonical %APPDATA% storage & bidirectional sync
    let db_path = {
        let app_data_dir = std::env::var("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("ScreenTime"))
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&app_data_dir);
        let app_data_db = app_data_dir.join("screentime.sqlite3");

        let local_db = std::path::PathBuf::from("screentime.sqlite3");
        let exe_dir_db = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("screentime.sqlite3")));

        // If local workspace has populated database and appdata is missing/smaller, sync to appdata
        if local_db.exists() {
            if let Ok(local_meta) = local_db.metadata() {
                let app_size = app_data_db.metadata().map(|m| m.len()).unwrap_or(0);
                if local_meta.len() > app_size {
                    let _ = std::fs::copy(&local_db, &app_data_db);
                }
            }
        } else if let Some(ref edb) = exe_dir_db {
            if edb.exists() {
                if let Ok(exe_meta) = edb.metadata() {
                    let app_size = app_data_db.metadata().map(|m| m.len()).unwrap_or(0);
                    if exe_meta.len() > app_size {
                        let _ = std::fs::copy(edb, &app_data_db);
                    }
                }
            }
        }

        // Keep local workspace synced if it exists
        if app_data_db.exists() && local_db.exists() {
            let _ = std::fs::copy(&app_data_db, &local_db);
        }

        // ALWAYS use canonical app_data_db
        app_data_db.to_string_lossy().to_string()
    };

    let db = Arc::new(Database::new(&db_path)?);

    // 3. Create & start background tracker
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    tracker.start();

    // 4. Start GUI & System Tray
    let gui = GuiService::new(Arc::clone(&db), Arc::clone(&tracker), 3456);
    gui.run_desktop_app();

    tracker.stop();
    Ok(())
}
