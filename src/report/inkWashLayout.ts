import { POSTER_FRAME_HEIGHT, offsetPackedY, splitFrameExtra } from "./posterFrame";
import type { PosterViewModel } from "./posterTypes";

export const INK_WASH_WIDTH = 720;
export const INK_WASH_SCALE = 2;
export const PAD_X = 56;
export const CONTENT_W = INK_WASH_WIDTH - PAD_X * 2;

const KAI = '"Kaiti SC", "STKaiti", "KaiTi", "Songti SC", "Noto Serif SC", serif';
const XING = '"Xingkai SC", "STXingkai", "Kaiti SC", "STKaiti", serif';

export type TextMeasure = (font: string, text: string) => number;

export function inkKai(weight: number, px: number): string {
  return `${weight} ${px}px ${KAI}`;
}

export function inkXing(weight: number, px: number): string {
  return `${weight} ${px}px ${XING}`;
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
  const tokens = text.split(/( · |\/|\s+\||\s+)/).filter((token) => token.length > 0);
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
    current += token;
  }
  flush();
  return lines.length > 0 ? lines : [text];
}

export type InkWashLayout = {
  height: number;
  numberSize: number;
  barH: number;
  comments: { y: number; text: string }[];
  sourceLines: { y: number; text: string }[];
  statLines: { y: number; text: string }[];
  y: {
    kicker: number;
    date: number;
    number: number;
    unit: number;
    bars: number;
    barLabels: number;
    sources: number | null;
    stats: number | null;
  };
};

const NUMBER_MAX = 128;
const BAR_H = 148;

export function layoutInkWashPoster(data: PosterViewModel, measure: TextMeasure): InkWashLayout {
  let numberSize = NUMBER_MAX;
  while (numberSize > 72 && measure(inkXing(700, numberSize), data.totalTokensLabel) > CONTENT_W) {
    numberSize -= 4;
  }

  const y = {
    kicker: 44,
    date: 76,
    number: 118,
    unit: 118 + numberSize + 18,
    bars: 0,
    barLabels: 0,
    sources: null as number | null,
    stats: null as number | null,
  };

  let cursor = y.unit + 36;
  const bodyFont = inkKai(500, 18);
  const comments: { y: number; text: string }[] = [];
  for (const comment of data.comments) {
    for (const line of wrapText(measure, bodyFont, comment, CONTENT_W)) {
      comments.push({ y: cursor, text: line });
      cursor += 30;
    }
  }

  cursor += 28;
  y.bars = cursor;
  y.barLabels = cursor + BAR_H + 10;
  cursor = y.barLabels + 28;

  const smallFont = inkKai(500, 14);
  const sourceLines: { y: number; text: string }[] = [];
  if (data.sources.length > 0) {
    const line = `来源占比：${data.sources.map((source) => `${source.label} ${source.pct}%`).join(" · ")}`;
    y.sources = cursor;
    for (const text of wrapText(measure, smallFont, line, CONTENT_W)) {
      sourceLines.push({ y: cursor, text });
      cursor += 22;
    }
    cursor += 10;
  }

  const statLines: { y: number; text: string }[] = [];
  if (data.stats.length > 0) {
    const line = data.stats.map((stat) => `${stat.label} ${stat.value}`).join("  |  ");
    y.stats = cursor;
    for (const text of wrapText(measure, smallFont, line, CONTENT_W)) {
      statLines.push({ y: cursor, text });
      cursor += 22;
    }
  }

  const packedHeight = cursor + 48;
  const { chartExtra, gaps } = splitFrameExtra(
    packedHeight,
    4,
    data.days.length > 0 ? 140 : 0,
  );
  const afterComments = gaps[0] ?? 0;
  const afterBars = gaps[1] ?? 0;
  const afterSources = gaps[2] ?? 0;
  const barH = BAR_H + chartExtra;
  y.bars += afterComments;
  y.barLabels = y.bars + barH + 10;
  const afterBarBlock = afterComments + chartExtra + afterBars;
  if (y.sources != null) {
    y.sources += afterBarBlock;
  }
  if (y.stats != null) {
    y.stats += afterBarBlock + afterSources;
  }
  return {
    height: packedHeight < POSTER_FRAME_HEIGHT ? POSTER_FRAME_HEIGHT : packedHeight,
    numberSize,
    barH,
    comments,
    sourceLines: offsetPackedY(sourceLines, afterBarBlock),
    statLines: offsetPackedY(statLines, afterBarBlock + afterSources),
    y,
  };
}

export const INK_BAR_HEIGHT = BAR_H;
