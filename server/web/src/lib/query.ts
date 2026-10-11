import type { CostMode } from "./cost";
import type { RangePreset } from "./range";

export interface ViewQuery {
  preset: RangePreset;
  mode: CostMode;
  fromDay: string;
  toDay: string;
}

const PRESETS: RangePreset[] = ["7d", "30d", "month", "custom"];
const MODES: CostMode[] = ["unified", "snapshot"];

function isPreset(value: string | null): value is RangePreset {
  return value !== null && (PRESETS as string[]).includes(value);
}

function isMode(value: string | null): value is CostMode {
  return value !== null && (MODES as string[]).includes(value);
}

/** hash 里 `?` 之后的查询串。路径解析见 `parseRoute`。 */
export function hashSearch(hash: string): URLSearchParams {
  const raw = hash.replace(/^#/, "");
  const index = raw.indexOf("?");
  return new URLSearchParams(index === -1 ? "" : raw.slice(index + 1));
}

export function parseViewQuery(hash: string): Partial<ViewQuery> {
  const params = hashSearch(hash);
  const next: Partial<ViewQuery> = {};
  const range = params.get("range");
  if (isPreset(range)) next.preset = range;
  const mode = params.get("mode");
  if (isMode(mode)) next.mode = mode;
  const fromDay = params.get("from");
  const toDay = params.get("to");
  if (fromDay) next.fromDay = fromDay;
  if (toDay) next.toDay = toDay;
  return next;
}

export function viewQueryString(query: ViewQuery): string {
  const params = new URLSearchParams();
  params.set("range", query.preset);
  params.set("mode", query.mode);
  if (query.preset === "custom") {
    if (query.fromDay) params.set("from", query.fromDay);
    if (query.toDay) params.set("to", query.toDay);
  }
  return params.toString();
}
