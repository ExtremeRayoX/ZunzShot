mod capture;
mod i18n;
mod settings;

use arboard::{Clipboard, ImageData};
use capture::{CaptureMetadata, CaptureSession};
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
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, State,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_updater::UpdaterExt;

static ALLOW_APP_EXIT: AtomicBool = AtomicBool::new(false);
static SINGLE_INSTANCE_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
const SINGLE_INSTANCE_ADDR: &str = "127.0.0.1:45893";
static AUTO_UPDATE_CHECKED: AtomicBool = AtomicBool::new(false);
static CAPTURE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
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
}

static LAST_MONITOR_BOUNDS: Mutex<Option<MonitorBounds>> = Mutex::new(None);

struct CaptureInProgressGuard;

impl Drop for CaptureInProgressGuard {
    fn drop(&mut self) {
        CAPTURE_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
fn force_windows_foreground_and_webview_focus(win: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
        ShowWindow, SW_SHOW,
    };

    if let Ok(raw_hwnd) = win.hwnd() {
        let hwnd = HWND(raw_hwnd.0);
        unsafe {
            let fg_hwnd = GetForegroundWindow();
            let fg_tid = if !fg_hwnd.0.is_null() {
                GetWindowThreadProcessId(fg_hwnd, None)
            } else {
                0
            };
            let cur_tid = GetCurrentThreadId();
            let attached = if fg_tid != 0 && fg_tid != cur_tid {
                AttachThreadInput(cur_tid, fg_tid, true).as_bool()
            } else {
                false
            };

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetActiveWindow(hwnd);
            let _ = SetFocus(Some(hwnd));

            if attached {
                let _ = AttachThreadInput(cur_tid, fg_tid, false);
            }
        }
    }
    let _ = win.set_focus();
    let _ = AsRef::<tauri::Webview<_>>::as_ref(win).set_focus();
}

pub struct AppState {
    pub settings: AppSettings,
    pub active_session: Option<CaptureSession>,
    pub tray: Option<TrayIcon>,
    pub shortcut_modes: HashMap<u32, String>,
    pub failed_shortcuts: Vec<String>,
    pub pending_tab: Option<String>,
    pub pending_capture_error: Option<String>,
    pub pinned_images: HashMap<String, Vec<u8>>,
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

fn register_shortcuts(app: &AppHandle, settings: &AppSettings) -> Vec<String> {
    let _op_guard = SHORTCUT_OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    SHORTCUTS_PAUSED.store(false, Ordering::SeqCst);

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
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { .. } = event {
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

fn show_settings_window(app: &AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app_handle.get_webview_window("main") {
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
            .find(|w| w.label == "main")
            .cloned();

        if let Some(cfg) = cfg {
            if let Ok(builder) = WebviewWindowBuilder::from_config(&app_handle, &cfg) {
                if let Ok(win) = builder.visible(true).build() {
                    attach_main_close_handler(&app_handle, &win);
                    focus_settings_window(&win);
                }
            }
        }
    });
}

fn open_settings_tab(app: &AppHandle, tab: &str) {
    {
        let state = app.state::<Mutex<AppState>>();
        state.lock().unwrap().pending_tab = Some(tab.to_string());
    }
    show_settings_window(app);
    let _ = app.emit("zunzshot://open-tab", tab);
}

fn ensure_overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(ov) = app.get_webview_window("overlay") {
        return Some(ov);
    }
    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "overlay")
        .cloned()?;
    WebviewWindowBuilder::from_config(app, &cfg)
        .ok()
        .and_then(|b| b.visible(false).build().ok())
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
    if let Some(ov) = app.get_webview_window("overlay") {
        let has_active_session = app
            .state::<Mutex<AppState>>()
            .lock()
            .unwrap()
            .active_session
            .is_some();
        if has_active_session && ov.is_visible().unwrap_or(false) {
            let _ = ov.emit("zunzshot://switch-mode", &mode);
            let ov_focus = ov.clone();
            let _ = app.run_on_main_thread(move || {
                #[cfg(target_os = "windows")]
                force_windows_foreground_and_webview_focus(&ov_focus);
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = ov_focus.set_focus();
                    let _ = AsRef::<tauri::Webview<_>>::as_ref(&ov_focus).set_focus();
                }
                let _ = ov_focus.emit("zunzshot://focus-canvas", ());
            });
            return;
        }
    }

    if CAPTURE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        let _capture_guard = CaptureInProgressGuard;

        if let Some(ov) = app.get_webview_window("overlay") {
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
                if let Some(ov) = app_hide.get_webview_window("overlay") {
                    if ov.is_visible().unwrap_or(false) {
                        hide_overlay_on_main_thread(&app_hide, &[], false);
                        hid_any = true;
                    }
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
                let sc = session.metadata.scale_factor.max(1.0);
                *LAST_MONITOR_BOUNDS.lock().unwrap() = Some(MonitorBounds {
                    phys_x,
                    phys_y,
                    phys_w,
                    phys_h,
                    scale: sc,
                });

                {
                    let state = app.state::<Mutex<AppState>>();
                    let mut guard = state.lock().unwrap();
                    guard.active_session = Some(session);
                }

                present_overlay_window(
                    &app,
                    (phys_x, phys_y, phys_w, phys_h),
                    true,
                    true,
                );
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
                if let Some(ov) = app_handle.get_webview_window("overlay") {
                    let _ = ov.hide();
                    let _ = ov.destroy();
                }
            });
        }
    } else {
        let app_handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = ensure_overlay_window(&app_handle);
        });
    }

    refresh_tray_menu(&app);
    register_shortcuts(&app, &new_settings);
    let _ = app.emit("zunzshot://settings-updated", &new_settings);
    if new_settings.auto_check_updates {
        maybe_spawn_auto_update(&app, 1);
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
        maybe_spawn_auto_update(&app, 1);
    }
    Ok(updated)
}

#[tauri::command]
fn set_shortcuts_paused(app: AppHandle, state: State<'_, Mutex<AppState>>, paused: bool) {
    if paused {
        let _op_guard = SHORTCUT_OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        SHORTCUTS_PAUSED.store(true, Ordering::SeqCst);
        let _ = app.global_shortcut().unregister_all();
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
fn get_capture_rgba(state: State<'_, Mutex<AppState>>) -> Result<Response, String> {
    let guard = state.lock().unwrap();
    let session = guard
        .active_session
        .as_ref()
        .ok_or_else(|| "No active capture session".to_string())?;

    let w = session.metadata.image_width;
    let h = session.metadata.image_height;
    let mut payload = Vec::with_capacity(8 + session.rgba_bytes.len());
    payload.extend_from_slice(&w.to_le_bytes());
    payload.extend_from_slice(&h.to_le_bytes());
    payload.extend_from_slice(&session.rgba_bytes);
    Ok(Response::new(payload))
}

static LAST_OVERLAY_RECT: std::sync::Mutex<Option<(i32, i32, u32, u32)>> =
    std::sync::Mutex::new(None);

#[cfg(target_os = "macos")]
fn matching_macos_screen_rect(
    mtm: objc2::MainThreadMarker,
    phys_x: i32,
    phys_y: i32,
    phys_w: u32,
    phys_h: u32,
    fallback_scale: f64,
) -> objc2_foundation::NSRect {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let screens = NSScreen::screens(mtm);
    let sc = fallback_scale.max(1.0);
    let primary_h = if !screens.is_empty() {
        screens.objectAtIndex(0).frame().size.height
    } else {
        phys_h as f64 / sc
    };

    let fallback_lx = phys_x as f64 / sc;
    let fallback_ly = phys_y as f64 / sc;
    let fallback_lw = phys_w as f64 / sc;
    let fallback_lh = phys_h as f64 / sc;
    let mut target_rect = NSRect::new(
        NSPoint::new(fallback_lx, primary_h - (fallback_ly + fallback_lh)),
        NSSize::new(fallback_lw, fallback_lh),
    );

    let count = screens.len();
    let mut best_dist = f64::MAX;
    for idx in 0..count {
        let screen = screens.objectAtIndex(idx);
        let sf = screen.frame();
        let s_scale = screen.backingScaleFactor().max(1.0);
        let size_match = ((sf.size.width * s_scale).round() - phys_w as f64).abs() <= 4.0
            && ((sf.size.height * s_scale).round() - phys_h as f64).abs() <= 4.0;
        let screen_top_y = primary_h - (sf.origin.y + sf.size.height);
        let dx = sf.origin.x * s_scale - phys_x as f64;
        let dy = screen_top_y * s_scale - phys_y as f64;
        let dist = dx * dx + dy * dy;
        if count == 1 || (size_match && dist < best_dist) {
            best_dist = dist;
            target_rect = sf;
        }
    }

    target_rect
}

fn apply_native_overlay_window(
    ov: &tauri::WebviewWindow,
    phys_x: i32,
    phys_y: i32,
    phys_w: u32,
    phys_h: u32,
    elevated: bool,
    emit_ready: bool,
) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior};

        if let Some(mtm) = MainThreadMarker::new() {
            if let Ok(ns_win_ptr) = ov.ns_window() {
                if !ns_win_ptr.is_null() {
                    let ns_win = unsafe { &*(ns_win_ptr as *const NSWindow) };
                    let ov_scale = ov.scale_factor().unwrap_or(2.0).max(1.0);
                    let target_rect = matching_macos_screen_rect(
                        mtm, phys_x, phys_y, phys_w, phys_h, ov_scale,
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
                        ns_win.makeKeyAndOrderFront(None);
                        ns_win.orderFrontRegardless();
                        #[allow(deprecated)]
                        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
                        let _ = ov.set_focus();
                        let _ = AsRef::<tauri::Webview<_>>::as_ref(ov).set_focus();
                    } else {
                        // Temporarily drop to normal window level so NSSavePanel / file picker appears on top.
                        ns_win.setLevel(0);
                    }

                    if emit_ready {
                        let _ = ov.emit("zunzshot://session-ready", ());
                    }
                    return;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_SHOWWINDOW,
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
                        SWP_SHOWWINDOW,
                    );
                }
            } else {
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_NOTOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
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
                            if ov_main.is_visible().unwrap_or(false) {
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

fn present_overlay_window(
    app: &AppHandle,
    rect: (i32, i32, u32, u32),
    elevated: bool,
    emit_ready: bool,
) {
    let (phys_x, phys_y, phys_w, phys_h) = rect;
    *LAST_OVERLAY_RECT.lock().unwrap() = Some(rect);
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

#[tauri::command]
fn set_overlay_elevated(app: AppHandle, elevated: bool) {
    let rect = *LAST_OVERLAY_RECT.lock().unwrap();
    if let Some(rect) = rect {
        if app.get_webview_window("overlay").is_some() {
            present_overlay_window(&app, rect, elevated, false);
        }
    }
}

#[tauri::command]
fn focus_overlay_window(app: AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(ov) = app_handle.get_webview_window("overlay") {
            if ov.is_visible().unwrap_or(false) {
                #[cfg(target_os = "windows")]
                force_windows_foreground_and_webview_focus(&ov);
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = ov.set_focus();
                    let _ = AsRef::<tauri::Webview<_>>::as_ref(&ov).set_focus();
                }
            }
        }
    });
}

fn hide_overlay_on_main_thread(app_handle: &AppHandle, pin_ids: &[String], destroy_ov: bool) {
    if let Some(ov) = app_handle.get_webview_window("overlay") {
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
            let _ = ov.destroy();
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

fn encode_query_component(input: &str) -> String {
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
) {
    let should_close = close_overlay.unwrap_or(false);
    let (ultra_light, pin_ids): (bool, Vec<String>) = if should_close {
        *LAST_OVERLAY_RECT.lock().unwrap() = None;
        let mut guard = state.lock().unwrap();
        guard.active_session = None;
        (
            guard.settings.ultra_light_mode,
            guard.pinned_images.keys().cloned().collect(),
        )
    } else {
        (false, Vec::new())
    };

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
                    let sf = matching_macos_screen_rect(
                        mtm, mon_phys_x, mon_phys_y, mon_phys_w, mon_phys_h, sc,
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

            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{
                    SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                };

                let phys_toast_x = mon_phys_x
                    + (((mon_phys_w as f64 - toast_w * sc) / 2.0).max(0.0)).round() as i32;
                let phys_toast_y = mon_phys_y + (38.0 * sc).round() as i32;
                let phys_toast_w = (toast_w * sc).round().max(1.0) as i32;
                let phys_toast_h = (toast_h * sc).round().max(1.0) as i32;
                if let Ok(hwnd) = win.hwnd() {
                    let hwnd = HWND(hwnd.0);
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            Some(HWND_TOPMOST),
                            phys_toast_x,
                            phys_toast_y,
                            phys_toast_w,
                            phys_toast_h,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        );
                    }
                }
            }

            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
            let Some(ov) = app_handle.get_webview_window("overlay") else {
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
        let _ = (app, temp_path, anchor_x, anchor_y, anchor_w, anchor_h);
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
            let ns_win_opt = app_handle
                .get_webview_window("overlay")
                .and_then(|ov| ov.ns_window().ok())
                .filter(|p| !p.is_null())
                .map(|p| unsafe { &*(p as *const NSWindow) });

            if let Some(ns_win) = ns_win_opt {
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
                if let Some(ns_win) = ns_win_opt {
                    ns_win.setLevel(1000);
                    ns_win.makeKeyAndOrderFront(None);
                }
            }
            let _ = app_handle.emit("zunzshot://print-result", printed);
        });
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(rect) = *LAST_OVERLAY_RECT.lock().unwrap() {
            present_overlay_window(&app, rect, false, false);
        }
        let app_handle = app.clone();
        std::thread::spawn(move || {
            let escaped = temp_path.to_string_lossy().replace('\'', "''");
            let status = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &format!("Start-Process -FilePath '{escaped}' -Verb Print"),
                ])
                .status();
            let ok = status.map(|s| s.success()).unwrap_or(false);
            if !ok {
                if let Some(rect) = *LAST_OVERLAY_RECT.lock().unwrap() {
                    present_overlay_window(&app_handle, rect, true, false);
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
    let (lang, mon_phys_x, mon_phys_y, mon_w, mon_h, sc) = {
        let mut guard = state.lock().unwrap();
        guard.pinned_images.insert(pin_id.clone(), png_bytes.to_vec());
        let (mx, my, mw, mh, scale) = match guard.active_session.as_ref() {
            Some(s) => (
                s.monitor_phys_x,
                s.monitor_phys_y,
                s.metadata.logical_width.max(1.0),
                s.metadata.logical_height.max(1.0),
                s.metadata.scale_factor.max(1.0),
            ),
            None => (0, 0, 1440.0, 900.0, 1.0),
        };
        (guard.settings.language.clone(), mx, my, mw, mh, scale)
    };

    let w = sel_w.min(mon_w);
    let h = sel_h.min(mon_h);
    let clamped_x = sel_x.clamp(0.0, (mon_w - w).max(0.0));
    let clamped_y = sel_y.clamp(0.0, (mon_h - h).max(0.0));
    let fallback_x = mon_phys_x as f64 / sc + clamped_x;
    let fallback_y = mon_phys_y as f64 / sc + clamped_y;

    let app_handle = app.clone();
    let id_for_win = pin_id.clone();
    let _ = app.run_on_main_thread(move || {
        #[allow(unused_mut)]
        let mut top_left_x = fallback_x;
        #[allow(unused_mut)]
        let mut top_left_y = fallback_y;
        #[allow(unused_mut)]
        let mut win_w = w;
        #[allow(unused_mut)]
        let mut win_h = h;
        #[cfg(target_os = "macos")]
        let mut cocoa_frame_opt: Option<objc2_foundation::NSRect> = None;

        #[cfg(target_os = "macos")]
        {
            use objc2::MainThreadMarker;
            use objc2_app_kit::{NSScreen, NSWindow};
            use objc2_foundation::{NSPoint, NSRect, NSSize};

            if let Some(mtm) = MainThreadMarker::new() {
                let screens = NSScreen::screens(mtm);
                let primary_h = if !screens.is_empty() {
                    screens.objectAtIndex(0).frame().size.height
                } else {
                    mon_h.max(900.0)
                };

                if let Some(ov_frame) = app_handle
                    .get_webview_window("overlay")
                    .and_then(|ov| ov.ns_window().ok())
                    .filter(|ptr| !ptr.is_null())
                    .map(|ptr| unsafe { (*(ptr as *const NSWindow)).frame() })
                {
                    win_w = sel_w.min(ov_frame.size.width.max(64.0));
                    win_h = sel_h.min(ov_frame.size.height.max(48.0));
                    let cx = sel_x.clamp(0.0, (ov_frame.size.width - win_w).max(0.0));
                    let cy = sel_y.clamp(0.0, (ov_frame.size.height - win_h).max(0.0));
                    let cocoa_x = ov_frame.origin.x + cx;
                    let cocoa_y = ov_frame.origin.y + ov_frame.size.height - (cy + win_h);
                    top_left_x = cocoa_x;
                    top_left_y = primary_h - (cocoa_y + win_h);
                    cocoa_frame_opt = Some(NSRect::new(
                        NSPoint::new(cocoa_x, cocoa_y),
                        NSSize::new(win_w, win_h),
                    ));
                } else {
                    let cocoa_y = primary_h - (fallback_y + win_h);
                    cocoa_frame_opt = Some(NSRect::new(
                        NSPoint::new(fallback_x, cocoa_y),
                        NSSize::new(win_w, win_h),
                    ));
                }
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
                #[cfg(target_os = "windows")]
                {
                    let phys_pin_x = mon_phys_x + (clamped_x * sc).round() as i32;
                    let phys_pin_y = mon_phys_y + (clamped_y * sc).round() as i32;
                    let phys_pin_w = (w * sc).round().max(64.0) as u32;
                    let phys_pin_h = (h * sc).round().max(48.0) as u32;
                    let _ = win.set_position(Position::Physical(PhysicalPosition::new(
                        phys_pin_x, phys_pin_y,
                    )));
                    let _ = win.set_size(Size::Physical(PhysicalSize::new(
                        phys_pin_w, phys_pin_h,
                    )));
                    let _ = win.show();
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

fn maybe_spawn_auto_update(app: &AppHandle, delay_secs: u64) {
    if AUTO_UPDATE_CHECKED.swap(true, Ordering::Relaxed) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if delay_secs > 0 {
            let _ = tauri::async_runtime::spawn_blocking(move || {
                std::thread::sleep(Duration::from_secs(delay_secs));
            })
            .await;
        }
        let can_check = handle
            .try_state::<Mutex<AppState>>()
            .and_then(|s| s.lock().ok().map(|g| g.settings.auto_check_updates && g.active_session.is_none()))
            .unwrap_or(false);
        if !can_check {
            return;
        }
        let Ok(updater) = handle.updater() else {
            return;
        };
        let Ok(Some(update)) = updater.check().await else {
            return;
        };
        if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
            let capture_active = handle
                .try_state::<Mutex<AppState>>()
                .and_then(|s| s.lock().ok().map(|g| g.active_session.is_some()))
                .unwrap_or(false);
            if !capture_active {
                SINGLE_INSTANCE_SHUTTING_DOWN.store(true, Ordering::SeqCst);
                ALLOW_APP_EXIT.store(true, Ordering::Relaxed);
                handle.restart();
            }
        }
    });
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
            consume_pending_capture_error,
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
                shortcut_modes: HashMap::new(),
                failed_shortcuts: Vec::new(),
                pending_tab: None,
                pending_capture_error: None,
                pinned_images: HashMap::new(),
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
                        SINGLE_INSTANCE_SHUTTING_DOWN.store(true, Ordering::SeqCst);
                        ALLOW_APP_EXIT.store(true, Ordering::Relaxed);
                        app.exit(0);
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
                        "check_updates" => open_settings_tab(app, "check-updates"),
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

            // Pre-create the capture overlay for instant 0ms response unless Ultra-Light Mode is enabled.
            if !initial_settings.ultra_light_mode {
                let _ = ensure_overlay_window(app.handle());
            }

            // Lazily create the Settings window only when onboarding or permission prompt is required;
            // otherwise keep it unloaded so ZunzShot uses far less memory while idle in the tray.
            if show_welcome || !initial_perm {
                show_settings_window(app.handle());
            }

            if initial_settings.auto_check_updates {
                maybe_spawn_auto_update(app.handle(), 4);
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
