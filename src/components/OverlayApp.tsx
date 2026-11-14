import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  AppWindow,
  ArrowUpRight,
  Check,
  Circle,
  CircleDashed,
  Clock,
  Copy,
  Highlighter,
  ImagePlus,
  Maximize2,
  MousePointer2,
  Palette,
  Pencil,
  Pin,
  Pipette,
  Printer,
  RotateCcw,
  Save,
  Share2,
  Square,
  SquareDashed,
  Sticker,
  Trash2,
  Type,
  Underline,
  X,
  Zap,
} from "lucide-react";
import {
  clampSelection,
  drawOverlayItem,
  formatFrozenClock,
  hitTestSelectionHandle,
  hitTestSticker,
  measureClockSticker,
  renderExportPngBytes,
  resetGeometryCache,
  type HandleId,
} from "../canvas/geometry";
import { getTranslations, type OverlayTooltipKey } from "../i18n/translations";
import {
  DEFAULT_OVERLAY_COMMANDS,
  detectPlatform,
  formatShortcutBadge,
  matchesAccelerator,
} from "../utils/shortcuts";
import type {
  AppSettings,
  CaptureMetadata,
  CaptureMode,
  ClockSticker,
  ClockStickerStyle,
  OverlayItem,
  PhotoMaskShape,
  PhotoSticker,
  SelectionBox,
  TextAnnotation,
  ToolType,
  WindowCandidate,
} from "../types/zunzshot";
import { Bold, Italic } from "lucide-react";

const PALETTE = [
  "#06d6c6",
  "#ef4444",
  "#f59e0b",
  "#10b981",
  "#3b82f6",
  "#a855f7",
  "#ffffff",
  "#0f172a",
];

const STROKE_WIDTHS = [2, 4, 7];

/** Checkerboard mosaic icon that distinguishes redaction/pixelation from a table grid. */
const PixelateIcon: React.FC<{ size?: number }> = ({ size = 15 }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 16 16"
    fill="none"
    aria-hidden="true"
  >
    <rect x="2" y="2" width="3" height="3" fill="currentColor" fillOpacity="0.95" />
    <rect x="5" y="2" width="3" height="3" fill="currentColor" fillOpacity="0.26" />
    <rect x="8" y="2" width="3" height="3" fill="currentColor" fillOpacity="0.95" />
    <rect x="11" y="2" width="3" height="3" fill="currentColor" fillOpacity="0.26" />

    <rect x="2" y="5" width="3" height="3" fill="currentColor" fillOpacity="0.26" />
    <rect x="5" y="5" width="3" height="3" fill="currentColor" fillOpacity="0.95" />
    <rect x="8" y="5" width="3" height="3" fill="currentColor" fillOpacity="0.26" />
    <rect x="11" y="5" width="3" height="3" fill="currentColor" fillOpacity="0.95" />

    <rect x="2" y="8" width="3" height="3" fill="currentColor" fillOpacity="0.95" />
    <rect x="5" y="8" width="3" height="3" fill="currentColor" fillOpacity="0.26" />
    <rect x="8" y="8" width="3" height="3" fill="currentColor" fillOpacity="0.95" />
    <rect x="11" y="8" width="3" height="3" fill="currentColor" fillOpacity="0.26" />

    <rect x="2" y="11" width="3" height="3" fill="currentColor" fillOpacity="0.26" />
    <rect x="5" y="11" width="3" height="3" fill="currentColor" fillOpacity="0.95" />
    <rect x="8" y="11" width="3" height="3" fill="currentColor" fillOpacity="0.26" />
    <rect x="11" y="11" width="3" height="3" fill="currentColor" fillOpacity="0.95" />

    <rect
      x="1.5"
      y="1.5"
      width="13"
      height="13"
      rx="2.2"
      stroke="currentColor"
      strokeWidth="1.25"
    />
  </svg>
);

const STATIC_IMAGE_MIMES = new Set([
  "image/png",
  "image/jpeg",
  "image/jpg",
  "image/webp",
  "image/bmp",
  "image/x-ms-bmp",
]);
const STATIC_IMAGE_EXT_RE = /\.(png|jpe?g|webp|bmp)$/i;

async function isStaticImageBlob(blob: Blob): Promise<boolean> {
  const mime = (blob.type || "").toLowerCase();
  const name =
    "name" in blob && typeof (blob as File).name === "string"
      ? (blob as File).name
      : "";

  if (mime === "image/gif" || /\.gif$/i.test(name)) {
    return false;
  }
  if (mime && !STATIC_IMAGE_MIMES.has(mime)) {
    return false;
  }
  if (!mime && name && !STATIC_IMAGE_EXT_RE.test(name)) {
    return false;
  }

  try {
    const header = new Uint8Array(await blob.slice(0, 64).arrayBuffer());
    if (header.length < 4) return false;

    // Reject GIF87a / GIF89a regardless of filename or MIME spoofing
    if (
      header[0] === 0x47 &&
      header[1] === 0x49 &&
      header[2] === 0x46 &&
      header[3] === 0x38
    ) {
      return false;
    }

    const isPng =
      header[0] === 0x89 &&
      header[1] === 0x50 &&
      header[2] === 0x4e &&
      header[3] === 0x47;
    const isJpeg =
      header[0] === 0xff && header[1] === 0xd8 && header[2] === 0xff;
    const isBmp = header[0] === 0x42 && header[1] === 0x4d;
    const isWebp =
      header.length >= 12 &&
      header[0] === 0x52 &&
      header[1] === 0x49 &&
      header[2] === 0x46 &&
      header[3] === 0x46 &&
      header[8] === 0x57 &&
      header[9] === 0x45 &&
      header[10] === 0x42 &&
      header[11] === 0x50;

    if (isWebp) {
      // Reject animated WebP (VP8X animation flag 0x02 or ANIM chunk)
      if (
        header.length >= 21 &&
        header[12] === 0x56 &&
        header[13] === 0x50 &&
        header[14] === 0x38 &&
        header[15] === 0x58 &&
        (header[20] & 0x02) !== 0
      ) {
        return false;
      }
      const scan = new Uint8Array(await blob.slice(0, 4096).arrayBuffer());
      for (let i = 12; i + 3 < scan.length; i++) {
        if (
          scan[i] === 0x41 &&
          scan[i + 1] === 0x4e &&
          scan[i + 2] === 0x49 &&
          scan[i + 3] === 0x4d
        ) {
          return false;
        }
      }
    }

    return isPng || isJpeg || isBmp || isWebp;
  } catch {
    return false;
  }
}

interface ItemHistoryState {
  past: OverlayItem[][];
  present: OverlayItem[];
  future: OverlayItem[][];
}

interface ToastState {
  msg: string;
  error?: boolean;
}

interface HoverTipState {
  key: OverlayTooltipKey;
  dock: "horizontal" | "vertical";
  rect: { left: number; top: number; right: number; bottom: number };
}

function encodeUtf8Hex(str: string): string {
  const bytes = new TextEncoder().encode(str);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function uid(prefix: string): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 9)}`;
}

export const OverlayApp: React.FC = () => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const loupeCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const sampleCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const dockRef = useRef<HTMLDivElement | null>(null);
  const actionsDockRef = useRef<HTMLDivElement | null>(null);
  const shareBtnRef = useRef<HTMLButtonElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const textOpenedAtRef = useRef<number>(0);
  const sessionOpenRef = useRef<boolean>(false);
  const activeSessionTokenRef = useRef<number>(0);
  const copyInFlightRef = useRef<boolean>(false);
  const toastTimerRef = useRef<number | null>(null);
  const shareRestoreCleanupRef = useRef<(() => void) | null>(null);
  const viewportRef = useRef<{ w: number; h: number }>({
    w: window.innerWidth,
    h: window.innerHeight,
  });
  const itemsCountRef = useRef<number>(0);
  const runtimePlatform = useMemo(() => detectPlatform(), []);

  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [metadata, setMetadata] = useState<CaptureMetadata | null>(null);
  const [bitmap, setBitmap] = useState<ImageBitmap | null>(null);
  const [sessionReady, setSessionReady] = useState<boolean>(false);
  const [viewport, setViewport] = useState<{ w: number; h: number }>({
    w: window.innerWidth,
    h: window.innerHeight,
  });
  viewportRef.current = viewport;

  const [captureMode, setCaptureMode] = useState<CaptureMode>("region");
  const [selection, setSelection] = useState<SelectionBox | null>(null);
  const selectionRef = useRef<SelectionBox | null>(null);

  const updateSelection = useCallback(
    (
      next:
        | SelectionBox
        | null
        | ((prev: SelectionBox | null) => SelectionBox | null)
    ) => {
      const resolved =
        typeof next === "function" ? next(selectionRef.current) : next;
      selectionRef.current = resolved;
      setSelection(resolved);
    },
    []
  );
  const [hoveredWin, setHoveredWin] = useState<WindowCandidate | null>(null);
  const [activeTool, setActiveTool] = useState<ToolType>("select");
  const [activeColor, setActiveColor] = useState<string>("#ef4444");
  const [strokeWidth, setStrokeWidth] = useState<number>(4);

  const [history, setHistory] = useState<ItemHistoryState>({
    past: [],
    present: [],
    future: [],
  });
  const items = history.present;
  itemsCountRef.current = items.length;
  const canUndo = history.past.length > 0;

  const [draftItem, setDraftItem] = useState<OverlayItem | null>(null);
  const [selectedStickerId, setSelectedStickerId] = useState<string | null>(null);
  const [activePopover, setActivePopover] = useState<
    "strokes" | "shapes" | "extras" | "palette" | null
  >(null);
  const [lastStrokeTool, setLastStrokeTool] = useState<
    "pen" | "highlight" | "underline"
  >("pen");
  const [lastShapeTool, setLastShapeTool] = useState<"rect" | "circle">("rect");
  const [nextStepNum, setNextStepNum] = useState<number>(1);
  const [loupeInfo, setLoupeInfo] = useState<{
    hex: string;
    x: number;
    y: number;
  } | null>(null);
  const [hoverTip, setHoverTip] = useState<HoverTipState | null>(null);
  const [dockSize, setDockSize] = useState<{ w: number; h: number }>({
    w: 306,
    h: 36,
  });
  const [actionsDockSize, setActionsDockSize] = useState<{ w: number; h: number }>({
    w: 36,
    h: 204,
  });
  const [toast, setToast] = useState<ToastState | null>(null);

  // Inline text input state (for creating new text or editing an existing one)
  const [textInput, setTextInput] = useState<{
    editingId: string | null;
    x: number;
    y: number;
    value: string;
    color: string;
    fontSize: number;
    hasBg: boolean;
    bold: boolean;
    italic: boolean;
  } | null>(null);

  // Interactive drag state kept in ref for smooth 60fps pointer tracking
  const dragRef = useRef<
    | { kind: "none" }
    | {
        kind: "creating-selection";
        startX: number;
        startY: number;
        shape: "rect" | "circle";
      }
    | {
        kind: "adjusting-selection";
        handle: HandleId;
        startX: number;
        startY: number;
        orig: SelectionBox;
      }
    | {
        kind: "drawing-item";
        tool: Exclude<ToolType, "select" | "text" | "eyedropper">;
        startX: number;
        startY: number;
      }
    | {
        kind: "dragging-sticker";
        id: string;
        resize: boolean;
        startX: number;
        startY: number;
        origX: number;
        origY: number;
        origW: number;
        origH: number;
        origItems: OverlayItem[];
        changed: boolean;
      }
  >({ kind: "none" });

  const lang = settings?.language ?? "es";
  const t = useMemo(() => getTranslations(lang), [lang]);

  const showToastMsg = useCallback((msg: string, error = false, ms = 2200) => {
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
      toastTimerRef.current = null;
    }
    setToast({ msg, error });
    if (ms > 0) {
      toastTimerRef.current = window.setTimeout(() => {
        setToast(null);
        toastTimerRef.current = null;
      }, ms);
    }
  }, []);

  const commitItems = useCallback((updater: (prev: OverlayItem[]) => OverlayItem[]) => {
    setHistory((h) => {
      const next = updater(h.present);
      if (next === h.present) return h;
      return {
        past: [...h.past.slice(-49), h.present],
        present: next,
        future: [],
      };
    });
  }, []);

  const updateItemsTransient = useCallback(
    (updater: (prev: OverlayItem[]) => OverlayItem[]) => {
      setHistory((h) => ({ ...h, present: updater(h.present) }));
    },
    []
  );

  const handleUndo = useCallback(() => {
    setActivePopover(null);
    setHistory((h) => {
      if (h.past.length === 0) return h;
      const previous = h.past[h.past.length - 1];
      return {
        past: h.past.slice(0, -1),
        present: previous,
        future: [h.present, ...h.future],
      };
    });
  }, []);

  const handleRedo = useCallback(() => {
    setActivePopover(null);
    setHistory((h) => {
      if (h.future.length === 0) return h;
      const next = h.future[0];
      return {
        past: [...h.past.slice(-49), h.present],
        present: next,
        future: h.future.slice(1),
      };
    });
  }, []);

  const clearCanvasBuffers = useCallback(() => {
    const canvas = canvasRef.current;
    if (canvas) {
      const ctx = canvas.getContext("2d");
      if (ctx) {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, canvas.width, canvas.height);
      }
      canvas.width = 1;
      canvas.height = 1;
    }
    const loupe = loupeCanvasRef.current;
    if (loupe) {
      const lctx = loupe.getContext("2d");
      if (lctx) {
        lctx.setTransform(1, 0, 0, 1, 0, 0);
        lctx.clearRect(0, 0, loupe.width, loupe.height);
      }
    }
    if (sampleCanvasRef.current) {
      sampleCanvasRef.current.width = 0;
      sampleCanvasRef.current.height = 0;
      sampleCanvasRef.current = null;
    }
    resetGeometryCache();
  }, []);

  const focusOverlayCanvas = useCallback(() => {
    try {
      window.focus();
      canvasRef.current?.focus({ preventScroll: true });
    } catch {
      // ignore focus errors
    }
  }, []);

  const resetOverlayState = useCallback(() => {
    if (shareRestoreCleanupRef.current) {
      shareRestoreCleanupRef.current();
      shareRestoreCleanupRef.current = null;
    }
    sessionOpenRef.current = false;
    copyInFlightRef.current = false;
    setSessionReady(false);
    dragRef.current = { kind: "none" };
    updateSelection(null);
    setHoveredWin(null);
    setHistory({ past: [], present: [], future: [] });
    setDraftItem(null);
    setTextInput(null);
    setSelectedStickerId(null);
    setActivePopover(null);
    setHoverTip(null);
    setNextStepNum(1);
    setLoupeInfo(null);
    setToast(null);
    setBitmap((prev) => {
      prev?.close();
      return null;
    });
    clearCanvasBuffers();
  }, [clearCanvasBuffers, updateSelection]);

  // Load current capture session from Rust
  const loadActiveSession = useCallback(async (fromSessionReady = false) => {
    const token = ++activeSessionTokenRef.current;
    setSessionReady(false);
    clearCanvasBuffers();

    try {
      const [cfg, meta] = await Promise.all([
        invoke<AppSettings>("get_settings"),
        invoke<CaptureMetadata | null>("get_capture_metadata"),
      ]);
      if (token !== activeSessionTokenRef.current) return;
      setSettings(cfg);
      if (!meta) {
        if (fromSessionReady) {
          void invoke("close_overlay").catch(() => {});
        }
        return;
      }

      const buf = await invoke<ArrayBuffer>("get_capture_rgba");
      if (token !== activeSessionTokenRef.current) return;

      const view = new DataView(buf);
      const imgW = view.getUint32(0, true);
      const imgH = view.getUint32(4, true);
      const pixels = new Uint8ClampedArray(buf, 8);
      const imgData = new ImageData(pixels, imgW, imgH);
      const bmp = await createImageBitmap(imgData);
      if (token !== activeSessionTokenRef.current) {
        bmp.close();
        return;
      }

      const rawVw = window.innerWidth;
      const rawVh = window.innerHeight;
      const metaVw = Math.round(meta.logicalWidth || rawVw);
      const metaVh = Math.round(meta.logicalHeight || rawVh);
      const vw = Math.abs(rawVw - metaVw) <= 12 && rawVw > 0 ? rawVw : metaVw;
      const vh = Math.abs(rawVh - metaVh) <= 12 && rawVh > 0 ? rawVh : metaVh;

      viewportRef.current = { w: vw, h: vh };
      setViewport({ w: vw, h: vh });
      setMetadata(meta);
      setBitmap((prev) => {
        prev?.close();
        return bmp;
      });

      sessionOpenRef.current = true;
      copyInFlightRef.current = false;
      dragRef.current = { kind: "none" };
      setCaptureMode(meta.mode);
      setHistory({ past: [], present: [], future: [] });
      setDraftItem(null);
      setSelectedStickerId(null);
      setActivePopover(null);
      setHoverTip(null);
      setNextStepNum(1);
      setLoupeInfo(null);
      setToast(null);
      setTextInput(null);
      setHoveredWin(null);
      setActiveTool("select");

      if (meta.mode === "fullscreen") {
        updateSelection({ x: 0, y: 0, w: vw, h: vh, shape: "rect" });
      } else {
        updateSelection(null);
      }

      setSessionReady(true);
      void invoke("focus_overlay_window").catch(() => {});
      window.requestAnimationFrame(() => {
        focusOverlayCanvas();
        window.setTimeout(focusOverlayCanvas, 40);
      });
    } catch (err) {
      console.error("Failed loading capture session:", err);
      if (fromSessionReady && token === activeSessionTokenRef.current) {
        void invoke("close_overlay").catch(() => {});
      }
    }
  }, [clearCanvasBuffers, focusOverlayCanvas, updateSelection]);

  useEffect(() => {
    void loadActiveSession(false);
    const unlistenReady = listen("zunzshot://session-ready", () => {
      void loadActiveSession(true);
    });
    const unlistenReset = listen("zunzshot://session-reset", () => {
      activeSessionTokenRef.current += 1;
      resetOverlayState();
    });
    const unlistenFocus = listen("zunzshot://focus-canvas", () => {
      focusOverlayCanvas();
    });
    const unlistenSwitchMode = listen<CaptureMode>(
      "zunzshot://switch-mode",
      (ev) => {
        const nextMode = ev.payload;
        if (!sessionOpenRef.current) {
          void loadActiveSession(true);
          return;
        }
        setCaptureMode(nextMode);
        setTextInput(null);
        setActivePopover(null);
        setLoupeInfo(null);
        setHoveredWin(null);

        if (nextMode === "fullscreen") {
          const vp = viewportRef.current;
          updateSelection({ x: 0, y: 0, w: vp.w, h: vp.h, shape: "rect" });
          setActiveTool("select");
        } else if (itemsCountRef.current === 0) {
          updateSelection(null);
        } else if (nextMode === "circle") {
          updateSelection((prev) =>
            prev ? { ...prev, shape: "circle" } : null
          );
        } else if (nextMode === "region") {
          updateSelection((prev) => (prev ? { ...prev, shape: "rect" } : null));
        }

        window.requestAnimationFrame(() => {
          focusOverlayCanvas();
        });
      }
    );
    const unlistenSettings = listen<AppSettings>(
      "zunzshot://settings-updated",
      (ev) => {
        setSettings(ev.payload);
      }
    );

    const onResize = () => {
      const nextVp = { w: window.innerWidth, h: window.innerHeight };
      viewportRef.current = nextVp;
      setViewport(nextVp);
    };
    window.addEventListener("resize", onResize);

    return () => {
      void unlistenReady.then((fn) => fn());
      void unlistenReset.then((fn) => fn());
      void unlistenFocus.then((fn) => fn());
      void unlistenSwitchMode.then((fn) => fn());
      void unlistenSettings.then((fn) => fn());
      window.removeEventListener("resize", onResize);
    };
  }, [focusOverlayCanvas, loadActiveSession, resetOverlayState, updateSelection]);

  const handleClose = useCallback(async () => {
    activeSessionTokenRef.current += 1;
    resetOverlayState();
    await invoke("close_overlay").catch(() => {});
  }, [resetOverlayState]);

  const samplePixelHex = useCallback(
    (px: number, py: number): string | null => {
      if (!bitmap || viewport.w <= 0 || viewport.h <= 0) return null;
      let sc = sampleCanvasRef.current;
      if (!sc) {
        sc = document.createElement("canvas");
        sc.width = 1;
        sc.height = 1;
        sampleCanvasRef.current = sc;
      }
      const sctx = sc.getContext("2d", { willReadFrequently: true });
      if (!sctx) return null;
      const bx = Math.max(
        0,
        Math.min(
          bitmap.width - 1,
          Math.round((px * bitmap.width) / viewport.w)
        )
      );
      const by = Math.max(
        0,
        Math.min(
          bitmap.height - 1,
          Math.round((py * bitmap.height) / viewport.h)
        )
      );
      sctx.clearRect(0, 0, 1, 1);
      sctx.drawImage(bitmap, bx, by, 1, 1, 0, 0, 1, 1);
      const d = sctx.getImageData(0, 0, 1, 1).data;
      const toHex = (v: number) => v.toString(16).padStart(2, "0").toUpperCase();
      return `#${toHex(d[0])}${toHex(d[1])}${toHex(d[2])}`;
    },
    [bitmap, viewport.w, viewport.h]
  );

  // Render zoomed 4x pixel grid & crosshair onto loupe canvas
  useEffect(() => {
    const canvas = loupeCanvasRef.current;
    if (!canvas || !loupeInfo || !bitmap || viewport.w <= 0 || viewport.h <= 0) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const GRID = 28;
    const CENTER = 14;
    const CELL = 4; // 28 * 4 = 112px
    const bx = Math.max(
      0,
      Math.min(
        bitmap.width - 1,
        Math.round((loupeInfo.x * bitmap.width) / viewport.w)
      )
    );
    const by = Math.max(
      0,
      Math.min(
        bitmap.height - 1,
        Math.round((loupeInfo.y * bitmap.height) / viewport.h)
      )
    );

    const sx0 = bx - CENTER;
    const sy0 = by - CENTER;
    const clipX = Math.max(0, sx0);
    const clipY = Math.max(0, sy0);
    const clipR = Math.min(bitmap.width, sx0 + GRID);
    const clipB = Math.min(bitmap.height, sy0 + GRID);
    const cw = Math.max(0, clipR - clipX);
    const ch = Math.max(0, clipB - clipY);

    ctx.fillStyle = "#0b101b";
    ctx.fillRect(0, 0, GRID * CELL, GRID * CELL);
    ctx.imageSmoothingEnabled = false;
    if (cw > 0 && ch > 0) {
      ctx.drawImage(
        bitmap,
        clipX,
        clipY,
        cw,
        ch,
        (clipX - sx0) * CELL,
        (clipY - sy0) * CELL,
        cw * CELL,
        ch * CELL
      );
    }

    // Center pixel reticle & crisp guide hairlines
    const cx = CENTER * CELL;
    const cy = CENTER * CELL;
    ctx.strokeStyle = "rgba(6, 214, 198, 0.5)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(cx + CELL / 2, 0);
    ctx.lineTo(cx + CELL / 2, cy);
    ctx.moveTo(cx + CELL / 2, cy + CELL);
    ctx.lineTo(cx + CELL / 2, GRID * CELL);
    ctx.moveTo(0, cy + CELL / 2);
    ctx.lineTo(cx, cy + CELL / 2);
    ctx.moveTo(cx + CELL, cy + CELL / 2);
    ctx.lineTo(GRID * CELL, cy + CELL / 2);
    ctx.stroke();

    ctx.strokeStyle = "#06d6c6";
    ctx.lineWidth = 1.5;
    ctx.strokeRect(cx - 0.5, cy - 0.5, CELL + 1, CELL + 1);
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = 1;
    ctx.strokeRect(cx + 0.5, cy + 0.5, CELL - 1, CELL - 1);
  }, [loupeInfo, bitmap, viewport.w, viewport.h]);

  // Keep inline textarea robustly focused when opened inside WKWebView
  const isEditingText = Boolean(textInput);
  useEffect(() => {
    if (!isEditingText) return;
    textOpenedAtRef.current = performance.now();
    const focusBox = () => {
      const el = textareaRef.current;
      if (el && document.activeElement !== el) {
        el.focus({ preventScroll: true });
      }
    };
    const raf = window.requestAnimationFrame(focusBox);
    const timer = window.setTimeout(focusBox, 16);
    return () => {
      window.cancelAnimationFrame(raf);
      window.clearTimeout(timer);
    };
  }, [isEditingText]);

  // Dynamically measure horizontal tools dock & vertical actions dock so auto-flip L placement stays exact
  useEffect(() => {
    const hEl = dockRef.current;
    const vEl = actionsDockRef.current;
    if ((!hEl && !vEl) || !selection) return;
    const measure = () => {
      if (hEl) {
        const rect = hEl.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0) {
          const w = Math.ceil(rect.width);
          const h = Math.ceil(rect.height);
          setDockSize((prev) =>
            Math.abs(prev.w - w) > 1 || Math.abs(prev.h - h) > 1 ? { w, h } : prev
          );
        }
      }
      if (vEl) {
        const rect = vEl.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0) {
          const w = Math.ceil(rect.width);
          const h = Math.ceil(rect.height);
          setActionsDockSize((prev) =>
            Math.abs(prev.w - w) > 1 || Math.abs(prev.h - h) > 1 ? { w, h } : prev
          );
        }
      }
    };
    measure();
    const ro = new ResizeObserver(measure);
    if (hEl) ro.observe(hEl);
    if (vEl) ro.observe(vEl);
    return () => ro.disconnect();
  }, [selection, lang]);

  // Render full-screen canvas every frame dependencies update
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    if (!bitmap || !metadata || !sessionReady) {
      const clearCtx = canvas.getContext("2d");
      if (clearCtx) {
        clearCtx.setTransform(1, 0, 0, 1, 0, 0);
        clearCtx.clearRect(0, 0, canvas.width, canvas.height);
      }
      return;
    }

    const dpr = window.devicePixelRatio || 1;
    const { w: cssW, h: cssH } = viewport;
    if (canvas.width !== Math.round(cssW * dpr) || canvas.height !== Math.round(cssH * dpr)) {
      canvas.width = Math.round(cssW * dpr);
      canvas.height = Math.round(cssH * dpr);
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.save();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    // 1. Base frozen screenshot
    ctx.drawImage(bitmap, 0, 0, cssW, cssH);

    // 2. Dim overlay with crisp selection / hovered window cutout
    if (selection) {
      ctx.fillStyle = "rgba(6, 10, 20, 0.52)";
      ctx.beginPath();
      ctx.rect(0, 0, cssW, cssH);
      if (selection.shape === "circle") {
        ctx.ellipse(
          selection.x + selection.w / 2,
          selection.y + selection.h / 2,
          Math.max(1, selection.w / 2),
          Math.max(1, selection.h / 2),
          0,
          0,
          Math.PI * 2,
          true
        );
      } else {
        ctx.rect(selection.x + selection.w, selection.y, -selection.w, selection.h);
      }
      ctx.fill("evenodd");

      // Selection border
      ctx.save();
      ctx.strokeStyle = "#06d6c6";
      ctx.lineWidth = 2;
      if (selection.shape === "circle") {
        // Subtle dashed bounding guide + bright circle border
        ctx.save();
        ctx.setLineDash([4, 4]);
        ctx.strokeStyle = "rgba(255,255,255,0.4)";
        ctx.lineWidth = 1;
        ctx.strokeRect(selection.x, selection.y, selection.w, selection.h);
        ctx.restore();

        ctx.beginPath();
        ctx.ellipse(
          selection.x + selection.w / 2,
          selection.y + selection.h / 2,
          Math.max(1, selection.w / 2),
          Math.max(1, selection.h / 2),
          0,
          0,
          Math.PI * 2
        );
        ctx.stroke();
      } else {
        ctx.strokeRect(selection.x, selection.y, selection.w, selection.h);
      }

      // 8 resize handles
      const { x, y, w, h } = selection;
      const pts: Array<[number, number]> = [
        [x, y],
        [x + w / 2, y],
        [x + w, y],
        [x + w, y + h / 2],
        [x + w, y + h],
        [x + w / 2, y + h],
        [x, y + h],
        [x, y + h / 2],
      ];
      for (const [hx, hy] of pts) {
        ctx.fillStyle = "#ffffff";
        ctx.strokeStyle = "#06d6c6";
        ctx.lineWidth = 1.8;
        ctx.beginPath();
        ctx.arc(hx, hy, 4.5, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
    } else if (captureMode === "window" && hoveredWin) {
      const wx = hoveredWin.rx * cssW;
      const wy = hoveredWin.ry * cssH;
      const ww = hoveredWin.rw * cssW;
      const wh = hoveredWin.rh * cssH;

      ctx.fillStyle = "rgba(6, 10, 20, 0.50)";
      ctx.beginPath();
      ctx.rect(0, 0, cssW, cssH);
      ctx.rect(wx + ww, wy, -ww, wh);
      ctx.fill("evenodd");

      ctx.strokeStyle = "#06d6c6";
      ctx.lineWidth = 3;
      ctx.strokeRect(wx, wy, ww, wh);
    } else {
      ctx.fillStyle = "rgba(6, 10, 20, 0.42)";
      ctx.fillRect(0, 0, cssW, cssH);
    }

    // 3. Draw committed items and active draft item
    const pixelSource = { bitmap, cssW, cssH };
    for (const item of items) {
      if (textInput?.editingId === item.id) continue;
      drawOverlayItem(
        ctx,
        item,
        metadata.capturedAtIso,
        lang,
        item.id === selectedStickerId,
        pixelSource
      );
    }
    if (draftItem) {
      drawOverlayItem(
        ctx,
        draftItem,
        metadata.capturedAtIso,
        lang,
        draftItem.type === "pixelate",
        pixelSource
      );
    }

    ctx.restore();
  }, [
    bitmap,
    metadata,
    sessionReady,
    viewport,
    selection,
    captureMode,
    hoveredWin,
    items,
    draftItem,
    selectedStickerId,
    textInput?.editingId,
    lang,
  ]);

  const exportCurrentPng = useCallback(
    async (selectionOverride?: SelectionBox): Promise<Uint8Array | null> => {
      const activeSel = selectionOverride ?? selectionRef.current ?? selection;
      if (!bitmap || !metadata || !activeSel) return null;
      const liveTrimmed = textInput?.value.trim() ?? "";
      const exportItems: OverlayItem[] =
        textInput && liveTrimmed.length > 0
          ? [
              ...items.filter((it) => it.id !== textInput.editingId),
              {
                id: textInput.editingId ?? "text_draft",
                type: "text",
                x: textInput.x,
                y: textInput.y,
                text: liveTrimmed,
                color: textInput.color,
                fontSize: textInput.fontSize,
                hasBg: textInput.hasBg,
                bold: textInput.bold,
                italic: textInput.italic,
              },
            ]
          : items;
      return renderExportPngBytes(
        bitmap,
        viewport.w,
        viewport.h,
        activeSel,
        exportItems,
        metadata.capturedAtIso,
        lang
      );
    },
    [bitmap, metadata, selection, viewport, items, textInput, lang]
  );

  const handleCopy = useCallback(
    async (selectionOverride?: SelectionBox) => {
      if (copyInFlightRef.current) return;
      copyInFlightRef.current = true;
      const activeSel = selectionOverride ?? selectionRef.current ?? selection;
      try {
        const bytes = await exportCurrentPng(activeSel ?? undefined);
        if (!bytes) {
          if (captureMode === "quickCopy") {
            updateSelection(null);
          }
          return;
        }
        await invoke("copy_image_to_clipboard", bytes);
        if (!sessionOpenRef.current) return;
        const subtitle = activeSel
          ? `${Math.round(activeSel.w)} × ${Math.round(activeSel.h)} px`
          : "";
        activeSessionTokenRef.current += 1;
        resetOverlayState();
        await invoke("show_copy_notification", {
          title: t.overlay.copiedMsg,
          subtitle,
          closeOverlay: true,
        }).catch(() => invoke("close_overlay").catch(() => {}));
      } catch (err) {
        console.error("Failed copying screenshot:", err);
        if (!sessionOpenRef.current) return;
        if (captureMode === "quickCopy") {
          updateSelection(null);
        }
        showToastMsg(
          lang === "es"
            ? "No se pudo copiar al portapapeles. Inténtalo de nuevo."
            : "Could not copy to clipboard. Please try again.",
          true,
          3200
        );
      } finally {
        copyInFlightRef.current = false;
      }
    },
    [
      exportCurrentPng,
      selection,
      captureMode,
      updateSelection,
      t.overlay.copiedMsg,
      resetOverlayState,
      showToastMsg,
      lang,
    ]
  );

  const restoreOverlayElevation = useCallback(() => {
    if (!sessionOpenRef.current) return;
    window.setTimeout(() => {
      if (sessionOpenRef.current) {
        void invoke("set_overlay_elevated", { elevated: true });
      }
    }, 80);
  }, []);

  const handleSave = useCallback(async () => {
    const bytes = await exportCurrentPng();
    if (!bytes) return;
    const isCircle = selection?.shape === "circle";
    const defaultExt = isCircle ? "png" : settings?.outputFormat ?? "png";
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const fileName = `ZunzShot_${stamp}.${defaultExt}`;
    const folder = settings?.saveFolder?.trim();
    const sep = folder && folder.includes("\\") ? "\\" : "/";
    const defaultPath = folder
      ? `${folder.replace(/[\\/]+$/, "")}${sep}${fileName}`
      : fileName;

    let picked: string | null = null;
    try {
      await invoke("set_overlay_elevated", { elevated: false });
      picked = await saveDialog({
        defaultPath,
        filters: [
          { name: "PNG Image", extensions: ["png"] },
          { name: "JPEG Image", extensions: ["jpg", "jpeg"] },
        ],
      });
    } finally {
      if (sessionOpenRef.current && (!picked || runtimePlatform !== "windows")) {
        await invoke("set_overlay_elevated", { elevated: true });
      }
    }

    if (!picked) return;
    try {
      await invoke<string>("save_image_to_disk", bytes, {
        headers: {
          "x-custom-path-hex": encodeUtf8Hex(picked),
        },
      });
    } catch {
      if (runtimePlatform === "windows" && sessionOpenRef.current) {
        await invoke("set_overlay_elevated", { elevated: true }).catch(() => {});
      }
      showToastMsg(t.overlay.saveErrorMsg, true, 2800);
      return;
    }
    const savedName = picked.split(/[\\/]/).pop() ?? "";
    if (runtimePlatform === "windows") {
      activeSessionTokenRef.current += 1;
      resetOverlayState();
      await invoke("show_copy_notification", {
        title: t.overlay.savedMsg,
        subtitle: savedName,
        closeOverlay: true,
      }).catch(() => invoke("close_overlay").catch(() => {}));
      return;
    }
    showToastMsg(`${t.overlay.savedMsg} ${savedName}`, false, 0);
    setTimeout(() => {
      setToast(null);
      void handleClose();
    }, 520);
  }, [
    exportCurrentPng,
    selection?.shape,
    settings?.outputFormat,
    settings?.saveFolder,
    t.overlay.savedMsg,
    t.overlay.saveErrorMsg,
    handleClose,
    resetOverlayState,
    runtimePlatform,
    showToastMsg,
  ]);

  const handleShare = useCallback(
    async (anchorRect?: DOMRect) => {
      const bytes = await exportCurrentPng();
      if (!bytes) return;
      try {
        await invoke(
          "share_image",
          bytes,
          anchorRect
            ? {
                headers: {
                  "x-anchor-x": String(Math.round(anchorRect.left)),
                  "x-anchor-y": String(Math.round(anchorRect.top)),
                  "x-anchor-w": String(Math.round(anchorRect.width)),
                  "x-anchor-h": String(Math.round(anchorRect.height)),
                },
              }
            : undefined
        );
        if (runtimePlatform === "macos") {
          shareRestoreCleanupRef.current?.();
          const cleanup = () => {
            window.removeEventListener("focus", onReturn);
            window.removeEventListener("pointerdown", onReturn);
            if (shareRestoreCleanupRef.current === cleanup) {
              shareRestoreCleanupRef.current = null;
            }
          };
          const onReturn = () => {
            cleanup();
            restoreOverlayElevation();
          };
          shareRestoreCleanupRef.current = cleanup;
          window.addEventListener("focus", onReturn, { once: true });
          window.addEventListener("pointerdown", onReturn, { once: true });
        } else if (runtimePlatform === "windows") {
          await handleClose();
        }
      } catch {
        if (sessionOpenRef.current) {
          await invoke("set_overlay_elevated", { elevated: true }).catch(() => {});
        }
        showToastMsg(t.overlay.shareErrorMsg, true, 2400);
      }
    },
    [
      exportCurrentPng,
      runtimePlatform,
      restoreOverlayElevation,
      handleClose,
      showToastMsg,
      t.overlay.shareErrorMsg,
    ]
  );

  const handlePrint = useCallback(async () => {
    const bytes = await exportCurrentPng();
    if (!bytes) return;
    let unlistenFn: (() => void) | null = null;
    let timeoutId: number | null = null;
    try {
      let resolvePrint!: (ok: boolean) => void;
      const donePromise = new Promise<boolean>((resolve) => {
        resolvePrint = resolve;
        timeoutId = window.setTimeout(() => resolve(false), 45000);
      });
      unlistenFn = await listen<boolean>("zunzshot://print-result", (ev) => {
        resolvePrint(Boolean(ev.payload));
      });
      await invoke("print_image", bytes);
      const printed = await donePromise;
      if (printed) {
        showToastMsg(t.overlay.printedMsg, false, 0);
        setTimeout(() => {
          setToast(null);
          void handleClose();
        }, 420);
      } else if (sessionOpenRef.current) {
        await invoke("set_overlay_elevated", { elevated: true }).catch(() => {});
        if (runtimePlatform === "windows") {
          showToastMsg(t.overlay.printErrorMsg, true, 2800);
        }
      }
    } catch {
      if (sessionOpenRef.current) {
        await invoke("set_overlay_elevated", { elevated: true }).catch(() => {});
        showToastMsg(t.overlay.printErrorMsg, true, 2800);
      }
    } finally {
      if (timeoutId !== null) window.clearTimeout(timeoutId);
      unlistenFn?.();
    }
  }, [
    exportCurrentPng,
    handleClose,
    runtimePlatform,
    showToastMsg,
    t.overlay.printedMsg,
    t.overlay.printErrorMsg,
  ]);

  const handlePin = useCallback(async () => {
    if (!selection) return;
    const bytes = await exportCurrentPng();
    if (!bytes) return;
    const pinX = Math.round(selection.x);
    const pinY = Math.round(selection.y);
    const pinW = Math.max(64, Math.round(selection.w));
    const pinH = Math.max(48, Math.round(selection.h));
    try {
      await invoke<string>("pin_capture", bytes, {
        headers: {
          "x-pin-x": String(pinX),
          "x-pin-y": String(pinY),
          "x-pin-w": String(pinW),
          "x-pin-h": String(pinH),
        },
      });
    } catch {
      showToastMsg(t.overlay.pinErrorMsg, true, 2600);
      return;
    }
    showToastMsg(t.overlay.pinnedMsg, false, 0);
    setTimeout(() => {
      setToast(null);
      void handleClose();
    }, 220);
  }, [
    selection,
    exportCurrentPng,
    showToastMsg,
    t.overlay.pinnedMsg,
    t.overlay.pinErrorMsg,
    handleClose,
  ]);

  const handlePickPhoto = useCallback(async () => {
    setActivePopover(null);
    try {
      await invoke("set_overlay_elevated", { elevated: false });
    } catch {
      // ignore if command fails
    }
    window.addEventListener("focus", restoreOverlayElevation, { once: true });
    const input = fileInputRef.current;
    if (input) {
      input.addEventListener("cancel", restoreOverlayElevation, { once: true });
      input.click();
    } else {
      restoreOverlayElevation();
    }
  }, [restoreOverlayElevation]);

  const addPhotoFromBlob = useCallback(
    async (blob: Blob) => {
      const validStatic = await isStaticImageBlob(blob);
      if (!validStatic) {
        showToastMsg(t.overlay.staticImageOnlyError, true, 2600);
        return;
      }

      const url = URL.createObjectURL(blob);
      const img = new Image();
      img.onload = () => {
        URL.revokeObjectURL(url);
        const targetBox = selection ?? {
          x: viewport.w * 0.25,
          y: viewport.h * 0.25,
          w: viewport.w * 0.5,
          h: viewport.h * 0.5,
          shape: "rect",
        };
        const maxSide = Math.min(220, Math.max(96, targetBox.w * 0.42));
        const ratio = img.width / Math.max(1, img.height);
        const w = ratio >= 1 ? maxSide : maxSide * ratio;
        const h = ratio >= 1 ? maxSide / ratio : maxSide;
        const sticker: PhotoSticker = {
          id: uid("photo"),
          type: "photo",
          x: targetBox.x + targetBox.w / 2 - w / 2,
          y: targetBox.y + targetBox.h / 2 - h / 2,
          w,
          h,
          shape: "rounded",
          img,
        };
        commitItems((prev) => [...prev, sticker]);
        setSelectedStickerId(sticker.id);
        setActivePopover(null);
        setActiveTool("select");
      };
      img.onerror = () => {
        URL.revokeObjectURL(url);
        showToastMsg(t.overlay.staticImageOnlyError, true, 2600);
      };
      img.src = url;
    },
    [selection, viewport, commitItems, showToastMsg, t.overlay.staticImageOnlyError]
  );

  const handleAddOrSelectClock = useCallback(() => {
    setActivePopover(null);
    const existing = items.find((i): i is ClockSticker => i.type === "clock");
    if (existing) {
      setSelectedStickerId(existing.id);
      setActiveTool("select");
      return;
    }

    const box = selection ?? {
      x: 40,
      y: 40,
      w: viewport.w - 80,
      h: viewport.h - 80,
      shape: "rect",
    };

    const clock: ClockSticker = {
      id: uid("clock"),
      type: "clock",
      x: box.x + 18,
      y: box.y + 18,
      scale: 1,
      style: settings?.clockStyle ?? "glass-dark",
      format24h: settings?.clockFormat24h ?? false,
      showSeconds: settings?.clockShowSeconds ?? false,
      showDate: settings?.clockShowDate ?? false,
      accentColor: "#06d6c6",
    };
    commitItems((prev) => [...prev, clock]);
    setSelectedStickerId(clock.id);
    setActiveTool("select");
  }, [items, selection, viewport, settings, commitItems]);

  // Global keyboard shortcuts inside overlay (configurable vertical commands, Undo, Paste Photo, Arrow nudge)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (textInput) {
        if (e.key === "Escape") {
          e.preventDefault();
          setTextInput(null);
        }
        return;
      }

      const cmdCancel = settings?.cmdCancel || DEFAULT_OVERLAY_COMMANDS.cmdCancel;
      const cmdCopy = settings?.cmdCopy || DEFAULT_OVERLAY_COMMANDS.cmdCopy;
      const cmdSave = settings?.cmdSave || DEFAULT_OVERLAY_COMMANDS.cmdSave;
      const cmdPin = settings?.cmdPin || DEFAULT_OVERLAY_COMMANDS.cmdPin;
      const cmdShare = settings?.cmdShare || DEFAULT_OVERLAY_COMMANDS.cmdShare;
      const cmdPrint = settings?.cmdPrint || DEFAULT_OVERLAY_COMMANDS.cmdPrint;

      if (matchesAccelerator(e, cmdCancel, runtimePlatform)) {
        e.preventDefault();
        e.stopPropagation();
        void handleClose();
        return;
      }
      if (selection && matchesAccelerator(e, cmdCopy, runtimePlatform)) {
        e.preventDefault();
        e.stopPropagation();
        void handleCopy();
        return;
      }
      if (selection && matchesAccelerator(e, cmdSave, runtimePlatform)) {
        e.preventDefault();
        e.stopPropagation();
        void handleSave();
        return;
      }
      if (selection && matchesAccelerator(e, cmdPin, runtimePlatform)) {
        e.preventDefault();
        e.stopPropagation();
        void handlePin();
        return;
      }
      if (selection && matchesAccelerator(e, cmdShare, runtimePlatform)) {
        e.preventDefault();
        e.stopPropagation();
        void handleShare(shareBtnRef.current?.getBoundingClientRect());
        return;
      }
      if (selection && matchesAccelerator(e, cmdPrint, runtimePlatform)) {
        e.preventDefault();
        e.stopPropagation();
        void handlePrint();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        void handleClose();
        return;
      }

      const mod = e.metaKey || e.ctrlKey;
      const keyLower = e.key.toLowerCase();

      // Suppress WebView2 / Chromium default browser accelerators (Print, Save, Find, Reload)
      if (
        e.key === "F5" ||
        (mod && (keyLower === "r" || keyLower === "f" || keyLower === "g" || keyLower === "p" || keyLower === "s"))
      ) {
        e.preventDefault();
        e.stopPropagation();
      }

      if (
        (mod && e.shiftKey && keyLower === "z") ||
        (e.ctrlKey && !e.metaKey && keyLower === "y")
      ) {
        e.preventDefault();
        e.stopPropagation();
        handleRedo();
        return;
      }
      if (mod && keyLower === "z") {
        e.preventDefault();
        e.stopPropagation();
        handleUndo();
        return;
      }

      if ((e.key === "Delete" || e.key === "Backspace") && selectedStickerId) {
        e.preventDefault();
        e.stopPropagation();
        commitItems((prev) => prev.filter((i) => i.id !== selectedStickerId));
        setSelectedStickerId(null);
        return;
      }

      // Arrow keys nudge selected sticker or capture selection box
      if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) {
        const step = e.shiftKey ? 10 : 1;
        const dx =
          e.key === "ArrowLeft" ? -step : e.key === "ArrowRight" ? step : 0;
        const dy =
          e.key === "ArrowUp" ? -step : e.key === "ArrowDown" ? step : 0;
        e.preventDefault();
        e.stopPropagation();

        if (selectedStickerId) {
          commitItems((prev) =>
            prev.map((it) =>
              it.id === selectedStickerId &&
              (it.type === "clock" ||
                it.type === "photo" ||
                it.type === "step" ||
                it.type === "text")
                ? { ...it, x: it.x + dx, y: it.y + dy }
                : it
            )
          );
        } else if (selection) {
          updateSelection((prev) =>
            prev
              ? clampSelection(
                  { ...prev, x: prev.x + dx, y: prev.y + dy },
                  viewport.w,
                  viewport.h
                )
              : null
          );
        }
      }
    };

    const onPaste = (e: ClipboardEvent) => {
      if (textInput) return;
      const clipItems = e.clipboardData?.items;
      if (!clipItems) return;
      for (const ci of clipItems) {
        if (ci.type.startsWith("image/")) {
          const file = ci.getAsFile();
          if (file) {
            e.preventDefault();
            void addPhotoFromBlob(file);
            break;
          }
        }
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("paste", onPaste);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("paste", onPaste);
    };
  }, [
    textInput,
    selectedStickerId,
    selection,
    viewport,
    runtimePlatform,
    settings?.cmdCancel,
    settings?.cmdCopy,
    settings?.cmdSave,
    settings?.cmdPin,
    settings?.cmdShare,
    settings?.cmdPrint,
    handleClose,
    handleUndo,
    handleRedo,
    commitItems,
    handleCopy,
    handleSave,
    handlePin,
    handleShare,
    handlePrint,
    addPhotoFromBlob,
  ]);

  const commitText = () => {
    if (!textInput) return;
    const draft = textInput;
    setTextInput(null);
    const trimmed = draft.value.trim();
    if (draft.editingId) {
      if (trimmed.length > 0) {
        commitItems((prev) =>
          prev.map((it) =>
            it.id === draft.editingId && it.type === "text"
              ? {
                  ...it,
                  text: trimmed,
                  color: draft.color,
                  fontSize: draft.fontSize,
                  hasBg: draft.hasBg,
                  bold: draft.bold,
                  italic: draft.italic,
                }
              : it
          )
        );
      } else {
        commitItems((prev) => prev.filter((it) => it.id !== draft.editingId));
      }
      return;
    }
    if (trimmed.length > 0) {
      commitItems((prev) => [
        ...prev,
        {
          id: uid("text"),
          type: "text",
          x: draft.x,
          y: draft.y,
          text: trimmed,
          color: draft.color,
          fontSize: draft.fontSize,
          hasBg: draft.hasBg,
          bold: draft.bold,
          italic: draft.italic,
        },
      ]);
    }
  };

  // Pointer handlers on canvas
  const handlePointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0 || !metadata) return;
    setActivePopover(null);
    setHoverTip(null);
    if (textInput) {
      commitText();
      return;
    }
    if (!selection || activeTool !== "text") {
      focusOverlayCanvas();
    }

    const px = e.clientX;
    const py = e.clientY;
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");

    // 1. Window mode: click locks hovered window
    if (!selection && captureMode === "window" && hoveredWin) {
      updateSelection({
        x: hoveredWin.rx * viewport.w,
        y: hoveredWin.ry * viewport.h,
        w: hoveredWin.rw * viewport.w,
        h: hoveredWin.rh * viewport.h,
        shape: "rect",
      });
      setHoveredWin(null);
      setLoupeInfo(null);
      return;
    }

    // 2. If no selection yet, start dragging new rectangle or circle
    if (!selection) {
      const shape = captureMode === "circle" ? "circle" : "rect";
      dragRef.current = {
        kind: "creating-selection",
        startX: px,
        startY: py,
        shape,
      };
      updateSelection({ x: px, y: py, w: 1, h: 1, shape });
      e.currentTarget.setPointerCapture(e.pointerId);
      return;
    }

    // 3. Check if user grabbed a sticker (Photo, Clock, Text, or Numbered Step) when in select mode (or existing text in text mode)
    if ((activeTool === "select" || activeTool === "text") && ctx) {
      const hit = hitTestSticker(
        ctx,
        px,
        py,
        items,
        metadata.capturedAtIso,
        lang
      );
      if (hit && (activeTool === "select" || hit.item.type === "text")) {
        if (hit.item.type === "text" && e.detail >= 2) {
          e.preventDefault();
          startEditingTextItem(hit.item);
          return;
        }
        setSelectedStickerId(hit.item.id);
        setActivePopover(null);
        dragRef.current = {
          kind: "dragging-sticker",
          id: hit.item.id,
          resize: hit.resizeCorner,
          startX: px,
          startY: py,
          origX:
            hit.item.type === "clock" ||
            hit.item.type === "photo" ||
            hit.item.type === "step" ||
            hit.item.type === "text"
              ? hit.item.x
              : 0,
          origY:
            hit.item.type === "clock" ||
            hit.item.type === "photo" ||
            hit.item.type === "step" ||
            hit.item.type === "text"
              ? hit.item.y
              : 0,
          origW: hit.item.type === "photo" ? hit.item.w : 0,
          origH: hit.item.type === "photo" ? hit.item.h : 0,
          origItems: items,
          changed: false,
        };
        e.currentTarget.setPointerCapture(e.pointerId);
        return;
      }
      if (activeTool === "select") {
        setSelectedStickerId(null);
      }
    }

    // 4. Check resize handles (always available) or interior move (when activeTool === "select")
    const handle = hitTestSelectionHandle(px, py, selection);
    if (handle && (handle !== "move" || activeTool === "select")) {
      dragRef.current = {
        kind: "adjusting-selection",
        handle,
        startX: px,
        startY: py,
        orig: { ...selection },
      };
      e.currentTarget.setPointerCapture(e.pointerId);
      return;
    }

    // 5. Text tool: place floating input without letting canvas reclaim focus on pointerup
    if (activeTool === "text") {
      e.preventDefault();
      textOpenedAtRef.current = performance.now();
      setSelectedStickerId(null);
      setActivePopover(null);
      setTextInput({
        editingId: null,
        x: px,
        y: py,
        value: "",
        color: activeColor,
        fontSize: settings?.textDefaultSize ?? 22,
        hasBg: settings?.textDefaultBg ?? true,
        bold: settings?.textDefaultBold ?? true,
        italic: settings?.textDefaultItalic ?? false,
      });
      return;
    }

    // 5b. Eyedropper tool: pick color under pointer, set active color & copy #HEX
    if (activeTool === "eyedropper") {
      const hex = samplePixelHex(px, py);
      if (hex) {
        setActiveColor(hex);
        updateSelectedText({ color: hex });
        setTextInput((prev) => (prev ? { ...prev, color: hex } : null));
        void invoke("copy_text_to_clipboard", { text: hex });
        showToastMsg(`${t.overlay.colorCopiedMsg} ${hex}`, false, 1800);
      }
      return;
    }

    // 6. Annotation tools (pen, arrow, rect, circle, underline, highlight, pixelate, step)
    if (activeTool !== "select") {
      dragRef.current = {
        kind: "drawing-item",
        tool: activeTool,
        startX: px,
        startY: py,
      };
      e.currentTarget.setPointerCapture(e.pointerId);
      if (activeTool === "highlight" || activeTool === "pen") {
        setDraftItem({
          id: uid(activeTool === "pen" ? "pen" : "hl"),
          type: activeTool,
          points: [{ x: px, y: py }],
          color: activeColor,
          strokeWidth,
        });
      } else if (activeTool === "step") {
        setDraftItem({
          id: uid("step"),
          type: "step",
          x: px,
          y: py,
          number: nextStepNum,
          color: activeColor,
          radius: strokeWidth === 2 ? 13 : strokeWidth === 4 ? 16 : 20,
        });
      }
    }
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const px = e.clientX;
    const py = e.clientY;
    const drag = dragRef.current;

    const isSelectingDrag =
      drag.kind === "creating-selection" || drag.kind === "adjusting-selection";
    const magnifierEnabled = settings?.showMagnifier ?? true;
    const showLoupe =
      Boolean(bitmap) &&
      !textInput &&
      ((magnifierEnabled && (!selection || isSelectingDrag)) ||
        activeTool === "eyedropper");

    if (showLoupe) {
      const hex = samplePixelHex(px, py);
      if (hex) {
        setLoupeInfo({ hex, x: px, y: py });
      }
    } else if (loupeInfo) {
      setLoupeInfo(null);
    }

    // Window hover detection before selection is locked
    if (!selection && captureMode === "window" && metadata) {
      const rx = px / Math.max(1, viewport.w);
      const ry = py / Math.max(1, viewport.h);
      const found =
        metadata.windows.find(
          (w) => rx >= w.rx && rx <= w.rx + w.rw && ry >= w.ry && ry <= w.ry + w.rh
        ) ?? null;
      setHoveredWin(found);
      return;
    }

    if (drag.kind === "none") return;

    if (drag.kind === "creating-selection") {
      let w = Math.abs(px - drag.startX);
      let h = Math.abs(py - drag.startY);
      if (e.shiftKey || drag.shape === "circle") {
        // Keep circular captures 1:1 easily unless Alt is held for free ellipse
        if (!e.altKey) {
          const side = Math.max(w, h);
          w = side;
          h = side;
        }
      }
      const x = px >= drag.startX ? drag.startX : drag.startX - w;
      const y = py >= drag.startY ? drag.startY : drag.startY - h;
      updateSelection(clampSelection({ x, y, w, h, shape: drag.shape }, viewport.w, viewport.h));
      return;
    }

    if (drag.kind === "adjusting-selection") {
      const dx = px - drag.startX;
      const dy = py - drag.startY;
      const o = drag.orig;
      let next: SelectionBox = { ...o };

      if (drag.handle === "move") {
        next.x = o.x + dx;
        next.y = o.y + dy;
      } else {
        if (drag.handle.includes("w")) {
          const nx = Math.min(o.x + dx, o.x + o.w - 16);
          next.w = o.w + (o.x - nx);
          next.x = nx;
        }
        if (drag.handle.includes("e")) {
          next.w = Math.max(16, o.w + dx);
        }
        if (drag.handle.includes("n")) {
          const ny = Math.min(o.y + dy, o.y + o.h - 16);
          next.h = o.h + (o.y - ny);
          next.y = ny;
        }
        if (drag.handle.includes("s")) {
          next.h = Math.max(16, o.h + dy);
        }
      }

      updateSelection(clampSelection(next, viewport.w, viewport.h));
      return;
    }

    if (drag.kind === "dragging-sticker") {
      const dx = px - drag.startX;
      const dy = py - drag.startY;
      if (Math.abs(dx) >= 1 || Math.abs(dy) >= 1) {
        drag.changed = true;
      }
      updateItemsTransient((prev) =>
        prev.map((it) => {
          if (it.id !== drag.id) return it;
          if (it.type === "photo") {
            if (drag.resize) {
              const aspect = drag.origW / Math.max(1, drag.origH);
              const nw = Math.max(36, drag.origW + dx);
              const nh = e.shiftKey ? Math.max(36, drag.origH + dy) : nw / aspect;
              return { ...it, w: nw, h: nh };
            }
            return { ...it, x: drag.origX + dx, y: drag.origY + dy };
          }
          if (it.type === "clock" || it.type === "step" || it.type === "text") {
            return { ...it, x: drag.origX + dx, y: drag.origY + dy };
          }
          return it;
        })
      );
      return;
    }

    if (drag.kind === "drawing-item") {
      if (drag.tool === "arrow") {
        setDraftItem({
          id: "draft",
          type: "arrow",
          x1: drag.startX,
          y1: drag.startY,
          x2: px,
          y2: py,
          color: activeColor,
          strokeWidth,
        });
      } else if (drag.tool === "underline") {
        // Snap nearly horizontal underlines cleanly like text underlining
        const snapY = Math.abs(py - drag.startY) < 18 && !e.altKey ? drag.startY : py;
        setDraftItem({
          id: "draft",
          type: "underline",
          x1: drag.startX,
          y1: drag.startY,
          x2: px,
          y2: snapY,
          color: activeColor,
          strokeWidth: Math.max(3, strokeWidth),
        });
      } else if (drag.tool === "rect" || drag.tool === "circle") {
        const x = Math.min(drag.startX, px);
        const y = Math.min(drag.startY, py);
        const w = Math.abs(px - drag.startX);
        const h = Math.abs(py - drag.startY);
        setDraftItem({
          id: "draft",
          type: drag.tool,
          x,
          y,
          w,
          h,
          color: activeColor,
          strokeWidth,
        });
      } else if (drag.tool === "pixelate") {
        const x = Math.min(drag.startX, px);
        const y = Math.min(drag.startY, py);
        const w = Math.abs(px - drag.startX);
        const h = Math.abs(py - drag.startY);
        setDraftItem({
          id: "draft",
          type: "pixelate",
          x,
          y,
          w,
          h,
          blockSize: strokeWidth === 2 ? 8 : strokeWidth === 4 ? 14 : 22,
        });
      } else if (drag.tool === "step") {
        setDraftItem((prev) =>
          prev && prev.type === "step" ? { ...prev, x: px, y: py } : prev
        );
      } else if (drag.tool === "highlight" || drag.tool === "pen") {
        setDraftItem((prev) => {
          if (!prev || prev.type !== drag.tool) return prev;
          if (prev.type !== "highlight" && prev.type !== "pen") return prev;
          const last = prev.points[prev.points.length - 1];
          if (last && Math.hypot(px - last.x, py - last.y) < 1.5) return prev;
          return { ...prev, points: [...prev.points, { x: px, y: py }] };
        });
      }
    }
  };

  const handlePointerUp = () => {
    const drag = dragRef.current;
    dragRef.current = { kind: "none" };

    if (
      (drag.kind === "creating-selection" || drag.kind === "adjusting-selection") &&
      activeTool !== "eyedropper"
    ) {
      setLoupeInfo(null);
    }

    if (drag.kind === "creating-selection") {
      const currentSel = selectionRef.current ?? selection;
      if (!currentSel || currentSel.w < 12 || currentSel.h < 12) {
        updateSelection(null);
        return;
      }
      if (captureMode === "quickCopy") {
        void handleCopy(currentSel);
      }
      return;
    }

    if (drag.kind === "dragging-sticker" && drag.changed) {
      const priorSnapshot = drag.origItems;
      setHistory((h) => ({
        past: [...h.past.slice(-49), priorSnapshot],
        present: h.present,
        future: [],
      }));
      return;
    }

    if (drag.kind === "drawing-item" && draftItem) {
      if (draftItem.type === "pixelate" && (draftItem.w < 6 || draftItem.h < 6)) {
        setDraftItem(null);
        return;
      }
      const itemToAdd = { ...draftItem, id: uid(draftItem.type) };
      commitItems((prev) => [...prev, itemToAdd]);
      setDraftItem(null);
      if (itemToAdd.type === "step") {
        setNextStepNum((n) => n + 1);
      }
    }
  };

  // Compute L-shaped docks: horizontal tools bar (below -> above -> inside bottom)
  // + vertical action bar (right -> left -> inside right) with collision-free auto-flip.
  const toolbarLayout = useMemo(() => {
    if (!selection) return null;
    const MARGIN = 8;
    const GAP = 8;
    const hW = Math.min(dockSize.w, Math.max(200, viewport.w - MARGIN * 2));
    const hH = dockSize.h;
    const vW = actionsDockSize.w;
    const vH = actionsDockSize.h;
    const selLeft = selection.x;
    const selTop = selection.y;
    const selRight = selection.x + selection.w;
    const selBottom = selection.y + selection.h;

    // 1. Horizontal tools bar (Y: outside below -> outside above -> inside bottom)
    let hTop: number;
    let hInsideBottom = false;
    if (selBottom + GAP + hH <= viewport.h - MARGIN) {
      hTop = selBottom + GAP;
    } else if (selTop - GAP - hH >= MARGIN) {
      hTop = selTop - GAP - hH;
    } else {
      hInsideBottom = true;
      hTop = Math.min(
        viewport.h - hH - MARGIN,
        Math.max(MARGIN, selBottom - hH - MARGIN)
      );
    }

    let hLeft = selRight - hW;
    if (hLeft < MARGIN) hLeft = Math.max(MARGIN, selLeft);
    hLeft = Math.max(MARGIN, Math.min(hLeft, viewport.w - hW - MARGIN));

    // 2. Vertical actions bar (X: outside right -> outside left -> inside right)
    let vLeft: number;
    let vInsideRight = false;
    if (selRight + GAP + vW <= viewport.w - MARGIN) {
      vLeft = selRight + GAP;
    } else if (selLeft - GAP - vW >= MARGIN) {
      vLeft = selLeft - GAP - vW;
    } else {
      vInsideRight = true;
      vLeft = Math.min(
        viewport.w - vW - MARGIN,
        Math.max(MARGIN, selRight - vW - MARGIN)
      );
    }

    // Anchor bottom of vertical bar cleanly at the selection corner (or just above inside horizontal dock)
    let vTop: number;
    if (hInsideBottom) {
      vTop = Math.max(MARGIN, hTop - GAP - vH);
    } else if (hTop >= selBottom) {
      vTop = Math.max(MARGIN, Math.min(selBottom - vH, hTop - GAP - vH));
    } else {
      vTop = Math.max(MARGIN, selBottom - vH);
    }

    if (hInsideBottom && vInsideRight) {
      hLeft = Math.max(MARGIN, Math.min(vLeft + vW - hW, viewport.w - hW - MARGIN));
    }

    // 3. Ensure zero overlap on extreme narrow/corner selections
    const overlaps =
      hLeft < vLeft + vW + 6 &&
      hLeft + hW + 6 > vLeft &&
      hTop < vTop + vH + 6 &&
      hTop + hH + 6 > vTop;

    if (overlaps) {
      if (vLeft - GAP - hW >= MARGIN) {
        hLeft = vLeft - GAP - hW;
      } else if (vLeft + vW + GAP + hW <= viewport.w - MARGIN) {
        hLeft = vLeft + vW + GAP;
      } else if (hTop - GAP - vH >= MARGIN) {
        vTop = hTop - GAP - vH;
      } else if (hTop + hH + GAP + vH <= viewport.h - MARGIN) {
        vTop = hTop + hH + GAP;
      }
    }

    const spaceBelow = viewport.h - (hTop + hH) - MARGIN;
    const spaceAbove = hTop - MARGIN;
    const popUpwards =
      spaceBelow < 175
        ? spaceAbove >= spaceBelow
        : hTop + hH <= selTop && spaceAbove >= 175;
    const popAlignRight = hLeft + hW >= Math.min(390, viewport.w - MARGIN);

    return { hLeft, hTop, vLeft, vTop, popUpwards, popAlignRight };
  }, [selection, viewport, dockSize, actionsDockSize]);

  const tipProps = useCallback(
    (key: OverlayTooltipKey, dock: "horizontal" | "vertical") => ({
      onMouseEnter: (e: React.MouseEvent<HTMLElement>) => {
        const r = e.currentTarget.getBoundingClientRect();
        setHoverTip({
          key,
          dock,
          rect: {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
          },
        });
      },
      onFocus: (e: React.FocusEvent<HTMLElement>) => {
        const r = e.currentTarget.getBoundingClientRect();
        setHoverTip({
          key,
          dock,
          rect: {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
          },
        });
      },
      onMouseLeave: () => {
        setHoverTip((prev) => (prev?.key === key ? null : prev));
      },
      onBlur: () => {
        setHoverTip((prev) => (prev?.key === key ? null : prev));
      },
      onPointerDown: () => {
        setHoverTip(null);
      },
    }),
    []
  );

  const renderedHoverTip = useMemo(() => {
    if (!hoverTip || !selection || !toolbarLayout) return null;
    const resolvedKey: OverlayTooltipKey =
      hoverTip.key === "cropToCircle" || hoverTip.key === "cropToRect"
        ? selection.shape === "circle"
          ? "cropToRect"
          : "cropToCircle"
        : hoverTip.key;
    const copy = t.overlay.tooltips[resolvedKey];
    const verticalCmdMap: Partial<Record<OverlayTooltipKey, string>> = {
      copy: settings?.cmdCopy || DEFAULT_OVERLAY_COMMANDS.cmdCopy,
      save: settings?.cmdSave || DEFAULT_OVERLAY_COMMANDS.cmdSave,
      pin: settings?.cmdPin || DEFAULT_OVERLAY_COMMANDS.cmdPin,
      share: settings?.cmdShare || DEFAULT_OVERLAY_COMMANDS.cmdShare,
      print: settings?.cmdPrint || DEFAULT_OVERLAY_COMMANDS.cmdPrint,
      cancel: settings?.cmdCancel || DEFAULT_OVERLAY_COMMANDS.cmdCancel,
    };
    const accel = verticalCmdMap[resolvedKey];
    const kbd = accel ? formatShortcutBadge(accel, runtimePlatform) : "";
    const { rect, dock } = hoverTip;
    const MARGIN = 10;

    if (dock === "vertical") {
      const preferLeft = rect.left >= 235;
      const placeRight = !preferLeft && viewport.w - rect.right >= 235;
      const y = Math.max(
        38,
        Math.min((rect.top + rect.bottom) / 2, viewport.h - 38)
      );
      const style: React.CSSProperties =
        preferLeft || !placeRight
          ? {
              left: Math.max(MARGIN, rect.left - 8),
              top: y,
              transform: "translate(-100%, -50%)",
            }
          : {
              left: rect.right + 8,
              top: y,
              transform: "translateY(-50%)",
            };
      return { copy, kbd, style };
    }

    const spaceAbove = rect.top - MARGIN;
    const spaceBelow = viewport.h - rect.bottom - MARGIN;
    const placeBelow = activePopover
      ? toolbarLayout.popUpwards
        ? spaceBelow >= 54
        : spaceAbove < 54
      : spaceBelow >= 56 || spaceBelow >= spaceAbove;

    const cx = (rect.left + rect.right) / 2;
    const top = placeBelow ? rect.bottom + 8 : rect.top - 8;
    const yTransform = placeBelow ? "translateY(0)" : "translateY(-100%)";

    let style: React.CSSProperties;
    if (cx < 120) {
      style = {
        left: Math.max(MARGIN, rect.left),
        top,
        transform: yTransform,
      };
    } else if (cx > viewport.w - 120) {
      style = {
        right: Math.max(MARGIN, viewport.w - rect.right),
        top,
        transform: yTransform,
      };
    } else {
      style = {
        left: cx,
        top,
        transform: placeBelow ? "translate(-50%, 0)" : "translate(-50%, -100%)",
      };
    }
    return { copy, kbd, style };
  }, [
    hoverTip,
    selection,
    toolbarLayout,
    t,
    viewport,
    activePopover,
    settings?.cmdCopy,
    settings?.cmdSave,
    settings?.cmdPin,
    settings?.cmdShare,
    settings?.cmdPrint,
    settings?.cmdCancel,
    runtimePlatform,
  ]);

  const selectedSticker = useMemo(
    () => items.find((i) => i.id === selectedStickerId) ?? null,
    [items, selectedStickerId]
  );

  const updateSelectedClock = (patch: Partial<ClockSticker>) => {
    if (!selectedSticker || selectedSticker.type !== "clock") return;
    commitItems((prev) =>
      prev.map((it) =>
        it.id === selectedSticker.id && it.type === "clock"
          ? { ...it, ...patch }
          : it
      )
    );
  };

  const updateSelectedPhoto = (patch: Partial<PhotoSticker>) => {
    if (!selectedSticker || selectedSticker.type !== "photo") return;
    commitItems((prev) =>
      prev.map((it) =>
        it.id === selectedSticker.id && it.type === "photo"
          ? { ...it, ...patch }
          : it
      )
    );
  };

  const updateSelectedText = (patch: Partial<TextAnnotation>) => {
    if (!selectedSticker || selectedSticker.type !== "text") return;
    commitItems((prev) =>
      prev.map((it) =>
        it.id === selectedSticker.id && it.type === "text"
          ? { ...it, ...patch }
          : it
      )
    );
  };

  const startEditingTextItem = (item: TextAnnotation) => {
    textOpenedAtRef.current = performance.now();
    setSelectedStickerId(null);
    setActivePopover(null);
    setActiveColor(item.color);
    setTextInput({
      editingId: item.id,
      x: item.x,
      y: item.y,
      value: item.text,
      color: item.color,
      fontSize: item.fontSize,
      hasBg: item.hasBg,
      bold: item.bold,
      italic: item.italic,
    });
  };

  const activeTextStyle = textInput
    ? {
        fontSize: textInput.fontSize,
        hasBg: textInput.hasBg,
        bold: textInput.bold,
        italic: textInput.italic,
        color: textInput.color,
      }
    : selectedSticker?.type === "text"
      ? {
          fontSize: selectedSticker.fontSize,
          hasBg: selectedSticker.hasBg,
          bold: selectedSticker.bold,
          italic: selectedSticker.italic,
          color: selectedSticker.color,
        }
      : null;

  const patchActiveTextStyle = (
    patch: Partial<
      Pick<TextAnnotation, "fontSize" | "hasBg" | "bold" | "italic" | "color">
    >
  ) => {
    if (textInput) {
      setTextInput((prev) => (prev ? { ...prev, ...patch } : null));
    } else if (selectedSticker?.type === "text") {
      updateSelectedText(patch);
    }
  };

  const removeActiveText = () => {
    if (textInput) {
      const targetId = textInput.editingId;
      setTextInput(null);
      if (targetId) {
        commitItems((prev) => prev.filter((it) => it.id !== targetId));
      }
      return;
    }
    if (selectedSticker?.type === "text") {
      const targetId = selectedSticker.id;
      setSelectedStickerId(null);
      commitItems((prev) => prev.filter((it) => it.id !== targetId));
    }
  };

  const frozenTimeLabel = useMemo(() => {
    if (!metadata) return "";
    const clock = selectedSticker?.type === "clock" ? selectedSticker : null;
    const formatted = formatFrozenClock(
      metadata.capturedAtIso,
      clock ? clock.format24h : settings?.clockFormat24h ?? false,
      true,
      true,
      lang
    );
    return `${formatted.timeText} · ${formatted.dateText ?? ""}`;
  }, [metadata, selectedSticker, settings, lang]);

  // Measure clock sticker warning silence
  void measureClockSticker;

  return (
    <div
      className="overlay-root"
      style={{
        opacity: sessionReady && bitmap ? 1 : 0,
        pointerEvents: sessionReady && bitmap ? "auto" : "none",
      }}
      onMouseDown={() => {
        if (!textInput) {
          focusOverlayCanvas();
        }
      }}
    >
      <canvas
        ref={canvasRef}
        tabIndex={-1}
        style={{ outline: "none" }}
        className={`overlay-canvas cursor-${
          !selection
            ? captureMode === "window"
              ? "pointer"
              : "crosshair"
            : activeTool === "select"
              ? "move"
              : activeTool === "text"
                ? "text"
                : "crosshair"
        }`}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onPointerLeave={() => setLoupeInfo(null)}
      />

      {/* Hidden file picker for static Photo Stickers only (no GIF) */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".png,.jpg,.jpeg,.webp,.bmp,image/png,image/jpeg,image/webp,image/bmp"
        className="hidden-input"
        onChange={(e) => {
          restoreOverlayElevation();
          const f = e.target.files?.[0];
          if (f) void addPhotoFromBlob(f);
          e.target.value = "";
        }}
      />

      {/* Top instruction & quick mode pill when selecting */}
      {!selection && (
        <div className="top-mode-bar">
          <div className="mode-pills">
            <button
              className={`mode-pill ${captureMode === "region" ? "active" : ""}`}
              onClick={() => setCaptureMode("region")}
            >
              <Square size={14} /> {t.modeRegion}
            </button>
            <button
              className={`mode-pill ${captureMode === "quickCopy" ? "active" : ""}`}
              onClick={() => setCaptureMode("quickCopy")}
              title={t.modeQuickCopyDesc}
            >
              <Zap size={14} /> {t.modeQuickCopy}
            </button>
            <button
              className={`mode-pill ${captureMode === "circle" ? "active" : ""}`}
              onClick={() => setCaptureMode("circle")}
            >
              <Circle size={14} /> {t.modeCircle}
            </button>
            <button
              className={`mode-pill ${captureMode === "window" ? "active" : ""}`}
              onClick={() => setCaptureMode("window")}
            >
              <AppWindow size={14} /> {t.modeWindow}
            </button>
            <button
              className={`mode-pill ${captureMode === "fullscreen" ? "active" : ""}`}
              onClick={() => {
                setCaptureMode("fullscreen");
                setLoupeInfo(null);
                updateSelection({
                  x: 0,
                  y: 0,
                  w: viewport.w,
                  h: viewport.h,
                  shape: "rect",
                });
              }}
            >
              <Maximize2 size={14} /> {t.modeFullscreen}
            </button>
            <button
              className="mode-pill cancel"
              onClick={() => void handleClose()}
              title={t.overlay.cancel}
            >
              <X size={14} />
            </button>
          </div>
          <div className="mode-hint">
            {captureMode === "quickCopy"
              ? t.overlay.hintQuickCopy
              : captureMode === "circle"
                ? t.overlay.hintCircle
                : captureMode === "window"
                  ? t.overlay.hintWindow
                  : t.overlay.hintRegion}
          </div>
        </div>
      )}

      {selection && captureMode === "quickCopy" && (
        <div className="top-mode-bar">
          <div className="mode-pills">
            <button
              className="mode-pill cancel"
              onClick={() => void handleClose()}
              title={t.overlay.cancel}
            >
              <X size={14} /> {t.overlay.cancel}
            </button>
          </div>
        </div>
      )}

      {/* Window hover label */}
      {!selection && captureMode === "window" && hoveredWin && (
        <div
          className="window-badge"
          style={{
            left: Math.max(12, hoveredWin.rx * viewport.w + 10),
            top: Math.max(58, hoveredWin.ry * viewport.h + 10),
          }}
        >
          <strong>{hoveredWin.appName}</strong> — {hoveredWin.title}
        </div>
      )}

      {/* Selection dimension readout (flips inside top-left if close to screen top) */}
      {selection && (
        <div
          className="dim-badge"
          style={{
            left: Math.max(10, selection.x + (selection.y < 36 ? 10 : 0)),
            top:
              selection.y >= 36
                ? selection.y - 28
                : Math.min(viewport.h - 32, selection.y + 10),
          }}
        >
          {Math.round(selection.w)} × {Math.round(selection.h)} px ·{" "}
          {captureMode === "quickCopy"
            ? t.modeQuickCopy
            : selection.shape === "circle"
              ? t.modeCircle
              : t.modeRegion}
        </div>
      )}

      {/* Inline Text Entry */}
      {textInput && (
        <textarea
          ref={textareaRef}
          autoFocus
          className="inline-text-box"
          style={{
            left: textInput.x,
            top: textInput.y,
            color: textInput.color,
            fontSize: textInput.fontSize,
            fontWeight: textInput.bold ? 700 : 400,
            fontStyle: textInput.italic ? "italic" : "normal",
            background: textInput.hasBg
              ? "rgba(15, 23, 42, 0.78)"
              : "rgba(15, 23, 42, 0.16)",
            borderColor: textInput.hasBg
              ? "rgba(56, 189, 248, 0.75)"
              : "rgba(255, 255, 255, 0.55)",
            textShadow: textInput.hasBg
              ? "none"
              : "0 1px 4px rgba(0, 0, 0, 0.7)",
          }}
          placeholder={t.overlay.textPlaceholder}
          value={textInput.value}
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
          onChange={(e) =>
            setTextInput((prev) => (prev ? { ...prev, value: e.target.value } : null))
          }
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Escape") {
              e.preventDefault();
              setTextInput(null);
              return;
            }
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              commitText();
            }
          }}
          onBlur={() => {
            if (performance.now() - textOpenedAtRef.current < 180) {
              textareaRef.current?.focus({ preventScroll: true });
              return;
            }
            commitText();
          }}
        />
      )}

      {/* L-Shaped Tool Docks: Slim Horizontal Tools (bottom) + Vertical Actions (right) */}
      {selection && toolbarLayout && captureMode !== "quickCopy" && (
        <>
          {/* Horizontal Editing Tools Bar */}
          <div
            className="floating-dock"
            style={{ left: toolbarLayout.hLeft, top: toolbarLayout.hTop }}
            onPointerDown={(e) => e.stopPropagation()}
          >
            <div ref={dockRef} className="dock-bar">
              {/* 1. Select */}
              <button
                className={`tool-btn ${activeTool === "select" ? "active" : ""}`}
                onClick={() => {
                  setActiveTool("select");
                  setActivePopover(null);
                  setLoupeInfo(null);
                }}
                aria-label={t.overlay.toolSelect}
                {...tipProps("select", "horizontal")}
              >
                <MousePointer2 size={15} />
              </button>

              {/* 2. Strokes Group (Pen / Highlight / Underline) */}
              <button
                className={`tool-btn has-sub ${
                  activeTool === "pen" ||
                  activeTool === "highlight" ||
                  activeTool === "underline"
                    ? "active"
                    : ""
                }`}
                onClick={() => {
                  setActiveTool(lastStrokeTool);
                  setSelectedStickerId(null);
                  setLoupeInfo(null);
                  setActivePopover((prev) => (prev === "strokes" ? null : "strokes"));
                }}
                aria-label={t.overlay.toolStrokes}
                {...tipProps("strokes", "horizontal")}
              >
                {lastStrokeTool === "highlight" ? (
                  <Highlighter size={15} />
                ) : lastStrokeTool === "underline" ? (
                  <Underline size={15} />
                ) : (
                  <Pencil size={15} />
                )}
                <span className="tool-sub-caret" />
              </button>

              {/* 3. Arrow */}
              <button
                className={`tool-btn ${activeTool === "arrow" ? "active" : ""}`}
                onClick={() => {
                  setActiveTool("arrow");
                  setActivePopover(null);
                  setSelectedStickerId(null);
                  setLoupeInfo(null);
                }}
                aria-label={t.overlay.toolArrow}
                {...tipProps("arrow", "horizontal")}
              >
                <ArrowUpRight size={15} />
              </button>

              {/* 4. Shapes Group (Rectangle / Circle) */}
              <button
                className={`tool-btn has-sub ${
                  activeTool === "rect" || activeTool === "circle" ? "active" : ""
                }`}
                onClick={() => {
                  setActiveTool(lastShapeTool);
                  setSelectedStickerId(null);
                  setLoupeInfo(null);
                  setActivePopover((prev) => (prev === "shapes" ? null : "shapes"));
                }}
                aria-label={t.overlay.toolShapes}
                {...tipProps("shapes", "horizontal")}
              >
                {lastShapeTool === "circle" ? (
                  <Circle size={15} />
                ) : (
                  <Square size={15} />
                )}
                <span className="tool-sub-caret" />
              </button>

              {/* 5. Text */}
              <button
                className={`tool-btn ${activeTool === "text" ? "active" : ""}`}
                onClick={() => {
                  setActiveTool("text");
                  setActivePopover(null);
                  setSelectedStickerId(null);
                  setLoupeInfo(null);
                }}
                aria-label={t.overlay.toolText}
                {...tipProps("text", "horizontal")}
              >
                <Type size={15} />
              </button>

              {/* 6. Pixelate */}
              <button
                className={`tool-btn ${activeTool === "pixelate" ? "active" : ""}`}
                onClick={() => {
                  setActiveTool("pixelate");
                  setActivePopover(null);
                  setSelectedStickerId(null);
                  setLoupeInfo(null);
                }}
                aria-label={t.overlay.toolPixelate}
                {...tipProps("pixelate", "horizontal")}
              >
                <PixelateIcon size={15} />
              </button>

              <span className="dock-sep" />

              {/* 7. Toggle Crop Shape (Rectangle <-> Circle) */}
              <button
                className={`tool-btn ${selection.shape === "circle" ? "sticker-accent" : ""}`}
                onClick={() => {
                  setActivePopover(null);
                  updateSelection((prev) =>
                    prev
                      ? {
                          ...prev,
                          shape: prev.shape === "circle" ? "rect" : "circle",
                        }
                      : null
                  );
                }}
                aria-label={
                  selection.shape === "circle"
                    ? t.overlay.toggleShapeToRect
                    : t.overlay.toggleShapeToCircle
                }
                {...tipProps(
                  selection.shape === "circle" ? "cropToRect" : "cropToCircle",
                  "horizontal"
                )}
              >
                {selection.shape === "circle" ? (
                  <SquareDashed size={15} />
                ) : (
                  <CircleDashed size={15} />
                )}
              </button>

              {/* 8. Extras & Stickers (Photo, Clock, Numbered Steps) */}
              <button
                className={`tool-btn sticker-accent ${
                  activePopover === "extras" || activeTool === "step" ? "active" : ""
                }`}
                onClick={() => {
                  setActivePopover((prev) => (prev === "extras" ? null : "extras"));
                  setSelectedStickerId(null);
                }}
                aria-label={t.overlay.toolExtras}
                {...tipProps("extras", "horizontal")}
              >
                <Sticker size={15} />
              </button>

              {/* 9. Color, Stroke Size & Eyedropper */}
              <button
                className={`tool-btn palette-trigger-btn ${
                  activePopover === "palette" || activeTool === "eyedropper"
                    ? "active"
                    : ""
                }`}
                onClick={() => {
                  setActivePopover((prev) => (prev === "palette" ? null : "palette"));
                  setSelectedStickerId(null);
                }}
                aria-label={t.overlay.toolColorSize}
                {...tipProps("palette", "horizontal")}
              >
                <span
                  className="palette-dot"
                  style={{
                    backgroundColor: activeColor,
                    boxShadow: `0 0 0 ${strokeWidth === 2 ? 1 : strokeWidth === 4 ? 2 : 3}px rgba(255,255,255,0.45)`,
                  }}
                />
              </button>

              {/* 10. Undo */}
              <button
                className="tool-btn"
                onClick={() => {
                  if (canUndo) handleUndo();
                }}
                aria-disabled={!canUndo}
                aria-label={t.overlay.undo}
                {...tipProps("undo", "horizontal")}
              >
                <RotateCcw size={14} />
              </button>
            </div>

            {/* Strokes Subtool Popover (Pen / Highlight / Underline) */}
            {activePopover === "strokes" && (
              <div
                className={`dock-popover subtool-popover ${
                  toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                } align-left`}
              >
                <div className="subtool-row">
                  {(
                    [
                      { id: "pen", icon: Pencil, short: t.overlay.shortPen, label: t.overlay.toolPen },
                      { id: "highlight", icon: Highlighter, short: t.overlay.shortHighlight, label: t.overlay.toolHighlight },
                      { id: "underline", icon: Underline, short: t.overlay.shortUnderline, label: t.overlay.toolUnderline },
                    ] as const
                  ).map(({ id, icon: Icon, short, label }) => (
                    <button
                      key={id}
                      className={`subtool-chip ${activeTool === id ? "active" : ""}`}
                      onClick={() => {
                        setLastStrokeTool(id);
                        setActiveTool(id);
                        setActivePopover(null);
                      }}
                      aria-label={label}
                    >
                      <Icon size={13} />
                      <span>{short}</span>
                    </button>
                  ))}
                </div>
              </div>
            )}

            {/* Shapes Subtool Popover (Rectangle / Circle) */}
            {activePopover === "shapes" && (
              <div
                className={`dock-popover subtool-popover ${
                  toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                } align-left`}
              >
                <div className="subtool-row">
                  {(
                    [
                      { id: "rect", icon: Square, short: t.overlay.shortRect, label: t.overlay.toolRect },
                      { id: "circle", icon: Circle, short: t.overlay.shortCircle, label: t.overlay.toolCircle },
                    ] as const
                  ).map(({ id, icon: Icon, short, label }) => (
                    <button
                      key={id}
                      className={`subtool-chip ${activeTool === id ? "active" : ""}`}
                      onClick={() => {
                        setLastShapeTool(id);
                        setActiveTool(id);
                        setActivePopover(null);
                      }}
                      aria-label={label}
                    >
                      <Icon size={13} />
                      <span>{short}</span>
                    </button>
                  ))}
                </div>
              </div>
            )}

            {/* Floating Extras & Stickers Popover (Photo, Clock, Numbered Steps) */}
            {activePopover === "extras" && (
              <div
                className={`dock-popover extras-popover ${
                  toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                } ${toolbarLayout.popAlignRight ? "align-right" : "align-left"}`}
              >
                <div className="extras-list">
                  <button
                    className="extras-item"
                    onClick={() => {
                      setActivePopover(null);
                      void handlePickPhoto();
                    }}
                  >
                    <span className="extras-item-left">
                      <ImagePlus size={14} />
                      <span>{t.overlay.toolPhoto}</span>
                    </span>
                  </button>

                  <button
                    className={`extras-item ${
                      items.some((i) => i.type === "clock") ? "active" : ""
                    }`}
                    onClick={() => {
                      setActivePopover(null);
                      handleAddOrSelectClock();
                    }}
                  >
                    <span className="extras-item-left">
                      <Clock size={14} />
                      <span>{t.overlay.toolClock}</span>
                    </span>
                  </button>

                  <div
                    className={`extras-step-card ${activeTool === "step" ? "active" : ""}`}
                  >
                    <button
                      className="extras-step-main"
                      onClick={() => {
                        setActiveTool("step");
                        setSelectedStickerId(null);
                        setActivePopover(null);
                        setLoupeInfo(null);
                      }}
                    >
                      <span
                        className="step-preview-dot"
                        style={{ backgroundColor: activeColor }}
                      >
                        {nextStepNum}
                      </span>
                      <span className="extras-step-text">
                        <span className="extras-step-title">{t.overlay.toolSteps}</span>
                        <span className="extras-step-sub">
                          {t.overlay.stepsPlacePrefix}
                          {nextStepNum}
                        </span>
                      </span>
                    </button>
                    <button
                      className="extras-reset-btn"
                      disabled={nextStepNum <= 1}
                      onClick={() => {
                        setNextStepNum(1);
                        setActiveTool("step");
                        setSelectedStickerId(null);
                        setLoupeInfo(null);
                      }}
                      aria-label={t.overlay.stepsResetBtn}
                    >
                      <RotateCcw size={11} />
                      <span>{t.overlay.stepsResetShort}</span>
                    </button>
                  </div>
                </div>
              </div>
            )}

            {/* Floating Color, Stroke & Eyedropper Palette Popover */}
            {activePopover === "palette" && (
              <div
                className={`dock-popover palette-popover ${
                  toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                } ${toolbarLayout.popAlignRight ? "align-right" : "align-left"}`}
              >
                <div className="popover-row">
                  <span className="popover-label">
                    <Palette size={12} /> {t.overlay.colorLabel}
                  </span>
                  <div className="color-swatches" aria-label={t.overlay.colorLabel}>
                    {PALETTE.map((c) => {
                      const currentColor =
                        textInput?.color ??
                        (selectedSticker?.type === "text"
                          ? selectedSticker.color
                          : activeColor);
                      return (
                        <button
                          key={c}
                          className={`swatch ${currentColor === c ? "selected" : ""}`}
                          style={{ backgroundColor: c }}
                          onMouseDown={(e) => {
                            if (textInput) e.preventDefault();
                          }}
                          onClick={() => {
                            setActiveColor(c);
                            if (textInput) {
                              setTextInput((prev) =>
                                prev ? { ...prev, color: c } : null
                              );
                            } else if (selectedSticker?.type === "text") {
                              updateSelectedText({ color: c });
                            }
                          }}
                        />
                      );
                    })}
                  </div>
                  <span className="dock-sep" />
                  <div className="stroke-sizes" aria-label={t.overlay.sizeLabel}>
                    {STROKE_WIDTHS.map((sw) => (
                      <button
                        key={sw}
                        className={`size-dot-btn ${strokeWidth === sw ? "selected" : ""}`}
                        onClick={() => setStrokeWidth(sw)}
                        title={`${sw}px`}
                      >
                        <span
                          className="size-dot"
                          style={{ width: sw + 4, height: sw + 4 }}
                        />
                      </button>
                    ))}
                  </div>
                  <span className="dock-sep" />
                  <button
                    className={`tool-btn ${activeTool === "eyedropper" ? "active" : ""}`}
                    onClick={() => {
                      setActiveTool("eyedropper");
                      setSelectedStickerId(null);
                      setActivePopover(null);
                    }}
                    title={t.overlay.toolEyedropper}
                  >
                    <Pipette size={14} />
                  </button>
                </div>
              </div>
            )}

            {/* Contextual Floating Popover for Selected Sticker (Clock or Photo) */}
            {!activePopover && selectedSticker?.type === "clock" && (
              <div
                className={`dock-popover sticker-popover ${
                  toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                } ${toolbarLayout.popAlignRight ? "align-right" : "align-left"}`}
              >
                <div className="inspector-header">
                  <span>
                    <Clock size={12} /> {t.overlay.clockConfigTitle} ·{" "}
                    <strong>{frozenTimeLabel}</strong>
                  </span>
                  <button
                    className="mini-pill"
                    onClick={() => setSelectedStickerId(null)}
                  >
                    <Check size={11} /> {t.overlay.done}
                  </button>
                </div>
                <div className="inspector-controls">
                  <button
                    className={`mini-pill ${selectedSticker.format24h ? "active" : ""}`}
                    onClick={() =>
                      updateSelectedClock({ format24h: !selectedSticker.format24h })
                    }
                  >
                    {selectedSticker.format24h ? t.overlay.clock24h : t.overlay.clock12h}
                  </button>
                  <button
                    className={`mini-pill ${selectedSticker.showSeconds ? "active" : ""}`}
                    onClick={() =>
                      updateSelectedClock({
                        showSeconds: !selectedSticker.showSeconds,
                      })
                    }
                  >
                    {t.overlay.clockSeconds}
                  </button>
                  <button
                    className={`mini-pill ${selectedSticker.showDate ? "active" : ""}`}
                    onClick={() =>
                      updateSelectedClock({ showDate: !selectedSticker.showDate })
                    }
                  >
                    {t.overlay.clockDate}
                  </button>

                  {(
                    ["glass-dark", "glass-light", "minimal", "neon"] as ClockStickerStyle[]
                  ).map((st) => (
                    <button
                      key={st}
                      className={`mini-pill ${selectedSticker.style === st ? "active" : ""}`}
                      onClick={() => updateSelectedClock({ style: st })}
                    >
                      {t.clockStyles[st]}
                    </button>
                  ))}

                  <button
                    className="mini-pill"
                    onClick={() =>
                      updateSelectedClock({
                        scale: Math.max(
                          0.65,
                          Number((selectedSticker.scale - 0.15).toFixed(2))
                        ),
                      })
                    }
                  >
                    A-
                  </button>
                  <button
                    className="mini-pill"
                    onClick={() =>
                      updateSelectedClock({
                        scale: Math.min(
                          2.2,
                          Number((selectedSticker.scale + 0.15).toFixed(2))
                        ),
                      })
                    }
                  >
                    A+
                  </button>

                  <button
                    className="mini-pill danger"
                    onClick={() => {
                      commitItems((prev) =>
                        prev.filter((i) => i.id !== selectedSticker.id)
                      );
                      setSelectedStickerId(null);
                    }}
                  >
                    <Trash2 size={12} /> {t.overlay.clockRemove}
                  </button>
                </div>
              </div>
            )}

            {!activePopover && selectedSticker?.type === "photo" && (
              <div
                className={`dock-popover sticker-popover ${
                  toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                } ${toolbarLayout.popAlignRight ? "align-right" : "align-left"}`}
              >
                <div className="inspector-controls">
                  <span className="muted">{t.overlay.photoShapeLabel}</span>
                  {(["rect", "rounded", "circle"] as PhotoMaskShape[]).map((sh) => (
                    <button
                      key={sh}
                      className={`mini-pill ${selectedSticker.shape === sh ? "active" : ""}`}
                      onClick={() => updateSelectedPhoto({ shape: sh })}
                    >
                      {sh === "rect"
                        ? t.overlay.photoRect
                        : sh === "rounded"
                          ? t.overlay.photoRounded
                          : t.overlay.photoCircle}
                    </button>
                  ))}
                  <button
                    className="mini-pill danger"
                    onClick={() => {
                      commitItems((prev) =>
                        prev.filter((i) => i.id !== selectedSticker.id)
                      );
                      setSelectedStickerId(null);
                    }}
                  >
                    <Trash2 size={12} /> {t.overlay.photoDelete}
                  </button>
                  <button
                    className="mini-pill"
                    onClick={() => setSelectedStickerId(null)}
                  >
                    <Check size={11} /> {t.overlay.done}
                  </button>
                </div>
              </div>
            )}

            {!activePopover &&
              (textInput !== null || selectedSticker?.type === "text") &&
              activeTextStyle && (
                <div
                  className={`dock-popover sticker-popover text-inspector-popover ${
                    toolbarLayout.popUpwards ? "popover-up" : "popover-down"
                  } ${toolbarLayout.popAlignRight ? "align-right" : "align-left"}`}
                  onMouseDown={(e) => {
                    if (textInput) e.preventDefault();
                  }}
                  onPointerDown={(e) => e.stopPropagation()}
                >
                  <div className="sticker-row">
                    <span className="sticker-title">{t.overlay.textConfigTitle}</span>
                    <button
                      type="button"
                      className={`mini-pill ${activeTextStyle.hasBg ? "active" : ""}`}
                      onClick={() => patchActiveTextStyle({ hasBg: true })}
                    >
                      {t.overlay.textWithBg}
                    </button>
                    <button
                      type="button"
                      className={`mini-pill ${!activeTextStyle.hasBg ? "active" : ""}`}
                      onClick={() => patchActiveTextStyle({ hasBg: false })}
                    >
                      {t.overlay.textNoBg}
                    </button>
                    <button
                      type="button"
                      className={`mini-pill icon-pill ${activeTextStyle.bold ? "active" : ""}`}
                      title={t.overlay.textBold}
                      aria-label={t.overlay.textBold}
                      onClick={() =>
                        patchActiveTextStyle({ bold: !activeTextStyle.bold })
                      }
                    >
                      <Bold size={12} />
                    </button>
                    <button
                      type="button"
                      className={`mini-pill icon-pill ${activeTextStyle.italic ? "active" : ""}`}
                      title={t.overlay.textItalic}
                      aria-label={t.overlay.textItalic}
                      onClick={() =>
                        patchActiveTextStyle({ italic: !activeTextStyle.italic })
                      }
                    >
                      <Italic size={12} />
                    </button>
                  </div>

                  <div className="sticker-row">
                    <button
                      type="button"
                      className="mini-pill"
                      title={t.overlay.textSizeDown}
                      aria-label={t.overlay.textSizeDown}
                      onClick={() =>
                        patchActiveTextStyle({
                          fontSize: Math.max(12, activeTextStyle.fontSize - 2),
                        })
                      }
                    >
                      A-
                    </button>
                    <span className="text-size-readout">
                      {activeTextStyle.fontSize}px
                    </span>
                    <button
                      type="button"
                      className="mini-pill"
                      title={t.overlay.textSizeUp}
                      aria-label={t.overlay.textSizeUp}
                      onClick={() =>
                        patchActiveTextStyle({
                          fontSize: Math.min(72, activeTextStyle.fontSize + 2),
                        })
                      }
                    >
                      A+
                    </button>
                    <span className="text-swatch-sep" />
                    <div className="text-inline-swatches">
                      {PALETTE.map((c) => (
                        <button
                          key={c}
                          type="button"
                          className={`text-mini-swatch ${
                            activeTextStyle.color === c ? "selected" : ""
                          }`}
                          style={{ backgroundColor: c }}
                          aria-label={c}
                          onClick={() => {
                            setActiveColor(c);
                            patchActiveTextStyle({ color: c });
                          }}
                        />
                      ))}
                    </div>
                  </div>

                  <div className="sticker-row">
                    {!textInput && selectedSticker?.type === "text" && (
                      <button
                        type="button"
                        className="mini-pill"
                        onClick={() => startEditingTextItem(selectedSticker)}
                      >
                        <Pencil size={11} /> {t.overlay.textEditBtn}
                      </button>
                    )}
                    <button
                      type="button"
                      className="mini-pill danger"
                      onClick={removeActiveText}
                    >
                      <Trash2 size={11} /> {t.overlay.textDelete}
                    </button>
                    <button
                      type="button"
                      className="mini-pill"
                      onClick={() => {
                        if (textInput) {
                          commitText();
                        } else {
                          setSelectedStickerId(null);
                        }
                      }}
                    >
                      <Check size={11} /> {t.overlay.done}
                    </button>
                  </div>
                </div>
              )}
          </div>

          {/* Vertical Output Actions Bar (completes the L along the right edge) */}
          <div
            className="floating-dock"
            style={{ left: toolbarLayout.vLeft, top: toolbarLayout.vTop }}
            onPointerDown={(e) => e.stopPropagation()}
          >
            <div ref={actionsDockRef} className="dock-bar-vertical">
              <button
                className="action-btn primary icon-only"
                onClick={() => void handleCopy()}
                aria-label={t.overlay.copy}
                {...tipProps("copy", "vertical")}
              >
                <Copy size={14} />
              </button>
              <button
                className="action-btn icon-only"
                onClick={() => void handleSave()}
                aria-label={t.overlay.save}
                {...tipProps("save", "vertical")}
              >
                <Save size={14} />
              </button>
              <button
                className="action-btn icon-only"
                onClick={() => void handlePin()}
                aria-label={t.overlay.pin}
                {...tipProps("pin", "vertical")}
              >
                <Pin size={14} />
              </button>
              <button
                ref={shareBtnRef}
                className="action-btn icon-only"
                onClick={(e) => void handleShare(e.currentTarget.getBoundingClientRect())}
                aria-label={t.overlay.share}
                {...tipProps("share", "vertical")}
              >
                <Share2 size={14} />
              </button>
              <button
                className="action-btn icon-only"
                onClick={() => void handlePrint()}
                aria-label={t.overlay.print}
                {...tipProps("print", "vertical")}
              >
                <Printer size={14} />
              </button>

              <span className="dock-sep-h" />

              <button
                className="action-btn danger icon-only"
                onClick={() => void handleClose()}
                aria-label={t.overlay.cancel}
                {...tipProps("cancel", "vertical")}
              >
                <X size={14} />
              </button>
            </div>
          </div>

          {renderedHoverTip && (
            <div
              className="dock-tooltip"
              style={renderedHoverTip.style}
              role="tooltip"
            >
              <div className="dock-tooltip-head">
                <span className="dock-tooltip-title">
                  {renderedHoverTip.copy.title}
                </span>
                {renderedHoverTip.kbd && (
                  <kbd className="dock-tooltip-kbd">{renderedHoverTip.kbd}</kbd>
                )}
              </div>
              <span className="dock-tooltip-desc">
                {renderedHoverTip.copy.desc}
              </span>
            </div>
          )}
        </>
      )}

      {/* Zoomed Pixel Loupe HUD (selecting or using Eyedropper) */}
      {loupeInfo && (
        <div
          className="loupe-hud"
          style={{
            left:
              loupeInfo.x > viewport.w - 146
                ? loupeInfo.x - 134
                : loupeInfo.x + 18,
            top:
              loupeInfo.y > viewport.h - 166
                ? loupeInfo.y - 154
                : loupeInfo.y + 18,
          }}
        >
          <canvas
            ref={loupeCanvasRef}
            width={112}
            height={112}
            className="loupe-canvas"
          />
          <div className="loupe-meta">
            <span
              className="loupe-swatch"
              style={{ backgroundColor: loupeInfo.hex }}
            />
            <span>{loupeInfo.hex}</span>
            <span>
              {Math.round(loupeInfo.x)},{Math.round(loupeInfo.y)}
            </span>
          </div>
        </div>
      )}

      {/* Action Confirmation / Validation Error Toast */}
      {toast && (
        <div className={`overlay-toast ${toast.error ? "error" : ""}`}>
          {toast.error ? <AlertTriangle size={16} /> : <Check size={16} />}{" "}
          {toast.msg}
        </div>
      )}
    </div>
  );
};
