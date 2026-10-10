import { useMemo, useState } from "react";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import {
  localOffsetMinutes,
  presetRange,
  queryBounds,
  type DayRange,
  type QueryBounds,
  type RangePreset,
} from "../lib/range";

const PRESETS: { id: RangePreset; label: string }[] = [
  { id: "7d", label: "近 7 天" },
  { id: "30d", label: "近 30 天" },
  { id: "month", label: "本月" },
  { id: "custom", label: "自定义" },
];

export interface ViewState {
  preset: RangePreset;
  setPreset: (preset: RangePreset) => void;
  custom: DayRange;
  setCustom: (range: DayRange) => void;
  mode: CostMode;
  setMode: (mode: CostMode) => void;
  /** 区间不合法（自定义日期没填全或起止颠倒）时为 `null`。 */
  bounds: QueryBounds | null;
  /** 浏览器当前 UTC 偏移（分钟），决定「按天」怎么切日界。 */
  offset: number;
}

/** 区间与费用口径的页面状态。`now` 在挂载时定下，避免每次渲染都换一个区间触发重新请求。 */
export function useViewState(): ViewState {
  const [now] = useState(() => new Date());
  const offset = useMemo(() => localOffsetMinutes(now), [now]);
  const [preset, setPreset] = useState<RangePreset>("30d");
  const [custom, setCustom] = useState<DayRange>(() => presetRange("30d", now, offset));
  const [mode, setMode] = useState<CostMode>("unified");
  const { fromDay, toDay } = preset === "custom" ? custom : presetRange(preset, now, offset);
  const bounds = useMemo(() => queryBounds({ fromDay, toDay }, offset), [fromDay, toDay, offset]);
  return { preset, setPreset, custom, setCustom, mode, setMode, bounds, offset };
}

export function RangeBar({ view }: { view: ViewState }) {
  return (
    <div className="flex flex-wrap items-center gap-4 rounded border border-slate-200 bg-white px-4 py-3 text-sm">
      <div className="flex gap-1" role="group" aria-label="时间范围">
        {PRESETS.map((p) => (
          <button
            key={p.id}
            type="button"
            aria-pressed={view.preset === p.id}
            onClick={() => view.setPreset(p.id)}
            className={`rounded px-3 py-1 ${view.preset === p.id ? "bg-blue-600 text-white" : "bg-slate-100 hover:bg-slate-200"}`}
          >
            {p.label}
          </button>
        ))}
      </div>
      {view.preset === "custom" && (
        <div className="flex items-center gap-2">
          <input
            type="date"
            aria-label="开始日期"
            value={view.custom.fromDay}
            onChange={(e) => view.setCustom({ ...view.custom, fromDay: e.target.value })}
            className="rounded border border-slate-300 px-2 py-1"
          />
          <span>至</span>
          <input
            type="date"
            aria-label="结束日期"
            value={view.custom.toDay}
            onChange={(e) => view.setCustom({ ...view.custom, toDay: e.target.value })}
            className="rounded border border-slate-300 px-2 py-1"
          />
        </div>
      )}
      <div className="ml-auto flex items-center gap-2" role="group" aria-label="费用口径">
        <span className="text-slate-500">费用口径</span>
        {(Object.keys(COST_MODE_LABEL) as CostMode[]).map((mode) => (
          <button
            key={mode}
            type="button"
            aria-pressed={view.mode === mode}
            onClick={() => view.setMode(mode)}
            className={`rounded px-3 py-1 ${view.mode === mode ? "bg-slate-800 text-white" : "bg-slate-100 hover:bg-slate-200"}`}
          >
            {COST_MODE_LABEL[mode]}
          </button>
        ))}
      </div>
    </div>
  );
}
