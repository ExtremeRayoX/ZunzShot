export type Language = "es" | "en";

export type CaptureMode =
  | "region"
  | "quickCopy"
  | "circle"
  | "window"
  | "fullscreen";

export type ClockStickerStyle = "glass-dark" | "glass-light" | "minimal" | "neon";

export interface AppSettings {
  language: Language;
  languageSelected: boolean;
  shortcutRegion: string;
  shortcutQuickCopy: string;
  shortcutCircle: string;
  shortcutWindow: string;
  shortcutFullscreen: string;
  cmdCopy: string;
  cmdSave: string;
  cmdPin: string;
  cmdShare: string;
  cmdPrint: string;
  cmdCancel: string;
  saveFolder: string;
  outputFormat: "png" | "jpg";
  launchAtStartup: boolean;
  showMagnifier: boolean;
  ultraLightMode: boolean;
  autoCheckUpdates: boolean;
  updaterPromptShown: boolean;
  clockFormat24h: boolean;
  clockShowSeconds: boolean;
  clockShowDate: boolean;
  clockStyle: ClockStickerStyle;
}

export interface WindowCandidate {
  id: number;
  title: string;
  appName: string;
  rx: number;
  ry: number;
  rw: number;
  rh: number;
}

export interface CaptureMetadata {
  mode: CaptureMode;
  capturedAtIso: string;
  capturedAtMs: number;
  imageWidth: number;
  imageHeight: number;
  logicalWidth: number;
  logicalHeight: number;
  scaleFactor: number;
  windows: WindowCandidate[];
}

export type SelectionShape = "rect" | "circle";

export interface SelectionBox {
  x: number;
  y: number;
  w: number;
  h: number;
  shape: SelectionShape;
}

export type ToolType =
  | "select"
  | "pen"
  | "arrow"
  | "rect"
  | "circle"
  | "underline"
  | "highlight"
  | "pixelate"
  | "text"
  | "step"
  | "eyedropper";

export interface PixelateAnnotation {
  id: string;
  type: "pixelate";
  x: number;
  y: number;
  w: number;
  h: number;
  blockSize: number;
}

export interface StepAnnotation {
  id: string;
  type: "step";
  x: number;
  y: number;
  number: number;
  color: string;
  radius: number;
}

export interface PenAnnotation {
  id: string;
  type: "pen";
  points: Array<{ x: number; y: number }>;
  color: string;
  strokeWidth: number;
}

export interface ArrowAnnotation {
  id: string;
  type: "arrow";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  color: string;
  strokeWidth: number;
}

export interface BoxAnnotation {
  id: string;
  type: "rect" | "circle";
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
  strokeWidth: number;
}

export interface UnderlineAnnotation {
  id: string;
  type: "underline";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  color: string;
  strokeWidth: number;
}

export interface HighlightAnnotation {
  id: string;
  type: "highlight";
  points: Array<{ x: number; y: number }>;
  color: string;
  strokeWidth: number;
}

export interface TextAnnotation {
  id: string;
  type: "text";
  x: number;
  y: number;
  text: string;
  color: string;
  fontSize: number;
  hasBg: boolean;
}

export type PhotoMaskShape = "rect" | "rounded" | "circle";

export interface PhotoSticker {
  id: string;
  type: "photo";
  x: number;
  y: number;
  w: number;
  h: number;
  shape: PhotoMaskShape;
  img: HTMLImageElement;
}

export interface ClockSticker {
  id: string;
  type: "clock";
  x: number;
  y: number;
  scale: number;
  style: ClockStickerStyle;
  format24h: boolean;
  showSeconds: boolean;
  showDate: boolean;
  accentColor: string;
}

export type OverlayItem =
  | PenAnnotation
  | ArrowAnnotation
  | BoxAnnotation
  | UnderlineAnnotation
  | HighlightAnnotation
  | PixelateAnnotation
  | StepAnnotation
  | TextAnnotation
  | PhotoSticker
  | ClockSticker;
