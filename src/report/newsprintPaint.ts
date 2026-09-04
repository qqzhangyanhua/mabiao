import type { PosterViewModel } from "./posterTypes";
import {
  BODY,
  BOX_H,
  CHART_H,
  CONTENT_W,
  HEADLINE,
  INSET,
  NEWSPRINT_CSS_WIDTH,
  NEWSPRINT_SCALE,
  PAD,
  PIE_R,
  TITLE,
  layoutNewsprintPoster,
  newsprintFont,
  type NewsprintLayout,
  type TextMeasure,
} from "./newsprintLayout";

export {
  layoutNewsprintPoster,
  newsprintFont,
  wrapText,
  type NewsprintLayout,
  type TextMeasure,
} from "./newsprintLayout";

const PAPER = "#e7d6b4";
const INK = "#1c1610";
const EDGE = "#1a1410";
const HATCH = [
  { angle: 45, spacing: 3.2 },
  { angle: -45, spacing: 3.2 },
  { angle: 0, spacing: 3.0 },
  { angle: 90, spacing: 3.0 },
  { angle: 30, spacing: 4.0 },
] as const;

function hatchAt(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  angle: number,
  spacing: number,
): void {
  ctx.save();
  ctx.strokeStyle = INK;
  ctx.lineWidth = 0.85;
  ctx.translate(cx, cy);
  ctx.rotate((angle * Math.PI) / 180);
  for (let y = -420; y <= 420; y += spacing) {
    ctx.beginPath();
    ctx.moveTo(-420, y);
    ctx.lineTo(420, y);
    ctx.stroke();
  }
  ctx.restore();
}

function hatchShape(
  ctx: CanvasRenderingContext2D,
  clip: () => void,
  cx: number,
  cy: number,
  pattern: (typeof HATCH)[number],
): void {
  ctx.save();
  ctx.beginPath();
  clip();
  ctx.clip();
  ctx.beginPath();
  clip();
  ctx.fillStyle = PAPER;
  ctx.fill();
  hatchAt(ctx, cx, cy, pattern.angle, pattern.spacing);
  ctx.restore();
  ctx.save();
  ctx.beginPath();
  clip();
  ctx.strokeStyle = INK;
  ctx.lineWidth = 1.2;
  ctx.stroke();
  ctx.restore();
}

function jaggedPaper(ctx: CanvasRenderingContext2D, height: number): void {
  const x = INSET;
  const y = INSET;
  const w = NEWSPRINT_CSS_WIDTH - INSET * 2;
  const h = height - INSET * 2;
  let seed = 55427;
  const rnd = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };
  const j = () => (rnd() - 0.5) * 5;
  const step = 9;
  ctx.beginPath();
  ctx.moveTo(x + j(), y + j());
  for (let px = x; px <= x + w; px += step) {
    ctx.lineTo(px, y + j());
  }
  for (let py = y; py <= y + h; py += step) {
    ctx.lineTo(x + w + j(), py);
  }
  for (let px = x + w; px >= x; px -= step) {
    ctx.lineTo(px, y + h + j());
  }
  for (let py = y + h; py >= y; py -= step) {
    ctx.lineTo(x + j(), py);
  }
  ctx.closePath();
}

function addGrain(ctx: CanvasRenderingContext2D, width: number, height: number): void {
  const pixels = ctx.getImageData(0, 0, width, height);
  let seed = 20260904;
  for (let i = 0; i < pixels.data.length; i += 4) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const n = (seed % 11) - 5;
    pixels.data[i] = pixels.data[i] + n;
    pixels.data[i + 1] = pixels.data[i + 1] + n - 1;
    pixels.data[i + 2] = pixels.data[i + 2] + n - 2;
  }
  ctx.putImageData(pixels, 0, 0);
}

function drawRule(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  thick: number,
): void {
  ctx.fillStyle = INK;
  ctx.fillRect(x, y, w, thick);
}

function drawMasthead(ctx: CanvasRenderingContext2D, kicker: string, y: number): void {
  ctx.fillStyle = INK;
  ctx.font = newsprintFont(900, TITLE);
  const parts = kicker.split(" · ");
  if (parts.length === 2 && parts[0] && parts[1]) {
    ctx.fillText(parts[0], PAD, y);
    const left = ctx.measureText(parts[0]).width;
    const dotX = PAD + left + 22;
    const dotY = y + TITLE * 0.4;
    ctx.beginPath();
    ctx.arc(dotX, dotY, 8.5, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillText(parts[1], dotX + 20, y);
    return;
  }
  ctx.fillText(kicker, PAD, y);
}

function drawBars(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: NewsprintLayout,
  x: number,
  y: number,
  w: number,
  h: number,
): void {
  const axis = 28;
  const plotX = x + axis;
  const plotW = w - axis;
  const plotH = h - 22;
  ctx.fillStyle = INK;
  ctx.font = newsprintFont(600, 13);
  ctx.fillText("按天节奏", x, y - 22);
  ctx.font = newsprintFont(500, 11);
  ctx.textAlign = "right";
  for (const tick of [0, 25, 50, 75, 100]) {
    const ty = y + plotH - (tick / 100) * plotH;
    ctx.fillText(String(tick), plotX - 6, ty - 5);
  }
  ctx.textAlign = "left";
  const n = data.days.length;
  const gap = 12;
  const barW = Math.min(30, (plotW - gap * Math.max(n - 1, 0)) / Math.max(n, 1));
  const pattern = HATCH[0];
  for (const [index, day] of data.days.entries()) {
    const bh = (day.tokens / layout.barMax) * plotH;
    const bx = plotX + index * (barW + gap);
    const by = y + plotH - bh;
    hatchShape(
      ctx,
      () => ctx.rect(bx, by, barW, Math.max(bh, 0.5)),
      bx + barW / 2,
      by + bh / 2,
      pattern,
    );
    ctx.fillStyle = INK;
    ctx.font = newsprintFont(600, 13);
    ctx.textAlign = "center";
    ctx.fillText(day.label, bx + barW / 2, y + plotH + 6);
    ctx.textAlign = "left";
  }
}

function drawPie(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  x: number,
  y: number,
  w: number,
): void {
  ctx.fillStyle = INK;
  ctx.font = newsprintFont(600, 13);
  ctx.fillText("来源占比", x, y - 22);
  const cx = x + Math.min(w / 2, 88);
  const cy = y + PIE_R + 4;
  let angle = -Math.PI / 2;
  for (const [index, source] of data.sources.entries()) {
    const slice = (source.pct / 100) * Math.PI * 2;
    const start = angle;
    const end = angle + slice;
    const pattern = HATCH[index % HATCH.length] ?? HATCH[0];
    ctx.save();
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.arc(cx, cy, PIE_R, start, end);
    ctx.closePath();
    ctx.fillStyle = PAPER;
    ctx.fill();
    ctx.clip();
    hatchAt(ctx, cx, cy, pattern.angle, pattern.spacing);
    ctx.restore();
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.arc(cx, cy, PIE_R, start, end);
    ctx.closePath();
    ctx.strokeStyle = INK;
    ctx.lineWidth = 1.2;
    ctx.stroke();
    if (source.pct >= 35) {
      const mid = start + slice / 2;
      ctx.fillStyle = INK;
      ctx.font = newsprintFont(700, 12);
      ctx.textAlign = "center";
      ctx.fillText(
        `${source.pct}%`,
        cx + Math.cos(mid) * PIE_R * 0.5,
        cy + Math.sin(mid) * PIE_R * 0.5 - 6,
      );
      ctx.textAlign = "left";
    }
    angle = end;
  }
  ctx.beginPath();
  ctx.arc(cx, cy, PIE_R, 0, Math.PI * 2);
  ctx.strokeStyle = INK;
  ctx.lineWidth = 1.3;
  ctx.stroke();
  let legendY = cy + PIE_R + 16;
  ctx.font = newsprintFont(500, 12);
  for (const source of data.sources) {
    ctx.fillStyle = INK;
    ctx.fillText(source.label, x, legendY);
    ctx.textAlign = "right";
    ctx.fillText(`${source.pct}%`, x + w - 4, legendY);
    ctx.textAlign = "left";
    legendY += 16;
  }
}

function drawPaper(ctx: CanvasRenderingContext2D, layout: NewsprintLayout): void {
  ctx.fillStyle = EDGE;
  ctx.fillRect(0, 0, NEWSPRINT_CSS_WIDTH, layout.height);
  jaggedPaper(ctx, layout.height);
  ctx.fillStyle = PAPER;
  ctx.fill();
}

function drawContent(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: NewsprintLayout,
): void {
  ctx.save();
  jaggedPaper(ctx, layout.height);
  ctx.clip();
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  ctx.fillStyle = INK;

  drawMasthead(ctx, data.kicker, layout.y.title);
  drawRule(ctx, PAD, layout.y.titleRule, CONTENT_W, 2.2);

  ctx.font = newsprintFont(500, 15);
  ctx.textAlign = "center";
  ctx.fillText(data.rangeLabel, PAD + CONTENT_W / 2, layout.y.date);
  ctx.textAlign = "left";

  ctx.lineWidth = 1.4;
  ctx.strokeStyle = INK;
  ctx.strokeRect(PAD, layout.y.box, CONTENT_W, BOX_H);
  ctx.font = newsprintFont(700, 16);
  ctx.fillText(`No. ${data.totalTokensLabel}`, PAD + 12, layout.y.box + 11);
  const boxRight =
    data.totalCostLabel != null ? `${data.totalUnit} ${data.totalCostLabel}` : data.totalUnit;
  ctx.textAlign = "right";
  ctx.fillText(boxRight, PAD + CONTENT_W - 12, layout.y.box + 11);
  ctx.textAlign = "left";

  ctx.font = newsprintFont(900, HEADLINE);
  for (const [index, line] of layout.headlineLines.entries()) {
    ctx.fillText(line, PAD, layout.y.headline + index * 40);
  }

  ctx.font = newsprintFont(500, BODY);
  for (const line of layout.body) {
    ctx.fillText(line.text, PAD, line.y);
  }

  if (layout.y.charts != null) {
    drawRule(ctx, PAD, layout.y.charts, CONTENT_W, 1.4);
    const split = data.days.length > 0 && data.sources.length > 0;
    const chartTop = layout.y.charts + 28;
    if (data.days.length > 0) {
      const barW = split ? CONTENT_W * 0.58 : CONTENT_W;
      drawBars(ctx, data, layout, PAD, chartTop, barW, CHART_H - 24);
    }
    if (data.sources.length > 0) {
      const pieX = split ? PAD + CONTENT_W * 0.62 : PAD;
      const pieW = split ? CONTENT_W * 0.38 : CONTENT_W;
      if (split) {
        ctx.beginPath();
        ctx.moveTo(PAD + CONTENT_W * 0.6, layout.y.charts);
        ctx.lineTo(PAD + CONTENT_W * 0.6, layout.y.charts + CHART_H + 20);
        ctx.strokeStyle = INK;
        ctx.lineWidth = 1.3;
        ctx.stroke();
      }
      drawPie(ctx, data, pieX, chartTop, pieW);
    }
  }

  if (layout.y.stats != null && layout.y.statsBottom != null && layout.statCols.length > 0) {
    drawRule(ctx, PAD, layout.y.stats, CONTENT_W, 1.4);
    drawRule(ctx, PAD, layout.y.statsBottom, CONTENT_W, 1.4);
    for (const [index, col] of layout.statCols.entries()) {
      if (index > 0) {
        ctx.beginPath();
        ctx.moveTo(col.x, layout.y.stats);
        ctx.lineTo(col.x, layout.y.statsBottom);
        ctx.strokeStyle = INK;
        ctx.lineWidth = 1.3;
        ctx.stroke();
      }
      ctx.fillStyle = INK;
      ctx.font = newsprintFont(600, 13);
      ctx.fillText(col.label, col.x + 10, layout.y.stats + 10);
      ctx.font = newsprintFont(800, 18);
      for (const [lineIndex, line] of col.lines.entries()) {
        ctx.fillText(line, col.x + 10, layout.y.stats + 30 + lineIndex * 20);
      }
    }
  }

  ctx.font = newsprintFont(500, 11);
  ctx.fillStyle = INK;
  ctx.fillText(`${data.totalTokensLabel} · 码表周报 · 每周一期 · 数据为准`, PAD, layout.y.footer);
  ctx.restore();
}

/** 在 2× 位图上绘旧报号外。预览缩到 720px，复制时直接导出画布。 */
export function paintNewsprintPoster(canvas: HTMLCanvasElement, data: PosterViewModel): void {
  const scratch = canvas.getContext("2d");
  if (!scratch) {
    return;
  }
  const measure: TextMeasure = (font, text) => {
    scratch.font = font;
    return scratch.measureText(text).width;
  };
  const layout = layoutNewsprintPoster(data, measure);
  canvas.width = NEWSPRINT_CSS_WIDTH * NEWSPRINT_SCALE;
  canvas.height = layout.height * NEWSPRINT_SCALE;
  canvas.style.width = `${NEWSPRINT_CSS_WIDTH}px`;
  canvas.style.height = `${layout.height}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.setTransform(NEWSPRINT_SCALE, 0, 0, NEWSPRINT_SCALE, 0, 0);
  drawPaper(ctx, layout);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  addGrain(ctx, canvas.width, canvas.height);
  ctx.setTransform(NEWSPRINT_SCALE, 0, 0, NEWSPRINT_SCALE, 0, 0);
  drawContent(ctx, data, layout);
}
