use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn default_true() -> bool {
    true
}

fn default_text_size() -> u32 {
    22
}

fn default_shortcut_quick_copy() -> String {
    "CommandOrControl+Shift+D".to_string()
}

fn default_cmd_copy() -> String {
    "CommandOrControl+C".to_string()
}

fn default_cmd_save() -> String {
    "CommandOrControl+S".to_string()
}

fn default_cmd_pin() -> String {
    "CommandOrControl+Shift+P".to_string()
}

fn default_cmd_share() -> String {
    "CommandOrControl+Shift+E".to_string()
}

fn default_cmd_print() -> String {
    "CommandOrControl+P".to_string()
}

fn default_cmd_cancel() -> String {
    "Escape".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: String,
    pub language_selected: bool,
    pub shortcut_region: String,
    #[serde(default = "default_shortcut_quick_copy")]
    pub shortcut_quick_copy: String,
    pub shortcut_circle: String,
    pub shortcut_window: String,
    pub shortcut_fullscreen: String,
    #[serde(default = "default_cmd_copy")]
    pub cmd_copy: String,
    #[serde(default = "default_cmd_save")]
    pub cmd_save: String,
    #[serde(default = "default_cmd_pin")]
    pub cmd_pin: String,
    #[serde(default = "default_cmd_share")]
    pub cmd_share: String,
    #[serde(default = "default_cmd_print")]
    pub cmd_print: String,
    #[serde(default = "default_cmd_cancel")]
    pub cmd_cancel: String,
    pub save_folder: String,
    pub output_format: String,
    pub launch_at_startup: bool,
    #[serde(default = "default_true")]
    pub show_magnifier: bool,
    #[serde(default)]
    pub ultra_light_mode: bool,
    #[serde(default = "default_true")]
    pub startup_animation: bool,
    #[serde(default = "default_true")]
    pub festive_themes: bool,
    #[serde(default)]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub updater_prompt_shown: bool,
    pub clock_format_24h: bool,
    pub clock_show_seconds: bool,
    pub clock_show_date: bool,
    pub clock_style: String,
    #[serde(default = "default_true")]
    pub text_default_bg: bool,
    #[serde(default = "default_true")]
    pub text_default_bold: bool,
    #[serde(default)]
    pub text_default_italic: bool,
    #[serde(default = "default_text_size")]
    pub text_default_size: u32,
}

impl AppSettings {
    pub fn default_with_app(app: &AppHandle) -> Self {
        let default_pictures = app
            .path()
            .picture_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("ZunzShot");

        Self {
            language: "es".to_string(),
            language_selected: false,
            shortcut_region: "CommandOrControl+Shift+X".to_string(),
            shortcut_quick_copy: default_shortcut_quick_copy(),
            shortcut_circle: "CommandOrControl+Shift+C".to_string(),
            shortcut_window: "CommandOrControl+Shift+W".to_string(),
            shortcut_fullscreen: "CommandOrControl+Shift+F".to_string(),
            cmd_copy: default_cmd_copy(),
            cmd_save: default_cmd_save(),
            cmd_pin: default_cmd_pin(),
            cmd_share: default_cmd_share(),
            cmd_print: default_cmd_print(),
            cmd_cancel: default_cmd_cancel(),
            save_folder: default_pictures.to_string_lossy().to_string(),
            output_format: "png".to_string(),
            launch_at_startup: false,
            show_magnifier: true,
            ultra_light_mode: false,
            startup_animation: true,
            festive_themes: true,
            auto_check_updates: false,
            updater_prompt_shown: false,
            clock_format_24h: false,
            clock_show_seconds: false,
            clock_show_date: false,
            clock_style: "glass-dark".to_string(),
            text_default_bg: true,
            text_default_bold: true,
            text_default_italic: false,
            text_default_size: 22,
        }
    }
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

pub fn load_settings(app: &AppHandle) -> AppSettings {
    let mut defaults = AppSettings::default_with_app(app);

    if let Ok(dir) = config_dir(app) {
        let settings_path = dir.join("settings.json");
        if settings_path.exists() {
            if let Ok(raw) = fs::read_to_string(&settings_path) {
                if let Ok(parsed) = serde_json::from_str::<AppSettings>(&raw) {
                    return parsed;
                }
            }
        }

        // Check language chosen during Windows NSIS installation
        let installer_lang_path = dir.join("installer_lang.txt");
        if installer_lang_path.exists() {
            if let Ok(lang_raw) = fs::read_to_string(&installer_lang_path) {
                let cleaned = lang_raw.trim().to_lowercase();
                if cleaned == "en" || cleaned == "es" {
                    defaults.language = cleaned;
                    defaults.language_selected = true;
                    let _ = save_settings(app, &defaults);
                    return defaults;
                }
            }
        }
    }

    // Fallback: detect system locale hint so first-run dialog highlights it
    if let Ok(lang_env) = std::env::var("LANG") {
        if lang_env.to_lowercase().starts_with("en") {
            defaults.language = "en".to_string();
        } else if lang_env.to_lowercase().starts_with("es") {
            defaults.language = "es".to_string();
        }
    }

    defaults
}

pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let dir = config_dir(app)?;
    let settings_path = dir.join("settings.json");
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(settings_path, json).map_err(|e| e.to_string())?;
    Ok(())
}
