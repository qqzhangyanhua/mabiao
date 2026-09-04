import { BEAD, FUSE_W, snap } from "./fuseBeadLayout";

export const BOARD = "#eef1f6";
export const NAVY = "#172b72";
export const CARD = "#0b173e";
export const YELLOW = "#f5c400";
export const PURPLE = "#8b5cf6";
export const WHITE = "#f8fafc";
export const INK = "#0f172a";
export const FACE = '"PingFang SC", "Hiragino Sans GB", "Helvetica Neue", sans-serif';

export function shade(hex: string, delta: number): string {
  const n = Number.parseInt(hex.slice(1), 16);
  const c = (shift: number) =>
    Math.max(0, Math.min(255, ((n >> shift) & 255) + delta))
      .toString(16)
      .padStart(2, "0");
  return `#${c(16)}${c(8)}${c(0)}`;
}

export function drawBead(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  color: string,
  solid = false,
  pitch = BEAD,
): void {
  const r = pitch / 2 + 0.12;
  const cx = x + pitch / 2;
  const cy = y + pitch / 2;
  ctx.fillStyle = "rgba(12,10,28,0.28)";
  ctx.beginPath();
  ctx.ellipse(cx + 0.55, cy + 1.05, r * 0.94, r * 0.72, 0, 0, Math.PI * 2);
  ctx.fill();
  const body = ctx.createRadialGradient(
    cx - r * 0.3,
    cy - r * 0.34,
    r * 0.08,
    cx,
    cy + r * 0.12,
    r,
  );
  body.addColorStop(0, shade(color, 62));
  body.addColorStop(0.4, color);
  body.addColorStop(1, shade(color, -46));
  ctx.fillStyle = body;
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "rgba(255,255,255,0.62)";
  ctx.beginPath();
  ctx.ellipse(cx - r * 0.3, cy - r * 0.34, r * 0.32, r * 0.16, -0.55, 0, Math.PI * 2);
  ctx.fill();
  const hr = solid ? r * 0.14 : r * 0.28;
  ctx.fillStyle = solid ? "rgba(10,8,24,0.2)" : "rgba(8,6,18,0.72)";
  ctx.beginPath();
  ctx.arc(cx, cy + 0.1, hr, 0, Math.PI * 2);
  ctx.fill();
  if (!solid) {
    ctx.fillStyle = "rgba(6,4,14,0.9)";
    ctx.beginPath();
    ctx.arc(cx, cy + 0.15, hr * 0.55, 0, Math.PI * 2);
    ctx.fill();
  }
}

export function drawPegboard(ctx: CanvasRenderingContext2D, height: number): void {
  ctx.fillStyle = BOARD;
  ctx.fillRect(0, 0, FUSE_W, height);
  for (let y = 0; y < height; y += BEAD) {
    for (let x = 0; x < FUSE_W; x += BEAD) {
      const cx = x + BEAD / 2;
      const cy = y + BEAD / 2;
      ctx.fillStyle = "rgba(145,155,175,0.28)";
      ctx.beginPath();
      ctx.arc(cx, cy + 0.55, 1.45, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#d0d6e2";
      ctx.beginPath();
      ctx.arc(cx, cy + 0.15, 1.25, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#ffffff";
      ctx.beginPath();
      ctx.arc(cx, cy - 0.2, 1.05, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

export function drawBeadBorder(ctx: CanvasRenderingContext2D, height: number): void {
  const c0 = 3;
  const c1 = Math.floor(FUSE_W / BEAD) - 4;
  const r0 = 3;
  const r1 = Math.floor(height / BEAD) - 4;
  for (let c = c0; c <= c1; c += 1) {
    drawBead(ctx, c * BEAD, r0 * BEAD, NAVY);
    drawBead(ctx, c * BEAD, (r0 + 1) * BEAD, NAVY);
    drawBead(ctx, c * BEAD, (r1 - 1) * BEAD, NAVY);
    drawBead(ctx, c * BEAD, r1 * BEAD, NAVY);
  }
  for (let r = r0 + 2; r <= r1 - 2; r += 1) {
    drawBead(ctx, c0 * BEAD, r * BEAD, NAVY);
    drawBead(ctx, (c0 + 1) * BEAD, r * BEAD, NAVY);
    drawBead(ctx, (c1 - 1) * BEAD, r * BEAD, NAVY);
    drawBead(ctx, c1 * BEAD, r * BEAD, NAVY);
  }
}

export function drawCard(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  radius = 16,
): void {
  ctx.fillStyle = CARD;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, radius);
  ctx.fill();
  ctx.fillStyle = "rgba(255,255,255,0.05)";
  for (let py = y + BEAD; py < y + h - BEAD / 2; py += BEAD) {
    for (let px = x + BEAD; px < x + w - BEAD / 2; px += BEAD) {
      ctx.beginPath();
      ctx.arc(px + BEAD / 2, py + BEAD / 2, 1.15, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  const gx0 = snap(x);
  const gx1 = snap(x + w - BEAD);
  const gy0 = snap(y);
  const gy1 = snap(y + h - BEAD);
  for (let gx = gx0; gx <= gx1; gx += BEAD) {
    drawBead(ctx, gx, gy0, NAVY);
    drawBead(ctx, gx, gy1, NAVY);
  }
  for (let gy = gy0 + BEAD; gy < gy1; gy += BEAD) {
    drawBead(ctx, gx0, gy, NAVY);
    drawBead(ctx, gx1, gy, NAVY);
  }
}

export function rasterBeadCells(
  text: string,
  fontPx: number,
  pitch = BEAD,
): { cells: [number, number][]; width: number } {
  if (typeof document === "undefined" || text.length === 0) {
    return { cells: [], width: 0 };
  }
  const off = document.createElement("canvas");
  const octx = off.getContext("2d", { willReadFrequently: true });
  if (!octx) {
    return { cells: [], width: 0 };
  }
  const font = `900 ${fontPx}px ${FACE}`;
  octx.font = font;
  const pad = pitch * 2;
  const width = Math.ceil(octx.measureText(text).width) + pad * 2;
  const height = fontPx + pad * 2;
  const scale = 2;
  off.width = width * scale;
  off.height = height * scale;
  octx.setTransform(scale, 0, 0, scale, 0, 0);
  octx.font = font;
  octx.textBaseline = "top";
  octx.fillStyle = "#000";
  octx.fillText(text, pad, pad);
  const img = octx.getImageData(0, 0, off.width, off.height);
  const cover = new Map<string, number>();
  const seen = new Map<string, number>();
  const cell = pitch * scale;
  for (let py = 0; py < off.height; py += 1) {
    for (let px = 0; px < off.width; px += 1) {
      const tx = Math.floor(px / cell) * pitch;
      const ty = Math.floor(py / cell) * pitch;
      const key = `${tx},${ty}`;
      seen.set(key, (seen.get(key) ?? 0) + 1);
      if ((img.data[(py * off.width + px) * 4 + 3] ?? 0) > 80) {
        cover.set(key, (cover.get(key) ?? 0) + 1);
      }
    }
  }
  const cells: [number, number][] = [];
  let used = 0;
  for (const [key, ink] of cover) {
    if (ink / (seen.get(key) ?? 1) < 0.28) {
      continue;
    }
    const [tx, ty] = key.split(",").map(Number);
    cells.push([(tx ?? 0) - pad, (ty ?? 0) - pad]);
    used = Math.max(used, (tx ?? 0) - pad + pitch);
  }
  return { cells, width: used };
}

export function beadText(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  fontPx: number,
  color: string,
  align: "left" | "center" | "right",
  pitch = BEAD,
): number {
  const { cells, width } = rasterBeadCells(text, fontPx, pitch);
  const originX =
    align === "center" ? x - width / 2 : align === "right" ? x - width : x;
  for (const [tx, ty] of cells) {
    drawBead(ctx, originX + tx, y + ty, color, true, pitch);
  }
  return width;
}

const DIGITS: Record<string, string[]> = {
  "0": [".111.", "1...1", "1...1", "1...1", "1...1", "1...1", ".111."],
  "1": ["..1..", ".11..", "..1..", "..1..", "..1..", "..1..", ".111."],
  "2": [".111.", "1...1", "....1", "..11.", ".1...", "1....", "11111"],
  "3": [".111.", "1...1", "....1", ".111.", "....1", "1...1", ".111."],
  "4": ["1...1", "1...1", "1...1", "11111", "....1", "....1", "....1"],
  "5": ["11111", "1....", "1....", "1111.", "....1", "1...1", ".111."],
  "6": [".111.", "1....", "1....", "1111.", "1...1", "1...1", ".111."],
  "7": ["11111", "....1", "...1.", "..1..", ".1...", ".1...", ".1..."],
  "8": [".111.", "1...1", "1...1", ".111.", "1...1", "1...1", ".111."],
  "9": [".111.", "1...1", "1...1", ".1111", "....1", "....1", ".111."],
  ".": [".", ".", ".", ".", ".", "1", "1"],
  $: ["..1..", ".1111", "1.1..", ".111.", "..1.1", "1111.", "..1.."],
  M: ["1...1", "11.11", "1.1.1", "1.1.1", "1...1", "1...1", "1...1"],
};

function digitWidth(text: string, pitch: number): number {
  let width = 0;
  for (const ch of [...text]) {
    const glyph = DIGITS[ch];
    width += glyph ? (glyph[0].length + 1) * pitch : pitch * 2;
  }
  return Math.max(0, width - pitch);
}

export function drawDigitString(
  ctx: CanvasRenderingContext2D,
  text: string,
  centerX: number,
  centerY: number,
  color: string,
  pitch = 8,
): void {
  let x = centerX - digitWidth(text, pitch) / 2;
  const y = centerY - (7 * pitch) / 2;
  for (const ch of [...text]) {
    const glyph = DIGITS[ch];
    if (!glyph) {
      x += pitch * 2;
      continue;
    }
    for (const [r, row] of glyph.entries()) {
      for (let c = 0; c < row.length; c += 1) {
        if (row[c] === "1") {
          drawBead(ctx, x + c * pitch, y + r * pitch, color, true, pitch);
        }
      }
    }
    x += (glyph[0].length + 1) * pitch;
  }
}

export function fillLabel(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  fontPx: number,
  color: string,
  align: CanvasTextAlign = "left",
): void {
  ctx.font = `900 ${fontPx}px ${FACE}`;
  ctx.fillStyle = color;
  ctx.textAlign = align;
  ctx.textBaseline = "middle";
  ctx.fillText(text, x, y);
}

export function pill(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  color: string,
): void {
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, h / 2);
  ctx.fill();
}
