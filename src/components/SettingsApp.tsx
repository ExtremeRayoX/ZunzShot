import React, { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { check, type Update } from "@tauri-apps/plugin-updater";
import {
  AlertTriangle,
  AppWindow,
  Camera,
  Check,
  Circle,
  Clock,
  Download,
  FolderOpen,
  Globe,
  Info,
  Keyboard,
  Lock,
  Maximize2,
  RefreshCw,
  RotateCcw,
  Settings,
  ShieldCheck,
  Square,
  Zap,
} from "lucide-react";
import { getTranslations } from "../i18n/translations";
import type {
  AppSettings,
  CaptureMode,
  ClockStickerStyle,
  Language,
} from "../types/zunzshot";
import {
  codeToHotkeyToken,
  DEFAULT_GLOBAL_SHORTCUTS,
  DEFAULT_OVERLAY_COMMANDS,
  formatKeyLabel,
  getEventModifiers,
  isModifierKeyEvent,
  normalizeAccelerator,
} from "../utils/shortcuts";

type TabId = "capture" | "general" | "shortcuts" | "stickers" | "about";

type GlobalShortcutKey =
  | "shortcutRegion"
  | "shortcutQuickCopy"
  | "shortcutCircle"
  | "shortcutWindow"
  | "shortcutFullscreen";

type OverlayCommandKey =
  | "cmdCopy"
  | "cmdSave"
  | "cmdPin"
  | "cmdShare"
  | "cmdPrint"
  | "cmdCancel";

type ShortcutFieldKey = GlobalShortcutKey | OverlayCommandKey;

const GLOBAL_SHORTCUT_FIELDS: readonly GlobalShortcutKey[] = [
  "shortcutRegion",
  "shortcutQuickCopy",
  "shortcutCircle",
  "shortcutWindow",
  "shortcutFullscreen",
];

const OVERLAY_CMD_FIELDS: readonly OverlayCommandKey[] = [
  "cmdCopy",
  "cmdSave",
  "cmdPin",
  "cmdShare",
  "cmdPrint",
  "cmdCancel",
];

export const SettingsApp: React.FC = () => {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [platform, setPlatform] = useState<string>("macos");
  const [hasPermission, setHasPermission] = useState<boolean | null>(null);
  const [appVersion, setAppVersion] = useState<string>("1.0.2");
  const [tab, setTab] = useState<TabId>("capture");
  const [savedBanner, setSavedBanner] = useState<boolean>(false);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<
    "idle" | "checking" | "up-to-date" | "available" | "downloading" | "installing" | "error"
  >("idle");
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);
  const [updateErrorMsg, setUpdateErrorMsg] = useState<string | null>(null);
  const [welcomeAutoUpdate, setWelcomeAutoUpdate] = useState<boolean>(false);
  const [recordingField, setRecordingField] = useState<ShortcutFieldKey | null>(null);
  const [liveTokens, setLiveTokens] = useState<string[]>([]);
  const [shortcutError, setShortcutError] = useState<string | null>(null);

  useEffect(() => {
    return () => {
      if (availableUpdate) {
        void availableUpdate.close().catch(() => {});
      }
    };
  }, [availableUpdate]);

  const runUpdateCheck = async () => {
    setUpdateState("checking");
    setUpdateErrorMsg(null);
    setDownloadProgress(0);
    try {
      if (availableUpdate) {
        await availableUpdate.close().catch(() => {});
        setAvailableUpdate(null);
      }
      const found = await check();
      if (found) {
        setAvailableUpdate(found);
        setUpdateState("available");
      } else {
        setUpdateState("up-to-date");
      }
    } catch (err) {
      setUpdateErrorMsg(
        err instanceof Error ? err.message : String(err ?? "")
      );
      setUpdateState("error");
    }
  };

  const installAvailableUpdate = async () => {
    if (!availableUpdate) return;
    setUpdateState("downloading");
    setDownloadProgress(0);
    setUpdateErrorMsg(null);
    let totalBytes = 0;
    let downloadedBytes = 0;
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          totalBytes = event.data.contentLength ?? 0;
          downloadedBytes = 0;
        } else if (event.event === "Progress") {
          downloadedBytes += event.data.chunkLength;
          if (totalBytes > 0) {
            setDownloadProgress(
              Math.min(99, Math.round((downloadedBytes / totalBytes) * 100))
            );
          }
        } else if (event.event === "Finished") {
          setDownloadProgress(100);
          setUpdateState("installing");
        }
      });
      setUpdateState("installing");
      await invoke("restart_app");
    } catch (err) {
      setUpdateErrorMsg(
        err instanceof Error ? err.message : String(err ?? "")
      );
      setUpdateState("error");
    }
  };

  useEffect(() => {
    void invoke<AppSettings>("get_settings").then(setSettings);
    void invoke<string>("get_platform").then(setPlatform);
    void invoke<boolean>("check_screen_capture_permission").then(setHasPermission);
    void invoke<string>("get_app_version").then(setAppVersion);
    void invoke<string | null>("consume_pending_capture_error").then((err) => {
      if (err) setCaptureError(err);
    });
    void invoke<string | null>("consume_pending_tab").then((pending) => {
      if (pending === "about") {
        setTab("about");
      } else if (pending === "check-updates") {
        setTab("about");
        void runUpdateCheck();
      } else if (pending === "general") {
        setTab("general");
      } else if (pending === "capture") {
        setTab("capture");
      }
    });

    const unlistenErr = listen<string>("zunzshot://capture-error", (ev) => {
      setCaptureError(ev.payload);
    });
    const unlistenPerm = listen<boolean>("zunzshot://permission-updated", (ev) => {
      setHasPermission(ev.payload);
    });
    const unlistenUpd = listen<AppSettings>(
      "zunzshot://settings-updated",
      (ev) => {
        setSettings(ev.payload);
      }
    );
    const unlistenTab = listen<string>("zunzshot://open-tab", (ev) => {
      if (ev.payload === "about") {
        setTab("about");
      } else if (ev.payload === "check-updates") {
        setTab("about");
        void runUpdateCheck();
      } else if (ev.payload === "general") {
        setTab("general");
      } else if (ev.payload === "capture") {
        setTab("capture");
      }
    });

    return () => {
      void unlistenErr.then((fn) => fn());
      void unlistenPerm.then((fn) => fn());
      void unlistenUpd.then((fn) => fn());
      void unlistenTab.then((fn) => fn());
    };
  }, []);

  // Poll and check on window focus while blocked on macOS so granting unlocks automatically
  useEffect(() => {
    if (platform !== "macos" || hasPermission !== false) return;
    const recheck = () => {
      void invoke<boolean>("check_screen_capture_permission").then(setHasPermission);
    };
    window.addEventListener("focus", recheck);
    const timer = window.setInterval(recheck, 1500);
    return () => {
      window.removeEventListener("focus", recheck);
      window.clearInterval(timer);
    };
  }, [platform, hasPermission]);

  const lang: Language = settings?.language ?? "es";
  const t = useMemo(() => getTranslations(lang), [lang]);

  // Pause global hotkeys while recording so pressed combos don't trigger screenshots,
  // and mark keys live as they are pressed down.
  useEffect(() => {
    if (!recordingField || tab !== "shortcuts" || !settings) {
      return;
    }

    const isOverlayCmd = (OVERLAY_CMD_FIELDS as readonly string[]).includes(
      recordingField
    );
    void invoke("set_shortcuts_paused", { paused: true });

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const mods = getEventModifiers(e, platform);

      // Esc alone cancels recording (unless explicitly re-assigning Escape to cmdCancel)
      const allowEscapeBind =
        recordingField === "cmdCancel" && settings.cmdCancel !== "Escape";
      if (e.code === "Escape" && mods.length === 0 && !allowEscapeBind) {
        setRecordingField(null);
        setLiveTokens([]);
        setShortcutError(null);
        return;
      }

      // Modifier pressed: show held keys live as they are pressed
      if (isModifierKeyEvent(e)) {
        setLiveTokens(mods);
        if (mods.length > 0) setShortcutError(null);
        return;
      }

      const mainToken = codeToHotkeyToken(e.code);

      // Global shortcuts always require > 1 key (modifier + main key)
      if (!isOverlayCmd && mods.length === 0) {
        setLiveTokens(mainToken ? [mainToken] : [e.key.toUpperCase()]);
        setShortcutError(t.shortcutNeedMoreKeys);
        return;
      }

      if (!mainToken) {
        setLiveTokens(mods);
        setShortcutError(t.shortcutUnsupportedKey);
        return;
      }

      const comboTokens = [...mods, mainToken];
      setLiveTokens(comboTokens);

      const nextCombo = comboTokens.join("+");
      const nextNorm = normalizeAccelerator(nextCombo, platform);
      const groupFields: readonly ShortcutFieldKey[] = isOverlayCmd
        ? OVERLAY_CMD_FIELDS
        : GLOBAL_SHORTCUT_FIELDS;
      const hasConflict = groupFields.some(
        (f) =>
          f !== recordingField &&
          normalizeAccelerator(settings[f], platform) === nextNorm
      );
      if (hasConflict) {
        setShortcutError(
          isOverlayCmd ? t.shortcutOverlayDuplicateErr : t.shortcutDuplicateErr
        );
        return;
      }

      const updated: AppSettings = { ...settings, [recordingField]: nextCombo };
      setRecordingField(null);
      setLiveTokens([]);
      setShortcutError(null);
      void invoke<AppSettings>("save_settings_cmd", { newSettings: updated }).then(
        (saved) => {
          setSettings(saved);
          setSavedBanner(true);
          window.setTimeout(() => setSavedBanner(false), 1800);
        }
      );
    };

    const onKeyUp = (e: KeyboardEvent) => {
      setLiveTokens(getEventModifiers(e, platform));
    };

    const onBlur = () => {
      setRecordingField(null);
      setLiveTokens([]);
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);

    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
      void invoke("set_shortcuts_paused", { paused: false });
    };
  }, [recordingField, tab, settings, platform, t]);

  useEffect(() => {
    if (tab !== "shortcuts" && recordingField) {
      setRecordingField(null);
      setLiveTokens([]);
      setShortcutError(null);
    }
  }, [tab, recordingField]);

  if (!settings || hasPermission === null) {
    return <div className="settings-loading">ZunzShot...</div>;
  }

  const handleLanguageChange = async (newLang: Language) => {
    const updated = await invoke<AppSettings>("set_language_cmd", {
      language: newLang,
    });
    setSettings(updated);
  };

  const handleCompleteOnboarding = async (
    language: Language | null,
    autoCheckUpdates: boolean
  ) => {
    const updated = await invoke<AppSettings>("complete_onboarding_cmd", {
      language,
      autoCheckUpdates,
    });
    setSettings(updated);
  };

  const handleSaveSettings = async (next: AppSettings) => {
    const saved = await invoke<AppSettings>("save_settings_cmd", {
      newSettings: next,
    });
    setSettings(saved);
    setSavedBanner(true);
    setTimeout(() => setSavedBanner(false), 1800);
  };

  const handlePickFolder = async () => {
    const dir = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: settings.saveFolder,
    });
    if (typeof dir === "string" && dir.length > 0) {
      setSettings({ ...settings, saveFolder: dir });
    }
  };

  const triggerCapture = async (mode: CaptureMode) => {
    setCaptureError(null);
    await invoke("start_capture", { mode });
  };

  const renderShortcutCard = (
    key: ShortcutFieldKey,
    label: string,
    isOverlayCmd: boolean
  ) => {
    const isRec = recordingField === key;
    const savedTokens = settings[key]
      .split("+")
      .map((p) => p.trim())
      .filter(Boolean);
    const shownTokens = isRec ? liveTokens : savedTokens;

    return (
      <div key={key} className="field-group">
        <label>{label}</label>
        <div
          role="button"
          tabIndex={0}
          className={`shortcut-recorder ${isOverlayCmd ? "compact" : ""} ${
            isRec ? "recording" : ""
          }`}
          onClick={() => {
            setShortcutError(null);
            setLiveTokens([]);
            setRecordingField(isRec ? null : key);
          }}
          onKeyDown={(e) => {
            if (!isRec && (e.key === "Enter" || e.key === " ")) {
              e.preventDefault();
              setShortcutError(null);
              setLiveTokens([]);
              setRecordingField(key);
            }
          }}
        >
          <div className="shortcut-keys">
            {shownTokens.length > 0 ? (
              shownTokens.map((tok, idx) => (
                <React.Fragment key={`${tok}-${idx}`}>
                  {idx > 0 && <span className="key-plus">+</span>}
                  <kbd className={`keycap ${isRec ? "live" : ""}`}>
                    {formatKeyLabel(tok, platform)}
                  </kbd>
                </React.Fragment>
              ))
            ) : (
              <span className="recording-placeholder">
                {isOverlayCmd
                  ? t.shortcutOverlayRecordingPlaceholder
                  : t.shortcutRecordingPlaceholder}
              </span>
            )}
            {isRec && shownTokens.length > 0 && (
              <>
                <span className="key-plus">+</span>
                <kbd className="keycap pending">...</kbd>
              </>
            )}
          </div>

          <span className="shortcut-action-pill">
            {isRec ? t.shortcutCancelBtn : t.shortcutChangeBtn}
          </span>
        </div>
      </div>
    );
  };

  // First-run Welcome Language Picker + Auto-updater opt-in (e.g. macOS first launch)
  if (!settings.languageSelected) {
    return (
      <div className="welcome-shell">
        <div className="welcome-card">
          <div className="brand-emblem">
            <img src="/zunzshot-logo.png" alt="ZunzShot" width="68" height="68" />
          </div>

          <h1>{t.welcomeTitle}</h1>
          <p className="welcome-sub">{t.welcomeSubtitle}</p>

          <div className="lang-cards">
            <button
              className={`lang-card ${settings.language === "es" ? "active" : ""}`}
              onClick={() => setSettings({ ...settings, language: "es" })}
            >
              <span className="lang-badge">ES</span>
              <strong>Español</strong>
            </button>
            <button
              className={`lang-card ${settings.language === "en" ? "active" : ""}`}
              onClick={() => setSettings({ ...settings, language: "en" })}
            >
              <span className="lang-badge">EN</span>
              <strong>English</strong>
            </button>
          </div>

          <div className="onboarding-updater-box">
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={welcomeAutoUpdate}
                onChange={(e) => setWelcomeAutoUpdate(e.target.checked)}
              />
              <span>{t.aboutSection.welcomeUpdaterOptIn}</span>
            </label>
            <p className="onboarding-updater-sub">
              {t.aboutSection.welcomeUpdaterSub}
            </p>
          </div>

          <button
            className="primary-cta"
            onClick={() =>
              void handleCompleteOnboarding(settings.language, welcomeAutoUpdate)
            }
          >
            {t.continueBtn}
          </button>
        </div>
      </div>
    );
  }

  // First-run Auto-updater prompt (for installs where language was pre-selected by installer)
  if (!settings.updaterPromptShown) {
    return (
      <div className="welcome-shell">
        <div className="welcome-card">
          <div className="brand-emblem">
            <img src="/zunzshot-logo.png" alt="ZunzShot" width="68" height="68" />
          </div>

          <h1>{t.aboutSection.onboardingTitle}</h1>
          <p className="welcome-sub">{t.aboutSection.onboardingSubtitle}</p>

          <div className="onboarding-actions">
            <button
              className="primary-cta"
              onClick={() => void handleCompleteOnboarding(null, true)}
            >
              {t.aboutSection.onboardingEnableBtn}
            </button>
            <button
              className="secondary-btn"
              style={{ justifyContent: "center", padding: "10px 14px" }}
              onClick={() => void handleCompleteOnboarding(null, false)}
            >
              {t.aboutSection.onboardingSkipBtn}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // Strict macOS Screen Recording Permission Gate — block ALL app options until verified
  if (platform === "macos" && !hasPermission) {
    const handleRequestPerm = async () => {
      const ok = await invoke<boolean>("request_screen_capture_permission");
      setHasPermission(ok);
    };

    return (
      <div className="welcome-shell">
        <div className="permission-gate-card">
          <div className="permission-gate-top">
            <img src="/zunzshot-logo.png" alt="ZunzShot" width="56" height="56" />
            <span className="permission-lock-pill">
              <Lock size={12} /> {t.permissionGate.badge}
            </span>
          </div>

          <h1>{t.permissionGate.title}</h1>
          <p className="permission-sub">{t.permissionGate.subtitle}</p>

          <ol className="permission-steps">
            <li>{t.permissionGate.step1}</li>
            <li>{t.permissionGate.step2}</li>
            <li>{t.permissionGate.step3}</li>
          </ol>

          <div className="permission-actions">
            <button
              className="primary-cta"
              onClick={() => void handleRequestPerm()}
            >
              <ShieldCheck size={16} /> {t.permissionGate.requestBtn}
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="settings-shell">
      <header className="settings-header">
        <div className="brand-row">
          <div className="brand-mini">
            <img src="/zunzshot-logo.png" alt="ZunzShot" width="40" height="40" />
          </div>
          <div>
            <h1>{t.appName}</h1>
            <p>{t.tagline}</p>
          </div>
        </div>
        <span className="version-badge-mini">v{appVersion}</span>
      </header>

      {/* Capture Error Helper (e.g. macOS Screen Recording permissions) */}
      {captureError && (
        <div className="error-banner">
          <div className="error-text">
            <AlertTriangle size={18} />
            <div>
              <strong>{t.captureErrorTitle}</strong>
              <p>{captureError}</p>
            </div>
          </div>
          {platform === "macos" && (
            <button
              className="secondary-btn"
              onClick={() => void invoke("open_screen_recording_settings")}
            >
              <ShieldCheck size={14} /> {t.openPermBtn}
            </button>
          )}
        </div>
      )}

      <div className="settings-workspace">
        {/* Clean Sidebar Separating Actions vs Configuration */}
        <aside className="settings-sidebar">
          <div className="sidebar-group">
            <span className="sidebar-group-label">{t.navQuickGroup}</span>
            <button
              className={`sidebar-btn ${tab === "capture" ? "active" : ""}`}
              onClick={() => setTab("capture")}
            >
              <Camera size={15} />
              <span>{t.navCapture}</span>
            </button>
          </div>

          <div className="sidebar-group">
            <span className="sidebar-group-label">{t.navSettingsGroup}</span>
            <button
              className={`sidebar-btn ${tab === "general" ? "active" : ""}`}
              onClick={() => setTab("general")}
            >
              <Settings size={15} />
              <span>{t.settingsTabs.general}</span>
            </button>
            <button
              className={`sidebar-btn ${tab === "shortcuts" ? "active" : ""}`}
              onClick={() => setTab("shortcuts")}
            >
              <Keyboard size={15} />
              <span>{t.settingsTabs.shortcuts}</span>
            </button>
            <button
              className={`sidebar-btn ${tab === "stickers" ? "active" : ""}`}
              onClick={() => setTab("stickers")}
            >
              <Clock size={15} />
              <span>{t.settingsTabs.stickers}</span>
            </button>
          </div>

          <div className="sidebar-footer">
            <button
              className={`sidebar-btn ${tab === "about" ? "active" : ""}`}
              onClick={() => setTab("about")}
            >
              <Info size={15} />
              <span>{t.settingsTabs.about}</span>
            </button>
          </div>
        </aside>

        {/* Main Content Pane */}
        <main className="tab-body">
          {tab === "capture" && (
            <section className="capture-hub">
              <div className="capture-hub-head">
                <h2>{t.quickActionsTitle}</h2>
                <p>{t.quickActionsSubtitle}</p>
              </div>
              <div className="quick-grid-2x2">
                <button
                  className="quick-card-pro featured-full"
                  onClick={() => void triggerCapture("quickCopy")}
                >
                  <div className="quick-card-icon">
                    <Zap size={20} />
                  </div>
                  <div className="quick-card-meta">
                    <strong>{t.modeQuickCopy}</strong>
                    <span className="quick-card-sub">{t.modeQuickCopyDesc}</span>
                    <code>
                      {settings.shortcutQuickCopy
                        .split("+")
                        .map((tok) => tok.trim())
                        .filter(Boolean)
                        .map((tok) => formatKeyLabel(tok, platform))
                        .join(" + ")}
                    </code>
                  </div>
                </button>
                {(
                  [
                    {
                      mode: "region",
                      label: t.modeRegion,
                      shortcut: settings.shortcutRegion,
                      Icon: Square,
                    },
                    {
                      mode: "circle",
                      label: t.modeCircle,
                      shortcut: settings.shortcutCircle,
                      Icon: Circle,
                    },
                    {
                      mode: "window",
                      label: t.modeWindow,
                      shortcut: settings.shortcutWindow,
                      Icon: AppWindow,
                    },
                    {
                      mode: "fullscreen",
                      label: t.modeFullscreen,
                      shortcut: settings.shortcutFullscreen,
                      Icon: Maximize2,
                    },
                  ] as const
                ).map(({ mode, label, shortcut, Icon }) => (
                  <button
                    key={mode}
                    className="quick-card-pro"
                    onClick={() => void triggerCapture(mode)}
                  >
                    <div className="quick-card-icon">
                      <Icon size={20} />
                    </div>
                    <div className="quick-card-meta">
                      <strong>{label}</strong>
                      <code>
                        {shortcut
                          .split("+")
                          .map((tok) => tok.trim())
                          .filter(Boolean)
                          .map((tok) => formatKeyLabel(tok, platform))
                          .join(" + ")}
                      </code>
                    </div>
                  </button>
                ))}
              </div>
            </section>
          )}

          {tab === "general" && (
            <div className="form-stack">
              <div className="field-group">
                <label className="label-inline">
                  <Globe size={14} /> {t.langLabel}
                </label>
                <div className="segmented">
                <button
                  className={lang === "es" ? "active" : ""}
                  onClick={() => void handleLanguageChange("es")}
                >
                  {t.langEs}
                </button>
                <button
                  className={lang === "en" ? "active" : ""}
                  onClick={() => void handleLanguageChange("en")}
                >
                  {t.langEn}
                </button>
              </div>
            </div>

            <div className="field-group">
              <label>{t.saveFolderLabel}</label>
              <div className="folder-row">
                <input
                  type="text"
                  value={settings.saveFolder}
                  onChange={(e) =>
                    setSettings({ ...settings, saveFolder: e.target.value })
                  }
                />
                <button className="secondary-btn" onClick={() => void handlePickFolder()}>
                  <FolderOpen size={14} /> {t.chooseFolder}
                </button>
              </div>
            </div>

            <div className="field-group">
              <label>{t.outputFormatLabel}</label>
              <select
                value={settings.outputFormat}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    outputFormat: e.target.value as "png" | "jpg",
                  })
                }
              >
                <option value="png">{t.pngAlphaHint}</option>
                <option value="jpg">{t.jpgHint}</option>
              </select>
            </div>

            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.launchAtStartup}
                onChange={(e) =>
                  setSettings({ ...settings, launchAtStartup: e.target.checked })
                }
              />
              <span>{t.launchStartup}</span>
            </label>

            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.showMagnifier}
                onChange={(e) =>
                  setSettings({ ...settings, showMagnifier: e.target.checked })
                }
              />
              <span>{t.showMagnifierLabel}</span>
            </label>

            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.autoCheckUpdates}
                onChange={(e) =>
                  setSettings({ ...settings, autoCheckUpdates: e.target.checked })
                }
              />
              <span>{t.aboutSection.autoUpdateLabel}</span>
            </label>

            <div className="ultra-light-group">
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.ultraLightMode}
                  onChange={(e) =>
                    setSettings({ ...settings, ultraLightMode: e.target.checked })
                  }
                />
                <span>{t.ultraLightLabel}</span>
              </label>
              {settings.ultraLightMode && (
                <div className="ultra-light-hint">
                  <Zap size={14} style={{ flexShrink: 0, marginTop: 2 }} />
                  <span>{t.ultraLightHint}</span>
                </div>
              )}
            </div>

            {platform === "macos" && (
              <div className="perm-tip">
                <span>{t.macosPermTip}</span>
                <button
                  className="secondary-btn"
                  onClick={() => void invoke("open_screen_recording_settings")}
                >
                  <ShieldCheck size={14} /> {t.openPermBtn}
                </button>
              </div>
            )}
          </div>
        )}

          {tab === "shortcuts" && (
            <div className="form-stack">
              <div className="shortcuts-section">
                <div className="shortcuts-section-head">
                  <h3 className="shortcuts-section-title">{t.shortcutsTitle}</h3>
                  <button
                    className="secondary-btn"
                    onClick={() =>
                      void handleSaveSettings({
                        ...settings,
                        ...DEFAULT_GLOBAL_SHORTCUTS,
                        ...DEFAULT_OVERLAY_COMMANDS,
                      })
                    }
                  >
                    <RotateCcw size={12} /> {t.shortcutResetDefaultsBtn}
                  </button>
                </div>
                <p className="muted-info">{t.shortcutHint}</p>

                {(
                  [
                    { key: "shortcutRegion", label: t.modeRegion },
                    { key: "shortcutQuickCopy", label: t.modeQuickCopy },
                    { key: "shortcutCircle", label: t.modeCircle },
                    { key: "shortcutWindow", label: t.modeWindow },
                    { key: "shortcutFullscreen", label: t.modeFullscreen },
                  ] as const
                ).map(({ key, label }) =>
                  renderShortcutCard(key, label, false)
                )}
              </div>

              <div className="shortcuts-section">
                <div className="shortcuts-section-head">
                  <h3 className="shortcuts-section-title">
                    {t.shortcutsOverlayTitle}
                  </h3>
                </div>
                <p className="muted-info">{t.shortcutsOverlayHint}</p>

                <div className="shortcuts-cmd-grid">
                  {(
                    [
                      { key: "cmdCopy", label: t.cmdLabels.cmdCopy },
                      { key: "cmdSave", label: t.cmdLabels.cmdSave },
                      { key: "cmdPin", label: t.cmdLabels.cmdPin },
                      { key: "cmdShare", label: t.cmdLabels.cmdShare },
                      { key: "cmdPrint", label: t.cmdLabels.cmdPrint },
                      { key: "cmdCancel", label: t.cmdLabels.cmdCancel },
                    ] as const
                  ).map(({ key, label }) =>
                    renderShortcutCard(key, label, true)
                  )}
                </div>
              </div>

              {shortcutError && (
                <div className="permission-warn-note">
                  <AlertTriangle size={14} />
                  <span>{shortcutError}</span>
                </div>
              )}
            </div>
          )}

        {tab === "stickers" && (
          <div className="form-stack">
            <p className="muted-info">{t.clockDefaultsTitle}</p>

            <div className="toggle-grid">
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.clockFormat24h}
                  onChange={(e) =>
                    setSettings({ ...settings, clockFormat24h: e.target.checked })
                  }
                />
                <span>{t.clockFormat24h}</span>
              </label>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.clockShowSeconds}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      clockShowSeconds: e.target.checked,
                    })
                  }
                />
                <span>{t.clockShowSeconds}</span>
              </label>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.clockShowDate}
                  onChange={(e) =>
                    setSettings({ ...settings, clockShowDate: e.target.checked })
                  }
                />
                <span>{t.clockShowDate}</span>
              </label>
            </div>

            <div className="field-group">
              <label>{t.clockStyleLabel}</label>
              <div className="segmented">
                {(
                  ["glass-dark", "glass-light", "minimal", "neon"] as ClockStickerStyle[]
                ).map((st) => (
                  <button
                    key={st}
                    className={settings.clockStyle === st ? "active" : ""}
                    onClick={() => setSettings({ ...settings, clockStyle: st })}
                  >
                    {t.clockStyles[st]}
                  </button>
                ))}
              </div>
            </div>

            {/* Live Preview of Clock Sticker */}
            <div className="clock-preview-stage">
              <div className={`clock-preview-pill style-${settings.clockStyle}`}>
                <div className="clock-preview-top">
                  <Clock size={16} />
                  <span>
                    {new Intl.DateTimeFormat(lang === "es" ? "es-ES" : "en-US", {
                      hour: "2-digit",
                      minute: "2-digit",
                      second: settings.clockShowSeconds ? "2-digit" : undefined,
                      hour12: !settings.clockFormat24h,
                    }).format(new Date())}
                  </span>
                </div>
                {settings.clockShowDate && (
                  <div className="clock-preview-date">
                    {new Intl.DateTimeFormat(lang === "es" ? "es-ES" : "en-US", {
                      day: "2-digit",
                      month: "short",
                      year: "numeric",
                    }).format(new Date())}
                  </div>
                )}
              </div>
            </div>
          </div>
        )}

        {tab === "about" && (
          <div className="about-panel">
            <img
              src="/zunzshot-logo.png"
              alt="ZunzShot"
              width="84"
              height="84"
              className="about-logo"
            />
            <h2>{t.aboutSection.title}</h2>
            <div className="version-pill">
              {t.aboutSection.versionLabel}: <strong>v{appVersion}</strong>
            </div>
            <p className="muted-info">
              {t.aboutSection.techLabel} ·{" "}
              {platform === "macos"
                ? "macOS"
                : platform === "windows"
                  ? "Windows"
                  : platform}
            </p>
            <p className="credits-line">{t.aboutSection.creditsLabel}</p>

            <div className="update-box">
              <button
                className="secondary-btn"
                onClick={() => void runUpdateCheck()}
                disabled={
                  updateState === "checking" ||
                  updateState === "downloading" ||
                  updateState === "installing"
                }
              >
                <RefreshCw
                  size={14}
                  className={updateState === "checking" ? "spin" : ""}
                />
                {updateState === "checking"
                  ? t.aboutSection.checking
                  : t.aboutSection.checkUpdatesBtn}
              </button>

              {updateState === "up-to-date" && (
                <div className="update-result">
                  <span className="update-ok">
                    <Check size={14} /> {t.aboutSection.upToDateMsg} (v{appVersion})
                  </span>
                </div>
              )}

              {updateState === "available" && availableUpdate && (
                <div className="update-available-card">
                  <strong>
                    {t.aboutSection.updateAvailableTitle} v{availableUpdate.version}
                  </strong>
                  {availableUpdate.body && (
                    <pre className="update-notes">{availableUpdate.body}</pre>
                  )}
                  <button
                    className="primary-cta small"
                    onClick={() => void installAvailableUpdate()}
                  >
                    <Download size={14} style={{ marginRight: 6, verticalAlign: -2 }} />
                    {t.aboutSection.installUpdateBtn}
                  </button>
                </div>
              )}

              {(updateState === "downloading" || updateState === "installing") && (
                <div className="update-progress-wrap">
                  <span>
                    {updateState === "installing"
                      ? t.aboutSection.installing
                      : `${t.aboutSection.downloading}${
                          downloadProgress > 0 ? ` ${downloadProgress}%` : ""
                        }`}
                  </span>
                  <div className="update-progress-track">
                    <div
                      className="update-progress-fill"
                      style={{
                        width: `${
                          updateState === "installing"
                            ? 100
                            : Math.max(8, downloadProgress)
                        }%`,
                      }}
                    />
                  </div>
                </div>
              )}

              {updateState === "error" && (
                <div className="update-error">
                  <AlertTriangle size={14} style={{ flexShrink: 0, marginTop: 1 }} />
                  <span>
                    {t.aboutSection.updateErrorMsg}
                    {updateErrorMsg ? ` (${updateErrorMsg})` : ""}
                  </span>
                </div>
              )}

              <div className="update-autocheck-card">
                <label className="checkbox-row">
                  <input
                    type="checkbox"
                    checked={settings.autoCheckUpdates}
                    onChange={(e) =>
                      void handleSaveSettings({
                        ...settings,
                        autoCheckUpdates: e.target.checked,
                      })
                    }
                  />
                  <span>{t.aboutSection.autoUpdateLabel}</span>
                </label>
                <span className="muted-info">{t.aboutSection.autoUpdateHint}</span>
              </div>
            </div>
          </div>
        )}
        </main>
      </div>

      <footer className="settings-footer">
        <span className="tray-note">{t.trayHint}</span>
        {(tab === "general" || tab === "shortcuts" || tab === "stickers") && (
          <div className="footer-actions">
            {savedBanner && (
              <span className="saved-pill">
                <Check size={14} /> {t.savedToast}
              </span>
            )}
            <button
              className="primary-cta small"
              onClick={() => void handleSaveSettings(settings)}
            >
              {t.saveSettings}
            </button>
          </div>
        )}
      </footer>
    </div>
  );
};
