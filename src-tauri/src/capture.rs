use chrono::Local;
use serde::Serialize;
use tauri::AppHandle;
use xcap::Monitor;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowCandidate {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    /// Normalized bounding box inside the captured monitor (0.0 .. 1.0)
    pub rx: f64,
    pub ry: f64,
    pub rw: f64,
    pub rh: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureMetadata {
    pub mode: String,
    pub captured_at_iso: String,
    pub captured_at_ms: i64,
    pub image_width: u32,
    pub image_height: u32,
    pub logical_width: f64,
    pub logical_height: f64,
    pub scale_factor: f64,
    pub windows: Vec<WindowCandidate>,
}

pub struct CaptureSession {
    pub metadata: CaptureMetadata,
    pub rgba_bytes: Vec<u8>,
    pub monitor_phys_x: i32,
    pub monitor_phys_y: i32,
    pub monitor_phys_w: u32,
    pub monitor_phys_h: u32,
}

/// Captures the monitor where the mouse cursor currently resides,
/// freezes the exact timestamp, and collects visible application windows.
pub fn capture_active_monitor(app: &AppHandle, mode: &str) -> Result<CaptureSession, String> {
    // Freeze the exact capture moment BEFORE grabbing pixels
    let now = Local::now();
    let captured_at_iso = now.to_rfc3339();
    let captured_at_ms = now.timestamp_millis();

    let cursor = app.cursor_position().ok();
    let tauri_monitors = app.available_monitors().map_err(|e| e.to_string())?;
    if tauri_monitors.is_empty() {
        return Err("No displays found".to_string());
    }

    // Pick Tauri monitor containing the cursor (fallback to primary / first)
    let target_tauri_mon = if let Some(pos) = cursor {
        tauri_monitors
            .iter()
            .find(|m| {
                let p = m.position();
                let s = m.size();
                pos.x >= p.x as f64
                    && pos.x < (p.x as f64 + s.width as f64)
                    && pos.y >= p.y as f64
                    && pos.y < (p.y as f64 + s.height as f64)
            })
            .or_else(|| tauri_monitors.first())
            .ok_or_else(|| "No monitor found".to_string())?
    } else {
        tauri_monitors
            .first()
            .ok_or_else(|| "No monitor found".to_string())?
    };

    let t_pos = target_tauri_mon.position();
    let t_size = target_tauri_mon.size();
    let t_scale = target_tauri_mon.scale_factor();
    let logical_w = t_size.width as f64 / t_scale;
    let logical_h = t_size.height as f64 / t_scale;

    let xcap_monitors = Monitor::all().map_err(|e| format!("Monitor enumeration failed: {e}"))?;
    if xcap_monitors.is_empty() {
        return Err("No capture monitors available".to_string());
    }

    // Match xcap monitor to target Tauri monitor
    let target_xcap = select_matching_xcap_monitor(&xcap_monitors, t_pos.x, t_pos.y, t_scale)
        .ok_or_else(|| "Unable to match target monitor".to_string())?;

    let mon_x = target_xcap.x().unwrap_or(0) as f64;
    let mon_y = target_xcap.y().unwrap_or(0) as f64;
    let mon_w = target_xcap.width().unwrap_or(logical_w as u32).max(1) as f64;
    let mon_h = target_xcap.height().unwrap_or(logical_h as u32).max(1) as f64;

    let captured_img = target_xcap
        .capture_image()
        .map_err(|e| format!("Screen capture failed (check permissions): {e}"))?;

    let img_w = captured_img.width();
    let img_h = captured_img.height();
    let rgba_bytes = captured_img.into_raw();

    // Collect visible windows overlapping this monitor
    let windows = collect_windows_on_monitor(mon_x, mon_y, mon_w, mon_h);

    Ok(CaptureSession {
        metadata: CaptureMetadata {
            mode: mode.to_string(),
            captured_at_iso,
            captured_at_ms,
            image_width: img_w,
            image_height: img_h,
            logical_width: logical_w,
            logical_height: logical_h,
            scale_factor: t_scale,
            windows,
        },
        rgba_bytes,
        monitor_phys_x: t_pos.x,
        monitor_phys_y: t_pos.y,
        monitor_phys_w: t_size.width,
        monitor_phys_h: t_size.height,
    })
}

fn select_matching_xcap_monitor(
    monitors: &[Monitor],
    phys_x: i32,
    phys_y: i32,
    scale: f64,
) -> Option<&Monitor> {
    let logical_x = (phys_x as f64 / scale).round() as i32;

    // Try matching either physical origin (Windows) or logical origin (macOS)
    monitors
        .iter()
        .find(|m| {
            let mx = m.x().unwrap_or(i32::MAX);
            let my = m.y().unwrap_or(i32::MAX);
            (mx - phys_x).abs() <= 4 && (my - phys_y).abs() <= 4
        })
        .or_else(|| {
            monitors.iter().find(|m| {
                let mx = m.x().unwrap_or(i32::MAX);
                let my = m.y().unwrap_or(i32::MAX);
                let logical_y = (phys_y as f64 / scale).round() as i32;
                (mx - logical_x).abs() <= 4 && (my - logical_y).abs() <= 4
            })
        })
        .or_else(|| monitors.iter().find(|m| m.is_primary().unwrap_or(false)))
        .or_else(|| monitors.first())
}

#[cfg(not(target_os = "windows"))]
fn collect_windows_on_monitor(
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
) -> Vec<WindowCandidate> {
    let Ok(all_windows) = xcap::Window::all() else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for win in all_windows {
        if win.is_minimized().unwrap_or(false) {
            continue;
        }

        let app_name = win.app_name().unwrap_or_default();
        let title = win.title().unwrap_or_default();

        // Skip our own windows and empty system overlays
        let lower_app = app_name.to_lowercase();
        if lower_app.contains("zunzshot")
            || lower_app == "window server"
            || lower_app == "dock"
            || title == "ZunzShot Capture"
        {
            continue;
        }

        let wx = win.x().unwrap_or(0) as f64;
        let wy = win.y().unwrap_or(0) as f64;
        let ww = win.width().unwrap_or(0) as f64;
        let wh = win.height().unwrap_or(0) as f64;

        if ww < 36.0 || wh < 36.0 {
            continue;
        }

        // Compute intersection with the active monitor
        let left = wx.max(mon_x);
        let top = wy.max(mon_y);
        let right = (wx + ww).min(mon_x + mon_w);
        let bottom = (wy + wh).min(mon_y + mon_h);

        let iw = right - left;
        let ih = bottom - top;
        if iw < 30.0 || ih < 30.0 {
            continue;
        }

        // Ignore desktop-sized root layers (e.g. Finder desktop background)
        if iw >= mon_w * 0.99 && ih >= mon_h * 0.99 && title.is_empty() {
            continue;
        }

        result.push(WindowCandidate {
            id: win.id().unwrap_or(0),
            title: if title.is_empty() {
                app_name.clone()
            } else {
                title
            },
            app_name,
            rx: (left - mon_x) / mon_w,
            ry: (top - mon_y) / mon_h,
            rw: iw / mon_w,
            rh: ih / mon_h,
        });
    }

    result
}

#[cfg(target_os = "windows")]
fn collect_windows_on_monitor(
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
) -> Vec<WindowCandidate> {
    collect_windows_on_monitor_windows(mon_x, mon_y, mon_w, mon_h)
}

#[cfg(target_os = "windows")]
fn collect_windows_on_monitor_windows(
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
) -> Vec<WindowCandidate> {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::mem;
    use std::path::Path;
    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, TRUE};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
        InternalGetWindowText, IsHungAppWindow, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE,
        WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn enum_hwnds_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        TRUE
    }

    fn process_app_name(pid: u32) -> String {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return String::new();
            };
            let mut buf = [0u16; 512];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut size,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            if !ok || size == 0 {
                return String::new();
            }
            let full = String::from_utf16_lossy(&buf[..size as usize]);
            Path::new(&full)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string()
        }
    }

    let mut hwnds: Vec<HWND> = Vec::with_capacity(128);
    let enum_res = unsafe { EnumWindows(Some(enum_hwnds_proc), LPARAM(&raw mut hwnds as isize)) };
    if enum_res.is_err() {
        return Vec::new();
    }

    let own_pid = unsafe { GetCurrentProcessId() };
    let mut pid_names: HashMap<u32, String> = HashMap::new();
    let mut result = Vec::new();

    for hwnd in hwnds {
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool()
                || !IsWindowVisible(hwnd).as_bool()
                || IsIconic(hwnd).as_bool()
            {
                continue;
            }

            // Exclude our own windows and any hung external app BEFORE any UI/DWM queries
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == own_pid || IsHungAppWindow(hwnd).as_bool() {
                continue;
            }

            let mut cloaked: u32 = 0;
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast::<c_void>(),
                mem::size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
            {
                continue;
            }

            let mut class_buf = [0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class_buf);
            if class_len <= 0 {
                continue;
            }
            let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);
            if class_name == "Progman" || class_name == "WorkerW" || class_name == "Button" {
                continue;
            }

            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            if (ex_style & (WS_EX_TOOLWINDOW.0 as usize)) != 0 && class_name != "Shell_TrayWnd" {
                continue;
            }

            let mut rect = RECT::default();
            let got_dwm = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut rect).cast::<c_void>(),
                mem::size_of::<RECT>() as u32,
            )
            .is_ok();
            if !got_dwm && GetWindowRect(hwnd, &mut rect).is_err() {
                continue;
            }

            let wx = rect.left as f64;
            let wy = rect.top as f64;
            let ww = (rect.right - rect.left) as f64;
            let wh = (rect.bottom - rect.top) as f64;
            if ww < 36.0 || wh < 36.0 {
                continue;
            }

            // Compute intersection with the active monitor before reading window title/process name
            let left = wx.max(mon_x);
            let top = wy.max(mon_y);
            let right = (wx + ww).min(mon_x + mon_w);
            let bottom = (wy + wh).min(mon_y + mon_h);

            let iw = right - left;
            let ih = bottom - top;
            if iw < 30.0 || ih < 30.0 {
                continue;
            }

            // InternalGetWindowText reads directly from user32 without sending blocking WM_GETTEXT
            let mut title_buf = [0u16; 512];
            let title_len = InternalGetWindowText(hwnd, &mut title_buf);
            let title = if title_len > 0 {
                String::from_utf16_lossy(&title_buf[..title_len as usize])
            } else {
                String::new()
            };

            if iw >= mon_w * 0.99 && ih >= mon_h * 0.99 && title.is_empty() {
                continue;
            }

            let app_name = pid_names
                .entry(pid)
                .or_insert_with(|| process_app_name(pid))
                .clone();

            let lower_app = app_name.to_lowercase();
            if lower_app.contains("zunzshot")
                || lower_app == "window server"
                || lower_app == "dock"
                || title == "ZunzShot Capture"
            {
                continue;
            }

            result.push(WindowCandidate {
                id: hwnd.0 as usize as u32,
                title: if title.is_empty() {
                    app_name.clone()
                } else {
                    title
                },
                app_name,
                rx: (left - mon_x) / mon_w,
                ry: (top - mon_y) / mon_h,
                rw: iw / mon_w,
                rh: ih / mon_h,
            });
        }
    }

    result
}
