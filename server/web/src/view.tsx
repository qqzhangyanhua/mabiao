import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import type { CostMode } from "./lib/cost";
import { parseViewQuery } from "./lib/query";
import {
  localOffsetMinutes,
  presetRange,
  queryBounds,
  type DayRange,
  type QueryBounds,
  type RangePreset,
} from "./lib/range";
import { buildHash, type Route } from "./lib/route";

export interface ViewState {
  preset: RangePreset;
  setPreset: (preset: RangePreset) => void;
  custom: DayRange;
  setCustom: (range: DayRange) => void;
  mode: CostMode;
  setMode: (mode: CostMode) => void;
  bounds: QueryBounds | null;
  offset: number;
  fromDay: string;
  toDay: string;
}

const ViewContext = createContext<ViewState | null>(null);

export function useView(): ViewState {
  const value = useContext(ViewContext);
  if (!value) throw new Error("useView 只能在登录后的壳里用");
  return value;
}

/** 带上当前区间 / 费用口径，换页不丢日期。 */
export function useHref(): (next: Route) => string {
  const view = useView();
  const query = {
    preset: view.preset,
    mode: view.mode,
    fromDay: view.fromDay,
    toDay: view.toDay,
  };
  return (next) => buildHash(next, query);
}

/** 区间与费用口径跟 hash 查询串走，换页不丢。 */
export function ViewProvider({ route, hash, children }: { route: Route; hash: string; children: ReactNode }) {
  const [now] = useState(() => new Date());
  const offset = useMemo(() => localOffsetMinutes(now), [now]);
  const parsed = parseViewQuery(hash);
  const preset = parsed.preset ?? "30d";
  const mode = parsed.mode ?? "unified";
  const fallback = presetRange(preset === "custom" ? "30d" : preset, now, offset);
  const fromDay = preset === "custom" ? (parsed.fromDay ?? fallback.fromDay) : fallback.fromDay;
  const toDay = preset === "custom" ? (parsed.toDay ?? fallback.toDay) : fallback.toDay;
  const bounds = useMemo(() => queryBounds({ fromDay, toDay }, offset), [fromDay, toDay, offset]);

  function write(next: { preset: RangePreset; mode: CostMode; fromDay: string; toDay: string }) {
    const nextHash = buildHash(route, next);
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  }

  const value: ViewState = {
    preset,
    setPreset: (next) => write({ preset: next, mode, fromDay, toDay }),
    custom: { fromDay, toDay },
    setCustom: (range) => write({ preset: "custom", mode, fromDay: range.fromDay, toDay: range.toDay }),
    mode,
    setMode: (next) => write({ preset, mode: next, fromDay, toDay }),
    bounds,
    offset,
    fromDay,
    toDay,
  };

  return <ViewContext.Provider value={value}>{children}</ViewContext.Provider>;
}
