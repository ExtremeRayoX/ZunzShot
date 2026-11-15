use crate::i18n::Lang;
use crate::settings::{config_dir, AppSettings};
use crate::{encode_query_component, AppState, CAPTURE_IN_PROGRESS};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const SPLASH_LABEL: &str = "splash";
const SPLASH_WIDTH: f64 = 900.0;
const SPLASH_HEIGHT: f64 = 600.0;
const SPLASH_DURATION_MS: u64 = 4150;
const SPLASH_FAILSAFE_MS: u64 = 9500;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_THEMES_COUNT: usize = 24;
const CACHE_FILENAME: &str = "splash-themes.json";
const FESTIVE_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/ExtremeRayoX/ZunzShot/main/assets/splash-festive.json";

static FESTIVE_SYNC_STARTED: AtomicBool = AtomicBool::new(false);
static OPEN_SETTINGS_AFTER_SPLASH: AtomicBool = AtomicBool::new(false);

fn default_schema_version() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FestiveManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub themes: Vec<FestiveTheme>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FestiveTheme {
    pub id: String,
    pub start_date: String,
    pub end_date: String,
    #[serde(default)]
    pub badge_es: Option<String>,
    #[serde(default)]
    pub badge_en: Option<String>,
    #[serde(default)]
    pub accent_color: Option<String>,
    #[serde(default)]
    pub accent_secondary: Option<String>,
    #[serde(default)]
    pub flower_color_start: Option<String>,
    #[serde(default)]
    pub flower_color_end: Option<String>,
    #[serde(default)]
    pub flower_aura_color: Option<String>,
    #[serde(default)]
    pub bird_glow_color: Option<String>,
    #[serde(default)]
    pub card_border_color: Option<String>,
    #[serde(default)]
    pub accessory: Option<String>,
    #[serde(default)]
    pub particles: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SplashLaunchTheme {
    pub badge: Option<String>,
    pub accent_color: Option<String>,
    pub accent_secondary: Option<String>,
    pub flower_color_start: Option<String>,
    pub flower_color_end: Option<String>,
    pub flower_aura_color: Option<String>,
    pub bird_glow_color: Option<String>,
    pub card_border_color: Option<String>,
    pub accessory: Option<String>,
    pub particles: Option<String>,
}

/// Validates and normalizes CSS hex/rgb/rgba color literals so remote manifests cannot inject CSS.
pub fn sanitize_css_color(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        let valid_len = matches!(hex.len(), 3 | 6 | 8);
        if valid_len && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(format!("#{}", hex.to_ascii_lowercase()));
        }
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if let Some(inner) = lower.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some(format!("rgb({r}, {g}, {b})"));
        }
        return None;
    }

    if let Some(inner) = lower
        .strip_prefix("rgba(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() == 4 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            let a = parts[3].parse::<f32>().ok()?;
            if a.is_finite() && (0.0..=1.0).contains(&a) {
                return Some(format!("rgba({r}, {g}, {b}, {a:.3})"));
            }
        }
    }

    None
}

fn sanitize_opt_color(val: Option<&str>) -> Option<String> {
    val.and_then(sanitize_css_color)
}

fn sanitize_badge(raw: &str) -> Option<String> {
    let normalized = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = collapsed.chars().take(40).collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn sanitize_opt_badge(val: Option<&str>) -> Option<String> {
    val.and_then(sanitize_badge)
}

fn sanitize_accessory(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "witch-hat" | "santa-hat" | "party-hat" | "heart" => {
            Some(raw.trim().to_ascii_lowercase())
        }
        _ => None,
    }
}

fn sanitize_particles(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bats" | "snow" | "confetti" | "hearts" | "sparkles" => {
            Some(raw.trim().to_ascii_lowercase())
        }
        _ => None,
    }
}

/// Validates schema version, date windows, and safe color/preset allowlists for a manifest.
pub fn sanitize_manifest(manifest: FestiveManifest) -> Option<FestiveManifest> {
    if manifest.schema_version != 1 {
        return None;
    }

    let mut clean_themes = Vec::new();
    for theme in manifest.themes.into_iter().take(MAX_THEMES_COUNT) {
        let id = theme.id.trim().to_string();
        if id.is_empty() || id.len() > 64 {
            continue;
        }
        let Ok(start) = NaiveDate::parse_from_str(theme.start_date.trim(), "%Y-%m-%d") else {
            continue;
        };
        let Ok(end) = NaiveDate::parse_from_str(theme.end_date.trim(), "%Y-%m-%d") else {
            continue;
        };
        if end < start {
            continue;
        }

        clean_themes.push(FestiveTheme {
            id,
            start_date: start.format("%Y-%m-%d").to_string(),
            end_date: end.format("%Y-%m-%d").to_string(),
            badge_es: sanitize_opt_badge(theme.badge_es.as_deref()),
            badge_en: sanitize_opt_badge(theme.badge_en.as_deref()),
            accent_color: sanitize_opt_color(theme.accent_color.as_deref()),
            accent_secondary: sanitize_opt_color(theme.accent_secondary.as_deref()),
            flower_color_start: sanitize_opt_color(theme.flower_color_start.as_deref()),
            flower_color_end: sanitize_opt_color(theme.flower_color_end.as_deref()),
            flower_aura_color: sanitize_opt_color(theme.flower_aura_color.as_deref()),
            bird_glow_color: sanitize_opt_color(theme.bird_glow_color.as_deref()),
            card_border_color: sanitize_opt_color(theme.card_border_color.as_deref()),
            accessory: theme.accessory.as_deref().and_then(sanitize_accessory),
            particles: theme.particles.as_deref().and_then(sanitize_particles),
        });
    }

    Some(FestiveManifest {
        schema_version: 1,
        themes: clean_themes,
    })
}

/// Returns the active sanitized festive theme matching `today`, if any.
pub fn select_active_theme(
    manifest: &FestiveManifest,
    lang: Lang,
    today: NaiveDate,
) -> Option<SplashLaunchTheme> {
    for theme in &manifest.themes {
        let Ok(start) = NaiveDate::parse_from_str(&theme.start_date, "%Y-%m-%d") else {
            continue;
        };
        let Ok(end) = NaiveDate::parse_from_str(&theme.end_date, "%Y-%m-%d") else {
            continue;
        };
        if today >= start && today <= end {
            let badge = match lang {
                Lang::En => theme
                    .badge_en
                    .clone()
                    .or_else(|| theme.badge_es.clone()),
                Lang::Es => theme
                    .badge_es
                    .clone()
                    .or_else(|| theme.badge_en.clone()),
            };
            return Some(SplashLaunchTheme {
                badge,
                accent_color: theme.accent_color.clone(),
                accent_secondary: theme.accent_secondary.clone(),
                flower_color_start: theme.flower_color_start.clone(),
                flower_color_end: theme.flower_color_end.clone(),
                flower_aura_color: theme.flower_aura_color.clone(),
                bird_glow_color: theme.bird_glow_color.clone(),
                card_border_color: theme.card_border_color.clone(),
                accessory: theme.accessory.clone(),
                particles: theme.particles.clone(),
            });
        }
    }
    None
}

fn cache_path(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).ok().map(|d| d.join(CACHE_FILENAME))
}

fn load_cached_manifest(app: &AppHandle) -> Option<FestiveManifest> {
    let path = cache_path(app)?;
    let meta = fs::metadata(&path).ok()?;
    if meta.len() as usize > MAX_MANIFEST_BYTES {
        return None;
    }
    let raw = fs::read(&path).ok()?;
    let parsed = serde_json::from_slice::<FestiveManifest>(&raw).ok()?;
    sanitize_manifest(parsed)
}

fn save_cached_manifest(app: &AppHandle, manifest: &FestiveManifest) {
    let Some(path) = cache_path(app) else {
        return;
    };
    let Ok(json) = serde_json::to_vec_pretty(manifest) else {
        return;
    };
    let tmp_path = path.with_extension("json.tmp");
    if fs::write(&tmp_path, &json).is_ok() {
        let _ = fs::rename(&tmp_path, &path);
    }
}

/// Removes the cached festive theme manifest when remote festive themes are disabled.
pub fn clear_cached_manifest(app: &AppHandle) {
    if let Some(path) = cache_path(app) {
        let _ = fs::remove_file(path);
    }
}

fn build_splash_url(lang: Lang, theme: Option<&SplashLaunchTheme>) -> String {
    let mut parts = vec![format!("lang={}", lang.code())];
    if let Some(t) = theme {
        let mut push_param = |k: &str, v: &Option<String>| {
            if let Some(val) = v {
                parts.push(format!("{k}={}", encode_query_component(val)));
            }
        };
        push_param("badge", &t.badge);
        push_param("a1", &t.accent_color);
        push_param("a2", &t.accent_secondary);
        push_param("f1", &t.flower_color_start);
        push_param("f2", &t.flower_color_end);
        push_param("fa", &t.flower_aura_color);
        push_param("bg", &t.bird_glow_color);
        push_param("cb", &t.card_border_color);
        push_param("acc", &t.accessory);
        push_param("pt", &t.particles);
    }
    format!("splash.html?{}", parts.join("&"))
}

fn can_show_splash_now(app: &AppHandle) -> bool {
    if CAPTURE_IN_PROGRESS.load(Ordering::SeqCst) {
        return false;
    }
    app.try_state::<Mutex<AppState>>()
        .and_then(|st| st.lock().ok().map(|g| g.active_session.is_none()))
        .unwrap_or(true)
}

fn flush_pending_settings_after_splash(app: &AppHandle) {
    if OPEN_SETTINGS_AFTER_SPLASH.swap(false, Ordering::SeqCst) {
        crate::show_settings_window(app);
    }
}

fn build_and_show_splash_window(app_handle: &AppHandle, url: String) {
    if app_handle.get_webview_window(SPLASH_LABEL).is_some() || !can_show_splash_now(app_handle) {
        flush_pending_settings_after_splash(app_handle);
        return;
    }

    let (win_lw, win_lh, pos_lx, pos_ly, phys_x, phys_y, phys_w, phys_h) =
        if let Some(mon) = app_handle.primary_monitor().ok().flatten() {
            let mp = mon.position();
            let ms = mon.size();
            let sc = mon.scale_factor().max(1.0);
            let mon_lw = ms.width as f64 / sc;
            let mon_lh = ms.height as f64 / sc;
            let lw = SPLASH_WIDTH.min((mon_lw * 0.96).max(360.0));
            let lh = SPLASH_HEIGHT.min((mon_lh * 0.94).max(280.0));
            let pw = (lw * sc).round().max(1.0) as i32;
            let ph = (lh * sc).round().max(1.0) as i32;
            let px = mp.x + ((ms.width as i32 - pw) / 2);
            let py = mp.y + ((ms.height as i32 - ph) / 2);
            (
                lw,
                lh,
                px as f64 / sc,
                py as f64 / sc,
                px,
                py,
                pw,
                ph,
            )
        } else {
            (
                SPLASH_WIDTH,
                SPLASH_HEIGHT,
                (1440.0 - SPLASH_WIDTH) / 2.0,
                (900.0 - SPLASH_HEIGHT) / 2.0,
                ((1440.0 - SPLASH_WIDTH) / 2.0) as i32,
                ((900.0 - SPLASH_HEIGHT) / 2.0) as i32,
                SPLASH_WIDTH as i32,
                SPLASH_HEIGHT as i32,
            )
        };

    let shown_once = Arc::new(AtomicBool::new(false));
    let shown_for_load = shown_once.clone();
    let app_for_load = app_handle.clone();

    let builder = WebviewWindowBuilder::new(app_handle, SPLASH_LABEL, WebviewUrl::App(url.into()))
        .title("ZunzShot")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .inner_size(win_lw, win_lh)
        .position(pos_lx, pos_ly)
        .visible(false)
        .on_page_load(move |win, payload| {
            if !matches!(payload.event(), PageLoadEvent::Finished) {
                return;
            }
            if shown_for_load.swap(true, Ordering::SeqCst) {
                return;
            }

            let app_ui = app_for_load.clone();
            let win_ui = win.clone();
            let _ = app_for_load.run_on_main_thread(move || {
                if !can_show_splash_now(&app_ui) {
                    let _ = win_ui.close();
                    flush_pending_settings_after_splash(&app_ui);
                    return;
                }

                let _ = win_ui.set_ignore_cursor_events(true);

                #[cfg(target_os = "windows")]
                {
                    let _ = (win_lw, win_lh);
                    use windows::Win32::Foundation::HWND;
                    use windows::Win32::UI::WindowsAndMessaging::{
                        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                    };

                    if let Ok(hwnd) = win_ui.hwnd() {
                        unsafe {
                            let _ = SetWindowPos(
                                HWND(hwnd.0),
                                Some(HWND_TOPMOST),
                                phys_x,
                                phys_y,
                                phys_w,
                                phys_h,
                                SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                        }
                    }
                }

                #[cfg(target_os = "macos")]
                {
                    let _ = (phys_x, phys_y, phys_w, phys_h);
                    use objc2::MainThreadMarker;
                    use objc2_app_kit::{NSScreen, NSWindow, NSWindowCollectionBehavior};
                    use objc2_foundation::NSPoint;

                    if let Some(mtm) = MainThreadMarker::new() {
                        let screens = NSScreen::screens(mtm);
                        let sf_opt = if !screens.is_empty() {
                            Some(screens.objectAtIndex(0).frame())
                        } else {
                            NSScreen::mainScreen(mtm).map(|s| s.frame())
                        };
                        if let Ok(ns_win_ptr) = win_ui.ns_window() {
                            if !ns_win_ptr.is_null() {
                                let ns_win = unsafe { &*(ns_win_ptr as *const NSWindow) };
                                if let Some(sf) = sf_opt {
                                    let cocoa_x =
                                        sf.origin.x + ((sf.size.width - win_lw) / 2.0);
                                    let cocoa_y =
                                        sf.origin.y + ((sf.size.height - win_lh) / 2.0);
                                    ns_win.setFrameOrigin(NSPoint::new(cocoa_x, cocoa_y));
                                }
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

                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    let _ = (win_lw, win_lh, phys_x, phys_y, phys_w, phys_h);
                    let _ = win_ui.show();
                }

                let app_close = app_ui.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(SPLASH_DURATION_MS));
                    let app_inner = app_close.clone();
                    let _ = app_close.run_on_main_thread(move || {
                        if let Some(w) = app_inner.get_webview_window(SPLASH_LABEL) {
                            let _ = w.close();
                        }
                        flush_pending_settings_after_splash(&app_inner);
                    });
                });
            });
        });

    if builder.build().is_ok() {
        let app_failsafe = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(SPLASH_FAILSAFE_MS));
            let app_inner = app_failsafe.clone();
            let _ = app_failsafe.run_on_main_thread(move || {
                if let Some(w) = app_inner.get_webview_window(SPLASH_LABEL) {
                    let _ = w.close();
                }
                flush_pending_settings_after_splash(&app_inner);
            });
        });
    } else {
        flush_pending_settings_after_splash(app_handle);
    }
}

/// Resolves active cached theme (local-only, 0ms) and prepares the startup splash URL if enabled.
pub fn resolve_startup_splash_url(app: &AppHandle, settings: &AppSettings) -> Option<String> {
    if settings.ultra_light_mode || !settings.startup_animation {
        return None;
    }

    let lang = Lang::from_code(&settings.language);
    let active_theme = if settings.festive_themes {
        let today = Local::now().date_naive();
        load_cached_manifest(app).and_then(|m| select_active_theme(&m, lang, today))
    } else {
        None
    };

    Some(build_splash_url(lang, active_theme.as_ref()))
}

/// Shows the non-blocking startup splash window once on initial process launch.
pub fn show_startup_splash_sync(app: &AppHandle, url: String, open_settings_after: bool) {
    OPEN_SETTINGS_AFTER_SPLASH.store(open_settings_after, Ordering::SeqCst);
    build_and_show_splash_window(app, url);
}

/// Dismisses any visible splash window before a capture begins so it never shows in screenshots.
pub fn dismiss_splash_before_capture(app: &AppHandle) -> bool {
    OPEN_SETTINGS_AFTER_SPLASH.store(false, Ordering::SeqCst);
    if app.get_webview_window(SPLASH_LABEL).is_none() {
        return false;
    }
    let app_close = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app_close.get_webview_window(SPLASH_LABEL) {
            let _ = w.close();
        }
    });
    true
}

fn build_https_client() -> Result<reqwest::Client, reqwest::Error> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("ZunzShot/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Fetches and caches `assets/splash-festive.json` quietly in the background for subsequent launches.
pub fn trigger_festive_theme_sync(app: &AppHandle, delay_secs: u64) {
    if FESTIVE_SYNC_STARTED.swap(true, Ordering::SeqCst) {
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

        let enabled = handle
            .try_state::<Mutex<AppState>>()
            .and_then(|s| {
                s.lock().ok().map(|g| {
                    !g.settings.ultra_light_mode
                        && g.settings.startup_animation
                        && g.settings.festive_themes
                })
            })
            .unwrap_or(false);

        if enabled {
            if let Ok(client) = build_https_client() {
                if let Ok(resp) = client.get(FESTIVE_MANIFEST_URL).send().await {
                    if resp.status().is_success() {
                        let too_large = resp
                            .content_length()
                            .is_some_and(|len| len as usize > MAX_MANIFEST_BYTES);
                        if !too_large {
                            if let Ok(bytes) = resp.bytes().await {
                                if bytes.len() <= MAX_MANIFEST_BYTES {
                                    if let Ok(parsed) =
                                        serde_json::from_slice::<FestiveManifest>(&bytes)
                                    {
                                        if let Some(clean) = sanitize_manifest(parsed) {
                                            save_cached_manifest(&handle, &clean);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        FESTIVE_SYNC_STARTED.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::{
        build_https_client, build_splash_url, sanitize_css_color, sanitize_manifest,
        select_active_theme, FestiveManifest, FestiveTheme,
    };
    use crate::i18n::Lang;
    use chrono::NaiveDate;

    #[test]
    fn sanitizes_valid_and_rejects_malicious_css_colors() {
        assert_eq!(sanitize_css_color("#FB923C").as_deref(), Some("#fb923c"));
        assert_eq!(sanitize_css_color("#abc").as_deref(), Some("#abc"));
        assert_eq!(
            sanitize_css_color("rgba(251, 146, 60, 0.5)").as_deref(),
            Some("rgba(251, 146, 60, 0.500)")
        );
        assert_eq!(sanitize_css_color("red; background:url(x)"), None);
        assert_eq!(sanitize_css_color("expression(alert(1))"), None);
        assert_eq!(sanitize_css_color("rgba(999, 0, 0, 1)"), None);
        assert_eq!(sanitize_css_color("#zzzzzz"), None);
    }

    #[test]
    fn manifest_sanitization_and_date_selection_enforce_allowlists() {
        let raw = FestiveManifest {
            schema_version: 1,
            themes: vec![
                FestiveTheme {
                    id: "bad-dates".to_string(),
                    start_date: "2027-11-10".to_string(),
                    end_date: "2027-11-01".to_string(),
                    badge_es: None,
                    badge_en: None,
                    accent_color: None,
                    accent_secondary: None,
                    flower_color_start: None,
                    flower_color_end: None,
                    flower_aura_color: None,
                    bird_glow_color: None,
                    card_border_color: None,
                    accessory: None,
                    particles: None,
                },
                FestiveTheme {
                    id: "halloween".to_string(),
                    start_date: "2027-10-25".to_string(),
                    end_date: "2027-11-02".to_string(),
                    badge_es: Some("  Edición\nHalloween  ".to_string()),
                    badge_en: Some("Halloween Edition".to_string()),
                    accent_color: Some("#fb923c".to_string()),
                    accent_secondary: Some("url(javascript:alert(1))".to_string()),
                    flower_color_start: None,
                    flower_color_end: None,
                    flower_aura_color: None,
                    bird_glow_color: None,
                    card_border_color: None,
                    accessory: Some("witch-hat".to_string()),
                    particles: Some("<script>".to_string()),
                },
            ],
        };

        let clean = sanitize_manifest(raw).expect("schema v1 should parse");
        assert_eq!(clean.themes.len(), 1);
        assert_eq!(clean.themes[0].accent_secondary, None);
        assert_eq!(clean.themes[0].particles, None);
        assert_eq!(clean.themes[0].accessory.as_deref(), Some("witch-hat"));

        let active_oct = select_active_theme(
            &clean,
            Lang::Es,
            NaiveDate::from_ymd_opt(2027, 10, 31).unwrap(),
        )
        .expect("should match on Halloween");
        assert_eq!(active_oct.badge.as_deref(), Some("Edición Halloween"));
        assert!(build_splash_url(Lang::Es, Some(&active_oct)).contains("acc=witch-hat"));

        let expired_nov = select_active_theme(
            &clean,
            Lang::Es,
            NaiveDate::from_ymd_opt(2027, 11, 3).unwrap(),
        );
        assert!(expired_nov.is_none());
    }

    #[test]
    fn https_client_builds_with_compiled_tls_provider() {
        let client = build_https_client();
        assert!(client.is_ok(), "reqwest client failed: {:?}", client.err());
    }
}
