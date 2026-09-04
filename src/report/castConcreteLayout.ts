import { POSTER_FRAME_HEIGHT, splitFrameExtra } from "./posterFrame";
import type { PosterViewModel } from "./posterTypes";

export const CONCRETE_W = 720;
export const CONCRETE_SCALE = 2;
export const PAD_X = 56;
export const CONTENT_W = CONCRETE_W - PAD_X * 2;

const FACE = '"Helvetica Neue", Helvetica, "PingFang SC", "Noto Sans SC", sans-serif';

export type TextMeasure = (font: string, text: string) => number;

export function concreteFont(weight: number, px: number): string {
  return `${weight} ${px}px ${FACE}`;
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
    current += token;
  }
  flush();
  return lines.length > 0 ? lines : [text];
}

export type CastConcreteLayout = {
  height: number;
  barH: number;
  comments: { y: number; text: string }[];
  sourceLine: string | null;
  y: {
    title: number;
    date: number;
    rule: number;
    cast: number;
    unit: number;
    comments: number | null;
    bars: number | null;
    sources: number | null;
    stats: number | null;
  };
};

export const BAR_H = 80;

export function layoutCastConcretePoster(
  data: PosterViewModel,
  measure: TextMeasure,
): CastConcreteLayout {
  const y = {
    title: 44,
    date: 128,
    rule: 166,
    cast: 186,
    unit: 234,
    comments: null as number | null,
    bars: null as number | null,
    sources: null as number | null,
    stats: null as number | null,
  };

  let cursor = y.unit + 40;
  const bodyFont = concreteFont(500, 17);
  const comments: { y: number; text: string }[] = [];
  if (data.comments.length > 0) {
    y.comments = cursor;
    for (const comment of data.comments) {
      for (const line of wrapText(measure, bodyFont, comment, CONTENT_W)) {
        comments.push({ y: cursor, text: line });
        cursor += 26;
      }
    }
    cursor += 18;
  }

  if (data.days.length > 0) {
    y.bars = cursor;
    cursor += 28 + BAR_H + 16;
  }

  let sourceLine: string | null = null;
  if (data.sources.length > 0) {
    y.sources = cursor;
    sourceLine = data.sources.map((source) => `${source.label} ${source.pct}%`).join("   ");
    cursor += 64;
  }

  if (data.stats.length > 0) {
    y.stats = cursor;
    cursor += data.stats.length * 28 + 8;
  }

  const packedHeight = cursor + 48;
  const { chartExtra, gaps } = splitFrameExtra(
    packedHeight,
    4,
    y.bars == null ? 0 : 160,
  );
  const afterComments = gaps[0] ?? 0;
  const afterBars = gaps[1] ?? 0;
  const afterSources = gaps[2] ?? 0;
  if (y.bars != null) {
    y.bars += afterComments;
  }
  const afterBarBlock = afterComments + chartExtra + afterBars;
  if (y.sources != null) {
    y.sources += afterBarBlock;
  }
  if (y.stats != null) {
    y.stats += afterBarBlock + afterSources;
  }
  return {
    height: packedHeight < POSTER_FRAME_HEIGHT ? POSTER_FRAME_HEIGHT : packedHeight,
    barH: BAR_H + chartExtra,
    comments,
    sourceLine,
    y,
  };
}
