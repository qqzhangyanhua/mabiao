import type { PosterViewModel } from "./posterTypes";
import { framePosterLayout, sizePosterCanvas } from "./posterFrame";
import {
  CONTENT_W,
  INK_WASH_SCALE,
  INK_WASH_WIDTH,
  PAD_X,
  inkKai,
  inkXing,
  layoutInkWashPoster,
  type InkWashLayout,
  type TextMeasure,
} from "./inkWashLayout";

export {
  layoutInkWashPoster,
  inkKai,
  wrapText,
  type InkWashLayout,
  type TextMeasure,
} from "./inkWashLayout";

const PAPER = "#f4efe6";
const INK = "#1a1816";
const SEAL = "#9c3b32";

function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

function addPaper(ctx: CanvasRenderingContext2D, height: number): void {
  ctx.fillStyle = PAPER;
  ctx.fillRect(0, 0, INK_WASH_WIDTH, height);
  const rnd = rng(88_421);
  ctx.strokeStyle = "rgba(90,70,50,0.045)";
  ctx.lineWidth = 0.8;
  for (let i = 0; i < 36; i += 1) {
    const x = rnd() * INK_WASH_WIDTH;
    const y = rnd() * height;
    ctx.beginPath();
    ctx.moveTo(x, y);
    ctx.lineTo(x + 30 + rnd() * 90, y + (rnd() - 0.5) * 8);
    ctx.stroke();
  }
  ctx.fillStyle = "rgba(50,40,30,0.07)";
  for (let i = 0; i < 90; i += 1) {
    ctx.beginPath();
    ctx.arc(rnd() * INK_WASH_WIDTH, rnd() * height, rnd() * 1.4, 0, Math.PI * 2);
    ctx.fill();
  }
}

function addGrain(ctx: CanvasRenderingContext2D, width: number, height: number): void {
  const pixels = ctx.getImageData(0, 0, width, height);
  let seed = 20260905;
  for (let i = 0; i < pixels.data.length; i += 4) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const n = (seed % 7) - 3;
    pixels.data[i] = pixels.data[i] + n;
    pixels.data[i + 1] = pixels.data[i + 1] + n;
    pixels.data[i + 2] = pixels.data[i + 2] + n - 1;
  }
  ctx.putImageData(pixels, 0, 0);
}

function drawSplash(ctx: CanvasRenderingContext2D): void {
  const rnd = rng(3341);
  ctx.save();
  ctx.fillStyle = INK;
  ctx.shadowColor = "rgba(26,24,22,0.35)";
  ctx.shadowBlur = 16;
  for (let i = 0; i < 9; i += 1) {
    ctx.globalAlpha = 0.06 + rnd() * 0.12;
    ctx.beginPath();
    ctx.ellipse(
      600 + rnd() * 80,
      28 + rnd() * 64,
      22 + rnd() * 52,
      14 + rnd() * 38,
      (rnd() - 0.5) * 1.6,
      0,
      Math.PI * 2,
    );
    ctx.fill();
  }
  ctx.shadowBlur = 0;
  ctx.globalAlpha = 0.16;
  for (let i = 0; i < 22; i += 1) {
    ctx.beginPath();
    ctx.arc(610 + rnd() * 95, 18 + rnd() * 88, 0.6 + rnd() * 2.2, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

function dryBrush(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  seed: number,
): void {
  const rnd = rng(seed);
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.globalCompositeOperation = "destination-out";
  for (let i = 0; i < 48; i += 1) {
    ctx.lineWidth = 0.5 + rnd() * 1.6;
    ctx.strokeStyle = `rgba(0,0,0,${0.28 + rnd() * 0.45})`;
    const px = x + rnd() * w;
    const py = y + rnd() * h;
    const len = 7 + rnd() * 16;
    const ang = (rnd() - 0.5) * 0.9;
    ctx.beginPath();
    ctx.moveTo(px, py);
    ctx.lineTo(px + Math.cos(ang) * len, py + Math.sin(ang) * len);
    ctx.stroke();
  }
  ctx.restore();
}

function drawBrushBar(
  ctx: CanvasRenderingContext2D,
  cx: number,
  top: number,
  bottom: number,
  maxW: number,
  seed: number,
): void {
  const rnd = rng(seed);
  const h = bottom - top;
  ctx.fillStyle = "rgba(22,20,18,0.94)";
  if (h < 16) {
    ctx.beginPath();
    ctx.ellipse(
      cx + (rnd() - 0.5) * 3,
      bottom - Math.max(h, 8) / 2,
      5 + rnd() * 4,
      Math.max(h, 8) * 0.55,
      (rnd() - 0.5) * 0.5,
      0,
      Math.PI * 2,
    );
    ctx.fill();
    return;
  }
  const lean = (rnd() - 0.5) * 5;
  const topW = maxW * (0.72 + rnd() * 0.18);
  const midW = maxW * (0.92 + rnd() * 0.1);
  const botW = maxW * (0.78 + rnd() * 0.16);
  const midY = top + h * (0.45 + rnd() * 0.12);
  ctx.beginPath();
  ctx.moveTo(cx - topW / 2 + lean, top + 3);
  ctx.quadraticCurveTo(cx - midW / 2 + lean * 0.4, midY, cx - botW / 2, bottom);
  ctx.lineTo(cx + botW / 2, bottom);
  ctx.quadraticCurveTo(cx + midW / 2 + lean * 0.4, midY, cx + topW / 2 + lean, top + 3);
  ctx.closePath();
  ctx.fill();
  ctx.beginPath();
  ctx.ellipse(cx + lean, top + 4, topW / 2, 6, 0, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  ctx.ellipse(cx, bottom - 3, botW / 2, 5, 0, 0, Math.PI * 2);
  ctx.fill();
  dryBrush(ctx, cx - maxW, top - 4, maxW * 2, h + 12, seed + 9);
}

function drawBrushNumber(
  ctx: CanvasRenderingContext2D,
  text: string,
  cy: number,
  size: number,
): void {
  const chars = [...text];
  ctx.font = inkXing(700, size);
  const widths = chars.map((ch) => ctx.measureText(ch).width);
  const tracking = -size * 0.04;
  const total = widths.reduce((sum, w) => sum + w, 0) + tracking * Math.max(chars.length - 1, 0);
  let x = (INK_WASH_WIDTH - total) / 2;
  const rnd = rng(text.length * 97 + size);
  ctx.fillStyle = INK;
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  for (const [index, ch] of chars.entries()) {
    const width = widths[index] ?? 0;
    ctx.save();
    const jx = (rnd() - 0.5) * 2.4;
    const jy = (rnd() - 0.5) * 5;
    const rot = (rnd() - 0.5) * 0.05;
    ctx.translate(x + width / 2 + jx, cy + size * 0.55 + jy);
    ctx.rotate(rot);
    ctx.globalAlpha = 0.22;
    ctx.fillText(ch, -width / 2 + 1.2, -size * 0.55 + 1.4);
    ctx.globalAlpha = 1;
    ctx.fillText(ch, -width / 2, -size * 0.55);
    ctx.restore();
    x += width + tracking;
  }
  dryBrush(ctx, PAD_X, cy - 8, CONTENT_W, size + 24, 4401);
}

function centerText(
  ctx: CanvasRenderingContext2D,
  text: string,
  y: number,
  font: string,
  color: string,
): void {
  ctx.font = font;
  ctx.fillStyle = color;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  ctx.fillText(text, INK_WASH_WIDTH / 2, y);
}

function drawContent(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: InkWashLayout,
): void {
  ctx.textBaseline = "top";
  drawSplash(ctx);
  centerText(ctx, data.kicker, layout.y.kicker, inkKai(600, 22), INK);
  centerText(ctx, data.rangeLabel, layout.y.date, inkKai(500, 15), "rgba(40,36,32,0.72)");
  drawBrushNumber(ctx, data.totalTokensLabel, layout.y.number, layout.numberSize);

  ctx.font = inkKai(600, 22);
  ctx.fillStyle = SEAL;
  ctx.textAlign = "center";
  const unit = data.totalUnit;
  const cost = data.totalCostLabel;
  if (cost) {
    const gap = 36;
    const unitW = ctx.measureText(unit).width;
    const costW = ctx.measureText(cost).width;
    const left = (INK_WASH_WIDTH - unitW - gap - costW) / 2;
    ctx.textAlign = "left";
    ctx.fillText(unit, left, layout.y.unit);
    ctx.fillText(cost, left + unitW + gap, layout.y.unit);
  } else {
    ctx.fillText(unit, INK_WASH_WIDTH / 2, layout.y.unit);
  }

  ctx.fillStyle = INK;
  ctx.font = inkKai(500, 18);
  ctx.textAlign = "center";
  for (const line of layout.comments) {
    ctx.fillText(line.text, INK_WASH_WIDTH / 2, line.y);
  }

  if (data.days.length > 0) {
    const slot = CONTENT_W / data.days.length;
    const maxW = Math.min(38, slot * 0.55);
    const max = Math.max(1, ...data.days.map((day) => day.tokens));
    for (const [index, day] of data.days.entries()) {
      const cx = PAD_X + slot * index + slot / 2;
      const h = (day.tokens / max) * layout.barH;
      const top = layout.y.bars + layout.barH - h;
      drawBrushBar(ctx, cx, top, layout.y.bars + layout.barH, maxW, 1200 + index * 17);
      ctx.textAlign = "center";
      ctx.fillStyle = INK;
      ctx.font = inkKai(500, 14);
      ctx.fillText(day.label, cx, layout.y.barLabels);
    }
  }

  ctx.fillStyle = "rgba(40,36,32,0.82)";
  ctx.font = inkKai(500, 14);
  ctx.textAlign = "center";
  for (const line of layout.sourceLines) {
    ctx.fillText(line.text, INK_WASH_WIDTH / 2, line.y);
  }
  for (const line of layout.statLines) {
    ctx.fillText(line.text, INK_WASH_WIDTH / 2, line.y);
  }
}

/** 在 2× 位图上绘水墨手札。预览缩到 720px，复制时直接导出画布。 */
export function paintInkWashPoster(canvas: HTMLCanvasElement, data: PosterViewModel): void {
  const scratch = canvas.getContext("2d");
  if (!scratch) {
    return;
  }
  const measure: TextMeasure = (font, text) => {
    scratch.font = font;
    return scratch.measureText(text).width;
  };
  const layout = framePosterLayout(layoutInkWashPoster(data, measure));
  sizePosterCanvas(canvas, INK_WASH_SCALE);
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.setTransform(INK_WASH_SCALE, 0, 0, INK_WASH_SCALE, 0, 0);
  addPaper(ctx, layout.height);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  addGrain(ctx, canvas.width, canvas.height);
  ctx.setTransform(INK_WASH_SCALE, 0, 0, INK_WASH_SCALE, 0, 0);
  drawContent(ctx, data, layout);
}
