/**
 * 会话时间线视口虚拟化：窗口仍可持有最多 1000 条，DOM 只挂视口附近几十行。
 */

export const TIMELINE_ROW_ESTIMATE = 96;
export const TIMELINE_OVERSCAN = 12;

export type TimelineVirtualRange = {
  start: number;
  end: number;
  paddingTop: number;
  paddingBottom: number;
  totalHeight: number;
};

export type TimelineHeightAnchor = {
  key: string;
  offset: number;
};

export function timelineRowHeight(
  key: string,
  measured: ReadonlyMap<string, number>,
  estimate = TIMELINE_ROW_ESTIMATE,
): number {
  return measured.get(key) ?? estimate;
}

export function timelineOffsetAt(
  keys: readonly string[],
  index: number,
  measured: ReadonlyMap<string, number>,
  estimate = TIMELINE_ROW_ESTIMATE,
): number {
  const limit = Math.min(Math.max(index, 0), keys.length);
  let offset = 0;
  for (let i = 0; i < limit; i += 1) {
    offset += timelineRowHeight(keys[i], measured, estimate);
  }
  return offset;
}

export function timelineVisibleRange({
  scrollTop,
  viewportHeight,
  keys,
  measured,
  overscan = TIMELINE_OVERSCAN,
  estimate = TIMELINE_ROW_ESTIMATE,
  preferEnd = false,
}: {
  scrollTop: number;
  viewportHeight: number;
  keys: readonly string[];
  measured: ReadonlyMap<string, number>;
  overscan?: number;
  estimate?: number;
  preferEnd?: boolean;
}): TimelineVirtualRange {
  const totalHeight = timelineOffsetAt(keys, keys.length, measured, estimate);
  if (keys.length === 0) {
    return { start: 0, end: 0, paddingTop: 0, paddingBottom: 0, totalHeight: 0 };
  }

  let viewH = viewportHeight;
  let top = scrollTop;
  if (viewH <= 0) {
    viewH = estimate * (overscan * 2 + 8);
    top = preferEnd ? Math.max(0, totalHeight - viewH) : 0;
  } else if (preferEnd && top <= 0 && totalHeight > viewH) {
    top = Math.max(0, totalHeight - viewH);
  }
  top = Math.min(Math.max(0, top), Math.max(0, totalHeight));

  let offset = 0;
  let start = keys.length;
  for (let i = 0; i < keys.length; i += 1) {
    const height = timelineRowHeight(keys[i], measured, estimate);
    if (offset + height > top) {
      start = i;
      break;
    }
    offset += height;
  }

  const viewEnd = top + viewH;
  let end = start;
  let running = timelineOffsetAt(keys, start, measured, estimate);
  while (end < keys.length && running < viewEnd) {
    running += timelineRowHeight(keys[end], measured, estimate);
    end += 1;
  }

  start = Math.max(0, start - overscan);
  end = Math.min(keys.length, end + overscan);
  const paddingTop = timelineOffsetAt(keys, start, measured, estimate);
  const afterEnd = timelineOffsetAt(keys, end, measured, estimate);
  return {
    start,
    end,
    paddingTop,
    paddingBottom: Math.max(0, totalHeight - afterEnd),
    totalHeight,
  };
}

export function timelineAnchorAtOffset(
  keys: readonly string[],
  scrollTop: number,
  measured: ReadonlyMap<string, number>,
  estimate = TIMELINE_ROW_ESTIMATE,
  eligible?: ReadonlySet<string>,
): TimelineHeightAnchor | null {
  let start = 0;
  for (const key of keys) {
    const height = timelineRowHeight(key, measured, estimate);
    if ((!eligible || eligible.has(key)) && start + height > scrollTop) {
      return { key, offset: start - scrollTop };
    }
    start += height;
  }
  if (eligible) {
    for (let i = keys.length - 1; i >= 0; i -= 1) {
      if (eligible.has(keys[i])) {
        return { key: keys[i], offset: 0 };
      }
    }
    return null;
  }
  return keys.length > 0 ? { key: keys[keys.length - 1], offset: 0 } : null;
}

export function timelineScrollTopForAnchor(
  anchor: TimelineHeightAnchor,
  keys: readonly string[],
  measured: ReadonlyMap<string, number>,
  estimate = TIMELINE_ROW_ESTIMATE,
): number | null {
  const index = keys.indexOf(anchor.key);
  if (index < 0) {
    return null;
  }
  return Math.max(0, timelineOffsetAt(keys, index, measured, estimate) - anchor.offset);
}

export function timelineScrollCorrection({
  itemOffset,
  previousHeight,
  nextHeight,
  scrollTop,
}: {
  itemOffset: number;
  previousHeight: number;
  nextHeight: number;
  scrollTop: number;
}): number {
  if (itemOffset + previousHeight <= scrollTop) {
    return nextHeight - previousHeight;
  }
  return 0;
}

export function pruneTimelineMeasurements(
  measured: Map<string, number>,
  keys: readonly string[],
): void {
  const allowed = new Set(keys);
  for (const key of measured.keys()) {
    if (!allowed.has(key)) {
      measured.delete(key);
    }
  }
}
