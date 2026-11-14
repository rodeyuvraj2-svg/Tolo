use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

static ICON_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn get_cache() -> &'static Mutex<HashMap<String, String>> {
    ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

const TOLO_LOGO_DATA_URI: &str = include_str!("../ui/logo_base64.txt");
const BRAVE_ICON: &str = include_str!("../ui/icons/brave.b64");
const ZAPFAST_ICON: &str = include_str!("../ui/icons/zapfast.b64");
const ANTIGRAVITY_ICON: &str = include_str!("../ui/icons/antigravity.b64");
const EXPLORER_ICON: &str = include_str!("../ui/icons/explorer.b64");
const TERMINAL_ICON: &str = include_str!("../ui/icons/terminal.b64");
const VSCODE_ICON: &str = include_str!("../ui/icons/vscode.b64");
const SCREENTIME_ICON: &str = include_str!("../ui/icons/screentime.b64");
const SNIPPINGTOOL_ICON: &str = include_str!("../ui/icons/snippingtool.b64");

/// Extracts or returns the application icon base64 data URI.
pub fn get_app_icon_data_uri(target: &str) -> Option<String> {
    if target.is_empty() || target == "__idle__" {
        return None;
    }

    let lower = target.to_lowercase();
    if lower.contains("tolo") {
        return Some(TOLO_LOGO_DATA_URI.trim().to_string());
    }
    if lower.contains("brave") {
        return Some(BRAVE_ICON.trim().to_string());
    }
    if lower.contains("zapfast") {
        return Some(ZAPFAST_ICON.trim().to_string());
    }
    if lower.contains("antigravity") {
        return Some(ANTIGRAVITY_ICON.trim().to_string());
    }
    if lower.contains("explorer") {
        return Some(EXPLORER_ICON.trim().to_string());
    }
    if lower.contains("windowsterminal") || lower.contains("terminal") || lower.ends_with("wt.exe") {
        return Some(TERMINAL_ICON.trim().to_string());
    }
    if lower.contains("code") || lower.contains("vscode") {
        return Some(VSCODE_ICON.trim().to_string());
    }
    if lower.contains("screentime") || lower.contains("screen-time") || lower.contains("screen time") {
        return Some(SCREENTIME_ICON.trim().to_string());
    }
    if lower.contains("snippingtool") || lower.contains("screensketch") {
        return Some(SNIPPINGTOOL_ICON.trim().to_string());
    }

    // Check cache first
    {
        let cache = get_cache().lock().unwrap();
        if let Some(uri) = cache.get(target) {
            return Some(uri.clone());
        }
    }

    let uri = unsafe { extract_icon_as_base64_bmp(target) };

    if let Some(ref u) = uri {
        let mut cache = get_cache().lock().unwrap();
        cache.insert(target.to_string(), u.clone());
    }

    uri
}

unsafe fn extract_icon_as_base64_bmp(path: &str) -> Option<String> {
    let mut wide_path: Vec<u16> = OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();

    let mut shfi = SHFILEINFOW::default();
    let res = SHGetFileInfoW(
        PCWSTR(wide_path.as_mut_ptr()),
        FILE_FLAGS_AND_ATTRIBUTES(0),
        Some(&mut shfi as *mut _),
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );

    if res == 0 || shfi.hIcon.0.is_null() {
        return None;
    }

    let h_icon: HICON = shfi.hIcon;
    let mut icon_info = ICONINFO::default();
    if GetIconInfo(h_icon, &mut icon_info).is_err() {
        let _ = DestroyIcon(h_icon);
        return None;
    }

    let hbm_color: HBITMAP = icon_info.hbmColor;
    let hbm_mask: HBITMAP = icon_info.hbmMask;

    let hdc_screen: HDC = GetDC(None);
    let hdc_mem: HDC = CreateCompatibleDC(Some(hdc_screen));

    let width = 32i32;
    let height = 32i32;

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (width * height * 4) as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
    };

    let mut pixel_buffer: Vec<u8> = vec![0u8; (width * height * 4) as usize];

    let lines = GetDIBits(
        hdc_mem,
        hbm_color,
        0,
        height as u32,
        Some(pixel_buffer.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Cleanup Win32 handles
    let _ = DeleteDC(hdc_mem);
    let _ = ReleaseDC(None, hdc_screen);
    let _ = DeleteObject(hbm_color.into());
    if !hbm_mask.0.is_null() {
        let _ = DeleteObject(hbm_mask.into());
    }
    let _ = DestroyIcon(h_icon);

    if lines == 0 {
        return None;
    }

    // Check if alpha channel exists; if all alpha bytes are 0, synthesize alpha
    let has_alpha = pixel_buffer.chunks(4).any(|p| p[3] != 0);
    if !has_alpha {
        for chunk in pixel_buffer.chunks_mut(4) {
            if chunk[0] != 0 || chunk[1] != 0 || chunk[2] != 0 {
                chunk[3] = 255;
            }
        }
    }

    // Assemble 32-bit BMP file in memory
    let file_header_size = 14u32;
    let info_header_size = 40u32;
    let pixel_data_size = (width * height * 4) as u32;
    let total_size = file_header_size + info_header_size + pixel_data_size;

    let mut bmp_bytes = Vec::with_capacity(total_size as usize);

    // BITMAPFILEHEADER
    bmp_bytes.extend_from_slice(b"BM");
    bmp_bytes.extend_from_slice(&total_size.to_le_bytes());
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // Reserved
    let offset_bits = file_header_size + info_header_size;
    bmp_bytes.extend_from_slice(&offset_bits.to_le_bytes());

    // BITMAPINFOHEADER
    bmp_bytes.extend_from_slice(&info_header_size.to_le_bytes());
    bmp_bytes.extend_from_slice(&width.to_le_bytes());
    bmp_bytes.extend_from_slice(&(-height).to_le_bytes()); // Top-down
    bmp_bytes.extend_from_slice(&1u16.to_le_bytes()); // Planes
    bmp_bytes.extend_from_slice(&32u16.to_le_bytes()); // Bits per pixel
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    bmp_bytes.extend_from_slice(&pixel_data_size.to_le_bytes());
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // Pixel data (BGRA)
    bmp_bytes.extend_from_slice(&pixel_buffer);

    let b64 = to_base64(&bmp_bytes);
    Some(format!("data:image/bmp;base64,{}", b64))
}

fn to_base64(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARSET[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARSET[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARSET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARSET[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}
