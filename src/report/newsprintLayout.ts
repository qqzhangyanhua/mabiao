import { POSTER_FRAME_HEIGHT, splitFrameExtra } from "./posterFrame";
import type { PosterViewModel } from "./posterTypes";

export const NEWSPRINT_CSS_WIDTH = 720;
export const NEWSPRINT_SCALE = 2;

const FACE = '"Songti SC", "Iowan Old Style", "STSong", "Noto Serif SC", "Times New Roman", serif';

export const PAD = 48;
export const INSET = 12;
export const CONTENT_W = NEWSPRINT_CSS_WIDTH - PAD * 2;
export const TITLE = 62;
export const HEADLINE = 34;
export const BODY = 17;
export const BOX_H = 38;
export const CHART_H = 248;
export const PIE_R = 72;

export type TextMeasure = (font: string, text: string) => number;

export function newsprintFont(weight: number, px: number): string {
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

export type NewsprintLayout = {
  height: number;
  chartH: number;
  headline: string;
  headlineLines: string[];
  body: { y: number; text: string }[];
  barMax: number;
  statCols: { x: number; w: number; label: string; lines: string[] }[];
  y: {
    title: number;
    titleRule: number;
    date: number;
    box: number;
    headline: number;
    charts: number | null;
    stats: number | null;
    statsBottom: number | null;
    footer: number;
  };
};

export function layoutNewsprintPoster(
  data: PosterViewModel,
  measure: TextMeasure,
): NewsprintLayout {
  const headline =
    data.totalCostLabel != null
      ? `${data.totalUnit} ${data.totalCostLabel}`
      : `${data.totalTokensLabel} ${data.totalUnit}`;
  const headlineFont = newsprintFont(900, HEADLINE);
  const bodyFont = newsprintFont(500, BODY);
  const valueFont = newsprintFont(700, 16);
  const headlineLines = wrapText(measure, headlineFont, headline, CONTENT_W);

  const y = {
    title: 40,
    titleRule: 40 + TITLE + 10,
    date: 40 + TITLE + 22,
    box: 40 + TITLE + 48,
    headline: 40 + TITLE + 48 + BOX_H + 18,
    charts: null as number | null,
    stats: null as number | null,
    statsBottom: null as number | null,
    footer: 0,
  };

  let cursor = y.headline + headlineLines.length * 40 + 8;
  const body: { y: number; text: string }[] = [];
  for (const comment of data.comments) {
    for (const line of wrapText(measure, bodyFont, comment, CONTENT_W)) {
      body.push({ y: cursor, text: line });
      cursor += 26;
    }
  }

  if (data.days.length > 0 || data.sources.length > 0) {
    cursor += 16;
    y.charts = cursor;
    cursor += CHART_H + 36;
  }

  const statW = CONTENT_W / Math.max(data.stats.length, 1);
  const statCols = data.stats.map((stat, index) => ({
    x: PAD + index * statW,
    w: statW,
    label: stat.label,
    lines: wrapText(measure, valueFont, stat.value, statW - 20),
  }));
  if (data.stats.length > 0) {
    const block = 28 + Math.max(...statCols.map((col) => col.lines.length), 1) * 20 + 16;
    y.stats = cursor;
    y.statsBottom = cursor + block;
    cursor = y.statsBottom + 14;
  }

  y.footer = cursor;
  const packedHeight = cursor + 22 + INSET + 8;
  const { chartExtra, gaps } = splitFrameExtra(
    packedHeight,
    4,
    y.charts == null ? 0 : 120,
  );
  const afterBody = gaps[0] ?? 0;
  const afterChart = gaps[1] ?? 0;
  const afterStats = gaps[2] ?? 0;
  if (y.charts != null) {
    y.charts += afterBody;
  }
  const afterCharts = afterBody + chartExtra + afterChart;
  if (y.stats != null && y.statsBottom != null) {
    y.stats += afterCharts;
    y.statsBottom += afterCharts;
  }
  y.footer += afterCharts + afterStats;
  return {
    height: packedHeight < POSTER_FRAME_HEIGHT ? POSTER_FRAME_HEIGHT : packedHeight,
    chartH: CHART_H + chartExtra,
    headline,
    headlineLines,
    body,
    barMax: Math.max(1, ...data.days.map((day) => day.tokens)),
    statCols,
    y,
  };
}
