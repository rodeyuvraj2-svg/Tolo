use std::sync::Arc;
use std::thread;
use chrono::{Local, NaiveDate};
use serde::Deserialize;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::windows::WindowExtWindows,
    window::WindowBuilder,
};
use tiny_http::{Header, Response, Server, StatusCode};
use wry::WebViewBuilder;

use crate::autostart;
use crate::db::{Database, HistoryDeleteCounts};
use crate::tracker::TrackerService;
use crate::tray::{self, CustomUserEvent, TrayService};

const EMBEDDED_HTML: &str = include_str!("../ui/index.html");

fn url_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    result.push(val);
                    i += 3;
                    continue;
                }
            }
        } else if bytes[i] == b'+' {
            result.push(b' ');
            i += 1;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

#[derive(Deserialize)]
struct AutostartRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct NotifyMinimizeRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct ThemeRequest {
    theme: String,
}

fn normalize_theme(theme: Option<&str>) -> &'static str {
    match theme {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "light",
    }
}

#[derive(Deserialize)]
struct DeleteHistoryRequest {
    start_date: Option<String>,
    end_date: Option<String>,
}

fn validate_history_range(
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Option<(String, String)>, &'static str> {
    match (start_date, end_date) {
        (None, None) => Ok(None),
        (Some(start), Some(end)) => {
            let parsed_start = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                .map_err(|_| "Start date must use YYYY-MM-DD.")?;
            let parsed_end = NaiveDate::parse_from_str(&end, "%Y-%m-%d")
                .map_err(|_| "End date must use YYYY-MM-DD.")?;
            if parsed_start.format("%Y-%m-%d").to_string() != start
                || parsed_end.format("%Y-%m-%d").to_string() != end
            {
                return Err("Dates must use YYYY-MM-DD.");
            }
            if parsed_start > parsed_end {
                return Err("Start date must be on or before the end date.");
            }
            Ok(Some((start, end)))
        }
        _ => Err("Provide both a start date and an end date, or neither to delete all history."),
    }
}

fn delete_history_from_request(
    db: &Database,
    body: &str,
) -> Result<HistoryDeleteCounts, (StatusCode, String)> {
    let req_data = serde_json::from_str::<DeleteHistoryRequest>(body)
        .map_err(|e| (StatusCode(400), format!("Invalid request: {}", e)))?;
    let range = validate_history_range(req_data.start_date, req_data.end_date)
        .map_err(|e| (StatusCode(400), e.to_string()))?;
    let result = match range {
        Some((start, end)) => db.delete_history(Some(&start), Some(&end)),
        None => db.delete_history(None, None),
    };
    result.map_err(|e| (StatusCode(500), e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{normalize_theme, validate_history_range};

    #[test]
    fn only_light_and_dark_themes_are_supported() {
        assert_eq!(normalize_theme(Some("light")), "light");
        assert_eq!(normalize_theme(Some("dark")), "dark");
        assert_eq!(normalize_theme(Some("system")), "light");
        assert_eq!(normalize_theme(None), "light");
    }

    #[test]
    fn validates_history_delete_ranges() {
        assert_eq!(
            validate_history_range(
                Some("2026-03-01".to_string()),
                Some("2026-03-31".to_string())
            )
            .unwrap(),
            Some(("2026-03-01".to_string(), "2026-03-31".to_string()))
        );
        assert!(validate_history_range(Some("2026-03-31".to_string()), Some("2026-03-01".to_string())).is_err());
        assert!(validate_history_range(Some("2026-03-01".to_string()), None).is_err());
        assert!(validate_history_range(Some("not-a-date".to_string()), Some("2026-03-01".to_string())).is_err());
    }
}

pub struct GuiService {
    db: Arc<Database>,
    tracker: Arc<TrackerService>,
    port: u16,
}

impl GuiService {
    pub fn new(db: Arc<Database>, tracker: Arc<TrackerService>, port: u16) -> Self {
        Self { db, tracker, port }
    }

    /// Starts the background HTTP server for live dashboard APIs and runs the native desktop window event loop.
    pub fn run_desktop_app(&self) {
        let mut server = None;
        let mut active_port = self.port;
        for p in self.port..self.port + 10 {
            let a = format!("127.0.0.1:{}", p);
            if let Ok(s) = Server::http(&a) {
                server = Some(s);
                active_port = p;
                break;
            }
        }
        let server = match server {
            Some(s) => s,
            None => return,
        };
        let addr = format!("127.0.0.1:{}", active_port);

        let db = Arc::clone(&self.db);
        let tracker = Arc::clone(&self.tracker);
        let tracker_api = Arc::clone(&self.tracker);

        // Run the HTTP API server in a background thread
        thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let url_str = request.url().to_string();
                let method = request.method().as_str();

                if url_str == "/" || url_str.starts_with("/?") {
                    let mut response = Response::from_string(EMBEDDED_HTML);
                    response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
                    let _ = request.respond(response);
                } else if url_str.starts_with("/api/summary") {
                    let today = Local::now().format("%Y-%m-%d").to_string();
                    let date = if let Some(idx) = url_str.find("date=") {
                        let query = &url_str[idx + 5..];
                        query.split('&').next().unwrap_or(&today).to_string()
                    } else {
                        today
                    };

                    let is_paused = db.get_setting("tracking_paused").ok().flatten().map(|v| v == "true").unwrap_or(false);
                    match db.get_dashboard_data(&date, is_paused) {
                        Ok(data) => {
                            let json = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
                            let mut response = Response::from_string(json);
                            response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                            let _ = request.respond(response);
                        }
                        Err(e) => {
                            let err_json = format!(r#"{{"error": "{}"}}"#, e);
                            let response = Response::from_string(err_json).with_status_code(StatusCode(500));
                            let _ = request.respond(response);
                        }
                    }
                } else if url_str.starts_with("/api/weekly") {
                    let today = Local::now().format("%Y-%m-%d").to_string();
                    let date = if let Some(idx) = url_str.find("date=") {
                        let query = &url_str[idx + 5..];
                        query.split('&').next().unwrap_or(&today).to_string()
                    } else {
                        today
                    };

                    match db.get_weekly_data(&date) {
                        Ok(data) => {
                            let json = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
                            let mut response = Response::from_string(json);
                            response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                            let _ = request.respond(response);
                        }
                        Err(e) => {
                            let err_json = format!(r#"{{"error": "{}"}}"#, e);
                            let response = Response::from_string(err_json).with_status_code(StatusCode(500));
                            let _ = request.respond(response);
                        }
                    }
                } else if url_str.starts_with("/api/app-details") {
                    let app_raw = if let Some(idx) = url_str.find("app=") {
                        let q = &url_str[idx + 4..];
                        q.split('&').next().unwrap_or("")
                    } else if let Some(idx) = url_str.find("name=") {
                        let q = &url_str[idx + 5..];
                        q.split('&').next().unwrap_or("")
                    } else {
                        ""
                    };
                    let app = url_decode(app_raw);

                    let days: i64 = if let Some(idx) = url_str.find("days=") {
                        let q = &url_str[idx + 5..];
                        q.split('&').next().and_then(|v| v.parse().ok()).unwrap_or(30)
                    } else {
                        30
                    };

                    match db.get_app_details(&app, days) {
                        Ok(data) => {
                            let json = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
                            let mut response = Response::from_string(json);
                            response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                            let _ = request.respond(response);
                        }
                        Err(e) => {
                            let err_json = format!(r#"{{"error": "{}"}}"#, e);
                            let response = Response::from_string(err_json).with_status_code(StatusCode(500));
                            let _ = request.respond(response);
                        }
                    }
                } else if method == "POST" && url_str == "/api/toggle-pause" {
                    let new_state = tracker_api.toggle_pause();
                    let json = format!(r#"{{"is_paused": {}}}"#, new_state);
                    let mut response = Response::from_string(json);
                    response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    let _ = request.respond(response);
                } else if method == "GET" && url_str == "/api/settings/autostart" {
                    let enabled = autostart::is_autostart_enabled();
                    let json = format!(r#"{{"autostart": {}}}"#, enabled);
                    let mut response = Response::from_string(json);
                    response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    let _ = request.respond(response);
                } else if method == "POST" && url_str == "/api/settings/autostart" {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    if let Ok(req_data) = serde_json::from_str::<AutostartRequest>(&body) {
                        let res = autostart::set_autostart(req_data.enabled);
                        let json = match res {
                            Ok(_) => format!(r#"{{"success": true, "autostart": {}}}"#, req_data.enabled),
                            Err(e) => format!(r#"{{"error": "{}"}}"#, e),
                        };
                        let mut response = Response::from_string(json);
                        response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    } else {
                        let response = Response::from_string("Bad Request").with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                    }
                } else if method == "POST" && url_str == "/api/history/delete" {
                    let mut body = String::new();
                    let result = match request.as_reader().read_to_string(&mut body) {
                        Ok(_) => delete_history_from_request(db.as_ref(), &body),
                        Err(e) => Err((StatusCode(400), format!("Could not read request: {}", e))),
                    };
                    let (status, payload) = match result {
                        Ok(counts) => {
                            tracker_api.reset_active_session();
                            (
                                StatusCode(200),
                                serde_json::json!({
                                    "success": true,
                                    "daily_records": counts.daily_records,
                                    "hourly_records": counts.hourly_records,
                                    "sessions": counts.sessions
                                }),
                            )
                        }
                        Err((status, error)) => (status, serde_json::json!({"error": error})),
                    };
                    let mut response = Response::from_string(payload.to_string()).with_status_code(status);
                    response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    let _ = request.respond(response);
                } else if method == "GET" && url_str == "/api/settings/notify-minimize" {
                    let enabled = db.get_setting("notify_on_minimize").ok().flatten().map(|v| v == "true").unwrap_or(false);
                    let json = format!(r#"{{"notify_minimize": {}}}"#, enabled);
                    let mut response = Response::from_string(json);
                    response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    let _ = request.respond(response);
                } else if method == "POST" && url_str == "/api/settings/notify-minimize" {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    if let Ok(req_data) = serde_json::from_str::<NotifyMinimizeRequest>(&body) {
                        let val = if req_data.enabled { "true" } else { "false" };
                        let res = db.set_setting("notify_on_minimize", val);
                        let json = match res {
                            Ok(_) => format!(r#"{{"success": true, "notify_minimize": {}}}"#, req_data.enabled),
                            Err(e) => format!(r#"{{"error": "{}"}}"#, e),
                        };
                        let mut response = Response::from_string(json);
                        response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    } else {
                        let response = Response::from_string("Bad Request").with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                    }
                } else if method == "GET" && url_str == "/api/settings/theme" {
                    let theme_result = db.get_setting("theme").and_then(|stored_theme| {
                        let theme = normalize_theme(stored_theme.as_deref());
                        if stored_theme.as_deref() != Some(theme) {
                            db.set_setting("theme", theme)?;
                        }
                        Ok(theme)
                    });
                    let (status, json) = match theme_result {
                        Ok(theme) => (StatusCode(200), serde_json::json!({"theme": theme})),
                        Err(error) => (
                            StatusCode(500),
                            serde_json::json!({"error": format!("Could not load theme: {error}")}),
                        ),
                    };
                    let mut response =
                        Response::from_string(json.to_string()).with_status_code(status);
                    response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    let _ = request.respond(response);
                } else if method == "POST" && url_str == "/api/settings/theme" {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    if let Ok(req_data) = serde_json::from_str::<ThemeRequest>(&body) {
                        let (status, json) = if matches!(req_data.theme.as_str(), "light" | "dark") {
                            match db.set_setting("theme", &req_data.theme) {
                                Ok(_) => (
                                    StatusCode(200),
                                    serde_json::json!({
                                        "success": true,
                                        "theme": req_data.theme
                                    }),
                                ),
                                Err(error) => (
                                    StatusCode(500),
                                    serde_json::json!({"error": error.to_string()}),
                                ),
                            }
                        } else {
                            (
                                StatusCode(400),
                                serde_json::json!({"error": "Theme must be light or dark."}),
                            )
                        };
                        let mut response =
                            Response::from_string(json.to_string()).with_status_code(status);
                        response.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    } else {
                        let response = Response::from_string("Bad Request").with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                    }
                } else {
                    let response = Response::from_string("Not Found").with_status_code(StatusCode(404));
                    let _ = request.respond(response);
                }
            }
        });

        // Native Tao Window Event Loop with Custom User Event Support
        let event_loop = EventLoopBuilder::<CustomUserEvent>::with_user_event().build();
        let proxy = event_loop.create_proxy();
        tray::set_event_loop_proxy(proxy);

        let window = WindowBuilder::new()
            .with_title("Tolo")
            .with_inner_size(LogicalSize::new(1260.0, 880.0))
            .with_resizable(true)
            .build(&event_loop)
            .expect("Failed to create native desktop window");

        // System Tray Integration
        let main_hwnd = window.hwnd();

        // Explicitly set window title bar and taskbar icons via modern Win32 LoadImageW
        unsafe {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
            use windows::Win32::UI::WindowsAndMessaging::{
                LoadImageW, SendMessageW, IMAGE_ICON, ICON_BIG, ICON_SMALL, LR_SHARED, WM_SETICON,
            };
            let h_module = GetModuleHandleW(None).unwrap_or_default();
            let h_icon_big = LoadImageW(Some(h_module.into()), windows::core::PCWSTR(1 as _), IMAGE_ICON, 48, 48, LR_SHARED).unwrap_or_default();
            let h_icon_small = LoadImageW(Some(h_module.into()), windows::core::PCWSTR(1 as _), IMAGE_ICON, 16, 16, LR_SHARED).unwrap_or_default();
            if !h_icon_big.0.is_null() {
                let hwnd = HWND(main_hwnd as _);
                SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(h_icon_big.0 as isize)));
                if !h_icon_small.0.is_null() {
                    SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(h_icon_small.0 as isize)));
                }
            }
        }

        let tray = Arc::new(TrayService::new(tracker.paused_handle(), main_hwnd));
        tray.start();
        tracker.set_tray(Arc::clone(&tray));

        let app_url = format!("http://{}", addr);

        let data_dir = std::env::var("LOCALAPPDATA")
            .map(|l| std::path::PathBuf::from(l).join("Tolo").join("WebView2"))
            .unwrap_or_else(|_| std::path::PathBuf::from("WebView2"));
        let _ = std::fs::create_dir_all(&data_dir);

        let mut web_context = wry::WebContext::new(Some(data_dir));
        let mut webview = Some(
            WebViewBuilder::new_with_web_context(&mut web_context)
                .with_url(&app_url)
                .build(&window)
                .expect("Failed to initialize WebView2 inside desktop window"),
        );

        println!("⚡ Native Tolo Desktop Application Launched with System Tray.");

        let tray_events = Arc::clone(&tray);
        let db_for_events = Arc::clone(&self.db);
        event_loop.run(move |event, _, control_flow| {
            let _ = (&webview, &window, &web_context);
            *control_flow = ControlFlow::Wait;

            match event {
                Event::UserEvent(CustomUserEvent::RestoreWindow) => {
                    if webview.is_none() {
                        match WebViewBuilder::new_with_web_context(&mut web_context)
                            .with_url(&app_url)
                            .build(&window)
                        {
                            Ok(restored_webview) => webview = Some(restored_webview),
                            Err(error) => {
                                let message = format!("Could not restore the Tolo window: {error}");
                                eprintln!("{message}");
                                tray_events.send_toast("Could not open Tolo", &message, true);
                                return;
                            }
                        }
                    }
                    window.set_visible(true);
                    window.set_focus();
                    unsafe {
                        use windows::Win32::Foundation::HWND;
                        use windows::Win32::UI::WindowsAndMessaging::{
                            SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
                        };
                        let hwnd = HWND(main_hwnd as _);
                        let _ = ShowWindow(hwnd, SW_SHOW);
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                        let _ = SetForegroundWindow(hwnd);
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    // Minimize to System Tray instead of quitting
                    window.set_visible(false);
                    webview.take();
                    unsafe {
                        use windows::Win32::Foundation::HWND;
                        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
                        let _ = ShowWindow(HWND(main_hwnd as _), SW_HIDE);

                        use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
                        use windows::Win32::System::Threading::GetCurrentProcess;
                        let _ = K32EmptyWorkingSet(GetCurrentProcess());
                    }

                    // Check notification settings: avoid annoying spam on every close!
                    let notify_enabled = db_for_events
                        .get_setting("notify_on_minimize")
                        .ok()
                        .flatten()
                        .map(|v| v == "true")
                        .unwrap_or(false);
                    let already_notified = db_for_events
                        .get_setting("tray_minimize_first_notice")
                        .ok()
                        .flatten()
                        .is_some();

                    if notify_enabled || !already_notified {
                        tray_events.send_toast(
                            "Tolo is running in the background",
                            "Tolo is active in your notification area and continuing to track your screen time.",
                            false,
                        );
                        let _ = db_for_events.set_setting("tray_minimize_first_notice", "true");
                    }
                }
                _ => (),
            }
        });
    }
}
