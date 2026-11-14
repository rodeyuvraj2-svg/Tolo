use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, LoadIconW, LoadImageW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, ShowWindow, TrackPopupMenu,
    TranslateMessage, HICON, HMENU, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
    MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, SW_RESTORE, SW_SHOW,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK,
    WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAYICON: u32 = WM_APP + 101;
const ID_TRAY_OPEN: usize = 1001;
const ID_TRAY_PAUSE: usize = 1002;
const ID_TRAY_QUIT: usize = 1003;

static TRAY_WINDOW_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static MAIN_WINDOW_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static TRAY_PAUSED_PTR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct TrayService {
    is_paused: Arc<AtomicBool>,
}

impl TrayService {
    pub fn new(is_paused: Arc<AtomicBool>, main_hwnd: isize) -> Self {
        MAIN_WINDOW_HWND.store(main_hwnd, Ordering::SeqCst);
        Self { is_paused }
    }

    /// Spawns the system tray icon message loop on a dedicated background thread.
    pub fn start(&self) {
        let _is_paused = Arc::clone(&self.is_paused);

        thread::spawn(move || unsafe {
            let class_name = w!("ToloTrayWindowClass");

            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(tray_wnd_proc),
                hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
                lpszClassName: class_name,
                ..Default::default()
            };

            let _ = RegisterClassW(&wnd_class);

            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("ToloTray"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None, // Message-only window
                None,
                None,
                None,
            ) {
                Ok(h) => h,
                Err(_) => return,
            };

            TRAY_WINDOW_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

            // Load Tolo application icon from embedded resource
            let h_module = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            let cx_sm = GetSystemMetrics(SM_CXSMICON);
            let cy_sm = GetSystemMetrics(SM_CYSMICON);
            let mut h_icon: HICON = LoadImageW(
                Some(h_module.into()),
                PCWSTR(1 as _),
                IMAGE_ICON,
                cx_sm,
                cy_sm,
                LR_SHARED,
            ).map(|h| HICON(h.0)).unwrap_or_default();

            if h_icon.0.is_null() {
                h_icon = LoadImageW(
                    Some(h_module.into()),
                    PCWSTR(1 as _),
                    IMAGE_ICON,
                    0,
                    0,
                    LR_DEFAULTSIZE | LR_SHARED,
                ).map(|h| HICON(h.0)).unwrap_or_default();
            }

            if h_icon.0.is_null() {
                h_icon = LoadIconW(Some(h_module.into()), PCWSTR(1 as _)).unwrap_or_default();
            }

            if h_icon.0.is_null() {
                h_icon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
            }

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NOTIFY_ICON_DATA_FLAGS(1 | 2 | 4), // NIF_MESSAGE | NIF_ICON | NIF_TIP
                uCallbackMessage: WM_TRAYICON,
                hIcon: h_icon,
                ..Default::default()
            };

            let tip = "Tolo — Screen Time & Digital Wellbeing\0";
            let tip_wide: Vec<u16> = tip.encode_utf16().collect();
            let copy_len = tip_wide.len().min(nid.szTip.len());
            nid.szTip[..copy_len].copy_from_slice(&tip_wide[..copy_len]);

            let _ = Shell_NotifyIconW(NIM_ADD, &nid);

            // Message loop
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // Cleanup tray icon on exit
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = DestroyWindow(hwnd);
        });
    }

    /// Sends a native Windows Action Center Toast Notification via the system tray icon.
    #[allow(dead_code)]
    pub fn send_toast(&self, title: &str, message: &str, is_warning: bool) {
        let tray_hwnd = TRAY_WINDOW_HWND.load(Ordering::SeqCst);
        if tray_hwnd == 0 {
            return;
        }

        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: HWND(tray_hwnd as *mut _),
                uID: 1,
                uFlags: NOTIFY_ICON_DATA_FLAGS(16), // NIF_INFO
                dwInfoFlags: if is_warning { NIIF_WARNING } else { NIIF_INFO },
                ..Default::default()
            };

            let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(std::iter::once(0)).collect();
            let copy_title_len = title_wide.len().min(nid.szInfoTitle.len());
            nid.szInfoTitle[..copy_title_len].copy_from_slice(&title_wide[..copy_title_len]);

            let msg_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(std::iter::once(0)).collect();
            let copy_msg_len = msg_wide.len().min(nid.szInfo.len());
            nid.szInfo[..copy_msg_len].copy_from_slice(&msg_wide[..copy_msg_len]);

            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }
}

pub fn restore_main_window() {
    let main_hwnd = MAIN_WINDOW_HWND.load(Ordering::SeqCst);
    if main_hwnd != 0 {
        unsafe {
            let hwnd = HWND(main_hwnd as *mut _);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = lparam.0 as u32;
            match event {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    restore_main_window();
                }
                WM_RBUTTONUP => {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);

                    let hmenu: HMENU = match CreatePopupMenu() {
                        Ok(m) => m,
                        Err(_) => return LRESULT(0),
                    };

                    let is_paused = TRAY_PAUSED_PTR.load(Ordering::SeqCst);
                    let pause_label = if is_paused { "▶️  Resume Tracking" } else { "⏸️  Pause Tracking" };

                    let open_text: Vec<u16> = OsStr::new("🖥️  Open Tolo")
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();
                    let pause_text: Vec<u16> = OsStr::new(pause_label)
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();
                    let quit_text: Vec<u16> = OsStr::new("❌  Exit Tolo")
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();

                    let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN, PCWSTR(open_text.as_ptr()));
                    let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_PAUSE, PCWSTR(pause_text.as_ptr()));
                    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                    let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_QUIT, PCWSTR(quit_text.as_ptr()));

                    let _ = SetForegroundWindow(hwnd);
                    let _ = TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), hwnd, None);
                    let _ = DestroyMenu(hmenu);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 as usize;
            match id {
                ID_TRAY_OPEN => {
                    restore_main_window();
                }
                ID_TRAY_PAUSE => {
                    let cur = TRAY_PAUSED_PTR.load(Ordering::SeqCst);
                    TRAY_PAUSED_PTR.store(!cur, Ordering::SeqCst);
                }
                ID_TRAY_QUIT => {
                    std::process::exit(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
