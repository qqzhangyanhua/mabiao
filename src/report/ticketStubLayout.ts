import type { PosterViewModel } from "./posterTypes";

export const TICKET_WIDTH = 720;
export const TICKET_SCALE = 2;
export const PAD_X = 64;
export const CONTENT_W = TICKET_WIDTH - PAD_X * 2;

const FACE = '"Iowan Old Style", "Times New Roman", "Songti SC", "Noto Serif SC", "STSong", serif';

export type TextMeasure = (font: string, text: string) => number;

export function ticketFont(weight: number, px: number): string {
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

function pad2(value: string): string {
  return value.padStart(2, "0");
}

/** 从周期文案抽出票号，例如 8月24日–8月30日 → No. 0824-0830。不是新数据。 */
export function ticketSerial(rangeLabel: string): string | null {
  const matches = [...rangeLabel.matchAll(/(\d{1,2})月(\d{1,2})日/g)];
  if (matches.length < 2) {
    return null;
  }
  const start = matches[0];
  const end = matches[1];
  if (!start?.[1] || !start[2] || !end?.[1] || !end[2]) {
    return null;
  }
  return `No. ${pad2(start[1])}${pad2(start[2])}-${pad2(end[1])}${pad2(end[2])}`;
}

export type TicketStubLayout = {
  height: number;
  serial: string | null;
  numberSize: number;
  comments: { y: number; text: string }[];
  sourceLines: { y: number; text: string }[];
  y: {
    kicker: number;
    date: number;
    number: number;
    unit: number;
    comments: number | null;
    chart: number | null;
    sources: number | null;
    stats: number | null;
    footer: number;
  };
};

export const TICKET_CHART_H = 92;

export function layoutTicketStubPoster(
  data: PosterViewModel,
  measure: TextMeasure,
): TicketStubLayout {
  let numberSize = 64;
  while (
    numberSize > 44 &&
    measure(ticketFont(700, numberSize), data.totalTokensLabel) > CONTENT_W
  ) {
    numberSize -= 2;
  }

  const y = {
    kicker: 36,
    date: 68,
    number: 102,
    unit: 102 + numberSize + 12,
    comments: null as number | null,
    chart: null as number | null,
    sources: null as number | null,
    stats: null as number | null,
    footer: 0,
  };

  let cursor = y.unit + 36;
  const bodyFont = ticketFont(500, 16);
  const comments: { y: number; text: string }[] = [];
  if (data.comments.length > 0) {
    y.comments = cursor;
    for (const comment of data.comments) {
      for (const line of wrapText(measure, bodyFont, comment, CONTENT_W)) {
        comments.push({ y: cursor, text: line });
        cursor += 24;
      }
    }
    cursor += 16;
  }

  if (data.days.length > 0) {
    y.chart = cursor;
    cursor += TICKET_CHART_H + 36;
  }

  const smallFont = ticketFont(500, 13);
  const sourceLines: { y: number; text: string }[] = [];
  if (data.sources.length > 0) {
    y.sources = cursor;
    cursor += 52;
    const line = data.sources.map((source) => `${source.label} ${source.pct}%`).join(" · ");
    for (const text of wrapText(measure, smallFont, line, CONTENT_W)) {
      sourceLines.push({ y: cursor, text });
      cursor += 20;
    }
    cursor += 28;
  }

  if (data.stats.length > 0) {
    y.stats = cursor;
    cursor += data.stats.length * 28 + 12;
  }

  y.footer = cursor + 8;
  return {
    height: cursor + 44,
    serial: ticketSerial(data.rangeLabel),
    numberSize,
    comments,
    sourceLines,
    y,
  };
}
