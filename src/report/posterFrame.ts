/** 周报海报逻辑宽度。各风格 CSS / canvas 都钉这个值。 */
export const POSTER_FRAME_WIDTH = 720;

/**
 * 浅色磨砂在标准七槽位（FAKE_POSTER）下的画布高度。
 * 其它风格铺进同一框，分享预览与导出 PNG 宽高比一致。
 */
export const POSTER_FRAME_HEIGHT = 1053;

export function framePosterLayout<T extends { height: number }>(layout: T): T {
  return { ...layout, height: POSTER_FRAME_HEIGHT };
}

/** 把短布局多出来的高度先喂主图，剩下均分给段间距。 */
export function splitFrameExtra(
  packedHeight: number,
  gapCount: number,
  chartBoostCap: number,
): { chartExtra: number; gaps: number[] } {
  const extra = POSTER_FRAME_HEIGHT - packedHeight;
  const gaps = Array.from({ length: Math.max(gapCount, 0) }, () => 0);
  if (extra <= 0) {
    return { chartExtra: 0, gaps };
  }
  if (gaps.length === 0) {
    return { chartExtra: extra, gaps };
  }
  const chartExtra = Math.min(Math.max(0, chartBoostCap), Math.round(extra * 0.55));
  let remain = extra - chartExtra;
  const base = Math.floor(remain / gaps.length);
  remain -= base * gaps.length;
  for (let i = 0; i < gaps.length; i += 1) {
    gaps[i] = base + (i < remain ? 1 : 0);
  }
  return { chartExtra, gaps };
}

export function offsetPackedY<T extends { y: number }>(items: T[], dy: number): T[] {
  if (dy === 0 || items.length === 0) {
    return items;
  }
  return items.map((item) => ({ ...item, y: item.y + dy }));
}

export function sizePosterCanvas(canvas: HTMLCanvasElement, bitmapScale: number): void {
  canvas.width = POSTER_FRAME_WIDTH * bitmapScale;
  canvas.height = POSTER_FRAME_HEIGHT * bitmapScale;
  canvas.style.width = `${POSTER_FRAME_WIDTH}px`;
  canvas.style.height = `${POSTER_FRAME_HEIGHT}px`;
}
