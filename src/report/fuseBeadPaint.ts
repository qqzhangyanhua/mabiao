import type { PosterViewModel } from "./posterTypes";
import { framePosterLayout, sizePosterCanvas } from "./posterFrame";
import {
  BEAD,
  FONT_BODY,
  FONT_COMMENT,
  FONT_DATE,
  FONT_HERO_LABEL,
  FONT_STAT,
  FUSE_SCALE,
  FUSE_W,
  layoutFuseBeadPoster,
  snap,
  type FuseBeadLayout,
} from "./fuseBeadLayout";
import {
  CARD,
  FACE,
  INK,
  PURPLE,
  WHITE,
  YELLOW,
  beadText,
  drawBead,
  drawBeadBorder,
  drawCard,
  drawDigitString,
  drawPegboard,
  fillLabel,
  pill,
} from "./fuseBeadDraw";

export { layoutFuseBeadPoster, type FuseBeadLayout } from "./fuseBeadLayout";

const ORANGE = "#f97316";
const TEAL = "#06b6d4";
const BLUE = "#3b82f6";

function stamp(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  matrix: string[],
  color: string,
  pitch = BEAD,
): void {
  for (const [r, row] of matrix.entries()) {
    for (let c = 0; c < row.length; c += 1) {
      if (row[c] === "1") {
        drawBead(
          ctx,
          x + (c - (row.length - 1) / 2) * pitch,
          y + (r - (matrix.length - 1) / 2) * pitch,
          color,
          false,
          pitch,
        );
      }
    }
  }
}

function drawStar(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  stamp(ctx, x, y, [".1.", "111", ".1.", "1.1"], YELLOW, 8);
}

function drawFlame(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  stamp(ctx, x, y, [".1.", ".11", "111", "111", ".1."], ORANGE, 7);
}

function drawMoon(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  stamp(ctx, x, y, [".11", "11.", "1..", "11.", ".11"], PURPLE, 7);
}

function drawClock(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  stamp(ctx, x, y, [".1.", "1.1", "111", "1.1", ".1."], YELLOW, 7);
  drawBead(ctx, x, y, INK, true, 7);
}

function drawSun(ctx: CanvasRenderingContext2D, x: number, y: number): void {
  stamp(ctx, x, y, [".1.", "111", ".1."], YELLOW, 8);
}

function drawTitle(ctx: CanvasRenderingContext2D, kicker: string, y: number): void {
  const parts = kicker.split(" · ");
  drawStar(ctx, 78, y + 40);
  drawStar(ctx, 642, y + 40);
  const pitch = 6;
  if (parts.length === 2 && parts[0] && parts[1]) {
    beadText(ctx, parts[0], FUSE_W / 2 - 16, y + 8, 70, YELLOW, "right", pitch);
    drawBead(ctx, FUSE_W / 2 - 3, y + 40, YELLOW, true, pitch);
    beadText(ctx, parts[1], FUSE_W / 2 + 16, y + 8, 70, PURPLE, "left", pitch);
  } else {
    beadText(ctx, kicker, FUSE_W / 2, y + 8, 70, YELLOW, "center", pitch);
  }
}

function drawDate(ctx: CanvasRenderingContext2D, rangeLabel: string, y: number): void {
  ctx.font = `800 ${FONT_DATE}px ${FACE}`;
  const w = Math.max(280, ctx.measureText(rangeLabel).width + 36);
  const x0 = FUSE_W / 2 - w / 2;
  pill(ctx, x0, y, w, 32, CARD);
  fillLabel(ctx, rangeLabel, FUSE_W / 2, y + 16, FONT_DATE, WHITE, "center");
  for (let i = 0; i < 5; i += 1) {
    drawBead(ctx, x0 - 18 - i * 12, y + 11, PURPLE, true, 8);
    drawBead(ctx, x0 + w + 10 + i * 12, y + 11, PURPLE, true, 8);
  }
}

function drawHero(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: FuseBeadLayout,
): void {
  const cardW = 295;
  const cardH = layout.heroH;
  const y = layout.y.hero;
  const left = 55;
  const right = 370;
  drawCard(ctx, left, y, cardW, cardH);
  pill(ctx, left + 16, y + 14, cardW - 32, 28, PURPLE);
  fillLabel(ctx, data.totalUnit, left + cardW / 2, y + 28, FONT_HERO_LABEL, YELLOW, "center");
  drawDigitString(ctx, data.totalTokensLabel, left + cardW / 2, y + 96, YELLOW, 8);

  drawCard(ctx, right, y, cardW, cardH);
  pill(ctx, right + 16, y + 14, cardW - 32, 28, YELLOW);
  const costTitle =
    data.totalCostLabel != null
      ? (data.comments[0]?.replace(/\s+\S+\s+token。$/, "") ?? data.totalUnit)
      : data.totalUnit;
  fillLabel(ctx, costTitle, right + cardW / 2, y + 28, FONT_HERO_LABEL, INK, "center");
  drawDigitString(ctx, data.totalCostLabel ?? data.totalTokensLabel, right + cardW / 2, y + 96, PURPLE, 8);
}

function drawComments(ctx: CanvasRenderingContext2D, layout: FuseBeadLayout): void {
  const x = 55;
  const w = 610;
  const h = layout.commentLineH;
  const icons = [drawFlame, drawMoon, drawClock];
  for (const [index, comment] of layout.comments.entries()) {
    pill(ctx, x, comment.y, w, h, CARD);
    icons[index % icons.length]?.(ctx, x + 28, comment.y + h / 2);
    ctx.font = `700 ${FONT_COMMENT}px ${FACE}`;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    let cursor = x + 52;
    const re = /(\d+(?:\.\d+)?(?:%|M)?|\$\d+(?:\.\d+)?|\d{1,2}:\d{2})/g;
    let last = 0;
    const cy = comment.y + h / 2;
    for (const match of comment.text.matchAll(re)) {
      const start = match.index ?? 0;
      if (start > last) {
        const part = comment.text.slice(last, start);
        ctx.fillStyle = WHITE;
        ctx.fillText(part, cursor, cy);
        cursor += ctx.measureText(part).width;
      }
      ctx.fillStyle = YELLOW;
      ctx.fillText(match[0], cursor, cy);
      cursor += ctx.measureText(match[0]).width;
      last = start + match[0].length;
    }
    if (last < comment.text.length) {
      ctx.fillStyle = WHITE;
      ctx.fillText(comment.text.slice(last), cursor, cy);
    }
  }
}

function drawBars(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: FuseBeadLayout,
): void {
  if (data.days.length === 0) {
    return;
  }
  const x = 55;
  const y = layout.y.bars;
  const w = 610;
  const h = layout.barH;
  drawCard(ctx, x, y, w, h);
  pill(ctx, x + 16, y + 14, 92, 24, PURPLE);
  fillLabel(ctx, "按天节奏", x + 62, y + 26, FONT_STAT, WHITE, "center");
  const max = Math.max(1, ...data.days.map((day) => day.tokens));
  const maxBeads = 13;
  const cols = 6;
  const slot = (w - 50) / data.days.length;
  const base = y + h - 36;
  for (const [index, day] of data.days.entries()) {
    const n = Math.max(1, Math.round((day.tokens / max) * maxBeads));
    const color = index % 2 === 0 ? PURPLE : YELLOW;
    const gx = snap(x + 28 + index * slot + slot / 2 - (cols * BEAD) / 2);
    for (let r = 0; r < n; r += 1) {
      for (let c = 0; c < cols; c += 1) {
        drawBead(ctx, gx + c * BEAD, base - (r + 1) * BEAD, color);
      }
    }
    fillLabel(ctx, day.label, gx + (cols * BEAD) / 2, base + 14, 15, WHITE, "center");
  }
}

function drawBottom(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: FuseBeadLayout,
): void {
  const left = 55;
  const right = 370;
  const colW = 295;
  const y = layout.y.bottom;
  if (data.sources.length > 0) {
    drawCard(ctx, left, y, colW, layout.sourceH);
    pill(ctx, left + 14, y + 14, 78, 24, PURPLE);
    fillLabel(ctx, "来源占比", left + 53, y + 26, FONT_STAT, WHITE, "center");
    const tones = [PURPLE, TEAL, BLUE, YELLOW];
    for (const [index, source] of data.sources.slice(0, 4).entries()) {
      const sy = y + 56 + index * 30;
      drawBead(ctx, left + 22, sy - 4, tones[index % tones.length] ?? YELLOW, true);
      fillLabel(ctx, source.label, left + 46, sy + 6, FONT_BODY, WHITE);
      fillLabel(ctx, `${source.pct}%`, left + colW - 18, sy + 6, 15, tones[index % tones.length] ?? YELLOW, "right");
    }
  }
  const stats = data.stats;
  if (stats[0]) {
    drawCard(ctx, right, y, colW, 92);
    fillLabel(ctx, stats[0].label, right + 18, y + 22, FONT_STAT, "#cbd5e1");
    fillLabel(ctx, stats[0].value, right + 118, y + 58, 36, YELLOW, "center");
    drawSun(ctx, right + 250, y + 56);
  }
  if (stats[1]) {
    const top = y + 106;
    drawCard(ctx, right, top, colW, 84);
    fillLabel(ctx, stats[1].label, right + 18, top + 20, FONT_STAT, "#cbd5e1");
    const models = stats[1].value.split(" · ").slice(0, 3);
    const tones = [PURPLE, BLUE, TEAL];
    for (const [index, model] of models.entries()) {
      const my = top + 40 + index * 14;
      drawBead(ctx, right + 18, my - 6, tones[index % tones.length] ?? PURPLE, true);
      fillLabel(ctx, model, right + 36, my, 12, WHITE);
    }
  }
  if (stats[2]) {
    drawCard(ctx, left, layout.y.footer, 610, 80);
    fillLabel(ctx, stats[2].label, left + 24, layout.y.footer + 40, 15, "#cbd5e1");
    const price = stats[2].value.match(/(\$\d+(?:\.\d+)?)/)?.[1] ?? stats[2].value;
    drawDigitString(ctx, price, FUSE_W / 2 + 10, layout.y.footer + 40, YELLOW, 8);
    drawFlame(ctx, left + 560, layout.y.footer + 40);
  }
}

function drawContent(
  ctx: CanvasRenderingContext2D,
  data: PosterViewModel,
  layout: FuseBeadLayout,
): void {
  drawBeadBorder(ctx, layout.height);
  drawTitle(ctx, data.kicker, layout.y.title);
  drawDate(ctx, data.rangeLabel, layout.y.date);
  drawHero(ctx, data, layout);
  drawComments(ctx, layout);
  drawBars(ctx, data, layout);
  drawBottom(ctx, data, layout);
}

/** 在 2× 位图上绘拼豆海报。预览缩到 720px，复制时直接导出画布。 */
export function paintFuseBeadPoster(canvas: HTMLCanvasElement, data: PosterViewModel): void {
  const layout = framePosterLayout(layoutFuseBeadPoster(data));
  sizePosterCanvas(canvas, FUSE_SCALE);
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.setTransform(FUSE_SCALE, 0, 0, FUSE_SCALE, 0, 0);
  drawPegboard(ctx, layout.height);
  drawContent(ctx, data, layout);
}
