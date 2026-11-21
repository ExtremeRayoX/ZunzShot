import React, { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  AppWindow,
  ArrowLeft,
  Camera,
  Check,
  ChevronRight,
  Circle,
  Clock,
  FolderOpen,
  Globe,
  Image as ImageIcon,
  Info,
  Keyboard,
  ListOrdered,
  Lock,
  Maximize2,
  RefreshCw,
  RotateCcw,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  Type,
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
  createEmptyChordState,
  DEFAULT_GLOBAL_SHORTCUTS,
  DEFAULT_OVERLAY_COMMANDS,
  formatKeyLabel,
  formatShortcutBadge,
  getEventModifiers,
  isModifierToken,
  isSingleGlobalShortcutAllowed,
  normalizeAccelerator,
  stepChordKeyDown,
  stepChordKeyUp,
} from "../utils/shortcuts";
import { UpdaterPanel } from "./UpdaterPanel";

type TabId = "capture" | "general" | "shortcuts" | "stickers" | "updates" | "about";

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
  const [savedSettings, setSavedSettings] = useState<AppSettings | null>(null);
  const [platform, setPlatform] = useState<string>("macos");
  const [hasPermission, setHasPermission] = useState<boolean | null>(null);
  const [appVersion, setAppVersion] = useState<string>("1.1.1");
  const [tab, setTab] = useState<TabId>("capture");
  const [extrasSubView, setExtrasSubView] = useState<"list" | "clock" | "text">("list");
  const [savedBanner, setSavedBanner] = useState<boolean>(false);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [welcomeAutoUpdate, setWelcomeAutoUpdate] = useState<boolean>(false);
  const [recordingField, setRecordingField] = useState<ShortcutFieldKey | null>(null);
  const [liveTokens, setLiveTokens] = useState<string[]>([]);
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [shortcutErrorField, setShortcutErrorField] = useState<ShortcutFieldKey | null>(null);
  const [failedShortcuts, setFailedShortcuts] = useState<string[]>([]);
  const shortcutSavedRef = useRef<boolean>(false);
  const settingsRef = useRef<AppSettings | null>(null);
  settingsRef.current = settings;

  useEffect(() => {
    if (tab !== "stickers") {
      setExtrasSubView("list");
    }
  }, [tab]);

  useEffect(() => {
    const applyPendingTab = (pending: string | null) => {
      if (
        pending === "capture" ||
        pending === "general" ||
        pending === "shortcuts" ||
        pending === "stickers" ||
        pending === "updates" ||
        pending === "about"
      ) {
        setTab(pending);
      }
    };

    void invoke<AppSettings>("get_settings").then((loaded) => {
      setSettings(loaded);
      setSavedSettings(loaded);
    });
    void invoke<string>("get_platform").then(setPlatform);
    void invoke<boolean>("check_screen_capture_permission").then(setHasPermission);
    void invoke<string>("get_app_version").then(setAppVersion);
    void invoke<string[]>("get_failed_shortcuts")
      .then(setFailedShortcuts)
      .catch(() => {});
    void invoke<string | null>("consume_pending_capture_error").then((err) => {
      if (err) setCaptureError(err);
    });
    void invoke<string | null>("consume_pending_tab").then(applyPendingTab);

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
        setSavedSettings(ev.payload);
      }
    );
    const unlistenAutoUpd = listen<boolean>(
      "zunzshot://auto-updates-updated",
      (ev) => {
        setSettings((prev) =>
          prev ? { ...prev, autoCheckUpdates: ev.payload } : prev
        );
        setSavedSettings((prev) =>
          prev ? { ...prev, autoCheckUpdates: ev.payload } : prev
        );
      }
    );
    const unlistenShortcuts = listen<string[]>(
      "zunzshot://shortcuts-status",
      (ev) => {
        setFailedShortcuts(ev.payload);
      }
    );
    const unlistenTab = listen<string>("zunzshot://open-tab", (ev) => {
      void invoke("consume_pending_tab").catch(() => {});
      applyPendingTab(ev.payload);
    });

    return () => {
      void unlistenErr.then((fn) => fn());
      void unlistenPerm.then((fn) => fn());
      void unlistenUpd.then((fn) => fn());
      void unlistenAutoUpd.then((fn) => fn());
      void unlistenShortcuts.then((fn) => fn());
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

  const hasSettings = Boolean(settings);

  const isRecordingShortcuts = Boolean(
    recordingField && tab === "shortcuts" && hasSettings
  );

  // Keep global OS hotkeys (and macOS default window menu accelerators) paused cleanly
  // across field switches without race conditions until recording actually ends.
  useEffect(() => {
    if (!isRecordingShortcuts) {
      return;
    }
    shortcutSavedRef.current = false;
    void invoke("set_shortcuts_paused", { paused: true }).catch(() => {});
    return () => {
      if (!shortcutSavedRef.current) {
        void invoke("set_shortcuts_paused", { paused: false }).catch(() => {});
      }
    };
  }, [isRecordingShortcuts]);

  // Capture full multi-key chords (2, 3, or more keys) live while held and commit on release.
  useEffect(() => {
    if (!recordingField || tab !== "shortcuts" || !hasSettings) {
      return;
    }

    let committed = false;
    let chord = createEmptyChordState();
    const isOverlayCmd = (OVERLAY_CMD_FIELDS as readonly string[]).includes(
      recordingField
    );

    const tryCommitShortcut = (mainToken: string | null, mods: string[]): boolean => {
      if (committed) return true;
      const currentSettings = settingsRef.current;
      if (!currentSettings) return false;

      if (!mainToken) {
        setLiveTokens(mods);
        setShortcutErrorField(recordingField);
        setShortcutError(t.shortcutUnsupportedKey);
        return false;
      }

      if (
        !isOverlayCmd &&
        mods.length === 0 &&
        !isSingleGlobalShortcutAllowed(mainToken, platform)
      ) {
        setLiveTokens([]);
        setShortcutErrorField(recordingField);
        setShortcutError(
          platform === "windows"
            ? t.shortcutNeedMoreKeysWindows
            : t.shortcutNeedMoreKeys
        );
        return false;
      }

      const comboTokens = [...mods, mainToken];
      const nextCombo = comboTokens.join("+");
      const nextNorm = normalizeAccelerator(nextCombo, platform);
      const groupFields: readonly ShortcutFieldKey[] = isOverlayCmd
        ? OVERLAY_CMD_FIELDS
        : GLOBAL_SHORTCUT_FIELDS;
      const hasConflict = groupFields.some(
        (f) =>
          f !== recordingField &&
          normalizeAccelerator(currentSettings[f], platform) === nextNorm
      );
      if (hasConflict) {
        setLiveTokens([]);
        setShortcutErrorField(recordingField);
        setShortcutError(
          isOverlayCmd ? t.shortcutOverlayDuplicateErr : t.shortcutDuplicateErr
        );
        return false;
      }

      committed = true;
      const updated: AppSettings = { ...currentSettings, [recordingField]: nextCombo };
      shortcutSavedRef.current = false;
      setSettings(updated);
      setRecordingField(null);
      setLiveTokens([]);
      setShortcutError(null);
      setShortcutErrorField(null);
      return true;
    };

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat) return;

      const currentSettings = settingsRef.current;
      if (!currentSettings) return;

      const currentMods = getEventModifiers(e, platform);

      // Esc alone cancels recording (unless explicitly re-assigning Escape to cmdCancel)
      const allowEscapeBind =
        recordingField === "cmdCancel" && currentSettings.cmdCancel !== "Escape";
      if (
        e.code === "Escape" &&
        currentMods.length === 0 &&
        chord.mods.length === 0 &&
        !chord.mainToken &&
        !allowEscapeBind
      ) {
        setRecordingField(null);
        setLiveTokens([]);
        setShortcutError(null);
        setShortcutErrorField(null);
        return;
      }

      const step = stepChordKeyDown(chord, e, platform);
      chord = step.state;
      setLiveTokens(step.liveTokens);
      if (step.unsupportedError) {
        setShortcutErrorField(recordingField);
        setShortcutError(t.shortcutUnsupportedKey);
      } else if (step.liveTokens.length > 0) {
        setShortcutError(null);
        setShortcutErrorField(null);
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (committed) return;
      e.preventDefault();
      e.stopPropagation();

      const step = stepChordKeyUp(chord, e, platform);
      chord = step.state;
      if (step.commit) {
        const ok = tryCommitShortcut(step.commit.mainToken, step.commit.mods);
        if (!ok) {
          chord = createEmptyChordState();
        }
        return;
      }
      setLiveTokens(step.liveTokens);
    };

    const onBlur = () => {
      chord = createEmptyChordState();
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
    };
  }, [recordingField, tab, hasSettings, platform, t]);

  useEffect(() => {
    if (tab !== "shortcuts" && recordingField) {
      setRecordingField(null);
      setLiveTokens([]);
      setShortcutError(null);
      setShortcutErrorField(null);
    }
  }, [tab, recordingField]);

  if (!settings || hasPermission === null) {
    return <div className="settings-loading">ZunzShot...</div>;
  }

  const handleLanguageChange = async (newLang: Language) => {
    setSaveError(null);
    try {
      const updated = await invoke<AppSettings>("set_language_cmd", {
        language: newLang,
      });
      setSavedSettings(updated);
      setSettings((prev) =>
        prev ? { ...prev, language: updated.language } : updated
      );
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err ?? ""));
    }
  };

  const handleCompleteOnboarding = async (
    language: Language | null,
    autoCheckUpdates: boolean
  ) => {
    setSaveError(null);
    try {
      const updated = await invoke<AppSettings>("complete_onboarding_cmd", {
        language,
        autoCheckUpdates,
      });
      setSettings(updated);
      setSavedSettings(updated);
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err ?? ""));
    }
  };

  const handleSaveSettings = async (next: AppSettings) => {
    setSaveError(null);
    setShortcutError(null);
    try {
      const saved = await invoke<AppSettings>("save_settings_cmd", {
        newSettings: next,
      });
      setSettings(saved);
      setSavedSettings(saved);
      setSavedBanner(true);
      setTimeout(() => setSavedBanner(false), 1800);
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err ?? ""));
    }
  };

  const handleToggleAutoCheckUpdates = async (enabled: boolean) => {
    setSaveError(null);
    try {
      const updated = await invoke<AppSettings>("set_auto_check_updates_cmd", {
        enabled,
      });
      setSettings((prev) =>
        prev ? { ...prev, autoCheckUpdates: updated.autoCheckUpdates } : updated
      );
      setSavedSettings((prev) =>
        prev ? { ...prev, autoCheckUpdates: updated.autoCheckUpdates } : updated
      );
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err ?? ""));
    }
  };

  const isGeneralDirty = Boolean(
    settings &&
      savedSettings &&
      (settings.saveFolder !== savedSettings.saveFolder ||
        settings.outputFormat !== savedSettings.outputFormat ||
        settings.launchAtStartup !== savedSettings.launchAtStartup ||
        settings.showMagnifier !== savedSettings.showMagnifier ||
        settings.ultraLightMode !== savedSettings.ultraLightMode ||
        settings.startupAnimation !== savedSettings.startupAnimation ||
        settings.festiveThemes !== savedSettings.festiveThemes)
  );

  const isShortcutsDirty = Boolean(
    settings &&
      savedSettings &&
      (settings.shortcutRegion !== savedSettings.shortcutRegion ||
        settings.shortcutQuickCopy !== savedSettings.shortcutQuickCopy ||
        settings.shortcutCircle !== savedSettings.shortcutCircle ||
        settings.shortcutWindow !== savedSettings.shortcutWindow ||
        settings.shortcutFullscreen !== savedSettings.shortcutFullscreen ||
        settings.cmdCopy !== savedSettings.cmdCopy ||
        settings.cmdSave !== savedSettings.cmdSave ||
        settings.cmdPin !== savedSettings.cmdPin ||
        settings.cmdShare !== savedSettings.cmdShare ||
        settings.cmdPrint !== savedSettings.cmdPrint ||
        settings.cmdCancel !== savedSettings.cmdCancel)
  );

  const isStickersDirty = Boolean(
    settings &&
      savedSettings &&
      (settings.clockFormat24h !== savedSettings.clockFormat24h ||
        settings.clockShowSeconds !== savedSettings.clockShowSeconds ||
        settings.clockShowDate !== savedSettings.clockShowDate ||
        settings.clockStyle !== savedSettings.clockStyle)
  );

  const isTextDefaultsDirty = Boolean(
    settings &&
      savedSettings &&
      (settings.textDefaultBg !== savedSettings.textDefaultBg ||
        settings.textDefaultBold !== savedSettings.textDefaultBold ||
        settings.textDefaultItalic !== savedSettings.textDefaultItalic ||
        settings.textDefaultSize !== savedSettings.textDefaultSize)
  );

  const renderInMenuSaveBar = (isDirty: boolean, onDiscard: () => void) => {
    if (!settings) return null;
    if (!isDirty) {
      return savedBanner ? (
        <div className="in-menu-saved-toast">
          <Check size={14} /> {t.savedToast}
        </div>
      ) : null;
    }
    return (
      <div className="in-menu-save-bar">
        <span className="in-menu-save-bar-hint">{t.unsavedChangesHint}</span>
        <div className="in-menu-save-bar-actions">
          <button
            type="button"
            className="secondary-btn small"
            onClick={onDiscard}
          >
            {t.discardChanges}
          </button>
          <button
            type="button"
            className="primary-cta small"
            onClick={() => void handleSaveSettings(settings)}
          >
            {t.saveSettings}
          </button>
        </div>
      </div>
    );
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
    try {
      await invoke("start_capture", { mode });
    } catch (err) {
      setCaptureError(err instanceof Error ? err.message : String(err ?? ""));
    }
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
    const hasLiveMainKey = isRec && shownTokens.some((tok) => !isModifierToken(tok));
    const fieldError = shortcutError && shortcutErrorField === key ? shortcutError : null;

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
            setShortcutErrorField(null);
            setLiveTokens([]);
            setRecordingField(isRec ? null : key);
          }}
          onKeyDown={(e) => {
            if (!isRec && (e.key === "Enter" || e.key === " ")) {
              e.preventDefault();
              setShortcutError(null);
              setShortcutErrorField(null);
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
                  : platform === "windows"
                    ? t.shortcutRecordingPlaceholderWindows
                    : t.shortcutRecordingPlaceholder}
              </span>
            )}
            {isRec && shownTokens.length > 0 && !hasLiveMainKey && (
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
        {fieldError && (
          <div className="permission-warn-note" style={{ marginTop: 4 }}>
            <AlertTriangle size={14} />
            <span>{fieldError}</span>
          </div>
        )}
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

      {/* Save Error Banner across all configuration tabs */}
      {saveError && (
        <div className="error-banner">
          <div className="error-text">
            <AlertTriangle size={18} />
            <div>
              <p>{saveError}</p>
            </div>
          </div>
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
              onClick={() => {
                setTab("stickers");
                setExtrasSubView("list");
              }}
            >
              <Sparkles size={15} />
              <span>{t.settingsTabs.stickers}</span>
            </button>
          </div>

          <div className="sidebar-footer">
            <button
              className={`sidebar-btn ${tab === "updates" ? "active" : ""}`}
              onClick={() => setTab("updates")}
            >
              <RefreshCw size={15} />
              <span>{t.settingsTabs.updates}</span>
            </button>
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

            <div className="ultra-light-group">
              <label
                className={`checkbox-row ${settings.ultraLightMode ? "is-disabled" : ""}`}
              >
                <input
                  type="checkbox"
                  disabled={settings.ultraLightMode}
                  checked={!settings.ultraLightMode && settings.startupAnimation}
                  onChange={(e) =>
                    setSettings({ ...settings, startupAnimation: e.target.checked })
                  }
                />
                <span>
                  {t.startupAnimationLabel}
                  {settings.ultraLightMode ? ` (${t.startupAnimationUltraLightHint})` : ""}
                </span>
              </label>

              <label
                className={`checkbox-row nested-checkbox-row ${
                  settings.ultraLightMode || !settings.startupAnimation ? "is-disabled" : ""
                }`}
              >
                <input
                  type="checkbox"
                  disabled={settings.ultraLightMode || !settings.startupAnimation}
                  checked={
                    !settings.ultraLightMode &&
                    settings.startupAnimation &&
                    settings.festiveThemes
                  }
                  onChange={(e) =>
                    setSettings({ ...settings, festiveThemes: e.target.checked })
                  }
                />
                <span>{t.festiveThemesLabel}</span>
              </label>
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

            {renderInMenuSaveBar(isGeneralDirty, () =>
              setSettings((prev) =>
                prev && savedSettings
                  ? {
                      ...prev,
                      saveFolder: savedSettings.saveFolder,
                      outputFormat: savedSettings.outputFormat,
                      launchAtStartup: savedSettings.launchAtStartup,
                      showMagnifier: savedSettings.showMagnifier,
                      ultraLightMode: savedSettings.ultraLightMode,
                      startupAnimation: savedSettings.startupAnimation,
                      festiveThemes: savedSettings.festiveThemes,
                    }
                  : prev
              )
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
                    onClick={() => {
                      setRecordingField(null);
                      setLiveTokens([]);
                      setShortcutError(null);
                      setShortcutErrorField(null);
                      setSettings({
                        ...settings,
                        ...DEFAULT_GLOBAL_SHORTCUTS,
                        ...DEFAULT_OVERLAY_COMMANDS,
                      });
                    }}
                  >
                    <RotateCcw size={12} /> {t.shortcutResetDefaultsBtn}
                  </button>
                </div>
                <p className="muted-info">
                  {platform === "windows" ? t.shortcutHintWindows : t.shortcutHint}
                </p>

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

              {shortcutError && !shortcutErrorField && (
                <div className="permission-warn-note">
                  <AlertTriangle size={14} />
                  <span>{shortcutError}</span>
                </div>
              )}

              {failedShortcuts.length > 0 && (
                <div className="permission-warn-note">
                  <AlertTriangle size={14} />
                  <span>
                    {t.shortcutOsConflictWarn}{" "}
                    {failedShortcuts
                      .map((s) => formatShortcutBadge(s, platform))
                      .join(", ")}
                  </span>
                </div>
              )}

              {renderInMenuSaveBar(isShortcutsDirty, () =>
                setSettings((prev) =>
                  prev && savedSettings
                    ? {
                        ...prev,
                        shortcutRegion: savedSettings.shortcutRegion,
                        shortcutQuickCopy: savedSettings.shortcutQuickCopy,
                        shortcutCircle: savedSettings.shortcutCircle,
                        shortcutWindow: savedSettings.shortcutWindow,
                        shortcutFullscreen: savedSettings.shortcutFullscreen,
                        cmdCopy: savedSettings.cmdCopy,
                        cmdSave: savedSettings.cmdSave,
                        cmdPin: savedSettings.cmdPin,
                        cmdShare: savedSettings.cmdShare,
                        cmdPrint: savedSettings.cmdPrint,
                        cmdCancel: savedSettings.cmdCancel,
                      }
                    : prev
                )
              )}
            </div>
          )}

        {tab === "stickers" && extrasSubView === "list" && (
          <div className="extras-hub">
            <div className="extras-hub-head">
              <h2>{t.extrasMenu.title}</h2>
              <p className="muted-info">{t.extrasMenu.subtitle}</p>
            </div>
            <div className="extras-list">
              <button
                type="button"
                className="extras-item-card interactive"
                onClick={() => setExtrasSubView("clock")}
              >
                <div className="extras-card-icon accent">
                  <Clock size={20} />
                </div>
                <div className="extras-card-body">
                  <strong>{t.extrasMenu.clockCardTitle}</strong>
                  <span>{t.extrasMenu.clockCardDesc}</span>
                </div>
                <div className="extras-card-meta">
                  <span className="extras-config-pill">{t.extrasMenu.configureBtn}</span>
                  <ChevronRight size={16} />
                </div>
              </button>

              <button
                type="button"
                className="extras-item-card interactive"
                onClick={() => setExtrasSubView("text")}
              >
                <div className="extras-card-icon accent">
                  <Type size={20} />
                </div>
                <div className="extras-card-body">
                  <strong>{t.extrasMenu.textCardTitle}</strong>
                  <span>{t.extrasMenu.textCardDesc}</span>
                </div>
                <div className="extras-card-meta">
                  <span className="extras-config-pill">
                    {t.extrasMenu.configureBtn} <ChevronRight size={14} />
                  </span>
                </div>
              </button>

              <div className="extras-item-card">
                <div className="extras-card-icon">
                  <ImageIcon size={20} />
                </div>
                <div className="extras-card-body">
                  <strong>{t.extrasMenu.photoCardTitle}</strong>
                  <span>{t.extrasMenu.photoCardDesc}</span>
                </div>
                <div className="extras-card-meta">
                  <span className="extras-badge">{t.extrasMenu.inEditorBadge}</span>
                </div>
              </div>

              <div className="extras-item-card">
                <div className="extras-card-icon">
                  <ListOrdered size={20} />
                </div>
                <div className="extras-card-body">
                  <strong>{t.extrasMenu.stepsCardTitle}</strong>
                  <span>{t.extrasMenu.stepsCardDesc}</span>
                </div>
                <div className="extras-card-meta">
                  <span className="extras-badge">{t.extrasMenu.inEditorBadge}</span>
                </div>
              </div>
            </div>
          </div>
        )}

        {tab === "stickers" && extrasSubView === "clock" && (
          <div className="form-stack">
            <div className="extras-subview-head">
              <button
                type="button"
                className="secondary-btn small"
                onClick={() => setExtrasSubView("list")}
              >
                <ArrowLeft size={14} /> {t.extrasMenu.backToExtrasBtn}
              </button>
              <span className="extras-subview-title">{t.extrasMenu.clockCardTitle}</span>
            </div>
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

            {renderInMenuSaveBar(isStickersDirty, () =>
              setSettings((prev) =>
                prev && savedSettings
                  ? {
                      ...prev,
                      clockFormat24h: savedSettings.clockFormat24h,
                      clockShowSeconds: savedSettings.clockShowSeconds,
                      clockShowDate: savedSettings.clockShowDate,
                      clockStyle: savedSettings.clockStyle,
                    }
                  : prev
              )
            )}
          </div>
        )}

        {tab === "stickers" && extrasSubView === "text" && (
          <div className="panel">
            <div className="extras-subview-head">
              <button
                type="button"
                className="secondary-btn"
                onClick={() => setExtrasSubView("list")}
              >
                <ArrowLeft size={14} /> {t.extrasMenu.backToExtrasBtn}
              </button>
              <span className="extras-subview-title">{t.textDefaults.title}</span>
            </div>

            <p className="muted-info">{t.textDefaults.subtitle}</p>

            <div className="toggle-grid">
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.textDefaultBg}
                  onChange={(e) =>
                    setSettings({ ...settings, textDefaultBg: e.target.checked })
                  }
                />
                <span>{t.textDefaults.defaultBg}</span>
              </label>

              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.textDefaultBold}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      textDefaultBold: e.target.checked,
                    })
                  }
                />
                <span>{t.textDefaults.defaultBold}</span>
              </label>

              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={settings.textDefaultItalic}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      textDefaultItalic: e.target.checked,
                    })
                  }
                />
                <span>{t.textDefaults.defaultItalic}</span>
              </label>
            </div>

            <div className="field-group">
              <label>{t.textDefaults.defaultSize}</label>
              <div className="segmented">
                {[16, 22, 28, 36].map((sz) => (
                  <button
                    key={sz}
                    type="button"
                    className={settings.textDefaultSize === sz ? "active" : ""}
                    onClick={() =>
                      setSettings({ ...settings, textDefaultSize: sz })
                    }
                  >
                    {sz}px
                  </button>
                ))}
              </div>
            </div>

            <div className="clock-preview-stage">
              <div
                className={`text-preview-pill ${
                  settings.textDefaultBg ? "with-bg" : "no-bg"
                }`}
                style={{
                  fontSize: settings.textDefaultSize,
                  fontWeight: settings.textDefaultBold ? 700 : 400,
                  fontStyle: settings.textDefaultItalic ? "italic" : "normal",
                }}
              >
                {t.textDefaults.previewSample}
              </div>
            </div>

            {renderInMenuSaveBar(isTextDefaultsDirty, () =>
              setSettings((prev) =>
                prev && savedSettings
                  ? {
                      ...prev,
                      textDefaultBg: savedSettings.textDefaultBg,
                      textDefaultBold: savedSettings.textDefaultBold,
                      textDefaultItalic: savedSettings.textDefaultItalic,
                      textDefaultSize: savedSettings.textDefaultSize,
                    }
                  : prev
              )
            )}
          </div>
        )}

        {tab === "updates" && (
          <UpdaterPanel
            mode="embedded"
            lang={settings.language}
            appVersion={appVersion}
            autoCheckUpdates={settings.autoCheckUpdates}
            onToggleAutoCheckUpdates={handleToggleAutoCheckUpdates}
          />
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
          </div>
        )}
        </main>
      </div>

      <footer className="settings-footer">
        <span className="tray-note">{t.trayHint}</span>
      </footer>
    </div>
  );
};
