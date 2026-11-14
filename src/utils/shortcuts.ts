export const MODIFIER_CODES = new Set([
  "MetaLeft",
  "MetaRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "CapsLock",
  "Fn",
]);

const SUPPORTED_SPECIAL_CODES = new Set([
  "Escape",
  "Space",
  "Enter",
  "Tab",
  "Backspace",
  "Delete",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "Insert",
  "PrintScreen",
  "Pause",
  "Backquote",
  "Minus",
  "Equal",
  "BracketLeft",
  "BracketRight",
  "Backslash",
  "Semicolon",
  "Quote",
  "Comma",
  "Period",
  "Slash",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Numpad0",
  "Numpad1",
  "Numpad2",
  "Numpad3",
  "Numpad4",
  "Numpad5",
  "Numpad6",
  "Numpad7",
  "Numpad8",
  "Numpad9",
  "NumpadAdd",
  "NumpadSubtract",
  "NumpadMultiply",
  "NumpadDivide",
  "NumpadDecimal",
  "NumpadEnter",
  "NumpadEqual",
]);

export const DEFAULT_GLOBAL_SHORTCUTS = {
  shortcutRegion: "CommandOrControl+Shift+X",
  shortcutQuickCopy: "CommandOrControl+Shift+D",
  shortcutCircle: "CommandOrControl+Shift+C",
  shortcutWindow: "CommandOrControl+Shift+W",
  shortcutFullscreen: "CommandOrControl+Shift+F",
} as const;

export const DEFAULT_OVERLAY_COMMANDS = {
  cmdCopy: "CommandOrControl+C",
  cmdSave: "CommandOrControl+S",
  cmdPin: "CommandOrControl+Shift+P",
  cmdShare: "CommandOrControl+Shift+E",
  cmdPrint: "CommandOrControl+P",
  cmdCancel: "Escape",
} as const;

export function detectPlatform(): "macos" | "windows" | "linux" {
  const nav =
    typeof navigator !== "undefined" ? navigator.platform.toLowerCase() : "";
  if (nav.includes("mac")) return "macos";
  if (nav.includes("win")) return "windows";
  return "linux";
}

export function codeToHotkeyToken(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return SUPPORTED_SPECIAL_CODES.has(code) ? code : null;
}

export function resolveRecordedHotkeyToken(e: KeyboardEvent): string | null {
  const byCode = codeToHotkeyToken(e.code);
  if (byCode) return byCode;
  if (e.key === "PrintScreen" || e.keyCode === 44) return "PrintScreen";
  return null;
}

export function resolveMainKeyToken(e: KeyboardEvent): string | null {
  return (
    resolveRecordedHotkeyToken(e) ??
    (e.key === "Escape"
      ? "Escape"
      : e.key.length === 1
        ? e.key.toUpperCase()
        : null)
  );
}

export function isSingleGlobalShortcutAllowed(
  mainToken: string | null,
  platform: string
): boolean {
  return (
    platform === "windows" &&
    typeof mainToken === "string" &&
    mainToken.toUpperCase() === "PRINTSCREEN"
  );
}

export function isModifierKeyEvent(e: KeyboardEvent): boolean {
  return (
    MODIFIER_CODES.has(e.code) ||
    e.key === "Shift" ||
    e.key === "Control" ||
    e.key === "Alt" ||
    e.key === "Meta"
  );
}

export function getEventModifiers(e: KeyboardEvent, platform: string): string[] {
  const mods: string[] = [];
  if (e.metaKey) mods.push(platform === "macos" ? "Command" : "Super");
  if (e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  return mods;
}

function normalizeToken(tok: string, platform: string): string {
  const isMac = platform === "macos";
  const u = tok.trim().toUpperCase();
  if (
    u === "COMMANDORCONTROL" ||
    u === "COMMANDORCTRL" ||
    u === "CMDORCTRL" ||
    u === "CMDORCONTROL"
  ) {
    return isMac ? "SUPER" : "CONTROL";
  }
  if (u === "COMMAND" || u === "CMD" || u === "SUPER") return "SUPER";
  if (u === "CONTROL" || u === "CTRL") return "CONTROL";
  if (u === "ALT" || u === "OPTION") return "ALT";
  if (u === "SHIFT") return "SHIFT";
  if (u === "ESC") return "ESCAPE";
  return u.replace(/^(KEY|DIGIT)/, "");
}

export function normalizeAccelerator(accel: string, platform: string): string {
  return accel
    .split("+")
    .filter((p) => p.trim().length > 0)
    .map((p) => normalizeToken(p, platform))
    .sort()
    .join("+");
}

export function keyboardEventToAccelerator(
  e: KeyboardEvent,
  platform: string
): string | null {
  if (isModifierKeyEvent(e)) return null;
  const main = resolveMainKeyToken(e);
  if (!main) return null;
  const mods = getEventModifiers(e, platform);
  return [...mods, main].join("+");
}

export function matchesAccelerator(
  e: KeyboardEvent,
  accel: string | undefined,
  platform: string
): boolean {
  if (!accel || !accel.trim()) return false;
  const evAccel = keyboardEventToAccelerator(e, platform);
  if (!evAccel) return false;
  return (
    normalizeAccelerator(evAccel, platform) ===
    normalizeAccelerator(accel, platform)
  );
}

export function formatKeyLabel(token: string, platform: string): string {
  const upper = token.trim().toUpperCase();
  const isMac = platform === "macos";
  if (
    upper === "COMMANDORCONTROL" ||
    upper === "COMMANDORCTRL" ||
    upper === "CMDORCTRL" ||
    upper === "CMDORCONTROL"
  ) {
    return isMac ? "⌘ Cmd" : "Ctrl";
  }
  if (upper === "COMMAND" || upper === "CMD" || upper === "SUPER") {
    return isMac ? "⌘ Cmd" : "⊞ Win";
  }
  if (upper === "CONTROL" || upper === "CTRL") {
    return isMac ? "⌃ Ctrl" : "Ctrl";
  }
  if (upper === "ALT" || upper === "OPTION") {
    return isMac ? "⌥ Opt" : "Alt";
  }
  if (upper === "SHIFT") return "⇧ Shift";
  if (upper === "ESCAPE" || upper === "ESC") return "Esc";
  if (upper === "PRINTSCREEN") return "PrtSc";
  if (upper === "SPACE") return "Space";
  if (upper === "ARROWUP") return "↑";
  if (upper === "ARROWDOWN") return "↓";
  if (upper === "ARROWLEFT") return "←";
  if (upper === "ARROWRIGHT") return "→";
  if (upper === "BACKQUOTE") return "`";
  if (upper === "MINUS") return "-";
  if (upper === "EQUAL") return "=";
  if (upper === "BRACKETLEFT") return "[";
  if (upper === "BRACKETRIGHT") return "]";
  if (upper === "BACKSLASH") return "\\";
  if (upper === "SEMICOLON") return ";";
  if (upper === "QUOTE") return "'";
  if (upper === "COMMA") return ",";
  if (upper === "PERIOD") return ".";
  if (upper === "SLASH") return "/";
  return token.trim().replace(/^(Key|Digit)/i, "").toUpperCase();
}

export function formatShortcutBadge(
  accel: string | undefined,
  platform: string
): string {
  if (!accel || !accel.trim()) return "";
  const isMac = platform === "macos";
  const parts = accel
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((tok) => {
      const upper = tok.toUpperCase();
      if (
        upper === "COMMANDORCONTROL" ||
        upper === "COMMANDORCTRL" ||
        upper === "CMDORCTRL" ||
        upper === "CMDORCONTROL"
      ) {
        return isMac ? "⌘" : "Ctrl";
      }
      if (upper === "COMMAND" || upper === "CMD" || upper === "SUPER") {
        return isMac ? "⌘" : "Win";
      }
      if (upper === "CONTROL" || upper === "CTRL") {
        return isMac ? "⌃" : "Ctrl";
      }
      if (upper === "ALT" || upper === "OPTION") {
        return isMac ? "⌥" : "Alt";
      }
      if (upper === "SHIFT") return isMac ? "⇧" : "Shift";
      return formatKeyLabel(tok, platform);
    });
  return isMac ? parts.join("") : parts.join("+");
}
