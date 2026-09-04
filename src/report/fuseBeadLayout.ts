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
};

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
  const sourceH = data.sources.length > 0 ? snap(180) : 0;
  const rightH = data.stats.length > 0 ? snap(180) : 0;
  const bottomH = Math.max(sourceH, rightH);
  y.footer = y.bottom + bottomH + (bottomH > 0 ? snap(14) : 0);
  const footerH = data.stats.length > 2 ? snap(80) : 0;
  return {
    height: snap(y.footer + footerH + (footerH > 0 ? 24 : 20)),
    y,
    comments,
    heroH,
    barH,
    commentLineH,
    sourceH,
  };
}
