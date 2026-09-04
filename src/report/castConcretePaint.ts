import type { PosterViewModel } from "./posterTypes";
import { framePosterLayout, sizePosterCanvas } from "./posterFrame";
import {
  CONCRETE_SCALE,
  CONCRETE_W,
  CONTENT_W,
  PAD_X,
  concreteFont,
  layoutCastConcretePoster,
  type CastConcreteLayout,
  type TextMeasure,
} from "./castConcreteLayout";

export {
  layoutCastConcretePoster,
  concreteFont,
  wrapText,
  type CastConcreteLayout,
  type TextMeasure,
} from "./castConcreteLayout";

const WALL = "#b6b5af";

function hash2(x: number, y: number, seed: number): number {
  let n = Math.imul(x, 374761393) ^ Math.imul(y, 668265263) ^ seed;
  n = Math.imul(n ^ (n >>> 13), 1274126177);
  return (n >>> 0) / 4294967296;
}

function valueNoise(x: number, y: number, scale: number, seed: number): number {
  const gx = x / scale;
  const gy = y / scale;
  const x0 = Math.floor(gx);
  const y0 = Math.floor(gy);
  const tx = gx - x0;
  const ty = gy - y0;
  const sx = tx * tx * (3 - 2 * tx);
  const sy = ty * ty * (3 - 2 * ty);
  const v00 = hash2(x0, y0, seed);
  const v10 = hash2(x0 + 1, y0, seed);
  const v01 = hash2(x0, y0 + 1, seed);
  const v11 = hash2(x0 + 1, y0 + 1, seed);
  return v00 + (v10 - v00) * sx + (v01 + (v11 - v01) * sx - (v00 + (v10 - v00) * sx)) * sy;
}

function splat(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  cx: number,
  cy: number,
  radius: number,
  dr: number,
  dg: number,
  db: number,
): void {
  const r = Math.ceil(radius);
  const r2 = radius * radius;
  for (let dy = -r; dy <= r; dy += 1) {
    for (let dx = -r; dx <= r; dx += 1) {
      const d2 = dx * dx + dy * dy;
      if (d2 > r2) {
        continue;
      }
      const x = cx + dx;
      const y = cy + dy;
      if (x < 0 || y < 0 || x >= width || y >= height) {
        continue;
      }
      const falloff = 1 - Math.sqrt(d2) / radius;
      const i = (y * width + x) * 4;
      data[i] += dr * falloff;
      data[i + 1] += dg * falloff;
      data[i + 2] += db * falloff;
    }
  }
}

function addTexture(ctx: CanvasRenderingContext2D, width: number, height: number): void {
  const pixels = ctx.getImageData(0, 0, width, height);
  const data = pixels.data;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const i = (y * width + x) * 4;
      const mottling =
        (valueNoise(x, y, 140, 11) - 0.5) * 14 +
        (valueNoise(x, y, 56, 23) - 0.5) * 8 +
        (valueNoise(x, y, 16, 41) - 0.5) * 5;
      const board = (valueNoise(x * 3.6, y * 0.38, 26, 55) - 0.5) * 6;
      const grain = (hash2(x, y, 91) - 0.5) * 11;
      const cool = (valueNoise(x, y, 88, 77) - 0.5) * 3;
      const n = mottling + board + grain;
      data[i] = data[i] + n + cool * 0.2;
      data[i + 1] = data[i + 1] + n + cool * 0.1;
      data[i + 2] = data[i + 2] + n + cool * 0.35;
      if (hash2(x, y, 201) > 0.994) {
        const pit = -16 - hash2(x, y, 202) * 14;
        data[i] += pit;
        data[i + 1] += pit;
        data[i + 2] += pit;
      }
    }
  }
  for (let p = 0; p < 380; p += 1) {
    const px = Math.floor(hash2(p, 1, 5) * width);
    const py = Math.floor(hash2(p, 2, 5) * height);
    const radius = 0.8 + hash2(p, 3, 5) * 2.2;
    const dark = hash2(p, 4, 5) < 0.68;
    const amp = dark ? -20 - hash2(p, 6, 5) * 12 : 12 + hash2(p, 6, 5) * 8;
    splat(data, width, height, px, py, radius, amp, amp + (dark ? -2 : 1), amp + (dark ? 1 : -1));
  }
  ctx.putImageData(pixels, 0, 0);
}

function smooth01(t: number): number {
  const x = t < 0 ? 0 : t > 1 ? 1 : t;
  return x * x * (3 - 2 * x);
}

function addSun(ctx: CanvasRenderingContext2D, width: number, height: number): void {
  const pixels = ctx.getImageData(0, 0, width, height);
  const data = pixels.data;
  for (let y = 0; y < height; y += 1) {
    const edge = width * (0.42 - (y / height) * 0.3);
    for (let x = 0; x < width; x += 1) {
      const i = (y * width + x) * 4;
      const t = smooth01((x - edge + 3) / 8);
      const mul = 1.16 - t * 0.54;
      const warm = (1 - t) * 4;
      data[i] = data[i] * mul + warm;
      data[i + 1] = data[i + 1] * mul + warm * 0.35;
      data[i + 2] = data[i + 2] * mul - (1 - t) * 2;
    }
  }
  ctx.putImageData(pixels, 0, 0);
}

function formTie(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  ctx.save();
  const dish = ctx.createRadialGradient(x, y, 2, x, y, 13);
  dish.addColorStop(0, "rgba(10,9,8,0.5)");
  dish.addColorStop(0.5, "rgba(10,9,8,0.12)");
  dish.addColorStop(1, "rgba(10,9,8,0)");
  ctx.fillStyle = dish;
  ctx.beginPath();
  ctx.arc(x, y, 13, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  ctx.arc(x, y, 4.2, 0, Math.PI * 2);
  ctx.fillStyle = "#141312";
  ctx.fill();
  ctx.beginPath();
  ctx.arc(x - 0.4, y - 0.5, 3.3, 0, Math.PI * 2);
  ctx.fillStyle = "#070706";
  ctx.fill();
  ctx.beginPath();
  ctx.arc(x + 1.1, y + 1.4, 3.6, 0.2, 1.6);
  ctx.strokeStyle = "rgba(230,226,218,0.18)";
  ctx.lineWidth = 0.9;
  ctx.stroke();
  ctx.restore();
}

function placeTies(ctx: CanvasRenderingContext2D, height: number): void {
  for (let y = 78; y < height - 40; y += 172) {
    formTie(ctx, 20, y);
    formTie(ctx, CONCRETE_W - 20, y);
  }
}

function fontPx(font: string): number {
  const match = /(\d+)px/.exec(font);
  return match ? Number(match[1]) : 16;
}

function rimGlyph(
  off: HTMLCanvasElement,
  text: string,
  font: string,
  pad: number,
  dx: number,
  dy: number,
  color: string,
): void {
  const o = off.getContext("2d");
  if (!o) {
    return;
  }
  o.setTransform(1, 0, 0, 1, 0, 0);
  o.globalCompositeOperation = "source-over";
  o.clearRect(0, 0, off.width, off.height);
  o.setTransform(CONCRETE_SCALE, 0, 0, CONCRETE_SCALE, 0, 0);
  o.font = font;
  o.textBaseline = "top";
  o.textAlign = "left";
  o.fillStyle = "#000";
  o.fillText(text, pad, pad);
  o.globalCompositeOperation = "destination-out";
  o.fillText(text, pad + dx, pad + dy);
  o.setTransform(1, 0, 0, 1, 0, 0);
  o.globalCompositeOperation = "source-in";
  o.fillStyle = color;
  o.fillRect(0, 0, off.width, off.height);
  o.globalCompositeOperation = "source-over";
}

function carveText(
  ctx: CanvasRenderingContext2D,
  scratch: HTMLCanvasElement,
  text: string,
  x: number,
  y: number,
  font: string,
): void {
  ctx.font = font;
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  const px = fontPx(font);
  const depth = Math.max(1.35, px * 0.048);
  const pad = Math.ceil(depth) + 4;
  const w = Math.ceil(ctx.measureText(text).width) + pad * 2;
  const h = Math.ceil(px * 1.35) + pad * 2;
  scratch.width = Math.max(1, Math.ceil(w * CONCRETE_SCALE));
  scratch.height = Math.max(1, Math.ceil(h * CONCRETE_SCALE));

  ctx.fillStyle = px >= 40 ? "rgba(32,30,28,0.34)" : "rgba(28,26,24,0.46)";
  ctx.fillText(text, x, y);

  rimGlyph(scratch, text, font, pad, depth, depth, "rgba(8,7,6,0.55)");
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(scratch, (x - pad) * CONCRETE_SCALE, (y - pad) * CONCRETE_SCALE);
  ctx.restore();

  rimGlyph(scratch, text, font, pad, -depth, -depth, "rgba(255,250,240,0.28)");
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(scratch, (x - pad) * CONCRETE_SCALE, (y - pad) * CONCRETE_SCALE);
  ctx.restore();
}

function carveRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number): void {
  if (w < 3 || h < 3) {
    return;
  }
  const d = Math.min(2.6, w * 0.2, h * 0.24);
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 1.5);
  ctx.fillStyle = "rgba(18,16,14,0.32)";
  ctx.fill();
  ctx.clip();
  ctx.fillStyle = "rgba(8,7,6,0.5)";
  ctx.fillRect(x, y, w, d);
  ctx.fillRect(x, y, d, h);
  ctx.fillStyle = "rgba(255,250,240,0.22)";
  ctx.fillRect(x, y + h - d, w, d);
  ctx.fillRect(x + w - d, y, d, h);
  ctx.restore();
}

function drawBars(
  ctx: CanvasRenderingContext2D,
  scratch: HTMLCanvasElement,
  data: PosterViewModel,
  y: number,
  barH: number,
): void {
  carveText(ctx, scratch, "按天节奏", PAD_X, y, concreteFont(600, 16));
  const max = Math.max(1, ...data.days.map((day) => day.tokens));
  const n = data.days.length;
  const slot = CONTENT_W / n;
  const barW = Math.min(24, slot * 0.34);
  const top = y + 26;
  const plotH = Math.max(14, barH - 8);
  for (const [index, day] of data.days.entries()) {
    const cx = PAD_X + index * slot + slot / 2;
    ctx.font = concreteFont(600, 16);
    const lw = ctx.measureText(day.label).width;
    carveText(ctx, scratch, day.label, cx - lw / 2, top, concreteFont(600, 16));
    const h = Math.max(14, (day.tokens / max) * plotH);
    carveRect(ctx, cx - barW / 2, top + 24 + (plotH - h), barW, h);
  }
}

function drawContent(
  ctx: CanvasRenderingContext2D,
  scratch: HTMLCanvasElement,
  data: PosterViewModel,
  layout: CastConcreteLayout,
): void {
  carveText(ctx, scratch, data.kicker, PAD_X, layout.y.title, concreteFont(800, 70));
  carveText(ctx, scratch, data.rangeLabel, PAD_X, layout.y.date, concreteFont(500, 18));
  carveRect(ctx, PAD_X, layout.y.rule, 176, 3);
  carveText(ctx, scratch, `Cast-in ${data.totalTokensLabel}`, PAD_X, layout.y.cast, concreteFont(700, 30));
  const unitLine =
    data.totalCostLabel != null ? `${data.totalUnit}  ${data.totalCostLabel}` : data.totalUnit;
  carveText(ctx, scratch, unitLine, PAD_X, layout.y.unit, concreteFont(500, 20));

  if (layout.y.comments != null) {
    for (const line of layout.comments) {
      carveText(ctx, scratch, line.text, PAD_X, line.y, concreteFont(500, 17));
    }
  }
  if (data.days.length > 0 && layout.y.bars != null) {
    drawBars(ctx, scratch, data, layout.y.bars, layout.barH);
  }
  if (layout.sourceLine && layout.y.sources != null) {
    carveText(ctx, scratch, "来源占比", PAD_X, layout.y.sources, concreteFont(600, 16));
    carveRect(ctx, PAD_X, layout.y.sources + 22, 72, 2);
    carveText(ctx, scratch, layout.sourceLine, PAD_X, layout.y.sources + 30, concreteFont(500, 16));
  }
  if (data.stats.length > 0 && layout.y.stats != null) {
    for (const [index, stat] of data.stats.entries()) {
      const sy = layout.y.stats + index * 28;
      carveText(ctx, scratch, stat.label, PAD_X, sy, concreteFont(500, 16));
      carveText(ctx, scratch, stat.value, PAD_X + 148, sy, concreteFont(500, 16));
    }
  }
}

/** 在 2× 位图上绘清水混凝土。预览缩到 720px，复制时直接导出画布。 */
export function paintCastConcretePoster(canvas: HTMLCanvasElement, data: PosterViewModel): void {
  const scratch = canvas.getContext("2d");
  if (!scratch) {
    return;
  }
  const measure: TextMeasure = (font, text) => {
    scratch.font = font;
    return scratch.measureText(text).width;
  };
  const layout = framePosterLayout(layoutCastConcretePoster(data, measure));
  sizePosterCanvas(canvas, CONCRETE_SCALE);
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  const glyph = document.createElement("canvas");
  ctx.setTransform(CONCRETE_SCALE, 0, 0, CONCRETE_SCALE, 0, 0);
  ctx.fillStyle = WALL;
  ctx.fillRect(0, 0, CONCRETE_W, layout.height);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  addTexture(ctx, canvas.width, canvas.height);
  ctx.setTransform(CONCRETE_SCALE, 0, 0, CONCRETE_SCALE, 0, 0);
  placeTies(ctx, layout.height);
  drawContent(ctx, glyph, data, layout);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  addSun(ctx, canvas.width, canvas.height);
}
