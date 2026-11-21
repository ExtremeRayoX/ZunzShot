mod capture;
mod i18n;
mod settings;
mod splash;

use arboard::{Clipboard, ImageData};
use capture::{CaptureMetadata, CaptureSession, MonitorNormalizedRect};
use chrono::Local;
use i18n::{tray_labels, Lang};
use settings::{load_settings, save_settings, AppSettings};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
#[cfg(target_os = "windows")]
use tauri::{PhysicalPosition, PhysicalSize, Position, Size};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_updater::UpdaterExt;

static ALLOW_APP_EXIT: AtomicBool = AtomicBool::new(false);
static SINGLE_INSTANCE_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
const SINGLE_INSTANCE_ADDR: &str = "127.0.0.1:45893";
static UPDATE_SENSOR_STARTED: AtomicBool = AtomicBool::new(false);
static UPDATE_SENSOR_SIGNAL: std::sync::LazyLock<(Mutex<u64>, std::sync::Condvar)> =
    std::sync::LazyLock::new(|| (Mutex::new(0), std::sync::Condvar::new()));
pub(crate) static CAPTURE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static SHORTCUTS_PAUSED: AtomicBool = AtomicBool::new(false);
static SHORTCUT_OP_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));
static NEXT_PIN_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TOAST_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy)]
struct MonitorBounds {
    phys_x: i32,
    phys_y: i32,
    phys_w: u32,
    phys_h: u32,
    scale: f64,
    display_id: u32,
    logical_x: f64,
    logical_y: f64,
    logical_w: f64,
    logical_h: f64,
}

static LAST_MONITOR_BOUNDS: Mutex<Option<MonitorBounds>> = Mutex::new(None);
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct MacScreenPlacement {
    index: usize,
    display_id: u32,
    global_lx: f64,
    global_ly: f64,
    lw: f64,
    lh: f64,
}
#[cfg(target_os = "macos")]
static LAST_MACOS_SCREENS: Mutex<(usize, Vec<MacScreenPlacement>)> = Mutex::new((0, Vec::new()));
#[cfg(target_os = "macos")]
static LAST_MACOS_VIRT_ORIGIN: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
#[cfg(target_os = "macos")]
static LAST_MACOS_CURSOR_SCALE: Mutex<f64> = Mutex::new(1.0);
#[cfg(target_os = "macos")]
static MACOS_DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static MACOS_DRAG_SEQ: AtomicU64 = AtomicU64::new(0);

struct CaptureInProgressGuard;

impl Drop for CaptureInProgressGuard {
    fn drop(&mut self) {
        CAPTURE_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
static MAIN_BUILD_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));
#[cfg(target_os = "windows")]
static UPDATER_BUILD_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));
#[cfg(target_os = "windows")]
static OVERLAY_BUILD_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));

#[cfg(target_os = "windows")]
fn force_windows_foreground_and_webview_focus(win: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsWindowVisible, SetForegroundWindow,
    };

    if !win.is_visible().unwrap_or(false) {
        return;
    }

    if let Ok(raw_hwnd) = win.hwnd() {
        let hwnd = HWND(raw_hwnd.0);
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return;
            }
            // Synthesize a no-op extended key release instead of AttachThreadInput so we never
            // couple our message queue to an unresponsive or elevated external window thread.
            keybd_event(0, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    let _ = win.set_focus();
    let _ = AsRef::<tauri::Webview<_>>::as_ref(win).set_focus();
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdatePreview {
    pub version: String,
    pub body: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterLaunchPayload {
    pub mode: String,
    pub preview: Option<PendingUpdatePreview>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwitchModePayload {
    mode: String,
    active_screen_index: usize,
    active_monitor: Option<MonitorNormalizedRect>,
}

pub struct AppState {
    pub settings: AppSettings,
    pub active_session: Option<CaptureSession>,
    pub tray: Option<TrayIcon>,
    #[cfg(target_os = "macos")]
    pub paused_app_menu: Option<Menu<tauri::Wry>>,
    pub shortcut_modes: HashMap<u32, String>,
    pub failed_shortcuts: Vec<String>,
    pub pending_tab: Option<String>,
    pub pending_updater_launch: Option<UpdaterLaunchPayload>,
    pub pending_capture_error: Option<String>,
    pub pinned_images: HashMap<String, Vec<u8>>,
    pub postponed_update: Option<(String, std::time::Instant, u64)>,
    pub notified_update: Option<(String, std::time::Instant)>,
}

enum SingleInstanceOutcome {
    Primary(Option<std::net::TcpListener>),
    DuplicateSignaled,
}

fn acquire_single_instance_listener() -> SingleInstanceOutcome {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};

    let Ok(addr) = SINGLE_INSTANCE_ADDR.parse::<SocketAddr>() else {
        return SingleInstanceOutcome::Primary(None);
    };

    for attempt in 0..8 {
        if let Ok(listener) = TcpListener::bind(addr) {
            return SingleInstanceOutcome::Primary(Some(listener));
        }

        if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(150)));
            if stream.write_all(b"ZUNZSHOT_WAKE").is_ok() {
                let _ = stream.flush();
                let mut buf = [0u8; 16];
                if let Ok(n) = stream.read(&mut buf) {
                    if &buf[..n] == b"ZUNZSHOT_OK" {
                        return SingleInstanceOutcome::DuplicateSignaled;
                    }
                }
            }
        }

        if attempt < 7 {
            std::thread::sleep(Duration::from_millis(45));
        }
    }

    SingleInstanceOutcome::Primary(None)
}

fn spawn_single_instance_responder(listener: std::net::TcpListener, app: AppHandle) {
    use std::io::{Read, Write};

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if SINGLE_INSTANCE_SHUTTING_DOWN.load(Ordering::SeqCst) {
                break;
            }
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            let mut buf = [0u8; 32];
            if let Ok(n) = stream.read(&mut buf) {
                if &buf[..n] == b"ZUNZSHOT_WAKE"
                    && !SINGLE_INSTANCE_SHUTTING_DOWN.load(Ordering::SeqCst)
                {
                    let _ = stream.write_all(b"ZUNZSHOT_OK");
                    let _ = stream.flush();
                    show_settings_window(&app);
                }
            }
        }
    });
}

fn has_screen_capture_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_core_graphics::CGPreflightScreenCaptureAccess()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[cfg(target_os = "macos")]
fn tcc_stamp_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("tcc-build-stamp.txt"))
}

#[cfg(target_os = "macos")]
fn tcc_prompted_stamp_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("tcc-prompted-stamp.txt"))
}

#[cfg(target_os = "macos")]
fn current_binary_stamp() -> String {
    let exe_meta = std::env::current_exe()
        .ok()
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| {
            let nanos = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("{}:{}", m.len(), nanos)
        })
        .unwrap_or_else(|| "unknown".to_string());
    format!("{}:{}", env!("CARGO_PKG_VERSION"), exe_meta)
}

#[cfg(target_os = "macos")]
fn write_tcc_stamp(app: &AppHandle, stamp: &str) {
    if let Some(path) = tcc_stamp_path(app) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, stamp);
    }
}

#[cfg(target_os = "macos")]
fn reset_macos_tcc_screen_capture() {
    // Without a paid Developer ID certificate, each ad-hoc build gets a new CDHash.
    // Clearing both the bundle identifier and the legacy v0.1.0 linker identifier removes
    // stale TCC entries that look "ON" in System Settings yet fail CGPreflightScreenCaptureAccess.
    for target_id in ["com.zunzshot.desktop", "zunzshot"] {
        let _ = std::process::Command::new("/usr/bin/tccutil")
            .args(["reset", "ScreenCapture", target_id])
            .output();
    }
    std::thread::sleep(Duration::from_millis(120));
}

#[cfg(target_os = "macos")]
fn cleanup_stale_tcc_on_reinstall(app: &AppHandle) {
    let stamp = current_binary_stamp();
    if has_screen_capture_permission() {
        write_tcc_stamp(app, &stamp);
        return;
    }

    let previous = tcc_stamp_path(app).and_then(|p| fs::read_to_string(p).ok());
    if previous.as_deref() != Some(stamp.as_str()) {
        reset_macos_tcc_screen_capture();
        write_tcc_stamp(app, &stamp);
        if let Some(p) = tcc_prompted_stamp_path(app) {
            let _ = fs::remove_file(p);
        }
    }
}

#[cfg(target_os = "macos")]
fn open_mac_screen_recording_pane() -> Result<(), String> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn build_tray_menu(app: &AppHandle, lang: Lang) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let labels = tray_labels(lang);
    let version = env!("CARGO_PKG_VERSION");
    let has_perm = has_screen_capture_permission();

    let item_region = MenuItemBuilder::with_id("capture_region", labels.capture_region)
        .enabled(has_perm)
        .build(app)?;
    let item_quick_copy = MenuItemBuilder::with_id("capture_quick_copy", labels.capture_quick_copy)
        .enabled(has_perm)
        .build(app)?;
    let item_circle = MenuItemBuilder::with_id("capture_circle", labels.capture_circle)
        .enabled(has_perm)
        .build(app)?;
    let item_window = MenuItemBuilder::with_id("capture_window", labels.capture_window)
        .enabled(has_perm)
        .build(app)?;
    let item_full = MenuItemBuilder::with_id("capture_fullscreen", labels.capture_fullscreen)
        .enabled(has_perm)
        .build(app)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let item_settings = MenuItemBuilder::with_id("open_settings", labels.settings)
        .enabled(has_perm)
        .build(app)?;
    let item_updates = MenuItemBuilder::with_id("check_updates", labels.check_updates)
        .enabled(has_perm)
        .build(app)?;
    let item_about = MenuItemBuilder::with_id("about", format!("{} (v{})", labels.about, version))
        .enabled(has_perm)
        .build(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let item_quit = MenuItemBuilder::with_id("quit", labels.quit).build(app)?;

    if !has_perm {
        let item_perm =
            MenuItemBuilder::with_id("grant_permission", labels.permission_required).build(app)?;
        let sep_perm = PredefinedMenuItem::separator(app)?;
        return MenuBuilder::new(app)
            .items(&[
                &item_perm,
                &sep_perm,
                &item_region,
                &item_quick_copy,
                &item_circle,
                &item_window,
                &item_full,
                &sep1,
                &item_settings,
                &item_updates,
                &item_about,
                &sep2,
                &item_quit,
            ])
            .build();
    }

    MenuBuilder::new(app)
        .items(&[
            &item_region,
            &item_quick_copy,
            &item_circle,
            &item_window,
            &item_full,
            &sep1,
            &item_settings,
            &item_updates,
            &item_about,
            &sep2,
            &item_quit,
        ])
        .build()
}

fn refresh_tray_menu(app: &AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let (lang, tray_opt) = {
        let guard = state.lock().unwrap();
        (Lang::from_code(&guard.settings.language), guard.tray.clone())
    };
    if let Some(tray) = tray_opt {
        if let Ok(menu) = build_tray_menu(app, lang) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

#[cfg(target_os = "macos")]
fn set_macos_app_menu_suspended(app: &AppHandle, suspended: bool) {
    let app_ui = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = app_ui.state::<Mutex<AppState>>();
        if suspended {
            let already_saved = state.lock().unwrap().paused_app_menu.is_some();
            if !already_saved {
                if let Ok(Some(prev_menu)) = app_ui.remove_menu() {
                    state.lock().unwrap().paused_app_menu = Some(prev_menu);
                }
            }
        } else {
            let saved = state.lock().unwrap().paused_app_menu.take();
            if let Some(menu) = saved {
                let _ = app_ui.set_menu(menu);
            }
        }
    });
}

fn register_shortcuts(app: &AppHandle, settings: &AppSettings) -> Vec<String> {
    let _op_guard = SHORTCUT_OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    SHORTCUTS_PAUSED.store(false, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    set_macos_app_menu_suspended(app, false);

    if !has_screen_capture_permission() {
        let state = app.state::<Mutex<AppState>>();
        let mut guard = state.lock().unwrap();
        guard.shortcut_modes.clear();
        guard.failed_shortcuts.clear();
        let _ = app.emit("zunzshot://shortcuts-status", &Vec::<String>::new());
        return Vec::new();
    }

    let pairs = [
        (&settings.shortcut_region, "region"),
        (&settings.shortcut_quick_copy, "quickCopy"),
        (&settings.shortcut_circle, "circle"),
        (&settings.shortcut_window, "window"),
        (&settings.shortcut_fullscreen, "fullscreen"),
    ];

    let mut map = HashMap::new();
    let mut failed = Vec::new();
    for (accel, mode) in pairs {
        let trimmed = accel.trim();
        if trimmed.is_empty() {
            continue;
        }
        match Shortcut::from_str(trimmed) {
            Ok(shortcut) => {
                let id = shortcut.id();
                // Explicitly unregister per-hotkey before registering in case unregister_all aborted
                // partway through on Windows, leaving a stale Win32 hotkey behind.
                let _ = gs.unregister(shortcut);
                if gs.register(shortcut).is_ok() {
                    map.insert(id, mode.to_string());
                } else {
                    failed.push(trimmed.to_string());
                }
            }
            Err(_) => {
                failed.push(trimmed.to_string());
            }
        }
    }

    {
        let state = app.state::<Mutex<AppState>>();
        let mut guard = state.lock().unwrap();
        guard.shortcut_modes = map;
        guard.failed_shortcuts = failed.clone();
    }
    let _ = app.emit("zunzshot://shortcuts-status", &failed);
    failed
}

#[tauri::command]
fn get_failed_shortcuts(state: State<'_, Mutex<AppState>>) -> Vec<String> {
    state.lock().unwrap().failed_shortcuts.clone()
}

fn attach_main_close_handler(app: &AppHandle, win: &WebviewWindow) {
    let app_for_close = app.clone();
    #[cfg(target_os = "windows")]
    let win_for_close = win.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            // Only re-register shortcuts if Settings was closed mid-recording while paused.
            if SHORTCUTS_PAUSED.load(Ordering::SeqCst) {
                let current = app_for_close
                    .state::<Mutex<AppState>>()
                    .lock()
                    .unwrap()
                    .settings
                    .clone();
                register_shortcuts(&app_for_close, &current);
            }
            #[cfg(target_os = "windows")]
            if !ALLOW_APP_EXIT.load(Ordering::Relaxed) {
                let ultra_light = app_for_close
                    .state::<Mutex<AppState>>()
                    .lock()
                    .unwrap()
                    .settings
                    .ultra_light_mode;
                if !ultra_light {
                    api.prevent_close();
                    let _ = win_for_close.hide();
                }
            }
            #[cfg(not(target_os = "windows"))]
            let _ = api;
        }
    });
}

#[cfg(target_os = "windows")]
fn attach_overlay_close_handler(app: &AppHandle, win: &WebviewWindow) {
    let app_for_close = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if !ALLOW_APP_EXIT.load(Ordering::Relaxed) {
                api.prevent_close();
                *LAST_OVERLAY_RECT.lock().unwrap() = None;
                let (ultra_light, pin_ids): (bool, Vec<String>) = {
                    let st = app_for_close.state::<Mutex<AppState>>();
                    let mut guard = st.lock().unwrap();
                    guard.active_session = None;
                    (
                        guard.settings.ultra_light_mode,
                        guard.pinned_images.keys().cloned().collect(),
                    )
                };
                hide_overlay_on_main_thread(&app_for_close, &pin_ids, ultra_light);
            }
        }
    });
}

fn focus_settings_window(win: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        force_windows_foreground_and_webview_focus(win);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win.set_focus();
        let _ = AsRef::<tauri::Webview<_>>::as_ref(win).set_focus();
    }
}

fn attach_updater_close_handler(app: &AppHandle, win: &WebviewWindow) {
    let app_for_close = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { .. } = event {
            if let Ok(mut guard) = app_for_close.state::<Mutex<AppState>>().lock() {
                guard.pending_updater_launch = None;
            }
        }
    });
}

fn show_configured_window<F>(
    app: &AppHandle,
    label: &'static str,
    #[cfg(target_os = "windows")] build_lock: &'static Mutex<()>,
    attach_close: F,
) where
    F: Fn(&AppHandle, &WebviewWindow) + Send + Sync + 'static,
{
    #[cfg(target_os = "windows")]
    {
        if let Some(win) = app.get_webview_window(label) {
            let _ = app.run_on_main_thread(move || {
                let _ = win.show();
                let _ = win.unminimize();
                focus_settings_window(&win);
            });
            return;
        }

        // Build WebView2 windows off the main thread so WebviewWindowBuilder::build() never
        // re-enters a synchronous COM/IPC pump on the UI thread.
        let app_handle = app.clone();
        std::thread::spawn(move || {
            let _build_guard = build_lock.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(win) = app_handle.get_webview_window(label) {
                let _ = app_handle.run_on_main_thread(move || {
                    let _ = win.show();
                    let _ = win.unminimize();
                    focus_settings_window(&win);
                });
                return;
            }

            let cfg = app_handle
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == label)
                .cloned();

            if let Some(cfg) = cfg {
                if let Ok(builder) = WebviewWindowBuilder::from_config(&app_handle, &cfg) {
                    if let Ok(win) = builder.visible(true).build() {
                        attach_close(&app_handle, &win);
                        let _ = app_handle.run_on_main_thread(move || {
                            let _ = win.show();
                            let _ = win.unminimize();
                            focus_settings_window(&win);
                        });
                    }
                }
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let app_handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app_handle.get_webview_window(label) {
                let _ = win.show();
                let _ = win.unminimize();
                focus_settings_window(&win);
                return;
            }

            let cfg = app_handle
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == label)
                .cloned();

            if let Some(cfg) = cfg {
                if let Ok(builder) = WebviewWindowBuilder::from_config(&app_handle, &cfg) {
                    if let Ok(win) = builder.visible(true).build() {
                        attach_close(&app_handle, &win);
                        focus_settings_window(&win);
                    }
                }
            }
        });
    }
}

pub(crate) fn show_settings_window(app: &AppHandle) {
    show_configured_window(
        app,
        "main",
        #[cfg(target_os = "windows")]
        &MAIN_BUILD_LOCK,
        attach_main_close_handler,
    );
}

fn show_updater_window(app: &AppHandle, mode: &str, preview: Option<PendingUpdatePreview>) {
    let payload = UpdaterLaunchPayload {
        mode: mode.to_string(),
        preview,
    };
    {
        let state = app.state::<Mutex<AppState>>();
        state.lock().unwrap().pending_updater_launch = Some(payload.clone());
    }
    show_configured_window(
        app,
        "updater",
        #[cfg(target_os = "windows")]
        &UPDATER_BUILD_LOCK,
        attach_updater_close_handler,
    );
    let _ = app.emit("zunzshot://updater-mode", &payload);
}

fn open_settings_tab(app: &AppHandle, tab: &str) {
    {
        let state = app.state::<Mutex<AppState>>();
        state.lock().unwrap().pending_tab = Some(tab.to_string());
    }
    show_settings_window(app);
    let _ = app.emit("zunzshot://open-tab", tab);
}

fn overlay_window_label(index: usize) -> String {
    if index == 0 {
        "overlay".to_string()
    } else {
        format!("overlay-{index}")
    }
}

fn overlay_window_index(label: &str) -> Option<usize> {
    if label == "overlay" {
        Some(0)
    } else {
        label.strip_prefix("overlay-")?.parse::<usize>().ok()
    }
}

fn all_overlay_windows(app: &AppHandle) -> Vec<(usize, WebviewWindow)> {
    let mut wins: Vec<(usize, WebviewWindow)> = app
        .webview_windows()
        .into_iter()
        .filter_map(|(label, win)| overlay_window_index(&label).map(|idx| (idx, win)))
        .collect();
    wins.sort_by_key(|(idx, _)| *idx);
    wins
}

fn focus_platform_overlay_window(ov: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    force_windows_foreground_and_webview_focus(ov);
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "macos")]
        if let Ok(ns_win_ptr) = ov.ns_window() {
            if !ns_win_ptr.is_null() {
                let ns_win = unsafe { &*(ns_win_ptr as *const objc2_app_kit::NSWindow) };
                ns_win.makeKeyAndOrderFront(None);
            }
        }
        let _ = ov.set_focus();
        let _ = AsRef::<tauri::Webview<_>>::as_ref(ov).set_focus();
    }
}

fn ensure_overlay_window_for_index(app: &AppHandle, index: usize) -> Option<WebviewWindow> {
    let label = overlay_window_label(index);
    #[cfg(target_os = "windows")]
    {
        let _build_guard = OVERLAY_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ov) = app.get_webview_window(&label) {
            return Some(ov);
        }
        let mut cfg = app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "overlay")
            .cloned()?;
        cfg.label = label;
        let win = WebviewWindowBuilder::from_config(app, &cfg)
            .ok()
            .and_then(|b| b.visible(false).build().ok())?;
        attach_overlay_close_handler(app, &win);
        Some(win)
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(ov) = app.get_webview_window(&label) {
            return Some(ov);
        }
        let mut cfg = app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "overlay")
            .cloned()?;
        cfg.label = label;
        WebviewWindowBuilder::from_config(app, &cfg)
            .ok()
            .and_then(|b| b.visible(false).build().ok())
    }
}

fn ensure_overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    ensure_overlay_window_for_index(app, 0)
}

fn trigger_capture(app: AppHandle, mode: String) {
    if !has_screen_capture_permission() {
        refresh_tray_menu(&app);
        let _ = app.emit("zunzshot://permission-updated", false);
        show_settings_window(&app);
        return;
    }

    // If an active capture overlay is already visible, switch its mode instead of
    // re-capturing over the dimmed overlay window.
    let existing_overlays = all_overlay_windows(&app);
    if existing_overlays
        .iter()
        .any(|(_, w)| w.is_visible().unwrap_or(false))
    {
        let switch_payload_opt = {
            let cursor_pos = app.cursor_position().ok();
            #[cfg(not(target_os = "windows"))]
            let mac_cursor_scale = (*LAST_MACOS_CURSOR_SCALE.lock().unwrap()).max(1.0);
            let state = app.state::<Mutex<AppState>>();
            let mut guard = state.lock().unwrap();
            guard.active_session.as_mut().map(|session| {
                if let Some(pos) = cursor_pos {
                    #[cfg(target_os = "windows")]
                    let (cx, cy) = (pos.x, pos.y);
                    #[cfg(not(target_os = "windows"))]
                    let (cx, cy) = (pos.x / mac_cursor_scale, pos.y / mac_cursor_scale);

                    if let Some(mon_idx) =
                        capture::find_monitor_index_at_point(&session.monitor_bounds, cx, cy)
                    {
                        if let Some(mon_rect) = session.metadata.monitors.get(mon_idx).cloned() {
                            session.metadata.active_monitor = Some(mon_rect);
                        }
                        if session.metadata.screens.len() > 1 {
                            session.metadata.active_screen_index = mon_idx;
                        }
                        if let Some(mb) = session.monitor_bounds.get(mon_idx) {
                            let mut bounds_guard = LAST_MONITOR_BOUNDS.lock().unwrap();
                            if let Some(ref mut last) = *bounds_guard {
                                #[cfg(target_os = "windows")]
                                {
                                    last.phys_x = mb.x.round() as i32;
                                    last.phys_y = mb.y.round() as i32;
                                    last.phys_w = mb.w.round().max(1.0) as u32;
                                    last.phys_h = mb.h.round().max(1.0) as u32;
                                    last.scale = mb.scale.max(1.0);
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    let sc = mb.scale.max(1.0);
                                    last.logical_x = mb.x;
                                    last.logical_y = mb.y;
                                    last.logical_w = mb.w;
                                    last.logical_h = mb.h;
                                    last.phys_x = (mb.x * sc).round() as i32;
                                    last.phys_y = (mb.y * sc).round() as i32;
                                    last.phys_w = (mb.w * sc).round().max(1.0) as u32;
                                    last.phys_h = (mb.h * sc).round().max(1.0) as u32;
                                    last.scale = sc;
                                    if let Some(scr) = session.metadata.screens.get(mon_idx) {
                                        last.display_id = scr.display_id;
                                    }
                                }
                            }
                        }
                    }
                }
                SwitchModePayload {
                    mode: mode.clone(),
                    active_screen_index: session.metadata.active_screen_index,
                    active_monitor: session.metadata.active_monitor.clone(),
                }
            })
        };
        if let Some(payload) = switch_payload_opt {
            let active_idx = payload.active_screen_index;
            for (_, ov) in &existing_overlays {
                if ov.is_visible().unwrap_or(false) {
                    let _ = ov.emit("zunzshot://switch-mode", &payload);
                }
            }
            let target_ov = existing_overlays
                .iter()
                .find(|(idx, _)| *idx == active_idx)
                .or_else(|| existing_overlays.first())
                .map(|(_, w)| w.clone());
            if let Some(ov_focus) = target_ov {
                let _ = app.run_on_main_thread(move || {
                    focus_platform_overlay_window(&ov_focus);
                    let _ = ov_focus.emit("zunzshot://focus-canvas", ());
                });
            }
            return;
        }
    }

    if CAPTURE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let had_splash = splash::dismiss_splash_before_capture(&app);

    std::thread::spawn(move || {
        let _capture_guard = CaptureInProgressGuard;
        if had_splash {
            std::thread::sleep(Duration::from_millis(60));
        }

        *LAST_OVERLAY_RECT.lock().unwrap() = None;
        #[cfg(target_os = "macos")]
        {
            MACOS_DRAG_ACTIVE.store(false, Ordering::SeqCst);
            *LAST_MACOS_SCREENS.lock().unwrap() = (0, Vec::new());
        }
        {
            let state = app.state::<Mutex<AppState>>();
            state.lock().unwrap().active_session = None;
        }

        for (_, ov) in all_overlay_windows(&app) {
            let _ = ov.emit("zunzshot://session-reset", ());
        }

        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let app_hide = app.clone();
        if app
            .run_on_main_thread(move || {
                let mut hid_any = false;
                if let Some(main_win) = app_hide.get_webview_window("main") {
                    if main_win.is_visible().unwrap_or(false) {
                        let _ = main_win.hide();
                        hid_any = true;
                    }
                }
                if all_overlay_windows(&app_hide)
                    .iter()
                    .any(|(_, w)| w.is_visible().unwrap_or(false))
                {
                    hide_overlay_on_main_thread(&app_hide, &[], false);
                    hid_any = true;
                }
                let _ = tx.send(hid_any);
            })
            .is_ok()
            && matches!(rx.recv_timeout(Duration::from_millis(500)), Ok(true))
        {
            std::thread::sleep(Duration::from_millis(140));
        }

        match capture::capture_active_monitor(&app, &mode) {
            Ok(session) => {
                let phys_x = session.monitor_phys_x;
                let phys_y = session.monitor_phys_y;
                let phys_w = session.monitor_phys_w;
                let phys_h = session.monitor_phys_h;
                let toast_sc = session.toast_scale.max(1.0);
                let active_screen_idx = session.metadata.active_screen_index;
                let active_screen_info = session
                    .metadata
                    .screens
                    .get(active_screen_idx)
                    .cloned()
                    .unwrap_or_else(|| session.metadata.screens[0].clone());
                let virt_lx = session.virt_logical_x;
                let virt_ly = session.virt_logical_y;
                *LAST_MONITOR_BOUNDS.lock().unwrap() = Some(MonitorBounds {
                    phys_x: session.toast_phys_x,
                    phys_y: session.toast_phys_y,
                    phys_w: session.toast_phys_w,
                    phys_h: session.toast_phys_h,
                    scale: toast_sc,
                    display_id: session.toast_display_id,
                    logical_x: virt_lx + active_screen_info.lx,
                    logical_y: virt_ly + active_screen_info.ly,
                    logical_w: active_screen_info.lw,
                    logical_h: active_screen_info.lh,
                });

                #[cfg(target_os = "macos")]
                let mac_placements: Vec<MacScreenPlacement> = {
                    let cursor_sc = app
                        .primary_monitor()
                        .ok()
                        .flatten()
                        .map(|m| m.scale_factor())
                        .unwrap_or(1.0)
                        .max(1.0);
                    *LAST_MACOS_VIRT_ORIGIN.lock().unwrap() = (virt_lx, virt_ly);
                    *LAST_MACOS_CURSOR_SCALE.lock().unwrap() = cursor_sc;
                    session
                        .metadata
                        .screens
                        .iter()
                        .map(|s| MacScreenPlacement {
                            index: s.index,
                            display_id: s.display_id,
                            global_lx: virt_lx + s.lx,
                            global_ly: virt_ly + s.ly,
                            lw: s.lw,
                            lh: s.lh,
                        })
                        .collect()
                };

                {
                    let state = app.state::<Mutex<AppState>>();
                    let mut guard = state.lock().unwrap();
                    guard.active_session = Some(session);
                }

                #[cfg(target_os = "windows")]
                {
                    let _ = ensure_overlay_window(&app);
                    present_overlay_window(
                        &app,
                        (phys_x, phys_y, phys_w, phys_h),
                        true,
                        true,
                    );
                }

                #[cfg(target_os = "macos")]
                {
                    let _ = (phys_x, phys_y, phys_w, phys_h);
                    *LAST_MACOS_SCREENS.lock().unwrap() =
                        (active_screen_idx, mac_placements.clone());
                    *LAST_OVERLAY_RECT.lock().unwrap() = Some((phys_x, phys_y, phys_w, phys_h));
                    present_macos_overlay_windows(
                        &app,
                        mac_placements,
                        active_screen_idx,
                        true,
                        true,
                    );
                }

                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    let _ = (virt_lx, virt_ly, active_screen_idx);
                    present_overlay_window(
                        &app,
                        (phys_x, phys_y, phys_w, phys_h),
                        true,
                        true,
                    );
                }
            }
            Err(err) => {
                {
                    let state = app.state::<Mutex<AppState>>();
                    state.lock().unwrap().pending_capture_error = Some(err.clone());
                }
                show_settings_window(&app);
                if let Some(main_win) = app.get_webview_window("main") {
                    let _ = main_win.emit("zunzshot://capture-error", err);
                }
            }
        }
    });
}

#[tauri::command]
fn get_settings(state: State<'_, Mutex<AppState>>) -> AppSettings {
    state.lock().unwrap().settings.clone()
}

static LAST_PERM_STATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn sync_permission_state(app: &AppHandle) -> bool {
    let granted = has_screen_capture_permission();
    let prev = LAST_PERM_STATE.swap(granted, std::sync::atomic::Ordering::Relaxed);
    if granted != prev {
        refresh_tray_menu(app);
        let settings = app.state::<Mutex<AppState>>().lock().unwrap().settings.clone();
        register_shortcuts(app, &settings);
        let _ = app.emit("zunzshot://permission-updated", granted);
    }
    granted
}

#[tauri::command]
fn save_settings_cmd(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    new_settings: AppSettings,
) -> Result<AppSettings, String> {
    if !has_screen_capture_permission() {
        return Err("Screen recording permission is required on macOS".to_string());
    }

    // Update shared state before disk I/O so any concurrent reader always sees latest settings.
    let (prev_settings, has_active_session) = {
        let mut guard = state.lock().unwrap();
        let prev = guard.settings.clone();
        guard.settings = new_settings.clone();
        (prev, guard.active_session.is_some())
    };

    if let Err(err) = save_settings(&app, &new_settings) {
        {
            let mut guard = state.lock().unwrap();
            guard.settings = prev_settings.clone();
        }
        if SHORTCUTS_PAUSED.load(Ordering::SeqCst) {
            register_shortcuts(&app, &prev_settings);
        }
        return Err(err);
    }

    // Sync OS autostart preference
    let autostart = app.autolaunch();
    if new_settings.launch_at_startup {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }

    if new_settings.ultra_light_mode {
        if !has_active_session {
            let app_handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                #[cfg(target_os = "windows")]
                hide_overlay_on_main_thread(&app_handle, &[], true);
                #[cfg(not(target_os = "windows"))]
                if let Some(ov) = app_handle.get_webview_window("overlay") {
                    let _ = ov.hide();
                    let _ = ov.destroy();
                }
            });
        }
    } else {
        let app_handle = app.clone();
        #[cfg(target_os = "windows")]
        std::thread::spawn(move || {
            let _ = ensure_overlay_window(&app_handle);
        });
        #[cfg(not(target_os = "windows"))]
        {
            let _ = app.run_on_main_thread(move || {
                let _ = ensure_overlay_window(&app_handle);
            });
        }
    }

    refresh_tray_menu(&app);
    register_shortcuts(&app, &new_settings);
    let _ = app.emit("zunzshot://settings-updated", &new_settings);
    if !new_settings.festive_themes {
        splash::clear_cached_manifest(&app);
    } else if !new_settings.ultra_light_mode && new_settings.startup_animation {
        splash::trigger_festive_theme_sync(&app, 1);
    }
    if new_settings.auto_check_updates {
        trigger_update_sensor(&app, 1);
    }
    Ok(new_settings)
}

#[tauri::command]
fn set_language_cmd(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    language: String,
) -> Result<AppSettings, String> {
    let normalized = Lang::from_code(&language).code().to_string();
    let updated = {
        let mut guard = state.lock().unwrap();
        guard.settings.language = normalized;
        guard.settings.language_selected = true;
        guard.settings.clone()
    };

    save_settings(&app, &updated)?;
    refresh_tray_menu(&app);
    let _ = app.emit("zunzshot://settings-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn complete_onboarding_cmd(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    language: Option<String>,
    auto_check_updates: bool,
) -> Result<AppSettings, String> {
    let updated = {
        let mut guard = state.lock().unwrap();
        if let Some(lang_code) = language {
            guard.settings.language = Lang::from_code(&lang_code).code().to_string();
        }
        guard.settings.language_selected = true;
        guard.settings.auto_check_updates = auto_check_updates;
        guard.settings.updater_prompt_shown = true;
        guard.settings.clone()
    };

    save_settings(&app, &updated)?;
    refresh_tray_menu(&app);
    let _ = app.emit("zunzshot://settings-updated", &updated);
    if updated.auto_check_updates {
        trigger_update_sensor(&app, 1);
    }
    Ok(updated)
}

#[tauri::command]
fn set_shortcuts_paused(app: AppHandle, state: State<'_, Mutex<AppState>>, paused: bool) {
    if paused {
        let _op_guard = SHORTCUT_OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        SHORTCUTS_PAUSED.store(true, Ordering::SeqCst);
        let _ = app.global_shortcut().unregister_all();
        #[cfg(target_os = "macos")]
        set_macos_app_menu_suspended(&app, true);
    } else {
        let settings = state.lock().unwrap().settings.clone();
        register_shortcuts(&app, &settings);
    }
}

#[tauri::command]
fn start_capture(app: AppHandle, mode: String) {
    trigger_capture(app, mode);
}

#[tauri::command]
fn get_capture_metadata(state: State<'_, Mutex<AppState>>) -> Option<CaptureMetadata> {
    state
        .lock()
        .unwrap()
        .active_session
        .as_ref()
        .map(|s| s.metadata.clone())
}

#[tauri::command]
fn get_capture_rgba(
    state: State<'_, Mutex<AppState>>,
    screen_index: Option<usize>,
) -> Result<Response, String> {
    let guard = state.lock().unwrap();
    let session = guard
        .active_session
        .as_ref()
        .ok_or_else(|| "No active capture session".to_string())?;

    let idx = screen_index.unwrap_or(0);
    let slice = session
        .screen_slices
        .get(idx)
        .or_else(|| session.screen_slices.first())
        .ok_or_else(|| "No capture screen slice available".to_string())?;

    let w = slice.info.pixel_w;
    let h = slice.info.pixel_h;
    let mut payload = Vec::with_capacity(8 + slice.rgba.len());
    payload.extend_from_slice(&w.to_le_bytes());
    payload.extend_from_slice(&h.to_le_bytes());
    payload.extend_from_slice(&slice.rgba);
    Ok(Response::new(payload))
}

#[tauri::command]
fn get_capture_region_rgba(
    state: State<'_, Mutex<AppState>>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<Response, String> {
    let guard = state.lock().unwrap();
    let session = guard
        .active_session
        .as_ref()
        .ok_or_else(|| "No active capture session".to_string())?;

    let (out_w, out_h, rgba) =
        capture::compose_virtual_region_rgba(&session.screen_slices, x, y, w, h)?;
    let mut payload = Vec::with_capacity(8 + rgba.len());
    payload.extend_from_slice(&out_w.to_le_bytes());
    payload.extend_from_slice(&out_h.to_le_bytes());
    payload.extend_from_slice(&rgba);
    Ok(Response::new(payload))
}

#[tauri::command]
#[allow(unused_variables)]
fn set_macos_drag_monitor(app: AppHandle, active: bool) {
    #[cfg(target_os = "macos")]
    {
        if !active {
            MACOS_DRAG_ACTIVE.store(false, Ordering::SeqCst);
            return;
        }
        let seq = MACOS_DRAG_SEQ.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        MACOS_DRAG_ACTIVE.store(true, Ordering::SeqCst);
        let app_handle = app.clone();
        std::thread::spawn(move || {
            use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGMouseButton};

            #[derive(Clone, serde::Serialize)]
            struct DragPointPayload {
                vx: f64,
                vy: f64,
            }

            let mut last_vx = f64::NAN;
            let mut last_vy = f64::NAN;
            while MACOS_DRAG_ACTIVE.load(Ordering::SeqCst)
                && MACOS_DRAG_SEQ.load(Ordering::SeqCst) == seq
            {
                let (virt_lx, virt_ly) = *LAST_MACOS_VIRT_ORIGIN.lock().unwrap();
                let cursor_scale = (*LAST_MACOS_CURSOR_SCALE.lock().unwrap()).max(1.0);
                let pos = app_handle.cursor_position().ok();
                let pressed = CGEventSource::button_state(
                    CGEventSourceStateID::CombinedSessionState,
                    CGMouseButton::Left,
                );

                if let Some(p) = pos {
                    let vx = p.x / cursor_scale - virt_lx;
                    let vy = p.y / cursor_scale - virt_ly;
                    if !pressed {
                        let _ = app_handle.emit("zunzshot://global-drag-up", DragPointPayload { vx, vy });
                        break;
                    }
                    if (vx - last_vx).abs() >= 0.25 || (vy - last_vy).abs() >= 0.25 {
                        last_vx = vx;
                        last_vy = vy;
                        let _ = app_handle.emit("zunzshot://global-drag-move", DragPointPayload { vx, vy });
                    }
                } else if !pressed {
                    let _ = app_handle.emit(
                        "zunzshot://global-drag-up",
                        DragPointPayload {
                            vx: if last_vx.is_finite() { last_vx } else { 0.0 },
                            vy: if last_vy.is_finite() { last_vy } else { 0.0 },
                        },
                    );
                    break;
                }

                std::thread::sleep(Duration::from_millis(8));
            }
            if MACOS_DRAG_SEQ.load(Ordering::SeqCst) == seq {
                MACOS_DRAG_ACTIVE.store(false, Ordering::SeqCst);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, active);
    }
}

static LAST_OVERLAY_RECT: std::sync::Mutex<Option<(i32, i32, u32, u32)>> =
    std::sync::Mutex::new(None);

#[cfg(target_os = "macos")]
fn ns_screen_rect_for_display(
    mtm: objc2::MainThreadMarker,
    display_id: u32,
    fallback_lx: f64,
    fallback_ly: f64,
    fallback_lw: f64,
    fallback_lh: f64,
) -> objc2_foundation::NSRect {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};

    let screens = NSScreen::screens(mtm);
    let primary_h = if !screens.is_empty() {
        screens.objectAtIndex(0).frame().size.height
    } else {
        fallback_lh.max(900.0)
    };

    if display_id != 0 {
        let key = NSString::from_str("NSScreenNumber");
        let count = screens.len();
        for idx in 0..count {
            let screen = screens.objectAtIndex(idx);
            let desc = screen.deviceDescription();
            if let Some(obj) = desc.objectForKey(&key) {
                if let Ok(num) = obj.downcast::<NSNumber>() {
                    if num.unsignedIntValue() == display_id {
                        return screen.frame();
                    }
                }
            }
        }
    }

    NSRect::new(
        NSPoint::new(fallback_lx, primary_h - (fallback_ly + fallback_lh)),
        NSSize::new(fallback_lw.max(1.0), fallback_lh.max(1.0)),
    )
}

#[cfg(target_os = "macos")]
fn apply_macos_screen_overlay_window(
    ov: &tauri::WebviewWindow,
    placement: MacScreenPlacement,
    elevated: bool,
    make_key: bool,
    emit_ready: bool,
) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(ns_win_ptr) = ov.ns_window() else {
        return;
    };
    if ns_win_ptr.is_null() {
        return;
    }
    let ns_win = unsafe { &*(ns_win_ptr as *const NSWindow) };
    let target_rect = ns_screen_rect_for_display(
        mtm,
        placement.display_id,
        placement.global_lx,
        placement.global_ly,
        placement.lw,
        placement.lh,
    );

    let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        | NSWindowCollectionBehavior::IgnoresCycle
        | NSWindowCollectionBehavior::Stationary;
    ns_win.setCollectionBehavior(behavior);
    ns_win.setHidesOnDeactivate(false);

    if elevated {
        // kCGScreenSaverWindowLevel (1000) renders cleanly over macOS Menu Bar, Notch & Dock.
        ns_win.setLevel(1000);
        ns_win.setFrame_display(target_rect, true);
        let _ = ov.show();
        ns_win.setLevel(1000);
        ns_win.setFrame_display(target_rect, true);
        if make_key {
            ns_win.makeKeyAndOrderFront(None);
            ns_win.orderFrontRegardless();
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            let _ = ov.set_focus();
            let _ = AsRef::<tauri::Webview<_>>::as_ref(ov).set_focus();
        } else {
            ns_win.orderFrontRegardless();
        }
    } else {
        // Temporarily drop to normal window level so NSSavePanel / file picker appears on top.
        ns_win.setLevel(0);
    }

    if emit_ready {
        let _ = ov.emit("zunzshot://session-ready", ());
    }
}

#[cfg(target_os = "macos")]
fn present_macos_overlay_windows(
    app: &AppHandle,
    placements: Vec<MacScreenPlacement>,
    active_screen_idx: usize,
    elevated: bool,
    emit_ready: bool,
) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        // Destroy any stale extra screen overlays if fewer monitors are connected now
        for (idx, win) in all_overlay_windows(&app_handle) {
            if idx >= placements.len() {
                let _ = win.hide();
                let _ = win.destroy();
            }
        }

        let mut active_win: Option<(WebviewWindow, MacScreenPlacement)> = None;
        for placement in &placements {
            if let Some(win) = ensure_overlay_window_for_index(&app_handle, placement.index) {
                if placement.index == active_screen_idx {
                    active_win = Some((win, *placement));
                } else {
                    apply_macos_screen_overlay_window(&win, *placement, elevated, false, emit_ready);
                }
            }
        }

        // Present and focus the cursor's active screen window last so it holds keyboard focus
        if let Some((win, placement)) = active_win {
            apply_macos_screen_overlay_window(&win, placement, elevated, true, emit_ready);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn apply_native_overlay_window(
    ov: &tauri::WebviewWindow,
    phys_x: i32,
    phys_y: i32,
    phys_w: u32,
    phys_h: u32,
    elevated: bool,
    emit_ready: bool,
) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE,
            SWP_NOMOVE, SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_SHOWWINDOW,
        };

        if let Ok(hwnd) = ov.hwnd() {
            let hwnd = HWND(hwnd.0);
            if elevated {
                let _ = ov.set_focusable(true);
                let _ = ov.set_position(Position::Physical(PhysicalPosition::new(
                    phys_x, phys_y,
                )));
                let _ = ov.set_size(Size::Physical(PhysicalSize::new(phys_w, phys_h)));
                let _ = ov.show();
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        phys_x,
                        phys_y,
                        phys_w as i32,
                        phys_h as i32,
                        SWP_SHOWWINDOW | SWP_NOSENDCHANGING,
                    );
                }
                let actual_sc = ov.scale_factor().unwrap_or(1.0).max(1.0);
                let actual_lw = phys_w as f64 / actual_sc;
                let actual_lh = phys_h as f64 / actual_sc;
                {
                    let app_st = ov.app_handle().state::<Mutex<AppState>>();
                    let mut guard = app_st.lock().unwrap();
                    if let Some(ref mut session) = guard.active_session {
                        session.metadata.scale_factor = actual_sc;
                        session.metadata.logical_width = actual_lw;
                        session.metadata.logical_height = actual_lh;
                    }
                }
            } else {
                // Temporarily hide the fullscreen overlay on Windows so native dialogs
                // (Save As, Print, Explorer) are never covered if the user clicks outside them.
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_NOTOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
                    );
                }
                let _ = ov.hide();
            }
            if elevated {
                force_windows_foreground_and_webview_focus(ov);
                let _ = ov.emit("zunzshot://focus-canvas", ());
                let ov_delayed = ov.clone();
                std::thread::spawn(move || {
                    for wait_ms in [35, 75] {
                        std::thread::sleep(Duration::from_millis(wait_ms));
                        let ov_main = ov_delayed.clone();
                        let _ = ov_delayed.run_on_main_thread(move || {
                            let has_rect = LAST_OVERLAY_RECT.lock().unwrap().is_some();
                            let has_session = ov_main
                                .app_handle()
                                .state::<Mutex<AppState>>()
                                .lock()
                                .unwrap()
                                .active_session
                                .is_some();
                            if has_rect && has_session && ov_main.is_visible().unwrap_or(false) {
                                force_windows_foreground_and_webview_focus(&ov_main);
                                let _ = ov_main.emit("zunzshot://focus-canvas", ());
                            }
                        });
                    }
                });
            }
            if emit_ready {
                let _ = ov.emit("zunzshot://session-ready", ());
            }
            return;
        }
    }

    if elevated {
        let _ = ov.set_position(Position::Physical(PhysicalPosition::new(phys_x, phys_y)));
        let _ = ov.set_size(Size::Physical(PhysicalSize::new(phys_w, phys_h)));
        let _ = ov.set_always_on_top(true);
        let _ = ov.set_visible_on_all_workspaces(true);
        let _ = ov.show();
        let _ = ov.set_focus();
        let _ = AsRef::<tauri::Webview<_>>::as_ref(ov).set_focus();
    } else {
        let _ = ov.set_always_on_top(false);
    }
    if emit_ready {
        let _ = ov.emit("zunzshot://session-ready", ());
    }
}

#[cfg(not(target_os = "macos"))]
fn present_overlay_window(
    app: &AppHandle,
    rect: (i32, i32, u32, u32),
    elevated: bool,
    emit_ready: bool,
) {
    let (phys_x, phys_y, phys_w, phys_h) = rect;
    *LAST_OVERLAY_RECT.lock().unwrap() = Some(rect);

    #[cfg(target_os = "windows")]
    {
        if app.get_webview_window("overlay").is_none() {
            if !elevated {
                return;
            }
            let app_handle = app.clone();
            std::thread::spawn(move || {
                if let Some(win) = ensure_overlay_window(&app_handle) {
                    let app_inner = app_handle.clone();
                    let _ = app_handle.run_on_main_thread(move || {
                        let has_rect = LAST_OVERLAY_RECT.lock().unwrap().is_some();
                        let has_session = app_inner
                            .state::<Mutex<AppState>>()
                            .lock()
                            .unwrap()
                            .active_session
                            .is_some();
                        if has_rect && has_session {
                            apply_native_overlay_window(
                                &win, phys_x, phys_y, phys_w, phys_h, elevated, emit_ready,
                            );
                        }
                    });
                }
            });
            return;
        }

        let app_handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if elevated {
                let has_rect = LAST_OVERLAY_RECT.lock().unwrap().is_some();
                let has_session = app_handle
                    .state::<Mutex<AppState>>()
                    .lock()
                    .unwrap()
                    .active_session
                    .is_some();
                if !has_rect || !has_session {
                    return;
                }
            }
            if let Some(win) = app_handle.get_webview_window("overlay") {
                apply_native_overlay_window(
                    &win, phys_x, phys_y, phys_w, phys_h, elevated, emit_ready,
                );
            }
        });
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let app_handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app_handle
                .get_webview_window("overlay")
                .or_else(|| ensure_overlay_window(&app_handle))
            {
                apply_native_overlay_window(&win, phys_x, phys_y, phys_w, phys_h, elevated, emit_ready);
            }
        });
    }
}

#[tauri::command]
fn set_overlay_elevated(app: AppHandle, elevated: bool) {
    #[cfg(target_os = "macos")]
    {
        let (active_idx, placements) = LAST_MACOS_SCREENS.lock().unwrap().clone();
        if !placements.is_empty() {
            present_macos_overlay_windows(&app, placements, active_idx, elevated, false);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let rect = *LAST_OVERLAY_RECT.lock().unwrap();
        if let Some(rect) = rect {
            if app.get_webview_window("overlay").is_some() {
                present_overlay_window(&app, rect, elevated, false);
            }
        }
    }
}

#[tauri::command]
fn focus_overlay_window(app: AppHandle, screen_index: Option<usize>) {
    let app_handle = app.clone();
    let target_label = overlay_window_label(screen_index.unwrap_or(0));
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "windows")]
        {
            let has_rect = LAST_OVERLAY_RECT.lock().unwrap().is_some();
            let has_session = app_handle
                .state::<Mutex<AppState>>()
                .lock()
                .unwrap()
                .active_session
                .is_some();
            if !has_rect || !has_session {
                return;
            }
        }
        if let Some(ov) = app_handle
            .get_webview_window(&target_label)
            .or_else(|| app_handle.get_webview_window("overlay"))
        {
            if ov.is_visible().unwrap_or(false) {
                focus_platform_overlay_window(&ov);
            }
        }
    });
}

fn hide_overlay_on_main_thread(app_handle: &AppHandle, pin_ids: &[String], destroy_ov: bool) {
    #[cfg(target_os = "macos")]
    MACOS_DRAG_ACTIVE.store(false, Ordering::SeqCst);

    for (_, ov) in all_overlay_windows(app_handle) {
        let _ = ov.emit("zunzshot://session-reset", ());
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = ov.hwnd() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE,
            };
            unsafe {
                let _ = SetWindowPos(
                    HWND(hwnd.0),
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
                );
            }
        }
        let _ = ov.hide();
        if destroy_ov {
            #[cfg(target_os = "windows")]
            {
                // Defer WebView2 teardown off the synchronous WebResourceRequested IPC stack.
                let app_destroy = app_handle.clone();
                let label_destroy = ov.label().to_string();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(60));
                    let app_inner = app_destroy.clone();
                    let _ = app_destroy.run_on_main_thread(move || {
                        let can_destroy = {
                            let has_rect = LAST_OVERLAY_RECT.lock().unwrap().is_some();
                            let st = app_inner.state::<Mutex<AppState>>();
                            let guard = st.lock().unwrap();
                            !has_rect
                                && guard.active_session.is_none()
                                && guard.settings.ultra_light_mode
                                && !CAPTURE_IN_PROGRESS.load(Ordering::SeqCst)
                        };
                        if can_destroy {
                            if let Some(ov_win) = app_inner.get_webview_window(&label_destroy) {
                                let _ = ov_win.destroy();
                            }
                        }
                    });
                });
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = ov.destroy();
            }
        }
    }
    // Re-assert pinned floating windows above normal apps once the fullscreen overlay is gone
    for pin_id in pin_ids {
        if let Some(pin_win) = app_handle.get_webview_window(pin_id) {
            #[cfg(target_os = "macos")]
            if let Ok(ptr) = pin_win.ns_window() {
                if !ptr.is_null() {
                    let ns_pin = unsafe { &*(ptr as *const objc2_app_kit::NSWindow) };
                    ns_pin.setHidesOnDeactivate(false);
                    ns_pin.setLevel(5);
                    ns_pin.orderFrontRegardless();
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = pin_win.set_always_on_top(true);
                let _ = pin_win.show();
            }
        }
    }
}

#[tauri::command]
fn close_overlay(app: AppHandle, state: State<'_, Mutex<AppState>>) {
    *LAST_OVERLAY_RECT.lock().unwrap() = None;
    #[cfg(target_os = "macos")]
    {
        MACOS_DRAG_ACTIVE.store(false, Ordering::SeqCst);
        *LAST_MACOS_SCREENS.lock().unwrap() = (0, Vec::new());
    }
    let (ultra_light, pin_ids): (bool, Vec<String>) = {
        let mut guard = state.lock().unwrap();
        guard.active_session = None;
        (
            guard.settings.ultra_light_mode,
            guard.pinned_images.keys().cloned().collect(),
        )
    };
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        hide_overlay_on_main_thread(&app_handle, &pin_ids, ultra_light);
    });
}

fn decode_hex_utf8(hex_str: &str) -> Option<String> {
    let bytes = hex_str.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let (chunks, _) = bytes.as_chunks::<2>();
    for chunk in chunks {
        let s = std::str::from_utf8(chunk).ok()?;
        let b = u8::from_str_radix(s, 16).ok()?;
        out.push(b);
    }
    String::from_utf8(out).ok()
}

fn with_clipboard_retry<T>(
    mut op: impl FnMut(&mut Clipboard) -> Result<T, String>,
) -> Result<T, String> {
    let mut last_err = String::new();
    for attempt in 0..8 {
        match Clipboard::new().map_err(|e| format!("Clipboard error: {e}")) {
            Ok(mut cb) => match op(&mut cb) {
                Ok(val) => return Ok(val),
                Err(err) => last_err = err,
            },
            Err(err) => last_err = err,
        }
        if attempt < 7 {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Err(last_err)
}

fn copy_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<(), String> {
    let decoded = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to decode PNG: {e}"))?
        .into_rgba8();

    let width = decoded.width() as usize;
    let height = decoded.height() as usize;
    let raw = decoded.into_raw();

    with_clipboard_retry(|clipboard| {
        clipboard
            .set_image(ImageData {
                width,
                height,
                bytes: Cow::Borrowed(&raw),
            })
            .map_err(|e| format!("Failed to copy image: {e}"))
    })
}

#[tauri::command]
fn copy_image_to_clipboard(request: Request<'_>) -> Result<(), String> {
    let png_bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        _ => return Err("Expected raw PNG bytes".to_string()),
    };
    copy_png_bytes_to_clipboard(png_bytes)
}

#[tauri::command]
fn copy_text_to_clipboard(text: String) -> Result<(), String> {
    with_clipboard_retry(|clipboard| {
        clipboard
            .set_text(&text)
            .map_err(|e| format!("Failed to copy text: {e}"))
    })
}

pub(crate) fn encode_query_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                use std::fmt::Write;
                let _ = write!(&mut out, "%{b:02X}");
            }
        }
    }
    out
}

#[tauri::command]
fn show_copy_notification(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    title: String,
    subtitle: String,
    close_overlay: Option<bool>,
    screen_index: Option<usize>,
) {
    let should_close = close_overlay.unwrap_or(false);
    let (ultra_light, pin_ids, screen_override): (bool, Vec<String>, Option<MonitorBounds>) = {
        let mut guard = state.lock().unwrap();
        let override_bounds = screen_index.and_then(|idx| {
            let s = guard.active_session.as_ref()?;
            let scr = s.metadata.screens.get(idx)?;
            let global_lx = s.virt_logical_x + scr.lx;
            let global_ly = s.virt_logical_y + scr.ly;
            Some(MonitorBounds {
                phys_x: (global_lx * scr.scale_factor).round() as i32,
                phys_y: (global_ly * scr.scale_factor).round() as i32,
                phys_w: scr.pixel_w,
                phys_h: scr.pixel_h,
                scale: scr.scale_factor,
                display_id: scr.display_id,
                logical_x: global_lx,
                logical_y: global_ly,
                logical_w: scr.lw,
                logical_h: scr.lh,
            })
        });
        if should_close {
            *LAST_OVERLAY_RECT.lock().unwrap() = None;
            #[cfg(target_os = "macos")]
            {
                MACOS_DRAG_ACTIVE.store(false, Ordering::SeqCst);
                *LAST_MACOS_SCREENS.lock().unwrap() = (0, Vec::new());
            }
            guard.active_session = None;
            (
                guard.settings.ultra_light_mode,
                guard.pinned_images.keys().cloned().collect(),
                override_bounds,
            )
        } else {
            (false, Vec::new(), override_bounds)
        }
    };
    if let Some(ov_bounds) = screen_override {
        *LAST_MONITOR_BOUNDS.lock().unwrap() = Some(ov_bounds);
    }

    let gen = NEXT_TOAST_ID.fetch_add(1, Ordering::Relaxed);
    let toast_id = format!("copy-toast-{gen}");
    let prev_id = if gen > 1 {
        Some(format!("copy-toast-{}", gen - 1))
    } else {
        None
    };

    let bounds = LAST_MONITOR_BOUNDS.lock().unwrap().unwrap_or(MonitorBounds {
        phys_x: 0,
        phys_y: 0,
        phys_w: 1440,
        phys_h: 900,
        scale: 1.0,
        display_id: 0,
        logical_x: 0.0,
        logical_y: 0.0,
        logical_w: 1440.0,
        logical_h: 900.0,
    });
    let mon_phys_x = bounds.phys_x;
    let mon_phys_y = bounds.phys_y;
    let mon_phys_w = bounds.phys_w;
    let mon_phys_h = bounds.phys_h;
    let sc = bounds.scale.max(1.0);
    let mon_w = (mon_phys_w as f64 / sc).max(1.0);
    let toast_w = 320.0;
    let toast_h = 62.0;
    let toast_x = mon_phys_x as f64 / sc + ((mon_w - toast_w) / 2.0).max(0.0);
    let toast_y = mon_phys_y as f64 / sc + 38.0;

    let url = format!(
        "toast.html?title={}&sub={}",
        encode_query_component(&title),
        encode_query_component(&subtitle)
    );

    let app_handle = app.clone();
    let id_for_create = toast_id.clone();

    #[cfg(target_os = "windows")]
    {
        if should_close {
            let app_hide = app.clone();
            let _ = app.run_on_main_thread(move || {
                hide_overlay_on_main_thread(&app_hide, &pin_ids, ultra_light);
            });
        }

        std::thread::spawn(move || {
            let _ = mon_phys_h;
            if let Some(old_id) = prev_id {
                if let Some(old_win) = app_handle.get_webview_window(&old_id) {
                    let _ = old_win.close();
                }
            }

            if let Ok(win) = WebviewWindowBuilder::new(
                &app_handle,
                &id_for_create,
                WebviewUrl::App(url.into()),
            )
            .title("ZunzShot")
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .focused(false)
            .inner_size(toast_w, toast_h)
            .position(toast_x, toast_y)
            .visible(false)
            .build()
            {
                let phys_toast_x = mon_phys_x
                    + (((mon_phys_w as f64 - toast_w * sc) / 2.0).max(0.0)).round() as i32;
                let phys_toast_y = mon_phys_y + (38.0 * sc).round() as i32;
                let phys_toast_w = (toast_w * sc).round().max(1.0) as i32;
                let phys_toast_h = (toast_h * sc).round().max(1.0) as i32;

                let win_for_show = win.clone();
                let _ = win.run_on_main_thread(move || {
                    use windows::Win32::Foundation::HWND;
                    use windows::Win32::UI::WindowsAndMessaging::{
                        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                    };

                    let _ = win_for_show.set_ignore_cursor_events(true);
                    if let Ok(hwnd) = win_for_show.hwnd() {
                        unsafe {
                            let _ = SetWindowPos(
                                HWND(hwnd.0),
                                Some(HWND_TOPMOST),
                                phys_toast_x,
                                phys_toast_y,
                                phys_toast_w,
                                phys_toast_h,
                                SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                        }
                    }
                });

                std::thread::sleep(Duration::from_millis(1850));
                let app_for_close = app_handle.clone();
                let id_for_close = id_for_create.clone();
                let _ = app_handle.run_on_main_thread(move || {
                    if let Some(w) = app_for_close.get_webview_window(&id_for_close) {
                        let _ = w.close();
                    }
                });
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    let _ = app.run_on_main_thread(move || {
        let _ = (mon_phys_h, mon_phys_w, sc, toast_w, toast_h);
        // Hide the fullscreen overlay BEFORE creating the toast window so the desktop
        // becomes responsive immediately on Windows even before WebView2 initializes the toast.
        if should_close {
            hide_overlay_on_main_thread(&app_handle, &pin_ids, ultra_light);
        }
        if let Some(old_id) = prev_id {
            if let Some(old_win) = app_handle.get_webview_window(&old_id) {
                let _ = old_win.close();
            }
        }

        if let Ok(win) =
            WebviewWindowBuilder::new(&app_handle, &id_for_create, WebviewUrl::App(url.into()))
                .title("ZunzShot")
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .focused(false)
                .inner_size(toast_w, toast_h)
                .position(toast_x, toast_y)
                .visible(false)
                .build()
        {
            let _ = win.set_ignore_cursor_events(true);

            #[cfg(target_os = "macos")]
            {
                use objc2::MainThreadMarker;
                use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
                use objc2_foundation::NSPoint;

                if let Some(mtm) = MainThreadMarker::new() {
                    let sf = ns_screen_rect_for_display(
                        mtm,
                        bounds.display_id,
                        bounds.logical_x,
                        bounds.logical_y,
                        bounds.logical_w,
                        bounds.logical_h,
                    );
                    if let Ok(ns_win_ptr) = win.ns_window() {
                        if !ns_win_ptr.is_null() {
                            let ns_win = unsafe { &*(ns_win_ptr as *const NSWindow) };
                            let cocoa_x = sf.origin.x + ((sf.size.width - toast_w) / 2.0).max(0.0);
                            let cocoa_y = sf.origin.y + sf.size.height - 38.0 - toast_h;
                            ns_win.setFrameOrigin(NSPoint::new(cocoa_x, cocoa_y));
                            ns_win.setHidesOnDeactivate(false);
                            ns_win.setIgnoresMouseEvents(true);
                            ns_win.setCollectionBehavior(
                                NSWindowCollectionBehavior::CanJoinAllSpaces
                                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                                    | NSWindowCollectionBehavior::IgnoresCycle,
                            );
                            ns_win.setLevel(1000);
                            ns_win.orderFrontRegardless();
                        }
                    }
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                let _ = win.show();
            }

            let app_for_close = app_handle.clone();
            let id_for_close = id_for_create.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1850));
                let app_inner = app_for_close.clone();
                let _ = app_for_close.run_on_main_thread(move || {
                    if let Some(w) = app_inner.get_webview_window(&id_for_close) {
                        let _ = w.close();
                    }
                });
            });
        }
    });
}

#[tauri::command]
fn save_image_to_disk(
    state: State<'_, Mutex<AppState>>,
    request: Request<'_>,
) -> Result<String, String> {
    let png_bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        _ => return Err("Expected raw PNG bytes".to_string()),
    };

    let headers = request.headers();
    let custom_path = headers
        .get("x-custom-path-hex")
        .and_then(|v| v.to_str().ok())
        .and_then(decode_hex_utf8)
        .filter(|s| !s.is_empty());
    let format = headers
        .get("x-output-format")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let (default_folder, default_format) = {
        let guard = state.lock().unwrap();
        (
            guard.settings.save_folder.clone(),
            guard.settings.output_format.clone(),
        )
    };

    let fmt = format.unwrap_or(default_format).to_lowercase();
    let target_path = if let Some(custom) = custom_path {
        PathBuf::from(custom)
    } else {
        let dir = PathBuf::from(default_folder);
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|e| format!("Could not create save folder: {e}"))?;
        }
        let ext = if fmt == "jpg" || fmt == "jpeg" {
            "jpg"
        } else {
            "png"
        };
        let stamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        dir.join(format!("ZunzShot_{stamp}.{ext}"))
    };

    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let ext_lower = target_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png")
        .to_lowercase();

    if ext_lower == "jpg" || ext_lower == "jpeg" {
        let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let rgb = img.into_rgb8();
        rgb.save_with_format(&target_path, image::ImageFormat::Jpeg)
            .map_err(|e| format!("Failed saving JPEG: {e}"))?;
    } else {
        fs::write(&target_path, png_bytes).map_err(|e| format!("Failed saving PNG: {e}"))?;
    }

    Ok(target_path.to_string_lossy().to_string())
}

fn write_temp_png(png_bytes: &[u8], prefix: &str) -> Result<PathBuf, String> {
    let stamp = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let dir = std::env::temp_dir().join("ZunzShot");
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Could not prepare temp folder: {e}"))?;
    } else if let Ok(entries) = fs::read_dir(&dir) {
        let max_age = Duration::from_secs(600);
        for entry in entries.flatten() {
            let p = entry.path();
            let is_png = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("png"))
                .unwrap_or(false);
            if is_png {
                let stale = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .map(|age| age > max_age)
                    .unwrap_or(false);
                if stale {
                    let _ = fs::remove_file(p);
                }
            }
        }
    }
    let path = dir.join(format!("{prefix}_{stamp}.png"));
    fs::write(&path, png_bytes).map_err(|e| format!("Failed writing temp PNG: {e}"))?;
    Ok(path)
}

#[tauri::command]
fn share_image(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let png_bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        _ => return Err("Expected raw PNG bytes".to_string()),
    };

    let headers = request.headers();
    let parse_f64 = |key: &str, fallback: f64| -> f64 {
        headers
            .get(key)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(fallback)
    };
    let anchor_x = parse_f64("x-anchor-x", 120.0);
    let anchor_y = parse_f64("x-anchor-y", 120.0);
    let anchor_w = parse_f64("x-anchor-w", 28.0);
    let anchor_h = parse_f64("x-anchor-h", 28.0);
    let screen_index = parse_f64("x-screen-index", 0.0).max(0.0) as usize;

    let temp_path = write_temp_png(png_bytes, "ZunzShot_Share")?;

    #[cfg(target_os = "macos")]
    {
        let app_handle = app.clone();
        let path_str = temp_path.to_string_lossy().to_string();
        let _ = app.run_on_main_thread(move || {
            use objc2::{AnyThread, MainThreadMarker};
            use objc2_app_kit::{NSApplication, NSSharingServicePicker, NSWindow};
            use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};

            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            for (_, win) in all_overlay_windows(&app_handle) {
                if let Ok(ptr) = win.ns_window() {
                    if !ptr.is_null() {
                        unsafe { (*(ptr as *const NSWindow)).setLevel(0) };
                    }
                }
            }
            let Some(ov) = app_handle
                .get_webview_window(&overlay_window_label(screen_index))
                .or_else(|| app_handle.get_webview_window("overlay"))
            else {
                return;
            };
            let Ok(ns_win_ptr) = ov.ns_window() else {
                return;
            };
            if ns_win_ptr.is_null() {
                return;
            }
            let ns_win = unsafe { &*(ns_win_ptr as *const NSWindow) };
            // Lower window level so macOS Sharing Picker popup & target apps (AirDrop, Mail, Messages) stay visible on top.
            ns_win.setLevel(0);
            ns_win.makeKeyAndOrderFront(None);
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

            let Some(content_view) = ns_win.contentView() else {
                return;
            };
            let frame = content_view.frame();
            let ns_y = if content_view.isFlipped() {
                anchor_y
            } else {
                (frame.size.height - (anchor_y + anchor_h)).max(0.0)
            };
            let target_rect = NSRect::new(
                NSPoint::new(anchor_x.max(0.0), ns_y),
                NSSize::new(anchor_w.max(1.0), anchor_h.max(1.0)),
            );

            let ns_path = NSString::from_str(&path_str);
            let url = NSURL::fileURLWithPath(&ns_path);
            let urls = NSArray::from_retained_slice(&[url]);
            let items: &NSArray = unsafe { &*(&*urls as *const NSArray<NSURL> as *const NSArray) };
            let picker = unsafe {
                NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), items)
            };
            picker.showRelativeToRect_ofView_preferredEdge(
                target_rect,
                &content_view,
                NSRectEdge::MinY,
            );
            let _ = picker;
        });
    }

    #[cfg(target_os = "windows")]
    {
        let _ = (anchor_x, anchor_y, anchor_w, anchor_h);
        if let Some(rect) = *LAST_OVERLAY_RECT.lock().unwrap() {
            present_overlay_window(&app, rect, false, false);
        }
        let arg = format!("/select,{}", temp_path.to_string_lossy());
        std::process::Command::new("explorer")
            .arg(arg)
            .spawn()
            .map_err(|e| format!("Failed opening File Explorer: {e}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, temp_path, anchor_x, anchor_y, anchor_w, anchor_h, screen_index);
    }

    Ok(())
}

#[tauri::command]
fn print_image(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let png_bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        _ => return Err("Expected raw PNG bytes".to_string()),
    };

    let temp_path = write_temp_png(png_bytes, "ZunzShot_Print")?;

    #[cfg(target_os = "macos")]
    {
        let app_handle = app.clone();
        let path_for_print = temp_path.clone();
        let _ = app.run_on_main_thread(move || {
            use objc2::{AnyThread, MainThreadMarker};
            use objc2_app_kit::{
                NSApplication, NSImage, NSImageView, NSPrintInfo, NSPrintOperation,
                NSPrintingPaginationMode, NSWindow,
            };
            use objc2_foundation::{NSCopying, NSPoint, NSRect, NSSize, NSString};

            let Some(mtm) = MainThreadMarker::new() else {
                let _ = app_handle.emit("zunzshot://print-result", false);
                return;
            };
            let ns_wins: Vec<&NSWindow> = all_overlay_windows(&app_handle)
                .into_iter()
                .filter_map(|(_, ov)| {
                    ov.ns_window()
                        .ok()
                        .filter(|p| !p.is_null())
                        .map(|p| unsafe { &*(p as *const NSWindow) })
                })
                .collect();

            for ns_win in &ns_wins {
                // Lower overlay below system Print Panel
                ns_win.setLevel(0);
            }
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

            let path_str = path_for_print.to_string_lossy().to_string();
            let ns_path = NSString::from_str(&path_str);
            let printed = if let Some(img) = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path)
            {
                let size = img.size();
                let image_view = NSImageView::imageViewWithImage(&img, mtm);
                image_view.setFrame(NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(size.width.max(1.0), size.height.max(1.0)),
                ));

                let print_info = NSPrintInfo::sharedPrintInfo().copy();
                print_info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
                print_info.setVerticalPagination(NSPrintingPaginationMode::Fit);
                print_info.setHorizontallyCentered(true);
                print_info.setVerticallyCentered(true);

                let op = NSPrintOperation::printOperationWithView_printInfo(&image_view, &print_info);
                op.setShowsPrintPanel(true);
                op.setShowsProgressPanel(true);
                op.runOperation()
            } else {
                false
            };

            let _ = fs::remove_file(&path_for_print);

            if !printed {
                for ns_win in &ns_wins {
                    ns_win.setLevel(1000);
                }
                if let Some(first) = ns_wins.first() {
                    first.makeKeyAndOrderFront(None);
                }
            }
            let _ = app_handle.emit("zunzshot://print-result", printed);
        });
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        if let Some(rect) = *LAST_OVERLAY_RECT.lock().unwrap() {
            present_overlay_window(&app, rect, false, false);
        }
        let app_handle = app.clone();
        std::thread::spawn(move || {
            let escaped = temp_path.to_string_lossy().replace('\'', "''");
            let script = format!(
                "try {{ Start-Process -FilePath '{escaped}' -Verb Print -Wait -ErrorAction Stop }} catch {{ Start-Process -FilePath 'mspaint.exe' -ArgumentList @('/p', '{escaped}') -ErrorAction Stop }}"
            );
            let status = std::process::Command::new("powershell")
                .creation_flags(CREATE_NO_WINDOW)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Sta",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &script,
                ])
                .status();
            let ok = status.map(|s| s.success()).unwrap_or(false);
            if !ok {
                let has_session = app_handle
                    .state::<Mutex<AppState>>()
                    .lock()
                    .unwrap()
                    .active_session
                    .is_some();
                if has_session {
                    if let Some(rect) = *LAST_OVERLAY_RECT.lock().unwrap() {
                        present_overlay_window(&app_handle, rect, true, false);
                    }
                }
            }
            let _ = app_handle.emit("zunzshot://print-result", ok);
            std::thread::sleep(Duration::from_secs(60));
            let _ = fs::remove_file(&temp_path);
        });
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = fs::remove_file(&temp_path);
        let _ = app.emit("zunzshot://print-result", false);
    }

    Ok(())
}

#[tauri::command]
fn pin_capture(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    request: Request<'_>,
) -> Result<String, String> {
    let png_bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        _ => return Err("Expected raw PNG bytes".to_string()),
    };

    let headers = request.headers();
    let parse_f64 = |key: &str, fallback: f64| -> f64 {
        headers
            .get(key)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(fallback)
    };
    let sel_x = parse_f64("x-pin-x", 120.0);
    let sel_y = parse_f64("x-pin-y", 120.0);
    let sel_w = parse_f64("x-pin-w", 320.0).max(64.0);
    let sel_h = parse_f64("x-pin-h", 200.0).max(48.0);

    let pin_id = format!("pin-{}", NEXT_PIN_ID.fetch_add(1, Ordering::Relaxed));
    let (lang, mon_phys_x, mon_phys_y, virt_lx, virt_ly, mon_w, mon_h, sc) = {
        let mut guard = state.lock().unwrap();
        guard.pinned_images.insert(pin_id.clone(), png_bytes.to_vec());
        let (mx, my, vlx, vly, mw, mh, scale) = match guard.active_session.as_ref() {
            Some(s) => (
                s.monitor_phys_x,
                s.monitor_phys_y,
                s.virt_logical_x,
                s.virt_logical_y,
                s.metadata.logical_width.max(1.0),
                s.metadata.logical_height.max(1.0),
                s.metadata.scale_factor.max(1.0),
            ),
            None => (0, 0, 0.0, 0.0, 1440.0, 900.0, 1.0),
        };
        (guard.settings.language.clone(), mx, my, vlx, vly, mw, mh, scale)
    };

    let w = sel_w.min(mon_w);
    let h = sel_h.min(mon_h);
    let clamped_x = sel_x.clamp(0.0, (mon_w - w).max(0.0));
    let clamped_y = sel_y.clamp(0.0, (mon_h - h).max(0.0));
    #[cfg(target_os = "windows")]
    let fallback_x = mon_phys_x as f64 / sc + clamped_x;
    #[cfg(target_os = "windows")]
    let fallback_y = mon_phys_y as f64 / sc + clamped_y;
    #[cfg(not(target_os = "windows"))]
    let fallback_x = virt_lx + clamped_x;
    #[cfg(not(target_os = "windows"))]
    let fallback_y = virt_ly + clamped_y;

    #[cfg(target_os = "windows")]
    {
        let _ = (sel_x, sel_y, sel_w, sel_h, mon_w, mon_h, virt_lx, virt_ly);
        let app_handle = app.clone();
        let id_for_win = pin_id.clone();
        std::thread::spawn(move || {
            let url = format!("pin.html?id={id_for_win}&lang={lang}");
            match WebviewWindowBuilder::new(&app_handle, &id_for_win, WebviewUrl::App(url.into()))
                .title("ZunzShot Pin")
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .resizable(true)
                .skip_taskbar(true)
                .shadow(true)
                .inner_size(w, h)
                .min_inner_size(64.0, 48.0)
                .position(fallback_x, fallback_y)
                .visible(false)
                .build()
            {
                Ok(win) => {
                    let app_for_destroy = app_handle.clone();
                    let id_for_destroy = id_for_win.clone();
                    win.on_window_event(move |event| {
                        if let WindowEvent::Destroyed = event {
                            let st = app_for_destroy.state::<Mutex<AppState>>();
                            st.lock().unwrap().pinned_images.remove(&id_for_destroy);
                        }
                    });
                    let phys_pin_x = mon_phys_x + (clamped_x * sc).round() as i32;
                    let phys_pin_y = mon_phys_y + (clamped_y * sc).round() as i32;
                    let phys_pin_w = (w * sc).round().max(64.0) as u32;
                    let phys_pin_h = (h * sc).round().max(48.0) as u32;
                    let win_for_show = win.clone();
                    let _ = win.run_on_main_thread(move || {
                        let _ = win_for_show.set_position(Position::Physical(
                            PhysicalPosition::new(phys_pin_x, phys_pin_y),
                        ));
                        let _ = win_for_show
                            .set_size(Size::Physical(PhysicalSize::new(phys_pin_w, phys_pin_h)));
                        let _ = win_for_show.show();
                    });
                }
                Err(err) => {
                    eprintln!("Failed creating pin window {id_for_win}: {err}");
                    let st = app_handle.state::<Mutex<AppState>>();
                    st.lock().unwrap().pinned_images.remove(&id_for_win);
                }
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (mon_phys_x, mon_phys_y, sc, sel_x, sel_y, sel_w, sel_h);
        let app_handle = app.clone();
        let id_for_win = pin_id.clone();
        let _ = app.run_on_main_thread(move || {
            let top_left_x = fallback_x;
            let top_left_y = fallback_y;
            let win_w = w;
            let win_h = h;
            #[cfg(target_os = "macos")]
            let mut cocoa_frame_opt: Option<objc2_foundation::NSRect> = None;

            #[cfg(target_os = "macos")]
            {
                use objc2::MainThreadMarker;
                use objc2_app_kit::NSScreen;
                use objc2_foundation::{NSPoint, NSRect, NSSize};

                if let Some(mtm) = MainThreadMarker::new() {
                    let screens = NSScreen::screens(mtm);
                    let primary_h = if !screens.is_empty() {
                        screens.objectAtIndex(0).frame().size.height
                    } else {
                        mon_h.max(900.0)
                    };
                    let cocoa_y = primary_h - (fallback_y + win_h);
                    cocoa_frame_opt = Some(NSRect::new(
                        NSPoint::new(fallback_x, cocoa_y),
                        NSSize::new(win_w, win_h),
                    ));
                }
            }

            let url = format!("pin.html?id={id_for_win}&lang={lang}");
            match WebviewWindowBuilder::new(&app_handle, &id_for_win, WebviewUrl::App(url.into()))
                .title("ZunzShot Pin")
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .resizable(true)
                .skip_taskbar(true)
                .shadow(true)
                .inner_size(win_w, win_h)
                .min_inner_size(64.0, 48.0)
                .position(top_left_x, top_left_y)
                .visible(false)
                .build()
            {
                Ok(win) => {
                    #[cfg(target_os = "macos")]
                    {
                        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

                        if let Ok(ns_win_ptr) = win.ns_window() {
                            if !ns_win_ptr.is_null() {
                                let ns_win = unsafe { &*(ns_win_ptr as *const NSWindow) };
                                // Must disable hidesOnDeactivate under ActivationPolicy::Accessory so the
                                // pinned screenshot stays floating when switching to any other macOS app.
                                ns_win.setHidesOnDeactivate(false);
                                ns_win.setCollectionBehavior(
                                    NSWindowCollectionBehavior::CanJoinAllSpaces
                                        | NSWindowCollectionBehavior::FullScreenAuxiliary,
                                );
                                ns_win.setLevel(5);
                                if let Some(frame) = cocoa_frame_opt {
                                    ns_win.setFrame_display(frame, true);
                                }
                                let _ = win.show();
                                ns_win.orderFrontRegardless();
                            }
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = win.show();
                    }
                    let app_for_destroy = app_handle.clone();
                    let id_for_destroy = id_for_win.clone();
                    win.on_window_event(move |event| {
                        if let WindowEvent::Destroyed = event {
                            let st = app_for_destroy.state::<Mutex<AppState>>();
                            st.lock().unwrap().pinned_images.remove(&id_for_destroy);
                        }
                    });
                }
                Err(err) => {
                    eprintln!("Failed creating pin window {id_for_win}: {err}");
                    let st = app_handle.state::<Mutex<AppState>>();
                    st.lock().unwrap().pinned_images.remove(&id_for_win);
                }
            }
        });
    }

    Ok(pin_id)
}

#[tauri::command]
fn get_pinned_png(state: State<'_, Mutex<AppState>>, id: String) -> Result<Response, String> {
    let bytes = state
        .lock()
        .unwrap()
        .pinned_images
        .get(&id)
        .cloned()
        .ok_or_else(|| "Pinned image not found".to_string())?;
    Ok(Response::new(bytes))
}

#[tauri::command]
fn copy_pinned_to_clipboard(state: State<'_, Mutex<AppState>>, id: String) -> Result<(), String> {
    let bytes = state
        .lock()
        .unwrap()
        .pinned_images
        .get(&id)
        .cloned()
        .ok_or_else(|| "Pinned image not found".to_string())?;
    copy_png_bytes_to_clipboard(&bytes)
}

#[tauri::command]
fn close_pinned_window(app: AppHandle, state: State<'_, Mutex<AppState>>, id: String) {
    state.lock().unwrap().pinned_images.remove(&id);
    if let Some(win) = app.get_webview_window(&id) {
        let _ = win.close();
    }
}

#[tauri::command]
fn consume_pending_tab(state: State<'_, Mutex<AppState>>) -> Option<String> {
    state.lock().unwrap().pending_tab.take()
}

#[tauri::command]
fn consume_pending_updater_launch(
    state: State<'_, Mutex<AppState>>,
) -> Option<UpdaterLaunchPayload> {
    state.lock().unwrap().pending_updater_launch.take()
}

#[tauri::command]
fn set_auto_check_updates_cmd(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    enabled: bool,
) -> Result<AppSettings, String> {
    if !has_screen_capture_permission() {
        return Err("Screen recording permission is required on macOS".to_string());
    }

    let (prev_settings, updated) = {
        let mut guard = state.lock().unwrap();
        let prev = guard.settings.clone();
        guard.settings.auto_check_updates = enabled;
        (prev, guard.settings.clone())
    };

    if let Err(err) = save_settings(&app, &updated) {
        let mut guard = state.lock().unwrap();
        guard.settings = prev_settings;
        return Err(err);
    }

    let _ = app.emit("zunzshot://auto-updates-updated", enabled);
    if enabled {
        trigger_update_sensor(&app, 1);
    } else {
        signal_update_sensor();
    }
    Ok(updated)
}

#[tauri::command]
fn consume_pending_capture_error(state: State<'_, Mutex<AppState>>) -> Option<String> {
    state.lock().unwrap().pending_capture_error.take()
}

#[tauri::command]
fn check_screen_capture_permission(app: AppHandle) -> bool {
    sync_permission_state(&app)
}

#[tauri::command]
fn request_screen_capture_permission(app: AppHandle) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let stamp = current_binary_stamp();
        let already_prompted = tcc_prompted_stamp_path(&app)
            .and_then(|p| fs::read_to_string(p).ok())
            .as_deref()
            == Some(stamp.as_str());

        if !already_prompted {
            if let Some(p) = tcc_prompted_stamp_path(&app) {
                if let Some(parent) = p.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(p, &stamp);
            }
            // Trigger macOS's native dialog ([Open System Settings] / [Deny]) alone so
            // System Settings only opens when the user clicks the button in that prompt.
            let _ = objc2_core_graphics::CGRequestScreenCaptureAccess();
        } else {
            // macOS only displays CGRequestScreenCaptureAccess() once per TCC entry;
            // subsequent clicks open the Screen Recording pane directly.
            open_mac_screen_recording_pane()?;
        }
    }
    #[cfg(windows)]
    {
        // Windows does not gate desktop screen capture behind a privacy lock
    }
    Ok(sync_permission_state(&app))
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    SINGLE_INSTANCE_SHUTTING_DOWN.store(true, Ordering::SeqCst);
    ALLOW_APP_EXIT.store(true, Ordering::Relaxed);
    app.restart();
}

#[tauri::command]
fn open_screen_recording_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        open_mac_screen_recording_pane()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("ms-settings:privacy")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn signal_update_sensor() {
    let (lock, cvar) = &*UPDATE_SENSOR_SIGNAL;
    if let Ok(mut seq) = lock.lock() {
        *seq = seq.wrapping_add(1);
        cvar.notify_all();
    }
}

fn wait_sensor_interval(duration: Duration) -> bool {
    let (lock, cvar) = &*UPDATE_SENSOR_SIGNAL;
    let Ok(guard) = lock.lock() else {
        std::thread::sleep(duration);
        return false;
    };
    let start_seq = *guard;
    let deadline = std::time::Instant::now() + duration;
    let mut current = guard;
    while !SINGLE_INSTANCE_SHUTTING_DOWN.load(Ordering::Relaxed) && *current == start_seq {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return false;
        };
        if remaining.is_zero() {
            return false;
        }
        let Ok((next_guard, timeout)) = cvar.wait_timeout(current, remaining) else {
            return false;
        };
        current = next_guard;
        if timeout.timed_out() {
            return false;
        }
    }
    true
}

const UPDATE_CHECK_INTERVAL_SECS: u64 = 30 * 60;
const UPDATE_DISMISS_COOLDOWN_SECS: u64 = 4 * 3600;
const SNOOZE_BOUNDARY_GRACE_SECS: u64 = 5;

fn normalize_snooze_hours(hours: Option<u64>) -> u64 {
    match hours.unwrap_or(4) {
        2 | 4 | 6 => hours.unwrap_or(4),
        _ => 4,
    }
}

fn snooze_still_active(elapsed: Duration, duration_secs: u64) -> bool {
    elapsed + Duration::from_secs(SNOOZE_BOUNDARY_GRACE_SECS) < Duration::from_secs(duration_secs)
}

fn next_sensor_sleep_duration(postponed: Option<(Duration, u64)>) -> Duration {
    let default_interval = Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS);
    let Some((elapsed, snooze_secs)) = postponed else {
        return default_interval;
    };
    let Some(remaining) = Duration::from_secs(snooze_secs).checked_sub(elapsed) else {
        return default_interval;
    };
    if remaining > Duration::from_secs(SNOOZE_BOUNDARY_GRACE_SECS) && remaining < default_interval {
        remaining
    } else {
        default_interval
    }
}

#[tauri::command]
fn postpone_update_reminder(
    state: State<'_, Mutex<AppState>>,
    version: Option<String>,
    hours: Option<u64>,
) -> u64 {
    let snooze_hours = normalize_snooze_hours(hours);
    let mut guard = state.lock().unwrap();
    if guard.pending_tab.as_deref() == Some("update-prompt") {
        guard.pending_tab = None;
    }
    guard.pending_updater_launch = None;
    let target_ver = version
        .filter(|v| !v.trim().is_empty())
        .or_else(|| guard.notified_update.as_ref().map(|(v, _)| v.clone()));
    // Clear notified_update so explicit 2h / 4h / 6h snooze governs the next alert accurately.
    guard.notified_update = None;
    if let Some(ver) = target_ver {
        guard.postponed_update = Some((ver, std::time::Instant::now(), snooze_hours * 3600));
    }
    snooze_hours
}

fn trigger_update_sensor(app: &AppHandle, initial_delay_secs: u64) {
    if UPDATE_SENSOR_STARTED.swap(true, Ordering::SeqCst) {
        signal_update_sensor();
        return;
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if initial_delay_secs > 0 {
            let _ = tauri::async_runtime::spawn_blocking(move || {
                wait_sensor_interval(Duration::from_secs(initial_delay_secs));
            })
            .await;
        }

        while !SINGLE_INSTANCE_SHUTTING_DOWN.load(Ordering::Relaxed) {
            let updater_already_open = handle
                .get_webview_window("updater")
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false);

            let can_check = !updater_already_open
                && handle
                    .try_state::<Mutex<AppState>>()
                    .and_then(|s| {
                        s.lock()
                            .ok()
                            .map(|g| g.settings.auto_check_updates && g.active_session.is_none())
                    })
                    .unwrap_or(false);

            if can_check {
                if let Ok(updater) = handle.updater() {
                    if let Ok(Some(update)) = updater.check().await {
                        let should_notify = handle
                            .try_state::<Mutex<AppState>>()
                            .and_then(|s| {
                                s.lock().ok().map(|mut guard| {
                                    let visible_now = handle
                                        .get_webview_window("updater")
                                        .and_then(|w| w.is_visible().ok())
                                        .unwrap_or(false);
                                    if guard.active_session.is_some() || visible_now {
                                        return false;
                                    }
                                    let snoozed = guard.postponed_update.as_ref().is_some_and(
                                        |(ver, at, snooze_secs)| {
                                            ver == &update.version
                                                && snooze_still_active(at.elapsed(), *snooze_secs)
                                        },
                                    );
                                    let recently_notified = guard
                                        .notified_update
                                        .as_ref()
                                        .is_some_and(|(ver, at)| {
                                            ver == &update.version
                                                && snooze_still_active(
                                                    at.elapsed(),
                                                    UPDATE_DISMISS_COOLDOWN_SECS,
                                                )
                                        });
                                    if snoozed || recently_notified {
                                        return false;
                                    }
                                    guard.notified_update =
                                        Some((update.version.clone(), std::time::Instant::now()));
                                    true
                                })
                            })
                            .unwrap_or(false);

                        if should_notify {
                            show_updater_window(
                                &handle,
                                "alert",
                                Some(PendingUpdatePreview {
                                    version: update.version.clone(),
                                    body: update.body.clone(),
                                }),
                            );
                        }
                    }
                }
            }

            let postponed_state = handle
                .try_state::<Mutex<AppState>>()
                .and_then(|s| s.lock().ok().and_then(|g| g.postponed_update.as_ref().map(|(_, at, secs)| (at.elapsed(), *secs))));
            let sleep_for = next_sensor_sleep_duration(postponed_state);
            let _ = tauri::async_runtime::spawn_blocking(move || {
                wait_sensor_interval(sleep_for);
            })
            .await;
        }
    });
}

fn quit_application(app: &AppHandle) {
    SINGLE_INSTANCE_SHUTTING_DOWN.store(true, Ordering::SeqCst);
    ALLOW_APP_EXIT.store(true, Ordering::SeqCst);
    signal_update_sensor();
    let _ = app.global_shortcut().unregister_all();
    *LAST_OVERLAY_RECT.lock().unwrap() = None;
    {
        let state = app.state::<Mutex<AppState>>();
        let mut guard = state.lock().unwrap();
        guard.active_session = None;
        let _ = guard.tray.take();
    }
    #[cfg(target_os = "windows")]
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(250));
        std::process::exit(0);
    });
    app.exit(0);
}

#[tauri::command]
fn get_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "linux"
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let single_instance_listener = match acquire_single_instance_listener() {
        SingleInstanceOutcome::DuplicateSignaled => return,
        SingleInstanceOutcome::Primary(listener) => listener,
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let mode_opt = {
                            let state = app.state::<Mutex<AppState>>();
                            let guard = state.lock().unwrap();
                            guard.shortcut_modes.get(&shortcut.id()).cloned()
                        };
                        if let Some(mode) = mode_opt {
                            trigger_capture(app.clone(), mode);
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings_cmd,
            set_language_cmd,
            complete_onboarding_cmd,
            set_shortcuts_paused,
            get_failed_shortcuts,
            start_capture,
            get_capture_metadata,
            get_capture_rgba,
            get_capture_region_rgba,
            set_macos_drag_monitor,
            set_overlay_elevated,
            focus_overlay_window,
            close_overlay,
            copy_image_to_clipboard,
            copy_text_to_clipboard,
            show_copy_notification,
            save_image_to_disk,
            share_image,
            print_image,
            pin_capture,
            get_pinned_png,
            copy_pinned_to_clipboard,
            close_pinned_window,
            consume_pending_tab,
            consume_pending_updater_launch,
            consume_pending_capture_error,
            set_auto_check_updates_cmd,
            postpone_update_reminder,
            open_screen_recording_settings,
            check_screen_capture_permission,
            request_screen_capture_permission,
            restart_app,
            get_platform,
            get_app_version,
        ])
        .setup(move |app| {
            if let Some(listener) = single_instance_listener {
                spawn_single_instance_responder(listener, app.handle().clone());
            }

            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                cleanup_stale_tcc_on_reinstall(app.handle());
            }

            let initial_settings = load_settings(app.handle());
            let lang = Lang::from_code(&initial_settings.language);
            let show_welcome =
                !initial_settings.language_selected || !initial_settings.updater_prompt_shown;
            let initial_perm = has_screen_capture_permission();
            LAST_PERM_STATE.store(initial_perm, std::sync::atomic::Ordering::Relaxed);

            app.manage(Mutex::new(AppState {
                settings: initial_settings.clone(),
                active_session: None,
                tray: None,
                #[cfg(target_os = "macos")]
                paused_app_menu: None,
                shortcut_modes: HashMap::new(),
                failed_shortcuts: Vec::new(),
                pending_tab: None,
                pending_updater_launch: None,
                pending_capture_error: None,
                pinned_images: HashMap::new(),
                postponed_update: None,
                notified_update: None,
            }));

            let tray_menu = build_tray_menu(app.handle(), lang)?;

            #[cfg(target_os = "macos")]
            let tray_icon_img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-mac.png"))?;
            #[cfg(not(target_os = "macos"))]
            let tray_icon_img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-win.png"))?;

            let tray_builder = TrayIconBuilder::new()
                .icon(tray_icon_img)
                .tooltip("ZunzShot")
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    let id = event.id().as_ref();
                    if id == "quit" {
                        quit_application(app);
                        return;
                    }
                    if id == "grant_permission" || !sync_permission_state(app) {
                        show_settings_window(app);
                        let _ = app.emit("zunzshot://permission-updated", false);
                        return;
                    }
                    match id {
                        "capture_region" => trigger_capture(app.clone(), "region".to_string()),
                        "capture_quick_copy" => {
                            trigger_capture(app.clone(), "quickCopy".to_string())
                        }
                        "capture_circle" => trigger_capture(app.clone(), "circle".to_string()),
                        "capture_window" => trigger_capture(app.clone(), "window".to_string()),
                        "capture_fullscreen" => {
                            trigger_capture(app.clone(), "fullscreen".to_string())
                        }
                        "open_settings" => open_settings_tab(app, "general"),
                        "check_updates" => show_updater_window(app, "manual", None),
                        "about" => open_settings_tab(app, "about"),
                        _ => {}
                    }
                });

            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder.icon_as_template(true);

            let tray = tray_builder.build(app)?;
            {
                let state = app.state::<Mutex<AppState>>();
                state.lock().unwrap().tray = Some(tray);
            }

            register_shortcuts(app.handle(), &initial_settings);

            let pending_settings_window = show_welcome || !initial_perm;
            let splash_url = splash::resolve_startup_splash_url(app.handle(), &initial_settings);
            let has_splash = splash_url.is_some();

            // Launch startup splash (if enabled and not in Ultra-Light Mode) before overlay pre-warm
            // on Windows so WebView2 initializes sequentially without competing at process startup.
            #[cfg(target_os = "windows")]
            {
                let ov_handle = app.handle().clone();
                let preload_overlay = !initial_settings.ultra_light_mode;
                std::thread::spawn(move || {
                    if let Some(url) = splash_url {
                        splash::show_startup_splash_sync(
                            &ov_handle,
                            url,
                            pending_settings_window,
                        );
                        if preload_overlay {
                            std::thread::sleep(Duration::from_millis(350));
                        }
                    }
                    if preload_overlay {
                        let _ = ensure_overlay_window(&ov_handle);
                    }
                });
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Some(url) = splash_url {
                    splash::show_startup_splash_sync(
                        app.handle(),
                        url,
                        pending_settings_window,
                    );
                }
                if !initial_settings.ultra_light_mode {
                    let _ = ensure_overlay_window(app.handle());
                }
            }

            // Defer onboarding / permission prompt until the startup splash completes so nothing opens behind it.
            if !has_splash && pending_settings_window {
                show_settings_window(app.handle());
            }

            if initial_settings.auto_check_updates {
                trigger_update_sensor(app.handle(), 6);
            }

            if !initial_settings.ultra_light_mode
                && initial_settings.startup_animation
                && initial_settings.festive_themes
            {
                splash::trigger_festive_theme_sync(app.handle(), 10);
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building ZunzShot");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            if !ALLOW_APP_EXIT.load(Ordering::Relaxed) {
                api.prevent_exit();
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            show_settings_window(app_handle);
        }
        _ => {
            let _ = app_handle;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        next_sensor_sleep_duration, normalize_snooze_hours, snooze_still_active, Shortcut,
        UPDATE_CHECK_INTERVAL_SECS,
    };
    use std::str::FromStr;
    use std::time::Duration;

    #[test]
    fn parses_standalone_and_modified_print_screen_shortcuts() {
        let single = Shortcut::from_str("PrintScreen").expect("standalone PrintScreen");
        assert!(single.mods.is_empty());

        let shifted = Shortcut::from_str("Shift+PrintScreen").expect("Shift+PrintScreen");
        assert!(!shifted.mods.is_empty());
        assert_ne!(single.id(), shifted.id());

        let ctrled = Shortcut::from_str("Control+PrintScreen").expect("Control+PrintScreen");
        assert_ne!(shifted.id(), ctrled.id());
    }

    #[test]
    fn parses_two_three_and_four_key_global_shortcuts() {
        let two = Shortcut::from_str("Control+S").expect("2-key Control+S");
        let three_mac = Shortcut::from_str("Command+Shift+X").expect("3-key Command+Shift+X");
        let three_ctrl = Shortcut::from_str("Control+Shift+S").expect("3-key Control+Shift+S");
        let three_alt = Shortcut::from_str("Control+Alt+F").expect("3-key Control+Alt+F");
        let four = Shortcut::from_str("Command+Alt+Shift+F").expect("4-key Command+Alt+Shift+F");
        assert_ne!(two.id(), three_ctrl.id());
        assert_ne!(three_mac.id(), three_ctrl.id());
        assert_ne!(three_alt.id(), four.id());
    }

    #[test]
    fn normalize_snooze_hours_accepts_two_four_six_only() {
        assert_eq!(normalize_snooze_hours(Some(2)), 2);
        assert_eq!(normalize_snooze_hours(Some(4)), 4);
        assert_eq!(normalize_snooze_hours(Some(6)), 6);
        assert_eq!(normalize_snooze_hours(None), 4);
        assert_eq!(normalize_snooze_hours(Some(1)), 4);
        assert_eq!(normalize_snooze_hours(Some(24)), 4);
    }

    #[test]
    fn snooze_boundary_wakes_on_exact_interval_multiple() {
        let two_hours = 2 * 3600;
        assert!(snooze_still_active(Duration::from_secs(3600), two_hours));
        assert!(!snooze_still_active(
            Duration::from_secs(two_hours - 2),
            two_hours
        ));
        assert!(!snooze_still_active(Duration::from_secs(two_hours), two_hours));
    }

    #[test]
    fn next_sensor_sleep_preserves_30_min_poll_and_clamps_snooze_tail() {
        let thirty_min = Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS);
        assert_eq!(thirty_min.as_secs(), 1800);
        assert_eq!(next_sensor_sleep_duration(None), thirty_min);

        let two_hours = 2 * 3600;
        assert_eq!(
            next_sensor_sleep_duration(Some((Duration::from_secs(1800), two_hours))),
            thirty_min
        );
        assert_eq!(
            next_sensor_sleep_duration(Some((Duration::from_secs(two_hours - 600), two_hours))),
            Duration::from_secs(600)
        );
    }
}
