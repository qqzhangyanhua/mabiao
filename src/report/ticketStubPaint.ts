import type { PosterViewModel } from "./posterTypes";
import {
  CONTENT_W,
  PAD_X,
  TICKET_CHART_H,
  TICKET_SCALE,
  TICKET_WIDTH,
  layoutTicketStubPoster,
  ticketFont,
  type TextMeasure,
  type TicketStubLayout,
} from "./ticketStubLayout";

export {
  layoutTicketStubPoster,
  ticketFont,
  ticketSerial,
  wrapText,
  type TextMeasure,
  type TicketStubLayout,
} from "./ticketStubLayout";

const PAPER = "#f3ead8";
const INK = "#2a2622";
const EDGE = "#2c241c";
const RULE = "#5c5348";
const STAMP = "#c45c4a";

function addGrain(ctx: CanvasRenderingContext2D, width: number, height: number): void {
  const pixels = ctx.getImageData(0, 0, width, height);
  let seed = 20260906;
  for (let i = 0; i < pixels.data.length; i += 4) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const n = (seed % 7) - 3;
    pixels.data[i] = pixels.data[i] + n;
    pixels.data[i + 1] = pixels.data[i + 1] + n - 1;
    pixels.data[i + 2] = pixels.data[i + 2] + n - 2;
  }
  ctx.putImageData(pixels, 0, 0);
}

function dashRule(ctx: CanvasRenderingContext2D, y: number): void {
  ctx.save();
  ctx.strokeStyle = RULE;
  ctx.lineWidth = 1;
  ctx.setLineDash([3, 4]);
  ctx.beginPath();
  ctx.moveTo(PAD_X, y);
  ctx.lineTo(PAD_X + CONTENT_W, y);
  ctx.stroke();
  ctx.restore();
}

function punchHoles(ctx: CanvasRenderingContext2D, height: number): void {
  ctx.fillStyle = EDGE;
  const r = 5;
  const step = 20;
  for (let y = 26; y < height - 22; y += step) {
    ctx.beginPath();
    ctx.arc(15, y, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.arc(TICKET_WIDTH - 15, y, r, 0, Math.PI * 2);
    ctx.fill();
  }
}

function drawStamp(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate((-18 * Math.PI) / 180);
  ctx.globalAlpha = 0.78;
  ctx.strokeStyle = STAMP;
  ctx.lineWidth = 2.4;
  ctx.beginPath();
  ctx.arc(0, 0, 44, 0, Math.PI * 2);
  ctx.stroke();
  ctx.lineWidth = 1.4;
  ctx.beginPath();
  ctx.arc(0, 0, 37, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = STAMP;
  ctx.font = ticketFont(700, 16);
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText("已核对", 0, 1);
  ctx.restore();
}

function drawLineChart(ctx: CanvasRenderingContext2D, data: PosterViewModel, y: number): void {
  ctx.fillStyle = INK;
  ctx.font = ticketFont(600, 13);
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  ctx.fillText("按天节奏", PAD_X, y);
  const plotY = y + 22;
  const plotH = TICKET_CHART_H - 40;
  const plotW = CONTENT_W;
  const max = Math.max(1, ...data.days.map((day) => day.tokens));
  const n = data.days.length;
  const step = n > 1 ? plotW / (n - 1) : plotW;
  ctx.strokeStyle = INK;
  ctx.lineWidth = 1.6;
  ctx.beginPath();
  for (const [index, day] of data.days.entries()) {
    const x = PAD_X + index * step;
    const py = plotY + plotH - (day.tokens / max) * plotH;
    if (index === 0) {
      ctx.moveTo(x, py);
    } else {
      ctx.lineTo(x, py);
    }
  }
  ctx.stroke();
  ctx.fillStyle = INK;
  for (const [index, day] of data.days.entries()) {
    const x = PAD_X + index * step;
    const py = plotY + plotH - (day.tokens / max) * plotH;
    ctx.beginPath();
    ctx.arc(x, py, 2.6, 0, Math.PI * 2);
    ctx.fill();
    ctx.font = ticketFont(500, 12);
    ctx.textAlign = "center";
    ctx.fillText(day.label, x, plotY + plotH + 8);
  }
  ctx.textAlign = "left";
}

function drawDotBar(
  ctx: CanvasRenderingContext2D,
  y: number,
  sources: PosterViewModel["sources"],
): void {
  const h = 8;
  let x = PAD_X;
  ctx.fillStyle = INK;
  for (const source of sources) {
    const w = Math.max(2, (CONTENT_W * source.pct) / 100);
    for (let px = x; px < x + w - 1; px += 4) {
      ctx.fillRect(px, y, 2, h);
    }
    x += w;
  }
  ctx.strokeStyle = INK;
  ctx.lineWidth = 1;
  ctx.strokeRect(PAD_X, y, CONTENT_W, h);
}

function drawTicket(ctx: CanvasRenderingContext2D, layout: TicketStubLayout): void {
  ctx.fillStyle = EDGE;
  ctx.fillRect(0, 0, TICKET_WIDTH, layout.height);
  ctx.fillStyle = PAPER;
  ctx.beginPath();
  ctx.roundRect(14, 8, TICKET_WIDTH - 28, layout.height - 16, 8);
  ctx.fill();
}

function drawContent(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: TicketStubLayout,
): void {
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  ctx.fillStyle = INK;
  ctx.textAlign = "left";

  ctx.font = ticketFont(600, 15);
  ctx.fillText(data.kicker, PAD_X, layout.y.kicker);
  if (layout.serial) {
    ctx.font = ticketFont(500, 13);
    ctx.textAlign = "right";
    ctx.fillText(layout.serial, PAD_X + CONTENT_W, layout.y.kicker + 1);
    ctx.textAlign = "left";
  }

  ctx.font = ticketFont(500, 14);
  ctx.fillText(data.rangeLabel, PAD_X, layout.y.date);

  ctx.font = ticketFont(700, layout.numberSize);
  ctx.fillText(data.totalTokensLabel, PAD_X, layout.y.number);

  ctx.font = ticketFont(500, 16);
  ctx.fillText(data.totalUnit, PAD_X, layout.y.unit);
  if (data.totalCostLabel) {
    const unitW = ctx.measureText(data.totalUnit).width;
    ctx.fillRect(PAD_X + unitW + 12, layout.y.unit + 2, 1, 16);
    ctx.fillText(data.totalCostLabel, PAD_X + unitW + 24, layout.y.unit);
  }

  if (layout.comments.length > 0 && layout.y.comments != null) {
    dashRule(ctx, layout.y.comments - 10);
    ctx.font = ticketFont(500, 16);
    for (const line of layout.comments) {
      ctx.fillText(line.text, PAD_X, line.y);
    }
  }

  if (data.days.length > 0 && layout.y.chart != null) {
    dashRule(ctx, layout.y.chart - 10);
    drawLineChart(ctx, data, layout.y.chart);
  }

  if (data.sources.length > 0 && layout.y.sources != null) {
    dashRule(ctx, layout.y.sources - 10);
    ctx.font = ticketFont(600, 13);
    ctx.fillText("来源占比", PAD_X, layout.y.sources);
    drawDotBar(ctx, layout.y.sources + 20, data.sources);
    ctx.font = ticketFont(500, 13);
    for (const line of layout.sourceLines) {
      ctx.fillText(line.text, PAD_X, line.y);
    }
  }

  if (data.stats.length > 0 && layout.y.stats != null) {
    dashRule(ctx, layout.y.stats - 10);
    const labelW = 136;
    for (const [index, stat] of data.stats.entries()) {
      const sy = layout.y.stats + index * 28;
      ctx.font = ticketFont(500, 14);
      ctx.fillText(stat.label, PAD_X, sy);
      ctx.font = ticketFont(600, 14);
      const value = wrapTextFit(ctx, stat.value, CONTENT_W - labelW - 100);
      ctx.fillText(value, PAD_X + labelW, sy);
    }
  }

  dashRule(ctx, layout.y.footer - 8);
  ctx.font = ticketFont(500, 12);
  ctx.fillStyle = RULE;
  ctx.fillText(`${data.totalTokensLabel} TOKEN · 码表周报`, PAD_X, layout.y.footer);
  drawStamp(ctx, TICKET_WIDTH - 108, layout.height - 78);
}

function wrapTextFit(ctx: CanvasRenderingContext2D, text: string, maxW: number): string {
  if (ctx.measureText(text).width <= maxW) {
    return text;
  }
  let cut = text;
  while (cut.length > 1 && ctx.measureText(`${cut}…`).width > maxW) {
    cut = cut.slice(0, -1);
  }
  return `${cut}…`;
}

/** 在 2× 位图上绘票据存根。预览缩到 720px，复制时直接导出画布。 */
export function paintTicketStubPoster(canvas: HTMLCanvasElement, data: PosterViewModel): void {
  const scratch = canvas.getContext("2d");
  if (!scratch) {
    return;
  }
  const measure: TextMeasure = (font, text) => {
    scratch.font = font;
    return scratch.measureText(text).width;
  };
  const layout = layoutTicketStubPoster(data, measure);
  canvas.width = TICKET_WIDTH * TICKET_SCALE;
  canvas.height = layout.height * TICKET_SCALE;
  canvas.style.width = `${TICKET_WIDTH}px`;
  canvas.style.height = `${layout.height}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.setTransform(TICKET_SCALE, 0, 0, TICKET_SCALE, 0, 0);
  drawTicket(ctx, layout);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  addGrain(ctx, canvas.width, canvas.height);
  ctx.setTransform(TICKET_SCALE, 0, 0, TICKET_SCALE, 0, 0);
  punchHoles(ctx, layout.height);
  drawContent(ctx, data, layout);
}
