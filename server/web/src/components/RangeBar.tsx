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
import { Card } from "./ui/card";
import { Input } from "./ui/input";
import { Segmented } from "./ui/segmented";

const PRESETS: { id: RangePreset; label: string }[] = [
  { id: "7d", label: "近 7 天" },
  { id: "30d", label: "近 30 天" },
  { id: "month", label: "本月" },
  { id: "custom", label: "自定义" },
];

const COST_OPTIONS = (Object.keys(COST_MODE_LABEL) as CostMode[]).map((id) => ({
  id,
  label: COST_MODE_LABEL[id],
}));

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
  fromDay: string;
  toDay: string;
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
  return { preset, setPreset, custom, setCustom, mode, setMode, bounds, offset, fromDay, toDay };
}

export function RangeBar({ view }: { view: ViewState }) {
  return (
    <Card className="flex flex-wrap items-center gap-4 px-4 py-3">
      <Segmented
        label="时间范围"
        value={view.preset}
        onChange={view.setPreset}
        options={PRESETS}
      />
      {view.preset === "custom" && (
        <div className="flex items-center gap-2 text-sm">
          <Input
            type="date"
            aria-label="开始日期"
            value={view.custom.fromDay}
            onChange={(e) => view.setCustom({ ...view.custom, fromDay: e.target.value })}
            className="w-auto"
          />
          <span className="text-muted-foreground">至</span>
          <Input
            type="date"
            aria-label="结束日期"
            value={view.custom.toDay}
            onChange={(e) => view.setCustom({ ...view.custom, toDay: e.target.value })}
            className="w-auto"
          />
        </div>
      )}
      <div className="ml-auto flex items-center gap-2">
        <span className="text-sm text-muted-foreground">费用口径</span>
        <Segmented
          label="费用口径"
          value={view.mode}
          onChange={view.setMode}
          options={COST_OPTIONS}
        />
      </div>
    </Card>
  );
}
