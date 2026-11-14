import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import {
  AlertTriangle,
  Bell,
  Check,
  Clock,
  Download,
  RefreshCw,
} from "lucide-react";
import { getTranslations } from "../i18n/translations";
import type { Language } from "../types/zunzshot";
import { ReleaseNotesView } from "./ReleaseNotesView";

export type UpdatePreview = {
  version: string;
  body?: string | null;
};

export type UpdaterLaunchMode = "manual" | "alert";

type UpdateUiState =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

const SNOOZE_HOURS_OPTIONS = [2, 4, 6] as const;

type UpdaterPanelProps = {
  mode: "standalone" | "embedded";
  launchMode?: UpdaterLaunchMode;
  autoCheckNonce?: number;
  lang: Language;
  appVersion: string;
  autoCheckUpdates: boolean;
  onToggleAutoCheckUpdates: (enabled: boolean) => Promise<void>;
  initialPreview?: UpdatePreview | null;
  onRequestClose?: () => void;
};

export const UpdaterPanel: React.FC<UpdaterPanelProps> = ({
  mode,
  launchMode = "manual",
  autoCheckNonce = 0,
  lang,
  appVersion,
  autoCheckUpdates,
  onToggleAutoCheckUpdates,
  initialPreview = null,
  onRequestClose,
}) => {
  const t = getTranslations(lang);
  const [updateState, setUpdateState] = useState<UpdateUiState>(
    mode === "standalone" && launchMode === "alert" && initialPreview
      ? "available"
      : "idle"
  );
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);
  const [updateErrorMsg, setUpdateErrorMsg] = useState<string | null>(null);
  const [showSnoozeOptions, setShowSnoozeOptions] = useState<boolean>(false);
  const [snoozedHoursNotice, setSnoozedHoursNotice] = useState<number | null>(null);
  const [savingAutoUpdate, setSavingAutoUpdate] = useState<boolean>(false);

  const availableUpdateRef = useRef<Update | null>(null);
  const updateStateRef = useRef<UpdateUiState>(updateState);
  updateStateRef.current = updateState;

  const replaceAvailableUpdate = useCallback((next: Update | null) => {
    const prev = availableUpdateRef.current;
    availableUpdateRef.current = next;
    setAvailableUpdate(next);
    if (prev && prev !== next) {
      void prev.close().catch(() => {});
    }
  }, []);

  useEffect(() => {
    return () => {
      if (availableUpdateRef.current) {
        void availableUpdateRef.current.close().catch(() => {});
        availableUpdateRef.current = null;
      }
    };
  }, []);

  const runUpdateCheck = useCallback(async () => {
    if (
      updateStateRef.current === "downloading" ||
      updateStateRef.current === "installing"
    ) {
      return;
    }
    setUpdateState("checking");
    setUpdateErrorMsg(null);
    setDownloadProgress(0);
    setShowSnoozeOptions(false);
    setSnoozedHoursNotice(null);
    try {
      replaceAvailableUpdate(null);
      const found = await check();
      if (found) {
        replaceAvailableUpdate(found);
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
  }, [replaceAvailableUpdate]);

  useEffect(() => {
    if (mode === "standalone" && autoCheckNonce > 0) {
      void runUpdateCheck();
    }
  }, [mode, autoCheckNonce, runUpdateCheck]);

  const installAvailableUpdate = useCallback(async () => {
    let target = availableUpdateRef.current;
    if (!target) {
      // If background alert preview rendered before live handle completed, fetch now.
      setUpdateState("checking");
      setUpdateErrorMsg(null);
      try {
        target = await check();
        if (!target) {
          setUpdateState("up-to-date");
          return;
        }
        replaceAvailableUpdate(target);
      } catch (err) {
        setUpdateErrorMsg(
          err instanceof Error ? err.message : String(err ?? "")
        );
        setUpdateState("error");
        return;
      }
    }

    setShowSnoozeOptions(false);
    setUpdateState("downloading");
    setDownloadProgress(0);
    setUpdateErrorMsg(null);
    let totalBytes = 0;
    let downloadedBytes = 0;
    try {
      await target.downloadAndInstall((event) => {
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
  }, [replaceAvailableUpdate]);

  const handleSelectSnoozeHours = async (hours: (typeof SNOOZE_HOURS_OPTIONS)[number]) => {
    const version = availableUpdate?.version ?? initialPreview?.version ?? null;
    try {
      await invoke("postpone_update_reminder", { version, hours });
    } catch {
      // Best-effort snooze persistence
    }
    setShowSnoozeOptions(false);
    if (mode === "standalone" && onRequestClose) {
      onRequestClose();
    } else {
      setSnoozedHoursNotice(hours);
    }
  };

  const handleToggleAutoCheck = async (enabled: boolean) => {
    setSavingAutoUpdate(true);
    try {
      await onToggleAutoCheckUpdates(enabled);
    } finally {
      setSavingAutoUpdate(false);
    }
  };

  const displayVersion = availableUpdate?.version ?? initialPreview?.version ?? null;
  const displayNotes = availableUpdate?.body ?? initialPreview?.body ?? null;
  const hasVisibleUpdate =
    updateState === "available" ||
    (mode === "standalone" &&
      launchMode === "alert" &&
      Boolean(displayVersion) &&
      updateState !== "up-to-date" &&
      updateState !== "downloading" &&
      updateState !== "installing");
  const isBusy =
    updateState === "checking" ||
    updateState === "downloading" ||
    updateState === "installing";
  const isAlertHero =
    mode === "standalone" && launchMode === "alert" && (hasVisibleUpdate || isBusy);

  const snoozeLabel = (hours: 2 | 4 | 6) => {
    if (hours === 2) return t.aboutSection.remind2h;
    if (hours === 4) return t.aboutSection.remind4h;
    return t.aboutSection.remind6h;
  };

  return (
    <div className={`updater-panel ${mode === "standalone" ? "is-standalone" : ""}`}>
      <div className="updater-hero">
        <div className={`updater-hero-icon ${isAlertHero ? "is-alert" : ""}`}>
          {isAlertHero ? <Bell size={22} /> : <RefreshCw size={22} className={updateState === "checking" ? "spin" : ""} />}
        </div>
        <div className="updater-hero-text">
          <h2>
            {isAlertHero
              ? t.aboutSection.updatePromptTitle
              : t.aboutSection.updatesTitle}
          </h2>
          <p className="updater-hero-sub">
            {isAlertHero
              ? t.aboutSection.updatePromptSubtitle
              : t.aboutSection.updatesSubtitle}
          </p>
        </div>
        <div className="version-pill updater-version-pill">
          {t.aboutSection.currentVersionLabel}: <strong>v{appVersion}</strong>
        </div>
      </div>

      {!(launchMode === "alert" && hasVisibleUpdate) && (
        <div className="updater-check-bar">
          <button
            type="button"
            className="secondary-btn"
            onClick={() => void runUpdateCheck()}
            disabled={isBusy}
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
            <span className="update-ok">
              <Check size={14} /> {t.aboutSection.upToDateMsg} (v{appVersion})
            </span>
          )}
        </div>
      )}

      {updateState === "checking" && launchMode === "alert" && !displayVersion && (
        <div className="update-prompt-status">
          <RefreshCw size={14} className="spin" />
          <span>{t.aboutSection.checking}</span>
        </div>
      )}

      {hasVisibleUpdate && displayVersion && (
        <div className="update-available-card updater-release-card">
          <div className="update-available-head">
            <div className="update-available-title-group">
              <strong>
                {t.aboutSection.updateAvailableTitle} v{displayVersion}
              </strong>
              <span className="version-badge-mini">
                v{appVersion} → v{displayVersion}
              </span>
            </div>
            {updateState === "checking" && (
              <span className="update-refresh-chip">
                <RefreshCw size={12} className="spin" />
              </span>
            )}
          </div>

          <div className="release-notes-section">
            <span className="release-notes-caption">
              {t.aboutSection.releaseNotesLabel}
            </span>
            <ReleaseNotesView
              body={displayNotes}
              emptyText={t.aboutSection.releaseNotesEmpty}
            />
          </div>

          {snoozedHoursNotice !== null && (
            <div className="update-snooze-confirmed">
              <Clock size={13} />
              <span>
                {t.aboutSection.remindScheduledMsg.replace(
                  "{hours}",
                  String(snoozedHoursNotice)
                )}
              </span>
            </div>
          )}

          {showSnoozeOptions ? (
            <div className="update-snooze-box">
              <div className="update-snooze-header">
                <span className="update-snooze-label">
                  <Clock size={13} /> {t.aboutSection.remindPromptLabel}
                </span>
                <button
                  type="button"
                  className="update-snooze-cancel"
                  onClick={() => setShowSnoozeOptions(false)}
                >
                  {t.aboutSection.remindCancelBtn}
                </button>
              </div>
              <div className="update-snooze-pills">
                {SNOOZE_HOURS_OPTIONS.map((hours) => (
                  <button
                    key={hours}
                    type="button"
                    className="update-snooze-pill"
                    onClick={() => void handleSelectSnoozeHours(hours)}
                  >
                    {snoozeLabel(hours)}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="updater-release-actions">
              <button
                type="button"
                className="secondary-btn"
                onClick={() => {
                  setSnoozedHoursNotice(null);
                  setShowSnoozeOptions(true);
                }}
              >
                <Clock size={14} /> {t.aboutSection.remindLaterBtn}
              </button>
              <button
                type="button"
                className="primary-cta small"
                onClick={() => void installAvailableUpdate()}
              >
                <Download size={14} />
                {launchMode === "alert"
                  ? t.aboutSection.updateNowBtn
                  : t.aboutSection.installUpdateBtn}
              </button>
            </div>
          )}
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
            checked={autoCheckUpdates}
            disabled={savingAutoUpdate}
            onChange={(e) => void handleToggleAutoCheck(e.target.checked)}
          />
          <span>{t.aboutSection.autoUpdateLabel}</span>
        </label>
        <span className="muted-info">{t.aboutSection.autoUpdateHint}</span>
      </div>
    </div>
  );
};
