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
pub struct MonitorNormalizedRect {
    pub rx: f64,
    pub ry: f64,
    pub rw: f64,
    pub rh: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedScreenInfo {
    pub index: usize,
    pub display_id: u32,
    pub lx: f64,
    pub ly: f64,
    pub lw: f64,
    pub lh: f64,
    pub pixel_w: u32,
    pub pixel_h: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

pub struct CapturedScreenSlice {
    pub info: CapturedScreenInfo,
    pub rgba: Vec<u8>,
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
    pub active_monitor: Option<MonitorNormalizedRect>,
    pub active_screen_index: usize,
    pub screens: Vec<CapturedScreenInfo>,
    pub windows: Vec<WindowCandidate>,
}

pub struct CaptureSession {
    pub metadata: CaptureMetadata,
    pub screen_slices: Vec<CapturedScreenSlice>,
    pub virt_logical_x: f64,
    pub virt_logical_y: f64,
    pub monitor_phys_x: i32,
    pub monitor_phys_y: i32,
    pub monitor_phys_w: u32,
    pub monitor_phys_h: u32,
    pub toast_phys_x: i32,
    pub toast_phys_y: i32,
    pub toast_phys_w: u32,
    pub toast_phys_h: u32,
    pub toast_scale: f64,
    pub toast_display_id: u32,
}

/// Captures all connected displays:
/// - On Windows: stitches all displays into one continuous virtual desktop slice.
/// - On macOS: captures every display at native resolution for per-screen overlay rendering.
pub fn capture_active_monitor(app: &AppHandle, mode: &str) -> Result<CaptureSession, String> {
    #[cfg(target_os = "windows")]
    {
        capture_all_monitors_windows(app, mode)
    }

    #[cfg(not(target_os = "windows"))]
    {
        capture_all_monitors_macos(app, mode)
    }
}

/// Composites an arbitrary region `[req_x, req_y, req_w, req_h]` (in virtual logical desktop coords)
/// from all overlapping native screen slices, resampling mixed-DPI slices cleanly onto a single RGBA buffer.
pub fn compose_virtual_region_rgba(
    slices: &[CapturedScreenSlice],
    req_x: f64,
    req_y: f64,
    req_w: f64,
    req_h: f64,
) -> Result<(u32, u32, Vec<u8>), String> {
    if slices.is_empty() {
        return Err("No captured screens available".to_string());
    }

    let safe_w = req_w.max(1.0);
    let safe_h = req_h.max(1.0);
    let req_right = req_x + safe_w;
    let req_bottom = req_y + safe_h;

    let overlapping: Vec<&CapturedScreenSlice> = slices
        .iter()
        .filter(|s| {
            s.info.lx < req_right
                && (s.info.lx + s.info.lw) > req_x
                && s.info.ly < req_bottom
                && (s.info.ly + s.info.lh) > req_y
        })
        .collect();

    let out_scale = overlapping
        .iter()
        .map(|s| s.info.scale_factor)
        .fold(1.0_f64, f64::max)
        .max(1.0);
    let out_w = (safe_w * out_scale).round().clamp(1.0, 32768.0) as u32;
    let out_h = (safe_h * out_scale).round().clamp(1.0, 32768.0) as u32;

    let total_pixels = (out_w as usize).saturating_mul(out_h as usize);
    let mut canvas = [6u8, 10, 20, 255].repeat(total_pixels);
    let out_stride = (out_w as usize) * 4;

    for slice in overlapping {
        let info = &slice.info;
        let ox0 = req_x.max(info.lx);
        let oy0 = req_y.max(info.ly);
        let ox1 = req_right.min(info.lx + info.lw);
        let oy1 = req_bottom.min(info.ly + info.lh);
        if ox1 <= ox0 || oy1 <= oy0 {
            continue;
        }

        let sx0 = ((ox0 - info.lx) * info.scale_factor)
            .floor()
            .clamp(0.0, info.pixel_w.saturating_sub(1) as f64) as u32;
        let sy0 = ((oy0 - info.ly) * info.scale_factor)
            .floor()
            .clamp(0.0, info.pixel_h.saturating_sub(1) as f64) as u32;
        let sx1 = ((ox1 - info.lx) * info.scale_factor)
            .ceil()
            .clamp((sx0 + 1) as f64, info.pixel_w as f64) as u32;
        let sy1 = ((oy1 - info.ly) * info.scale_factor)
            .ceil()
            .clamp((sy0 + 1) as f64, info.pixel_h as f64) as u32;
        let sw = sx1.saturating_sub(sx0);
        let sh = sy1.saturating_sub(sy0);

        let dx0 = ((ox0 - req_x) * out_scale)
            .round()
            .clamp(0.0, out_w as f64) as u32;
        let dy0 = ((oy0 - req_y) * out_scale)
            .round()
            .clamp(0.0, out_h as f64) as u32;
        let dx1 = ((ox1 - req_x) * out_scale)
            .round()
            .clamp(dx0 as f64, out_w as f64) as u32;
        let dy1 = ((oy1 - req_y) * out_scale)
            .round()
            .clamp(dy0 as f64, out_h as f64) as u32;
        let dw = dx1.saturating_sub(dx0);
        let dh = dy1.saturating_sub(dy0);

        if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
            continue;
        }

        let src_stride = (info.pixel_w as usize) * 4;
        if sw == dw && sh == dh {
            let copy_bytes = (dw as usize) * 4;
            for row in 0..(dh as usize) {
                let src_off = ((sy0 as usize) + row) * src_stride + (sx0 as usize) * 4;
                let dst_off = ((dy0 as usize) + row) * out_stride + (dx0 as usize) * 4;
                if src_off + copy_bytes <= slice.rgba.len() && dst_off + copy_bytes <= canvas.len() {
                    canvas[dst_off..dst_off + copy_bytes]
                        .copy_from_slice(&slice.rgba[src_off..src_off + copy_bytes]);
                }
            }
        } else {
            let sub_stride = (sw as usize) * 4;
            let mut sub_rgba = vec![0u8; sub_stride.saturating_mul(sh as usize)];
            for row in 0..(sh as usize) {
                let src_off = ((sy0 as usize) + row) * src_stride + (sx0 as usize) * 4;
                let sub_off = row * sub_stride;
                if src_off + sub_stride <= slice.rgba.len()
                    && sub_off + sub_stride <= sub_rgba.len()
                {
                    sub_rgba[sub_off..sub_off + sub_stride]
                        .copy_from_slice(&slice.rgba[src_off..src_off + sub_stride]);
                }
            }
            if let Some(sub_img) = image::RgbaImage::from_raw(sw, sh, sub_rgba) {
                let resized =
                    image::imageops::resize(&sub_img, dw, dh, image::imageops::FilterType::Triangle);
                let res_raw = resized.as_raw();
                let res_stride = (dw as usize) * 4;
                for row in 0..(dh as usize) {
                    let src_off = row * res_stride;
                    let dst_off = ((dy0 as usize) + row) * out_stride + (dx0 as usize) * 4;
                    if src_off + res_stride <= res_raw.len() && dst_off + res_stride <= canvas.len()
                    {
                        canvas[dst_off..dst_off + res_stride]
                            .copy_from_slice(&res_raw[src_off..src_off + res_stride]);
                    }
                }
            }
        }
    }

    Ok((out_w, out_h, canvas))
}

#[cfg(target_os = "windows")]
fn capture_all_monitors_windows(app: &AppHandle, mode: &str) -> Result<CaptureSession, String> {
    // Freeze the exact capture moment BEFORE grabbing pixels
    let now = Local::now();
    let captured_at_iso = now.to_rfc3339();
    let captured_at_ms = now.timestamp_millis();

    let cursor = app.cursor_position().ok();
    let xcap_monitors = Monitor::all().map_err(|e| format!("Monitor enumeration failed: {e}"))?;
    if xcap_monitors.is_empty() {
        return Err("No capture monitors available".to_string());
    }

    struct CapturedSlice {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        scale: f64,
        is_primary: bool,
        rgba: Vec<u8>,
    }

    let mut slices: Vec<CapturedSlice> = Vec::with_capacity(xcap_monitors.len());
    let mut last_err = String::from("No readable display output");

    for mon in xcap_monitors {
        let mx = mon.x().unwrap_or(0);
        let my = mon.y().unwrap_or(0);
        let is_primary = mon.is_primary().unwrap_or(false);
        let scale = f64::from(mon.scale_factor().unwrap_or(1.0)).max(1.0);
        match mon.capture_image() {
            Ok(img) => {
                let w = img.width();
                let h = img.height();
                if w > 0 && h > 0 {
                    slices.push(CapturedSlice {
                        x: mx,
                        y: my,
                        w,
                        h,
                        scale,
                        is_primary,
                        rgba: img.into_raw(),
                    });
                }
            }
            Err(err) => {
                last_err = err.to_string();
            }
        }
    }

    if slices.is_empty() {
        return Err(format!("Screen capture failed: {last_err}"));
    }

    let virt_x = slices.iter().map(|s| s.x).min().unwrap_or(0);
    let virt_y = slices.iter().map(|s| s.y).min().unwrap_or(0);
    let virt_right = slices.iter().map(|s| s.x + s.w as i32).max().unwrap_or(1);
    let virt_bottom = slices.iter().map(|s| s.y + s.h as i32).max().unwrap_or(1);
    let virt_w = (virt_right - virt_x).max(1) as u32;
    let virt_h = (virt_bottom - virt_y).max(1) as u32;

    let active_idx = cursor
        .and_then(|pos| {
            slices.iter().position(|s| {
                pos.x >= s.x as f64
                    && pos.x < (s.x as f64 + s.w as f64)
                    && pos.y >= s.y as f64
                    && pos.y < (s.y as f64 + s.h as f64)
            })
        })
        .or_else(|| slices.iter().position(|s| s.is_primary))
        .unwrap_or(0);

    let active_mon = &slices[active_idx];
    let active_monitor = Some(MonitorNormalizedRect {
        rx: (active_mon.x - virt_x) as f64 / virt_w as f64,
        ry: (active_mon.y - virt_y) as f64 / virt_h as f64,
        rw: active_mon.w as f64 / virt_w as f64,
        rh: active_mon.h as f64 / virt_h as f64,
    });
    let toast_phys_x = active_mon.x;
    let toast_phys_y = active_mon.y;
    let toast_phys_w = active_mon.w;
    let toast_phys_h = active_mon.h;
    let toast_scale = active_mon.scale;

    let init_scale = slices
        .iter()
        .find(|s| s.is_primary)
        .map(|s| s.scale)
        .unwrap_or(active_mon.scale)
        .max(1.0);
    let logical_w = virt_w as f64 / init_scale;
    let logical_h = virt_h as f64 / init_scale;

    let rgba_bytes = if slices.len() == 1
        && slices[0].x == virt_x
        && slices[0].y == virt_y
        && slices[0].w == virt_w
        && slices[0].h == virt_h
    {
        slices.into_iter().next().unwrap().rgba
    } else {
        let covered_area: u64 = slices.iter().map(|s| u64::from(s.w) * u64::from(s.h)).sum();
        let virt_area = u64::from(virt_w) * u64::from(virt_h);
        let total_pixels = (virt_w as usize).saturating_mul(virt_h as usize);
        let mut canvas = if covered_area < virt_area {
            // Fill dead corners of asymmetrical monitor layouts with opaque dark slate
            // so transparent overlay windows never let pointer hits fall through.
            [6u8, 10, 20, 255].repeat(total_pixels)
        } else {
            vec![0u8; total_pixels.saturating_mul(4)]
        };
        let virt_stride = (virt_w as usize) * 4;
        for slice in slices {
            let dst_x = (slice.x - virt_x).max(0) as usize;
            let dst_y = (slice.y - virt_y).max(0) as usize;
            let src_stride = (slice.w as usize) * 4;
            let copy_bytes = src_stride.min((virt_w as usize).saturating_sub(dst_x) * 4);
            let rows = (slice.h as usize).min((virt_h as usize).saturating_sub(dst_y));
            for row in 0..rows {
                let src_off = row * src_stride;
                let dst_off = (dst_y + row) * virt_stride + dst_x * 4;
                if src_off + copy_bytes <= slice.rgba.len() && dst_off + copy_bytes <= canvas.len() {
                    canvas[dst_off..dst_off + copy_bytes]
                        .copy_from_slice(&slice.rgba[src_off..src_off + copy_bytes]);
                }
            }
        }
        canvas
    };

    // Collect visible windows overlapping any screen inside the virtual desktop
    let windows = collect_windows_on_monitor(
        virt_x as f64,
        virt_y as f64,
        virt_w as f64,
        virt_h as f64,
    );

    let virt_screen = CapturedScreenInfo {
        index: 0,
        display_id: 0,
        lx: 0.0,
        ly: 0.0,
        lw: logical_w,
        lh: logical_h,
        pixel_w: virt_w,
        pixel_h: virt_h,
        scale_factor: init_scale,
        is_primary: true,
    };

    Ok(CaptureSession {
        metadata: CaptureMetadata {
            mode: mode.to_string(),
            captured_at_iso,
            captured_at_ms,
            image_width: virt_w,
            image_height: virt_h,
            logical_width: logical_w,
            logical_height: logical_h,
            scale_factor: init_scale,
            active_monitor,
            active_screen_index: 0,
            screens: vec![virt_screen.clone()],
            windows,
        },
        screen_slices: vec![CapturedScreenSlice {
            info: virt_screen,
            rgba: rgba_bytes,
        }],
        virt_logical_x: virt_x as f64 / init_scale,
        virt_logical_y: virt_y as f64 / init_scale,
        monitor_phys_x: virt_x,
        monitor_phys_y: virt_y,
        monitor_phys_w: virt_w,
        monitor_phys_h: virt_h,
        toast_phys_x,
        toast_phys_y,
        toast_phys_w,
        toast_phys_h,
        toast_scale,
        toast_display_id: 0,
    })
}

#[cfg(not(target_os = "windows"))]
fn capture_all_monitors_macos(app: &AppHandle, mode: &str) -> Result<CaptureSession, String> {
    // Freeze the exact capture moment BEFORE grabbing pixels
    let now = Local::now();
    let captured_at_iso = now.to_rfc3339();
    let captured_at_ms = now.timestamp_millis();

    let cursor = app.cursor_position().ok();
    let cursor_scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0)
        .max(1.0);
    let xcap_monitors = Monitor::all().map_err(|e| format!("Monitor enumeration failed: {e}"))?;
    if xcap_monitors.is_empty() {
        return Err("No capture monitors available".to_string());
    }

    struct RawMacMonitor {
        display_id: u32,
        raw_lx: f64,
        raw_ly: f64,
        raw_lw: f64,
        raw_lh: f64,
        pixel_w: u32,
        pixel_h: u32,
        scale_factor: f64,
        is_primary: bool,
        rgba: Vec<u8>,
    }

    let mut captured: Vec<RawMacMonitor> = Vec::with_capacity(xcap_monitors.len());
    let mut last_err = String::from("Screen capture failed (check permissions)");

    for mon in xcap_monitors {
        let display_id = mon.id().unwrap_or(0);
        let raw_lx = mon.x().unwrap_or(0) as f64;
        let raw_ly = mon.y().unwrap_or(0) as f64;
        let raw_lw = mon.width().unwrap_or(1).max(1) as f64;
        let raw_lh = mon.height().unwrap_or(1).max(1) as f64;
        let is_primary = mon.is_primary().unwrap_or(false);
        let hint_scale = f64::from(mon.scale_factor().unwrap_or(1.0)).max(1.0);

        match mon.capture_image() {
            Ok(img) => {
                let pixel_w = img.width();
                let pixel_h = img.height();
                if pixel_w == 0 || pixel_h == 0 {
                    continue;
                }
                let scale_factor = (pixel_w as f64 / raw_lw).max(hint_scale).max(1.0);
                captured.push(RawMacMonitor {
                    display_id,
                    raw_lx,
                    raw_ly,
                    raw_lw,
                    raw_lh,
                    pixel_w,
                    pixel_h,
                    scale_factor,
                    is_primary,
                    rgba: img.into_raw(),
                });
            }
            Err(err) => {
                last_err = format!("Screen capture failed (check permissions): {err}");
            }
        }
    }

    if captured.is_empty() {
        return Err(last_err);
    }

    // Keep primary display at index 0 so it reuses the pre-warmed "overlay" window
    captured.sort_by_key(|m| !m.is_primary);

    let virt_lx = captured
        .iter()
        .map(|m| m.raw_lx)
        .fold(f64::INFINITY, f64::min);
    let virt_ly = captured
        .iter()
        .map(|m| m.raw_ly)
        .fold(f64::INFINITY, f64::min);
    let virt_right = captured
        .iter()
        .map(|m| m.raw_lx + m.raw_lw)
        .fold(f64::NEG_INFINITY, f64::max);
    let virt_bottom = captured
        .iter()
        .map(|m| m.raw_ly + m.raw_lh)
        .fold(f64::NEG_INFINITY, f64::max);
    let virt_lw = (virt_right - virt_lx).max(1.0);
    let virt_lh = (virt_bottom - virt_ly).max(1.0);

    let primary_scale = captured
        .iter()
        .find(|m| m.is_primary)
        .map(|m| m.scale_factor)
        .unwrap_or(cursor_scale)
        .max(1.0);
    let eff_cursor_scale = if cursor_scale > 1.0 {
        cursor_scale
    } else {
        primary_scale
    };

    let active_screen_index = cursor
        .and_then(|pos| {
            let cx = pos.x / eff_cursor_scale;
            let cy = pos.y / eff_cursor_scale;
            captured.iter().position(|m| {
                cx >= m.raw_lx
                    && cx < (m.raw_lx + m.raw_lw)
                    && cy >= m.raw_ly
                    && cy < (m.raw_ly + m.raw_lh)
            })
        })
        .unwrap_or(0);

    let mut screens: Vec<CapturedScreenInfo> = Vec::with_capacity(captured.len());
    let mut screen_slices: Vec<CapturedScreenSlice> = Vec::with_capacity(captured.len());
    let mut screen_sizes: Vec<(f64, f64)> = Vec::with_capacity(captured.len());
    let mut active_raw_lx = captured[0].raw_lx;
    let mut active_raw_ly = captured[0].raw_ly;

    for (index, m) in captured.into_iter().enumerate() {
        if index == active_screen_index {
            active_raw_lx = m.raw_lx;
            active_raw_ly = m.raw_ly;
        }
        screen_sizes.push((m.raw_lw, m.raw_lh));
        let info = CapturedScreenInfo {
            index,
            display_id: m.display_id,
            lx: m.raw_lx - virt_lx,
            ly: m.raw_ly - virt_ly,
            lw: m.raw_lw,
            lh: m.raw_lh,
            pixel_w: m.pixel_w,
            pixel_h: m.pixel_h,
            scale_factor: m.scale_factor,
            is_primary: m.is_primary,
        };
        screen_slices.push(CapturedScreenSlice {
            info: info.clone(),
            rgba: m.rgba,
        });
        screens.push(info);
    }

    let active_info = &screens[active_screen_index];
    let active_monitor = Some(MonitorNormalizedRect {
        rx: active_info.lx / virt_lw,
        ry: active_info.ly / virt_lh,
        rw: active_info.lw / virt_lw,
        rh: active_info.lh / virt_lh,
    });

    let scale_factor = active_info.scale_factor;
    let (image_width, image_height) = if screens.len() == 1 {
        (active_info.pixel_w, active_info.pixel_h)
    } else {
        (
            (virt_lw * scale_factor).round().max(1.0) as u32,
            (virt_lh * scale_factor).round().max(1.0) as u32,
        )
    };

    let windows = collect_windows_on_monitor(virt_lx, virt_ly, virt_lw, virt_lh, &screen_sizes);

    Ok(CaptureSession {
        metadata: CaptureMetadata {
            mode: mode.to_string(),
            captured_at_iso,
            captured_at_ms,
            image_width,
            image_height,
            logical_width: virt_lw,
            logical_height: virt_lh,
            scale_factor,
            active_monitor,
            active_screen_index,
            screens: screens.clone(),
            windows,
        },
        screen_slices,
        virt_logical_x: virt_lx,
        virt_logical_y: virt_ly,
        monitor_phys_x: (virt_lx * primary_scale).round() as i32,
        monitor_phys_y: (virt_ly * primary_scale).round() as i32,
        monitor_phys_w: (virt_lw * primary_scale).round().max(1.0) as u32,
        monitor_phys_h: (virt_lh * primary_scale).round().max(1.0) as u32,
        toast_phys_x: (active_raw_lx * active_info.scale_factor).round() as i32,
        toast_phys_y: (active_raw_ly * active_info.scale_factor).round() as i32,
        toast_phys_w: active_info.pixel_w,
        toast_phys_h: active_info.pixel_h,
        toast_scale: active_info.scale_factor,
        toast_display_id: active_info.display_id,
    })
}

#[cfg(not(target_os = "windows"))]
fn collect_windows_on_monitor(
    mon_x: f64,
    mon_y: f64,
    mon_w: f64,
    mon_h: f64,
    screen_sizes: &[(f64, f64)],
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

        // Ignore desktop-sized root layers (e.g. Finder desktop background on any monitor)
        let is_root_layer = title.is_empty()
            && ((iw >= mon_w * 0.99 && ih >= mon_h * 0.99)
                || screen_sizes
                    .iter()
                    .any(|&(sw, sh)| iw >= sw * 0.99 && ih >= sh * 0.99));
        if is_root_layer {
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
            let is_tray_bar =
                class_name == "Shell_TrayWnd" || class_name == "Shell_SecondaryTrayWnd";
            if (ex_style & (WS_EX_TOOLWINDOW.0 as usize)) != 0 && !is_tray_bar {
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

            if title.is_empty() && !is_tray_bar {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_solid_slice(
        index: usize,
        lx: f64,
        ly: f64,
        lw: f64,
        lh: f64,
        scale_factor: f64,
        rgba_pixel: [u8; 4],
    ) -> CapturedScreenSlice {
        let pixel_w = (lw * scale_factor).round() as u32;
        let pixel_h = (lh * scale_factor).round() as u32;
        let rgba = rgba_pixel.repeat((pixel_w as usize) * (pixel_h as usize));
        CapturedScreenSlice {
            info: CapturedScreenInfo {
                index,
                display_id: (index + 1) as u32,
                lx,
                ly,
                lw,
                lh,
                pixel_w,
                pixel_h,
                scale_factor,
                is_primary: index == 0,
            },
            rgba,
        }
    }

    fn pixel_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let off = ((y as usize) * (width as usize) + (x as usize)) * 4;
        [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
    }

    #[test]
    fn compose_single_screen_exact_copy() {
        let s0 = make_solid_slice(0, 0.0, 0.0, 100.0, 80.0, 2.0, [10, 20, 30, 255]);
        let (w, h, buf) = compose_virtual_region_rgba(&[s0], 10.0, 20.0, 30.0, 25.0).unwrap();
        assert_eq!((w, h), (60, 50));
        assert_eq!(pixel_at(&buf, w, 0, 0), [10, 20, 30, 255]);
        assert_eq!(pixel_at(&buf, w, 59, 49), [10, 20, 30, 255]);
    }

    #[test]
    fn compose_mixed_dpi_dual_monitor_spans_retina_and_1x() {
        let retina = make_solid_slice(0, 0.0, 0.0, 200.0, 100.0, 2.0, [200, 10, 20, 255]);
        let external = make_solid_slice(1, 200.0, 0.0, 200.0, 100.0, 1.0, [20, 180, 40, 255]);
        let (w, h, buf) =
            compose_virtual_region_rgba(&[retina, external], 150.0, 25.0, 100.0, 40.0).unwrap();
        assert_eq!((w, h), (200, 80));
        // Left half (first 50 logical points -> 100 px) comes from Retina display
        assert_eq!(pixel_at(&buf, w, 10, 10), [200, 10, 20, 255]);
        assert_eq!(pixel_at(&buf, w, 99, 79), [200, 10, 20, 255]);
        // Right half (next 50 logical points -> 100 px) is upscaled 1x -> 2x cleanly
        assert_eq!(pixel_at(&buf, w, 100, 0), [20, 180, 40, 255]);
        assert_eq!(pixel_at(&buf, w, 199, 79), [20, 180, 40, 255]);
    }

    #[test]
    fn compose_asymmetric_layout_fills_dead_corners() {
        let top_left = make_solid_slice(0, 0.0, 0.0, 100.0, 50.0, 1.0, [100, 110, 120, 255]);
        let bottom_right =
            make_solid_slice(1, 100.0, 20.0, 100.0, 80.0, 1.0, [10, 200, 210, 255]);
        let (w, h, buf) =
            compose_virtual_region_rgba(&[top_left, bottom_right], 50.0, 0.0, 100.0, 100.0)
                .unwrap();
        assert_eq!((w, h), (100, 100));
        assert_eq!(pixel_at(&buf, w, 10, 10), [100, 110, 120, 255]);
        // Bottom-left quadrant (x=10 < 50, y=80 >= 50) is outside both screens -> #060a14
        assert_eq!(pixel_at(&buf, w, 10, 80), [6, 10, 20, 255]);
        // Bottom-right quadrant (x=80 >= 50, y=80 >= 20) comes from screen 1
        assert_eq!(pixel_at(&buf, w, 80, 80), [10, 200, 210, 255]);
    }
}
