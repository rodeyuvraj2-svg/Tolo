use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, REG_VALUE_TYPE,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APP_NAME: PCWSTR = w!("Tolo");
const LEGACY_APP_NAME: PCWSTR = w!("ScreenTime");

/// Checks if Tolo is configured to run on Windows startup.
pub fn is_autostart_enabled() -> bool {
    unsafe {
        let mut h_key = HKEY::default();
        let err = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), KEY_READ, &mut h_key);
        if err != WIN32_ERROR(0) {
            return false;
        }

        let mut data_type = REG_VALUE_TYPE(REG_SZ.0);
        let mut data_size = 0u32;
        let query_res = RegQueryValueExW(
            h_key,
            APP_NAME,
            None,
            Some(&mut data_type),
            None,
            Some(&mut data_size),
        );

        let _ = RegCloseKey(h_key);
        query_res == WIN32_ERROR(0) && data_size > 0
    }
}

/// Enables or disables running Tolo on Windows startup.
pub fn set_autostart(enable: bool) -> Result<(), String> {
    unsafe {
        let mut h_key = HKEY::default();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), KEY_WRITE, &mut h_key);
        if open_res != WIN32_ERROR(0) {
            return Err("Failed to open Windows Run registry key".to_string());
        }

        // Clean up legacy key if it exists
        let _ = RegDeleteValueW(h_key, LEGACY_APP_NAME);

        let result = if enable {
            // Get current executable path
            let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe_str = current_exe.to_string_lossy().to_string();
            let cmd = format!("\"{}\" --minimized", exe_str);

            let wide_cmd: Vec<u16> = OsStr::new(&cmd)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let byte_slice = std::slice::from_raw_parts(
                wide_cmd.as_ptr() as *const u8,
                wide_cmd.len() * 2,
            );

            let set_res = RegSetValueExW(
                h_key,
                APP_NAME,
                Some(0),
                REG_VALUE_TYPE(REG_SZ.0),
                Some(byte_slice),
            );

            if set_res == WIN32_ERROR(0) {
                Ok(())
            } else {
                Err(format!("Failed to set registry value, error code: {}", set_res.0))
            }
        } else {
            let del_res = RegDeleteValueW(h_key, APP_NAME);
            // 2 is ERROR_FILE_NOT_FOUND
            if del_res == WIN32_ERROR(0) || del_res.0 == 2 {
                Ok(())
            } else {
                Err(format!("Failed to remove registry value, error code: {}", del_res.0))
            }
        };

        let _ = RegCloseKey(h_key);
        result
    }
}

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Ensures modern Windows Start Menu shortcut exists for clean branding on new user machines.
pub fn ensure_shortcuts() {
    if let Ok(app_data) = std::env::var("APPDATA") {
        let start_menu_path = std::path::PathBuf::from(&app_data)
            .join("Microsoft\\Windows\\Start Menu\\Programs\\Tolo.lnk");
        
        if !start_menu_path.exists() {
            if let Ok(current_exe) = std::env::current_exe() {
                let current_dir = current_exe.parent().unwrap_or(std::path::Path::new(""));
                let ps_cmd = format!(
                    "$wsh = New-Object -ComObject WScript.Shell; \
                     $sc = $wsh.CreateShortcut('{start_lnk}'); \
                     $sc.TargetPath = '{exe}'; \
                     $sc.IconLocation = '{exe},0'; \
                     $sc.WorkingDirectory = '{dir}'; \
                     $sc.Description = 'Tolo'; \
                     $sc.Save()",
                    start_lnk = start_menu_path.display(),
                    exe = current_exe.display(),
                    dir = current_dir.display()
                );

                let pwsh_candidates = [
                    "powershell",
                    "pwsh",
                    "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                ];
                for bin in pwsh_candidates {
                    let mut cmd = std::process::Command::new(bin);
                    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd]);
                    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
                    if let Ok(output) = cmd.output() {
                        if output.status.success() {
                            break;
                        }
                    }
                }

                // Notify Windows Shell to refresh icon & Start Menu cache
                unsafe {
                    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_FLUSH};
                    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_FLUSH, None, None);
                }
            }
        }
    }
}
