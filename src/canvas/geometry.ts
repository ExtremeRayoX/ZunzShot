import type {
  ClockSticker,
  Language,
  OverlayItem,
  PhotoSticker,
  SelectionBox,
  TextAnnotation,
} from "../types/zunzshot";

export type HandleId =
  | "nw"
  | "n"
  | "ne"
  | "e"
  | "se"
  | "s"
  | "sw"
  | "w"
  | "move";

export function clampSelection(
  sel: SelectionBox,
  maxW: number,
  maxH: number
): SelectionBox {
  const w = Math.max(12, Math.min(sel.w, maxW));
  const h = Math.max(12, Math.min(sel.h, maxH));
  const x = Math.max(0, Math.min(sel.x, maxW - w));
  const y = Math.max(0, Math.min(sel.y, maxH - h));
  return { ...sel, x, y, w, h };
}

export function hitTestSelectionHandle(
  px: number,
  py: number,
  sel: SelectionBox,
  pad = 10
): HandleId | null {
  const { x, y, w, h } = sel;
  const cx = x + w / 2;
  const cy = y + h / 2;

  const corners: Array<[HandleId, number, number]> = [
    ["nw", x, y],
    ["ne", x + w, y],
    ["se", x + w, y + h],
    ["sw", x, y + h],
    ["n", cx, y],
    ["e", x + w, cy],
    ["s", cx, y + h],
    ["w", x, cy],
  ];

  for (const [id, hx, hy] of corners) {
    if (Math.abs(px - hx) <= pad && Math.abs(py - hy) <= pad) {
      return id;
    }
  }

  if (sel.shape === "circle") {
    const rx = w / 2;
    const ry = h / 2;
    if (rx > 0 && ry > 0) {
      const norm = ((px - cx) / rx) ** 2 + ((py - cy) / ry) ** 2;
      if (norm <= 1.0) return "move";
    }
  } else if (px >= x && px <= x + w && py >= y && py <= y + h) {
    return "move";
  }

  return null;
}

export function formatFrozenClock(
  capturedAtIso: string,
  format24h: boolean,
  showSeconds: boolean,
  showDate: boolean,
  lang: Language
): { timeText: string; dateText: string | null } {
  const date = new Date(capturedAtIso);
  const locale = lang === "es" ? "es-ES" : "en-US";

  const timeText = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: showSeconds ? "2-digit" : undefined,
    hour12: !format24h,
  }).format(date);

  const dateText = showDate
    ? new Intl.DateTimeFormat(locale, {
        day: "2-digit",
        month: "short",
        year: "numeric",
      }).format(date)
    : null;

  return { timeText, dateText };
}

export function measureClockSticker(
  ctx: CanvasRenderingContext2D,
  sticker: ClockSticker,
  capturedAtIso: string,
  lang: Language
): { w: number; h: number; timeText: string; dateText: string | null } {
  const { timeText, dateText } = formatFrozenClock(
    capturedAtIso,
    sticker.format24h,
    sticker.showSeconds,
    sticker.showDate,
    lang
  );

  const s = sticker.scale;
  ctx.save();
  ctx.font = `700 ${Math.round(21 * s)}px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`;
  const timeW = ctx.measureText(timeText).width;
  let dateW = 0;
  if (dateText) {
    ctx.font = `600 ${Math.round(11 * s)}px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`;
    dateW = ctx.measureText(dateText).width;
  }
  ctx.restore();

  const iconWidth = 14 * s;
  const iconGap = 6 * s;
  const topRowW = iconWidth + iconGap + timeW;
  const padX = 16 * s;
  const w = Math.max(topRowW + padX * 2, dateW + padX * 2);
  const h = (dateText ? 54 : 40) * s;
  return { w, h, timeText, dateText };
}

export function drawClockSticker(
  ctx: CanvasRenderingContext2D,
  sticker: ClockSticker,
  capturedAtIso: string,
  lang: Language,
  selected = false
): { x: number; y: number; w: number; h: number } {
  const { w, h, timeText, dateText } = measureClockSticker(
    ctx,
    sticker,
    capturedAtIso,
    lang
  );
  const s = sticker.scale;
  const x = sticker.x;
  const y = sticker.y;
  const radius = Math.min(14 * s, h / 2);

  ctx.save();
  ctx.shadowColor = "rgba(0, 0, 0, 0.35)";
  ctx.shadowBlur = 14 * s;
  ctx.shadowOffsetY = 4 * s;

  drawRoundedRectPath(ctx, x, y, w, h, radius);

  if (sticker.style === "glass-dark") {
    ctx.fillStyle = "rgba(15, 23, 42, 0.78)";
    ctx.fill();
    ctx.shadowColor = "transparent";
    ctx.lineWidth = 1.2 * s;
    ctx.strokeStyle = "rgba(255, 255, 255, 0.24)";
    ctx.stroke();
  } else if (sticker.style === "glass-light") {
    ctx.fillStyle = "rgba(255, 255, 255, 0.86)";
    ctx.fill();
    ctx.shadowColor = "transparent";
    ctx.lineWidth = 1.2 * s;
    ctx.strokeStyle = "rgba(15, 23, 42, 0.14)";
    ctx.stroke();
  } else if (sticker.style === "neon") {
    ctx.fillStyle = "rgba(9, 14, 26, 0.88)";
    ctx.fill();
    ctx.shadowColor = "rgba(6, 214, 198, 0.65)";
    ctx.shadowBlur = 12 * s;
    ctx.lineWidth = 1.8 * s;
    ctx.strokeStyle = "#06d6c6";
    ctx.stroke();
  } else {
    // minimal
    ctx.fillStyle = "rgba(0, 0, 0, 0.55)";
    ctx.fill();
    ctx.shadowColor = "transparent";
  }

  ctx.shadowColor = "transparent";
  const textColor =
    sticker.style === "glass-light"
      ? "#0f172a"
      : sticker.style === "neon"
        ? "#67f3e8"
        : "#ffffff";
  const subColor =
    sticker.style === "glass-light"
      ? "#475569"
      : sticker.style === "neon"
        ? "#a5f3fc"
        : "rgba(255, 255, 255, 0.75)";

  ctx.font = `700 ${Math.round(21 * s)}px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`;
  const timeW = ctx.measureText(timeText).width;
  const cr = 7 * s;
  const iconGap = 6 * s;
  const topRowW = cr * 2 + iconGap + timeW;
  const topStartX = x + (w - topRowW) / 2;

  // Center clock glyph + time together so the centered date below aligns cleanly
  const cx = topStartX + cr;
  const cy = y + (dateText ? 19 * s : h / 2);
  ctx.strokeStyle = sticker.style === "neon" ? "#06d6c6" : sticker.accentColor;
  ctx.lineWidth = 1.8 * s;
  ctx.beginPath();
  ctx.arc(cx, cy, cr, 0, Math.PI * 2);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(cx, cy - cr * 0.55);
  ctx.lineTo(cx, cy);
  ctx.lineTo(cx + cr * 0.45, cy + cr * 0.15);
  ctx.stroke();

  // Time text
  ctx.fillStyle = textColor;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ctx.fillText(timeText, topStartX + cr * 2 + iconGap, cy + 1 * s);

  if (dateText) {
    ctx.fillStyle = subColor;
    ctx.font = `600 ${Math.round(11 * s)}px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`;
    ctx.textAlign = "center";
    ctx.fillText(dateText, x + w / 2, y + h - 11.5 * s);
  }

  if (selected) {
    ctx.setLineDash([4, 3]);
    ctx.strokeStyle = "#38bdf8";
    ctx.lineWidth = 1.5;
    drawRoundedRectPath(ctx, x - 3, y - 3, w + 6, h + 6, radius + 2);
    ctx.stroke();
  }

  ctx.restore();
  return { x, y, w, h };
}

export function drawPhotoSticker(
  ctx: CanvasRenderingContext2D,
  sticker: PhotoSticker,
  selected = false
): void {
  const { x, y, w, h, shape, img } = sticker;
  ctx.save();
  ctx.shadowColor = "rgba(0,0,0,0.35)";
  ctx.shadowBlur = 12;
  ctx.shadowOffsetY = 4;

  ctx.beginPath();
  if (shape === "circle") {
    ctx.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
  } else if (shape === "rounded") {
    drawRoundedRectPath(ctx, x, y, w, h, Math.min(18, Math.min(w, h) * 0.18));
  } else {
    ctx.rect(x, y, w, h);
  }
  ctx.closePath();

  // Fill slightly before clipping for clean shadow
  ctx.fillStyle = "rgba(0,0,0,0.01)";
  ctx.fill();
  ctx.shadowColor = "transparent";

  ctx.save();
  ctx.clip();
  if (img && img.complete && img.naturalWidth > 0) {
    ctx.drawImage(img, x, y, w, h);
  }
  ctx.restore();

  // Subtle white frame on rounded/circle Instagram-style photo sticker
  ctx.lineWidth = 2;
  ctx.strokeStyle = "rgba(255,255,255,0.85)";
  ctx.stroke();

  if (selected) {
    ctx.setLineDash([4, 3]);
    ctx.strokeStyle = "#38bdf8";
    ctx.lineWidth = 1.8;
    ctx.strokeRect(x - 3, y - 3, w + 6, h + 6);
    // Corner resize handle
    ctx.setLineDash([]);
    ctx.fillStyle = "#06d6c6";
    ctx.fillRect(x + w - 5, y + h - 5, 10, 10);
  }
  ctx.restore();
}

export interface PixelSource {
  bitmap: ImageBitmap;
  cssW: number;
  cssH: number;
  offsetX?: number;
  offsetY?: number;
}

let pixelScratchCanvas: HTMLCanvasElement | null = null;

/** Releases backing pixel memory held by the pixelation scratch canvas between sessions. */
export function resetGeometryCache(): void {
  if (pixelScratchCanvas) {
    pixelScratchCanvas.width = 0;
    pixelScratchCanvas.height = 0;
    pixelScratchCanvas = null;
  }
}

function getPixelScratch(w: number, h: number): [HTMLCanvasElement, CanvasRenderingContext2D | null] {
  if (!pixelScratchCanvas) {
    pixelScratchCanvas = document.createElement("canvas");
  }
  pixelScratchCanvas.width = w;
  pixelScratchCanvas.height = h;
  return [pixelScratchCanvas, pixelScratchCanvas.getContext("2d")];
}

export function drawOverlayItem(
  ctx: CanvasRenderingContext2D,
  item: OverlayItem,
  capturedAtIso: string,
  lang: Language,
  selected = false,
  pixelSource?: PixelSource
): void {
  if (item.type === "clock") {
    drawClockSticker(ctx, item, capturedAtIso, lang, selected);
    return;
  }
  if (item.type === "photo") {
    drawPhotoSticker(ctx, item, selected);
    return;
  }

  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  if (item.type === "pen") {
    const pts = item.points;
    if (pts.length > 0) {
      ctx.strokeStyle = item.color;
      ctx.lineWidth = item.strokeWidth;
      ctx.beginPath();
      ctx.moveTo(pts[0].x, pts[0].y);
      if (pts.length === 1) {
        ctx.lineTo(pts[0].x + 0.1, pts[0].y);
      } else if (pts.length === 2) {
        ctx.lineTo(pts[1].x, pts[1].y);
      } else {
        for (let i = 1; i < pts.length - 1; i++) {
          const midX = (pts[i].x + pts[i + 1].x) / 2;
          const midY = (pts[i].y + pts[i + 1].y) / 2;
          ctx.quadraticCurveTo(pts[i].x, pts[i].y, midX, midY);
        }
        const last = pts[pts.length - 1];
        ctx.lineTo(last.x, last.y);
      }
      ctx.stroke();
    }
  } else if (item.type === "arrow") {
    const { x1, y1, x2, y2, color, strokeWidth } = item;
    const angle = Math.atan2(y2 - y1, x2 - x1);
    const headLen = Math.max(14, strokeWidth * 4.2);

    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = strokeWidth;

    const endX = x2 - Math.cos(angle) * (headLen * 0.55);
    const endY = y2 - Math.sin(angle) * (headLen * 0.55);
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(endX, endY);
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(x2, y2);
    ctx.lineTo(
      x2 - headLen * Math.cos(angle - Math.PI / 6),
      y2 - headLen * Math.sin(angle - Math.PI / 6)
    );
    ctx.lineTo(
      x2 - headLen * Math.cos(angle + Math.PI / 6),
      y2 - headLen * Math.sin(angle + Math.PI / 6)
    );
    ctx.closePath();
    ctx.fill();
  } else if (item.type === "rect") {
    ctx.strokeStyle = item.color;
    ctx.lineWidth = item.strokeWidth;
    ctx.strokeRect(item.x, item.y, item.w, item.h);
  } else if (item.type === "circle") {
    ctx.strokeStyle = item.color;
    ctx.lineWidth = item.strokeWidth;
    ctx.beginPath();
    ctx.ellipse(
      item.x + item.w / 2,
      item.y + item.h / 2,
      Math.max(1, Math.abs(item.w / 2)),
      Math.max(1, Math.abs(item.h / 2)),
      0,
      0,
      Math.PI * 2
    );
    ctx.stroke();
  } else if (item.type === "underline") {
    ctx.strokeStyle = item.color;
    ctx.lineWidth = Math.max(2, item.strokeWidth);
    ctx.beginPath();
    ctx.moveTo(item.x1, item.y1);
    ctx.lineTo(item.x2, item.y2);
    ctx.stroke();
  } else if (item.type === "highlight") {
    if (item.points.length > 0) {
      ctx.globalAlpha = 0.36;
      ctx.strokeStyle = item.color;
      ctx.lineWidth = Math.max(14, item.strokeWidth * 4.5);
      ctx.beginPath();
      ctx.moveTo(item.points[0].x, item.points[0].y);
      for (let i = 1; i < item.points.length; i++) {
        ctx.lineTo(item.points[i].x, item.points[i].y);
      }
      if (item.points.length === 1) {
        ctx.lineTo(item.points[0].x + 0.1, item.points[0].y);
      }
      ctx.stroke();
    }
  } else if (item.type === "text") {
    const m = measureTextAnnotation(ctx, item);
    ctx.font = m.font;
    ctx.textBaseline = "top";
    if (item.hasBg) {
      ctx.fillStyle = "rgba(15, 23, 42, 0.72)";
      drawRoundedRectPath(ctx, m.boxX, m.boxY, m.boxW, m.boxH, 6);
      ctx.fill();
    } else {
      ctx.shadowColor = "rgba(0, 0, 0, 0.65)";
      ctx.shadowBlur = 4;
    }
    ctx.fillStyle = item.color;
    m.lines.forEach((line, idx) => {
      ctx.fillText(line, item.x, item.y + idx * m.lineH);
    });
    if (selected) {
      ctx.save();
      ctx.shadowColor = "transparent";
      ctx.shadowBlur = 0;
      ctx.strokeStyle = "#38bdf8";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([4, 3]);
      drawRoundedRectPath(
        ctx,
        m.boxX - 3,
        m.boxY - 3,
        m.boxW + 6,
        m.boxH + 6,
        8
      );
      ctx.stroke();
      ctx.restore();
    }
  } else if (item.type === "pixelate") {
    const rw = Math.max(1, item.w);
    const rh = Math.max(1, item.h);
    if (pixelSource && item.w >= 2 && item.h >= 2) {
      const ox = pixelSource.offsetX ?? 0;
      const oy = pixelSource.offsetY ?? 0;
      const ix0 = Math.max(item.x, ox);
      const iy0 = Math.max(item.y, oy);
      const ix1 = Math.min(item.x + rw, ox + pixelSource.cssW);
      const iy1 = Math.min(item.y + rh, oy + pixelSource.cssH);
      const iw = ix1 - ix0;
      const ih = iy1 - iy0;
      if (iw >= 1 && ih >= 1) {
        const scaleX = pixelSource.bitmap.width / Math.max(1, pixelSource.cssW);
        const scaleY = pixelSource.bitmap.height / Math.max(1, pixelSource.cssH);
        const sx = Math.max(0, Math.min(pixelSource.bitmap.width - 1, (ix0 - ox) * scaleX));
        const sy = Math.max(0, Math.min(pixelSource.bitmap.height - 1, (iy0 - oy) * scaleY));
        const sw = Math.max(1, Math.min(pixelSource.bitmap.width - sx, iw * scaleX));
        const sh = Math.max(1, Math.min(pixelSource.bitmap.height - sy, ih * scaleY));
        const cols = Math.max(1, Math.round(iw / Math.max(4, item.blockSize)));
        const rows = Math.max(1, Math.round(ih / Math.max(4, item.blockSize)));
        const [scratch, sCtx] = getPixelScratch(cols, rows);
        if (sCtx) {
          sCtx.imageSmoothingEnabled = true;
          sCtx.clearRect(0, 0, cols, rows);
          sCtx.drawImage(pixelSource.bitmap, sx, sy, sw, sh, 0, 0, cols, rows);
          ctx.imageSmoothingEnabled = false;
          ctx.drawImage(scratch, 0, 0, cols, rows, ix0, iy0, iw, ih);
        }
      }
    }
    if (selected) {
      ctx.setLineDash([4, 3]);
      ctx.strokeStyle = "#06d6c6";
      ctx.lineWidth = 1.5;
      ctx.strokeRect(item.x, item.y, rw, rh);
    }
  } else if (item.type === "step") {
    const r = item.radius;
    ctx.shadowColor = "rgba(0, 0, 0, 0.38)";
    ctx.shadowBlur = 8;
    ctx.shadowOffsetY = 2;
    ctx.fillStyle = item.color;
    ctx.beginPath();
    ctx.arc(item.x, item.y, r, 0, Math.PI * 2);
    ctx.fill();

    ctx.shadowColor = "transparent";
    ctx.lineWidth = 2;
    ctx.strokeStyle = "rgba(255, 255, 255, 0.92)";
    ctx.stroke();

    if (selected) {
      ctx.setLineDash([3, 3]);
      ctx.strokeStyle = "#38bdf8";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(item.x, item.y, r + 4, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
    }

    const isLight = item.color.toLowerCase() === "#ffffff";
    ctx.fillStyle = isLight ? "#0f172a" : "#ffffff";
    ctx.font = `800 ${Math.round(r * 1.1)}px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(item.number), item.x, item.y + 0.5);
  }

  ctx.restore();
}

function drawRoundedRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number
): void {
  const radius = Math.max(0, Math.min(r, w / 2, h / 2));
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.lineTo(x + w - radius, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + radius);
  ctx.lineTo(x + w, y + h - radius);
  ctx.quadraticCurveTo(x + w, y + h, x + w - radius, y + h);
  ctx.lineTo(x + radius, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - radius);
  ctx.lineTo(x, y + radius);
  ctx.quadraticCurveTo(x, y, x + radius, y);
  ctx.closePath();
}

export function hitTestSticker(
  ctx: CanvasRenderingContext2D,
  px: number,
  py: number,
  items: OverlayItem[],
  capturedAtIso: string,
  lang: Language
): { item: OverlayItem; resizeCorner: boolean } | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.type === "photo") {
      const nearCorner =
        Math.abs(px - (item.x + item.w)) <= 12 &&
        Math.abs(py - (item.y + item.h)) <= 12;
      if (nearCorner) return { item, resizeCorner: true };
      if (
        px >= item.x &&
        px <= item.x + item.w &&
        py >= item.y &&
        py <= item.y + item.h
      ) {
        return { item, resizeCorner: false };
      }
    } else if (item.type === "clock") {
      const box = measureClockSticker(ctx, item, capturedAtIso, lang);
      if (
        px >= item.x &&
        px <= item.x + box.w &&
        py >= item.y &&
        py <= item.y + box.h
      ) {
        return { item, resizeCorner: false };
      }
    } else if (item.type === "step") {
      if (Math.hypot(px - item.x, py - item.y) <= item.radius + 4) {
        return { item, resizeCorner: false };
      }
    } else if (item.type === "text") {
      const m = measureTextAnnotation(ctx, item);
      const hitW = Math.max(m.boxW, 28);
      if (
        px >= m.boxX &&
        px <= m.boxX + hitW &&
        py >= m.boxY &&
        py <= m.boxY + m.boxH
      ) {
        return { item, resizeCorner: false };
      }
    }
  }
  return null;
}

export function buildTextFont(
  fontSize: number,
  bold: boolean,
  italic: boolean
): string {
  return `${italic ? "italic " : ""}${bold ? "700" : "400"} ${fontSize}px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`;
}

export function measureTextAnnotation(
  ctx: CanvasRenderingContext2D,
  item: TextAnnotation
): {
  font: string;
  lines: string[];
  lineH: number;
  maxW: number;
  boxX: number;
  boxY: number;
  boxW: number;
  boxH: number;
} {
  ctx.save();
  const font = buildTextFont(item.fontSize, item.bold, item.italic);
  ctx.font = font;
  const lines = item.text.split("\n");
  const lineH = item.fontSize * 1.22;
  let maxW = 0;
  for (const line of lines) {
    maxW = Math.max(maxW, ctx.measureText(line).width);
  }
  ctx.restore();
  return {
    font,
    lines,
    lineH,
    maxW,
    boxX: item.x - 6,
    boxY: item.y - 4,
    boxW: maxW + 12,
    boxH: lines.length * lineH + 6,
  };
}

/**
 * Renders the final cropped selection (rect or transparent circle) plus all
 * annotations and Instagram stickers at native physical resolution.
 */
export async function renderExportPngBytes(
  baseBitmap: ImageBitmap,
  cssW: number,
  cssH: number,
  selection: SelectionBox,
  items: OverlayItem[],
  capturedAtIso: string,
  lang: Language,
  bitmapOriginX = 0,
  bitmapOriginY = 0
): Promise<Uint8Array> {
  const scaleX = baseBitmap.width / cssW;
  const scaleY = baseBitmap.height / cssH;

  const outX = Math.max(0, Math.round((selection.x - bitmapOriginX) * scaleX));
  const outY = Math.max(0, Math.round((selection.y - bitmapOriginY) * scaleY));
  const outW = Math.max(1, Math.round(selection.w * scaleX));
  const outH = Math.max(1, Math.round(selection.h * scaleY));

  const outCanvas = document.createElement("canvas");
  outCanvas.width = outW;
  outCanvas.height = outH;
  const ctx = outCanvas.getContext("2d");
  if (!ctx) throw new Error("Could not acquire export 2D context");

  ctx.save();
  if (selection.shape === "circle") {
    ctx.beginPath();
    ctx.ellipse(outW / 2, outH / 2, outW / 2, outH / 2, 0, 0, Math.PI * 2);
    ctx.closePath();
    ctx.clip();
  }

  // Draw crisp physical pixels from the captured monitor (or stitched composite region)
  ctx.drawImage(
    baseBitmap,
    outX,
    outY,
    Math.min(outW, Math.max(1, baseBitmap.width - outX)),
    Math.min(outH, Math.max(1, baseBitmap.height - outY)),
    0,
    0,
    outW,
    outH
  );

  // Transform virtual logical overlay coordinates into cropped physical pixel space
  ctx.translate(-(outX + bitmapOriginX * scaleX), -(outY + bitmapOriginY * scaleY));
  ctx.scale(scaleX, scaleY);

  for (const item of items) {
    drawOverlayItem(ctx, item, capturedAtIso, lang, false, {
      bitmap: baseBitmap,
      cssW,
      cssH,
      offsetX: bitmapOriginX,
      offsetY: bitmapOriginY,
    });
  }
  ctx.restore();

  const blob = await new Promise<Blob>((resolve, reject) => {
    outCanvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("PNG export failed"))),
      "image/png"
    );
  });

  const buf = await blob.arrayBuffer();
  return new Uint8Array(buf);
}
