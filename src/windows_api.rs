use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::ProcessStatus::GetProcessImageFileNameW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

use crate::models::WindowInfo;

/// Retrieves the current foreground window's details (PID, executable path, process name, title).
pub fn get_foreground_window_info() -> Option<WindowInfo> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        // Get window title
        let text_len = GetWindowTextLengthW(hwnd);
        let mut title = String::new();
        if text_len > 0 {
            let mut buffer: Vec<u16> = vec![0; (text_len + 1) as usize];
            let read_len = GetWindowTextW(hwnd, &mut buffer);
            if read_len > 0 {
                title = String::from_utf16_lossy(&buffer[..read_len as usize]);
            }
        }

        // Get process executable path
        let h_proc_result = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        let exec_path = match h_proc_result {
            Ok(h_proc) => {
                let path = get_process_path(h_proc);
                let _ = CloseHandle(h_proc);
                path
            }
            Err(_) => String::new(),
        };

        if exec_path.is_empty() {
            return None;
        }

        let path_obj = Path::new(&exec_path);
        let app_name = path_obj
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let display_name = format_display_name(&app_name);

        Some(WindowInfo {
            process_id: pid,
            executable_path: exec_path,
            app_name,
            display_name,
            window_title: title,
        })
    }
}

/// Infers the application category based on process name and executable path.
pub fn infer_category(raw_name: &str, exec_path: &str) -> &'static str {
    let lower_name = raw_name.to_lowercase();
    let lower_path = exec_path.to_lowercase();

    if lower_name.contains("chrome")
        || lower_name.contains("brave")
        || lower_name.contains("edge")
        || lower_name.contains("firefox")
        || lower_name.contains("opera")
        || lower_name.contains("browser")
        || lower_name.contains("vivaldi")
        || lower_name.contains("arc")
    {
        return "Browser";
    }

    if lower_name.contains("antigravity")
        || lower_name.contains("code")
        || lower_name.contains("terminal")
        || lower_name.contains("powershell")
        || lower_name.contains("pwsh")
        || lower_name.contains("cmd")
        || lower_name.contains("devenv")
        || lower_name.contains("cursor")
        || lower_name.contains("visual studio")
        || lower_path.contains("microsoft.windowsterminal")
        || lower_name.contains("idea")
        || lower_name.contains("rustrover")
    {
        return "Development";
    }

    if lower_name.contains("zapfast")
        || lower_name.contains("whatsapp")
        || lower_name.contains("telegram")
        || lower_name.contains("discord")
        || lower_name.contains("slack")
        || lower_name.contains("teams")
        || lower_name.contains("signal")
        || lower_name.contains("skype")
        || lower_name.contains("zoom")
    {
        return "Communication";
    }

    if lower_name.contains("explorer")
        || lower_name.contains("snippingtool")
        || lower_name.contains("screensketch")
        || lower_name.contains("tolo")
        || lower_name.contains("screentime")
        || lower_name.contains("screen-time")
        || lower_name.contains("taskmgr")
        || lower_name.contains("settings")
        || lower_name.contains("calculator")
        || lower_name.contains("notepad")
        || lower_name.contains("shellexperience")
        || lower_name.contains("applicationframehost")
        || lower_name.contains("setup")
    {
        return "Utility";
    }

    "Uncategorized"
}

/// Helper function to extract the full process image name from a process handle.
unsafe fn get_process_path(h_proc: HANDLE) -> String {
    let mut buffer: Vec<u16> = vec![0; 1024];
    let mut size: u32 = buffer.len() as u32;

    if QueryFullProcessImageNameW(h_proc, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buffer.as_mut_ptr()), &mut size).is_ok() {
        return OsString::from_wide(&buffer[..size as usize])
            .to_string_lossy()
            .to_string();
    }

    // Fallback: GetProcessImageFileNameW
    let len = GetProcessImageFileNameW(h_proc, &mut buffer);
    if len > 0 {
        return OsString::from_wide(&buffer[..len as usize])
            .to_string_lossy()
            .to_string();
    }

    String::new()
}

/// Returns the user's idle time in seconds using Windows `GetLastInputInfo`.
pub fn get_idle_time_seconds() -> u64 {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };

        if GetLastInputInfo(&mut lii).as_bool() {
            let current_tick = GetTickCount64();
            let last_input = lii.dwTime as u64;
            if current_tick >= last_input {
                return (current_tick - last_input) / 1000;
            }
        }
    }
    0
}

/// Converts binary name like "chrome" or "screen-time-tracker" to friendly name.
fn format_display_name(raw_name: &str) -> String {
    match raw_name.to_lowercase().as_str() {
        "chrome" => "Google Chrome".to_string(),
        "brave" => "Brave Browser".to_string(),
        "msedge" => "Microsoft Edge".to_string(),
        "firefox" => "Mozilla Firefox".to_string(),
        "code" => "Visual Studio Code".to_string(),
        "devenv" => "Visual Studio".to_string(),
        "explorer" => "Windows Explorer".to_string(),
        "discord" => "Discord".to_string(),
        "spotify" => "Spotify".to_string(),
        "slack" => "Slack".to_string(),
        "steam" => "Steam".to_string(),
        "notepad" => "Notepad".to_string(),
        "zapfast" => "ZapFast".to_string(),
        "windowsterminal" => "Windows Terminal".to_string(),
        "tolo" | "screentime" | "screen-time-tracker" => "Tolo".to_string(),
        _ => {
            // Capitalize words
            let mut words = Vec::new();
            for part in raw_name.replace(['-', '_'], " ").split_whitespace() {
                let mut chars = part.chars();
                if let Some(first) = chars.next() {
                    let title_cased: String = first.to_uppercase().collect::<String>() + chars.as_str();
                    words.push(title_cased);
                }
            }
            if words.is_empty() {
                raw_name.to_string()
            } else {
                words.join(" ")
            }
        }
    }
}
