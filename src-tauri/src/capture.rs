use chrono::Local;
use serde::Serialize;
use tauri::AppHandle;
use xcap::{Monitor, Window};

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

fn collect_windows_on_monitor(
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
) -> Vec<WindowCandidate> {
    let Ok(all_windows) = Window::all() else {
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
