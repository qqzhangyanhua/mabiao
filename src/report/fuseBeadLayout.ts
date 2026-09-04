import type { PosterViewModel } from "./posterTypes";

export const FUSE_W = 720;
export const FUSE_SCALE = 2;
export const BEAD = 10;

export const FONT_TITLE = 54;
export const FONT_DATE = 16;
export const FONT_HERO_LABEL = 14;
export const FONT_HERO_VALUE = 48;
export const FONT_COMMENT = 15;
export const FONT_BODY = 14;
export const FONT_STAT = 13;

export function snap(value: number): number {
  return Math.round(value / BEAD) * BEAD;
}

function charWidth(ch: string, fontPx: number): number {
  if (ch === " ") {
    return fontPx * 0.4;
  }
  return ch.charCodeAt(0) < 128 ? fontPx * 0.62 : fontPx;
}

function tokenWidth(text: string, fontPx: number): number {
  let width = 0;
  for (const ch of [...text]) {
    width += charWidth(ch, fontPx);
  }
  return width;
}

export function wrapBeadText(text: string, fontPx: number, maxWidth: number): string[] {
  if (text.length === 0) {
    return [];
  }
  const lines: string[] = [];
  let current = "";
  let width = 0;
  const flush = () => {
    if (current !== "") {
      lines.push(current);
      current = "";
      width = 0;
    }
  };
  for (const token of text.split(/(\s+)/)) {
    const w = tokenWidth(token, fontPx);
    if (current !== "" && width + w > maxWidth) {
      flush();
    }
    if (current === "" && w > maxWidth) {
      for (const ch of [...token]) {
        const cw = charWidth(ch, fontPx);
        if (current !== "" && width + cw > maxWidth) {
          flush();
        }
        current += ch;
        width += cw;
      }
      continue;
    }
    current += token;
    width += w;
  }
  flush();
  return lines.length > 0 ? lines : [text];
}

export type FuseBeadComment = {
  y: number;
  lines: string[];
  text: string;
};

export type FuseBeadLayout = {
  height: number;
  y: {
    title: number;
    date: number;
    hero: number;
    comments: number;
    bars: number;
    bottom: number;
    footer: number;
  };
  comments: FuseBeadComment[];
  heroH: number;
  barH: number;
  commentLineH: number;
  sourceH: number;
  sourceHeadH: number;
  sourceRowH: number;
  sourceFont: number;
};

const SOURCE_HEAD_H = 56;
const SOURCE_TAIL_H = 24;
const SOURCE_ROW_MAX = 30;
const SOURCE_ROW_MIN = BEAD * 2;
const SOURCE_FONT_MAX = FONT_BODY;
const SOURCE_FONT_MIN = 11;
/** 右侧两张统计卡的高度；来源卡先尽量排进这个高度，排不下再长高。 */
const SOURCE_FIT_H = 180;

function layoutSourceCard(count: number): {
  sourceH: number;
  sourceHeadH: number;
  sourceRowH: number;
  sourceFont: number;
} {
  if (count <= 0) {
    return {
      sourceH: 0,
      sourceHeadH: SOURCE_HEAD_H,
      sourceRowH: SOURCE_ROW_MAX,
      sourceFont: SOURCE_FONT_MAX,
    };
  }
  const comfortable = SOURCE_HEAD_H + count * SOURCE_ROW_MAX + SOURCE_TAIL_H;
  if (comfortable <= SOURCE_FIT_H) {
    return {
      sourceH: snap(comfortable),
      sourceHeadH: SOURCE_HEAD_H,
      sourceRowH: SOURCE_ROW_MAX,
      sourceFont: SOURCE_FONT_MAX,
    };
  }
  const inner = SOURCE_FIT_H - SOURCE_HEAD_H - SOURCE_TAIL_H;
  const sourceRowH = Math.max(SOURCE_ROW_MIN, Math.floor(inner / count));
  const t = (sourceRowH - SOURCE_ROW_MIN) / (SOURCE_ROW_MAX - SOURCE_ROW_MIN);
  const sourceFont = Math.round(SOURCE_FONT_MIN + t * (SOURCE_FONT_MAX - SOURCE_FONT_MIN));
  return {
    sourceH: snap(SOURCE_HEAD_H + count * sourceRowH + SOURCE_TAIL_H),
    sourceHeadH: SOURCE_HEAD_H,
    sourceRowH,
    sourceFont,
  };
}

export function layoutFuseBeadPoster(data: PosterViewModel): FuseBeadLayout {
  const y = {
    title: snap(36),
    date: snap(100),
    hero: snap(150),
    comments: 0,
    bars: 0,
    bottom: 0,
    footer: 0,
  };
  const heroH = snap(150);
  y.comments = snap(y.hero + heroH + 16);
  const commentLineH = snap(50);
  const comments: FuseBeadComment[] = [];
  let cursor = y.comments;
  for (const text of data.comments) {
    comments.push({
      y: cursor,
      lines: wrapBeadText(text, FONT_COMMENT, FUSE_W - 160),
      text,
    });
    cursor += commentLineH + snap(8);
  }
  if (comments.length === 0) {
    cursor = y.comments;
  }
  y.bars = snap(cursor + 8);
  const barH = data.days.length > 0 ? snap(200) : 0;
  y.bottom = y.bars + (barH > 0 ? barH + snap(14) : 0);
  const sourceCard = layoutSourceCard(data.sources.length);
  const rightH = data.stats.length > 0 ? snap(SOURCE_FIT_H) : 0;
  const bottomH = Math.max(sourceCard.sourceH, rightH);
  y.footer = y.bottom + bottomH + (bottomH > 0 ? snap(14) : 0);
  const footerH = data.stats.length > 2 ? snap(80) : 0;
  return {
    height: snap(y.footer + footerH + (footerH > 0 ? 24 : 20)),
    y,
    comments,
    heroH,
    barH,
    commentLineH,
    ...sourceCard,
  };
}
