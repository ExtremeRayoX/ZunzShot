import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getTranslations } from "../i18n/translations";
import type { AppSettings, Language } from "../types/zunzshot";
import {
  UpdaterPanel,
  type UpdatePreview,
  type UpdaterLaunchMode,
} from "./UpdaterPanel";

type UpdaterLaunchPayload = {
  mode?: string;
  preview?: UpdatePreview | null;
};

export const UpdaterWindowApp: React.FC = () => {
  const [lang, setLang] = useState<Language>("es");
  const [appVersion, setAppVersion] = useState<string>("1.0.9");
  const [autoCheckUpdates, setAutoCheckUpdates] = useState<boolean>(false);
  const [ready, setReady] = useState<boolean>(false);
  const [launchMode, setLaunchMode] = useState<UpdaterLaunchMode>("manual");
  const [preview, setPreview] = useState<UpdatePreview | null>(null);
  const [autoCheckNonce, setAutoCheckNonce] = useState<number>(0);

  useEffect(() => {
    let active = true;

    void Promise.all([
      invoke<AppSettings>("get_settings"),
      invoke<string>("get_app_version"),
      invoke<UpdaterLaunchPayload | null>("consume_pending_updater_launch"),
    ]).then(([loadedSettings, version, launch]) => {
      if (!active) return;
      setLang(loadedSettings.language);
      setAutoCheckUpdates(loadedSettings.autoCheckUpdates);
      setAppVersion(version);
      if (launch?.mode === "alert") {
        setLaunchMode("alert");
      } else {
        setLaunchMode("manual");
      }
      setPreview(launch?.preview ?? null);
      setAutoCheckNonce(1);
      setReady(true);
    });

    const unlistenMode = listen<UpdaterLaunchPayload>(
      "zunzshot://updater-mode",
      (ev) => {
        void invoke("consume_pending_updater_launch").catch(() => {});
        const nextMode: UpdaterLaunchMode =
          ev.payload?.mode === "alert" ? "alert" : "manual";
        setLaunchMode(nextMode);
        setPreview(ev.payload?.preview ?? null);
        setAutoCheckNonce((n) => n + 1);
      }
    );

    const unlistenSettings = listen<AppSettings>(
      "zunzshot://settings-updated",
      (ev) => {
        setLang(ev.payload.language);
        setAutoCheckUpdates(ev.payload.autoCheckUpdates);
      }
    );

    const unlistenAuto = listen<boolean>(
      "zunzshot://auto-updates-updated",
      (ev) => {
        setAutoCheckUpdates(ev.payload);
      }
    );

    return () => {
      active = false;
      void unlistenMode.then((fn) => fn());
      void unlistenSettings.then((fn) => fn());
      void unlistenAuto.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const t = getTranslations(lang);
    const title =
      launchMode === "alert"
        ? `ZunzShot — ${t.aboutSection.updatePromptTitle}`
        : `ZunzShot — ${t.settingsTabs.updates}`;
    document.title = title;
    void getCurrentWindow()
      .setTitle(title)
      .catch(() => {});
  }, [lang, launchMode]);

  const handleToggleAutoCheckUpdates = async (enabled: boolean) => {
    const updated = await invoke<AppSettings>("set_auto_check_updates_cmd", {
      enabled,
    });
    setAutoCheckUpdates(updated.autoCheckUpdates);
  };

  const handleCloseWindow = () => {
    void getCurrentWindow()
      .close()
      .catch(() => {});
  };

  if (!ready) {
    return (
      <div className="updater-window-shell">
        <div className="loading-card">ZunzShot...</div>
      </div>
    );
  }

  return (
    <div className="updater-window-shell">
      <header className="updater-window-brand">
        <img src="/zunzshot-logo.png" alt="ZunzShot" width="26" height="26" />
        <strong>ZunzShot</strong>
      </header>
      <UpdaterPanel
        mode="standalone"
        launchMode={launchMode}
        autoCheckNonce={autoCheckNonce}
        lang={lang}
        appVersion={appVersion}
        autoCheckUpdates={autoCheckUpdates}
        onToggleAutoCheckUpdates={handleToggleAutoCheckUpdates}
        initialPreview={preview}
        onRequestClose={handleCloseWindow}
      />
    </div>
  );
};
