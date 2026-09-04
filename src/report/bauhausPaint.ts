import type { PosterViewModel } from "./posterTypes";

export const BAUHAUS_CSS_WIDTH = 720;
export const BAUHAUS_SCALE = 2;

const PAPER = "#f6f1e6";
const INK = "#121212";
const RED = "#e30613";
const YELLOW = "#f0c400";
const BLUE = "#1d4fd7";
const WHITE = "#ffffff";

const DAY_FILLS = [INK, RED, YELLOW, INK, BLUE, INK, RED] as const;
const INSIGHT_DOTS = [INK, RED, YELLOW, BLUE] as const;
const SOURCE_SWATCHES = [RED, YELLOW, BLUE, INK] as const;
const STAT_DOTS = [BLUE, INK, YELLOW] as const;

const PAD_L = 48;
const PAD_R = 36;
const PAD_T = 42;
const PAD_B = 32;
const CONTENT_RIGHT = BAUHAUS_CSS_WIDTH - PAD_R;
const STEM_X = 652;
const CHART_RIGHT = 628;

const FACE =
  '"PingFang SC", "Hiragino Sans GB", "Helvetica Neue", Helvetica, "Noto Sans SC", sans-serif';

export type TextMeasure = (font: string, text: string) => number;

export function bauhausFont(weight: number, px: number): string {
  return `${weight} ${px}px ${FACE}`;
}

function colorAt(palette: readonly string[], index: number): string {
  return palette[index % palette.length] ?? palette[0];
}

function wrapChars(measure: TextMeasure, font: string, text: string, maxWidth: number): string[] {
  const lines: string[] = [];
  let current = "";
  for (const ch of [...text]) {
    const next = current + ch;
    if (current !== "" && measure(font, next) > maxWidth) {
      lines.push(current);
      current = ch;
    } else {
      current = next;
    }
  }
  if (current !== "") {
    lines.push(current);
  }
  return lines;
}

export function wrapText(
  measure: TextMeasure,
  font: string,
  text: string,
  maxWidth: number,
): string[] {
  if (text.length === 0) {
    return [];
  }
  if (maxWidth <= 0) {
    return [text];
  }
  const tokens = text.split(/( · |\/|\s+)/).filter((token) => token.length > 0);
  const lines: string[] = [];
  let current = "";
  const flush = () => {
    if (current !== "") {
      lines.push(current);
      current = "";
    }
  };
  for (const token of tokens) {
    const next = current + token;
    if (current !== "" && measure(font, next) > maxWidth) {
      flush();
    }
    if (current === "" && measure(font, token) > maxWidth) {
      for (const piece of wrapChars(measure, font, token, maxWidth)) {
        lines.push(piece);
      }
      continue;
    }
    current = current + token;
  }
  flush();
  return lines.length > 0 ? lines : [text];
}

export type BauhausStatCell = {
  label: string;
  lines: string[];
  dot: string;
};

export type BauhausLayout = {
  height: number;
  cost: { x: number; y: number; w: number; h: number } | null;
  comments: { y: number; text: string }[];
  barW: number;
  barGap: number;
  barMax: number;
  sourceColW: number;
  statColW: number;
  statRows: { left: BauhausStatCell | null; right: BauhausStatCell | null; height: number }[];
  y: {
    kicker: number;
    range: number;
    total: number;
    unit: number;
    insight: number | null;
    daysTitle: number | null;
    bars: number | null;
    daysRule: number | null;
    sourcesTitle: number | null;
    sources: number | null;
    stats: number | null;
    statsBottom: number | null;
    strips: number;
  };
};

const KICKER = 52;
const RANGE = 17;
const TOTAL = 80;
const UNIT = 24;
const COST = 22;
const BODY = 16;
const LABEL = 15;
const VALUE = 20;
const BAR_H = 156;

export function layoutBauhausPoster(data: PosterViewModel, measure: TextMeasure): BauhausLayout {
  const unitFont = bauhausFont(800, UNIT);
  const costFont = bauhausFont(800, COST);
  const valueFont = bauhausFont(800, VALUE);

  const y = {
    kicker: PAD_T,
    range: PAD_T + KICKER + 8,
    total: PAD_T + KICKER + 8 + RANGE + 28,
    unit: 0,
    insight: null as number | null,
    daysTitle: null as number | null,
    bars: null as number | null,
    daysRule: null as number | null,
    sourcesTitle: null as number | null,
    sources: null as number | null,
    stats: null as number | null,
    statsBottom: null as number | null,
    strips: 0,
  };
  y.unit = y.total + TOTAL + 10;

  const costH = COST + 12;
  const cost =
    data.totalCostLabel == null
      ? null
      : {
          x: PAD_L + measure(unitFont, data.totalUnit) + 14,
          y: y.unit - 2,
          w: measure(costFont, data.totalCostLabel) + 22,
          h: costH,
        };

  let cursor = y.unit + Math.max(UNIT, costH) + 30;
  const comments: { y: number; text: string }[] = [];
  if (data.comments.length > 0) {
    y.insight = cursor;
    cursor += 28;
    for (const text of data.comments) {
      comments.push({ y: cursor, text });
      cursor += 26;
    }
    cursor += 22;
  }

  const barGap = 16;
  const barCount = Math.max(data.days.length, 1);
  const barW =
    data.days.length === 0 ? 0 : (CHART_RIGHT - PAD_L - barGap * (barCount - 1)) / barCount;
  const barMax = Math.max(1, ...data.days.map((day) => day.tokens));

  if (data.days.length > 0) {
    y.daysTitle = cursor;
    cursor += 28;
    y.bars = cursor;
    cursor += BAR_H + 26;
    y.daysRule = cursor;
    cursor += 22;
  }

  const sourceColW = 132;
  if (data.sources.length > 0) {
    y.sourcesTitle = cursor;
    cursor += 26;
    y.sources = cursor;
    cursor += 52;
  }

  const statColW = (CONTENT_RIGHT - PAD_L) / 2;
  const valueWidth = statColW - 32;
  const cells: BauhausStatCell[] = data.stats.map((stat, index) => ({
    label: stat.label,
    lines: wrapText(measure, valueFont, stat.value, valueWidth),
    dot: colorAt(STAT_DOTS, index),
  }));
  const statRows: BauhausLayout["statRows"] = [];
  for (let i = 0; i < cells.length; i += 2) {
    const left = cells[i] ?? null;
    const right = cells[i + 1] ?? null;
    const lineCount = Math.max(left?.lines.length ?? 1, right?.lines.length ?? 1, 1);
    statRows.push({ left, right, height: 22 + 8 + lineCount * 24 });
  }
  if (statRows.length > 0) {
    cursor += 8;
    y.stats = cursor;
    const block = statRows.reduce((sum, row) => sum + row.height, 0) + 16;
    y.statsBottom = cursor + block;
    cursor = y.statsBottom + 16;
  }

  y.strips = cursor;
  return {
    height: cursor + 12 + PAD_B,
    cost,
    comments,
    barW,
    barGap,
    barMax,
    sourceColW,
    statColW,
    statRows,
    y,
  };
}

function fillRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  color: string,
): void {
  ctx.fillStyle = color;
  ctx.fillRect(x, y, w, h);
}

function fillCircle(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  r: number,
  color: string,
): void {
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * 2);
  ctx.fill();
}

function fillTracked(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  tracking: number,
): void {
  let cursor = x;
  for (const ch of [...text]) {
    ctx.fillText(ch, cursor, y);
    cursor += ctx.measureText(ch).width + tracking;
  }
}

function addPaperGrain(ctx: CanvasRenderingContext2D, width: number, height: number): void {
  const pixels = ctx.getImageData(0, 0, width, height);
  let seed = 20260904;
  for (let i = 0; i < pixels.data.length; i += 4) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const n = (seed % 7) - 3;
    pixels.data[i] = pixels.data[i] + n;
    pixels.data[i + 1] = pixels.data[i + 1] + n;
    pixels.data[i + 2] = pixels.data[i + 2] + n;
  }
  ctx.putImageData(pixels, 0, 0);
}

function drawPaper(ctx: CanvasRenderingContext2D, height: number): void {
  fillRect(ctx, 0, 0, BAUHAUS_CSS_WIDTH, height, PAPER);
}

function drawDecor(ctx: CanvasRenderingContext2D, height: number): void {
  fillCircle(ctx, 598, 72, 44, YELLOW);
  fillRect(ctx, 650, 28, 62, 62, RED);
  fillRect(ctx, 554, 124, 90, 16, YELLOW);
  fillRect(ctx, 554, 148, 72, 5, BLUE);

  const stemTop = 278;
  const stemH = Math.min(300, height - stemTop - 220);
  fillRect(ctx, STEM_X, stemTop, 18, Math.max(120, stemH), INK);
  fillCircle(ctx, 698, 360, 10, INK);

  ctx.save();
  ctx.translate(616, 226);
  ctx.rotate((-28 * Math.PI) / 180);
  ctx.fillStyle = BLUE;
  ctx.beginPath();
  ctx.moveTo(0, -80);
  ctx.bezierCurveTo(40, -42, 44, 22, 12, 80);
  ctx.bezierCurveTo(-6, 70, -42, 16, -36, -30);
  ctx.bezierCurveTo(-32, -58, -16, -76, 0, -80);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawBauhausPoster(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: BauhausLayout,
): void {
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  drawDecor(ctx, layout.height);

  ctx.fillStyle = INK;
  ctx.font = bauhausFont(900, KICKER);
  fillTracked(ctx, data.kicker, PAD_L, layout.y.kicker, -1.4);

  ctx.font = bauhausFont(700, RANGE);
  ctx.fillText(data.rangeLabel, PAD_L, layout.y.range);

  ctx.font = bauhausFont(900, TOTAL);
  fillTracked(ctx, data.totalTokensLabel, PAD_L, layout.y.total, -2.6);

  ctx.font = bauhausFont(800, UNIT);
  ctx.fillText(data.totalUnit, PAD_L, layout.y.unit);
  if (layout.cost && data.totalCostLabel) {
    fillRect(ctx, layout.cost.x, layout.cost.y, layout.cost.w, layout.cost.h, RED);
    ctx.fillStyle = WHITE;
    ctx.font = bauhausFont(800, COST);
    ctx.fillText(data.totalCostLabel, layout.cost.x + 11, layout.cost.y + 6);
    ctx.fillStyle = INK;
  }

  if (layout.y.insight != null) {
    fillRect(ctx, PAD_L, layout.y.insight + 7, 16, 5, INK);
    ctx.font = bauhausFont(800, BODY);
    ctx.fillText("Insight:", PAD_L + 24, layout.y.insight);
    ctx.font = bauhausFont(600, BODY);
    for (const [index, comment] of layout.comments.entries()) {
      fillCircle(ctx, PAD_L + 6, comment.y + 8, 6, colorAt(INSIGHT_DOTS, index));
      ctx.fillStyle = INK;
      ctx.fillText(comment.text, PAD_L + 22, comment.y);
    }
  }

  if (layout.y.daysTitle != null && layout.y.bars != null && data.days.length > 0) {
    fillRect(ctx, PAD_L, layout.y.daysTitle + 2, 14, 14, BLUE);
    ctx.font = bauhausFont(800, LABEL);
    ctx.fillText("按天节奏", PAD_L + 22, layout.y.daysTitle);
    for (const [index, day] of data.days.entries()) {
      const h = (day.tokens / layout.barMax) * BAR_H;
      const x = PAD_L + index * (layout.barW + layout.barGap);
      const y = layout.y.bars + BAR_H - h;
      fillRect(ctx, x, y, layout.barW, Math.max(h, 0), colorAt(DAY_FILLS, index));
      ctx.fillStyle = INK;
      ctx.font = bauhausFont(700, 15);
      ctx.textAlign = "center";
      ctx.fillText(day.label, x + layout.barW / 2, layout.y.bars + BAR_H + 8);
      ctx.textAlign = "left";
    }
    if (layout.y.daysRule != null) {
      fillRect(ctx, PAD_L, layout.y.daysRule, CONTENT_RIGHT - PAD_L, 2, BLUE);
    }
  }

  if (layout.y.sourcesTitle != null && layout.y.sources != null && data.sources.length > 0) {
    fillRect(ctx, PAD_L, layout.y.sourcesTitle + 2, 14, 14, YELLOW);
    ctx.font = bauhausFont(800, LABEL);
    ctx.fillText("来源占比", PAD_L + 22, layout.y.sourcesTitle);
    for (const [index, source] of data.sources.entries()) {
      const x = PAD_L + index * layout.sourceColW;
      fillRect(ctx, x, layout.y.sources + 2, 14, 14, colorAt(SOURCE_SWATCHES, index));
      ctx.fillStyle = INK;
      ctx.font = bauhausFont(700, 13);
      ctx.fillText(source.label, x + 20, layout.y.sources);
      ctx.font = bauhausFont(800, 18);
      ctx.fillText(`${source.pct}%`, x + 20, layout.y.sources + 20);
    }
  }

  if (layout.y.stats != null && layout.y.statsBottom != null && layout.statRows.length > 0) {
    const mid = PAD_L + layout.statColW;
    fillRect(ctx, mid, layout.y.stats, 2, layout.y.statsBottom - layout.y.stats, BLUE);
    fillRect(ctx, PAD_L, layout.y.statsBottom, CONTENT_RIGHT - PAD_L, 2, BLUE);
    let rowY = layout.y.stats + 10;
    for (const row of layout.statRows) {
      for (const [col, cell] of [row.left, row.right].entries()) {
        if (!cell) {
          continue;
        }
        const x = PAD_L + col * layout.statColW + 16;
        fillCircle(ctx, x + 6, rowY + 8, 7, cell.dot);
        ctx.fillStyle = INK;
        ctx.font = bauhausFont(800, LABEL);
        ctx.fillText(cell.label, x + 22, rowY);
        ctx.font = bauhausFont(800, VALUE);
        for (const [lineIndex, line] of cell.lines.entries()) {
          ctx.fillText(line, x + 22, rowY + 22 + lineIndex * 24);
        }
      }
      rowY += row.height;
    }
  }

  fillRect(ctx, PAD_L, layout.y.strips, 248, 12, INK);
  fillRect(ctx, PAD_L + 264, layout.y.strips, 78, 12, RED);
  fillRect(ctx, PAD_L + 358, layout.y.strips, 128, 12, BLUE);
}

/** 在 2× 位图上绘构成海报。预览用 CSS 缩到 720px，复制时直接导出画布。 */
export function paintBauhausPoster(canvas: HTMLCanvasElement, data: PosterViewModel): void {
  const scratch = canvas.getContext("2d");
  if (!scratch) {
    return;
  }
  const measure: TextMeasure = (font, text) => {
    scratch.font = font;
    return scratch.measureText(text).width;
  };
  const layout = layoutBauhausPoster(data, measure);
  canvas.width = BAUHAUS_CSS_WIDTH * BAUHAUS_SCALE;
  canvas.height = layout.height * BAUHAUS_SCALE;
  canvas.style.width = `${BAUHAUS_CSS_WIDTH}px`;
  canvas.style.height = `${layout.height}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.setTransform(BAUHAUS_SCALE, 0, 0, BAUHAUS_SCALE, 0, 0);
  drawPaper(ctx, layout.height);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  addPaperGrain(ctx, canvas.width, canvas.height);
  ctx.setTransform(BAUHAUS_SCALE, 0, 0, BAUHAUS_SCALE, 0, 0);
  drawBauhausPoster(ctx, data, layout);
}
