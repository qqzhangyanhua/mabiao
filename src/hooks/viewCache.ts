import type { ConversationFocus, Filter, Grain, View } from "../types";
import { parseDateValue } from "../lib/calendar";
import { rangeFromPreset } from "../lib/format";
import { emptyFilter } from "./usage/constants";

export const views: View[] = [
  "overview",
  "trend",
  "application",
  "model",
  "provider",
  "project",
  "conversations",
  "cursor",
  "cursor-sessions",
  "worktime",
  "instructions",
  "settings",
];

export type ViewScope = {
  filter: Filter;
  preset: string;
};

const DEFAULT_RANGE_PRESET = "7";

export function emptyViewScope(): ViewScope {
  return {
    filter: {
      ...emptyFilter,
      ...rangeFromPreset(DEFAULT_RANGE_PRESET),
      sources: [],
      models: [],
      projects: [],
      providers: [],
    },
    preset: DEFAULT_RANGE_PRESET,
  };
}

export function initialViewScopes(): Record<View, ViewScope> {
  const seed = emptyViewScope();
  const scopes = {} as Record<View, ViewScope>;
  for (const view of views) {
    scopes[view] = {
      preset: seed.preset,
      filter: {
        ...seed.filter,
        sources: [],
        models: [],
        projects: [],
        providers: [],
      },
    };
  }
  return scopes;
}

function sameItems(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  const other = new Set(right);
  return left.every((item) => other.has(item));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

/** Topbar 筛选跨页共用。用量来源不覆盖对话记录来源：Cursor Agent 往往没有消耗记录。 */
export function syncSharedFilters(
  scopes: Record<View, ViewScope>,
  filter: Filter,
  preset: string,
  origin: View,
): Record<View, ViewScope> {
  let changed = false;
  const next = { ...scopes };
  for (const view of views) {
    const scope = next[view];
    const nextSources =
      view === "conversations"
        ? origin === "conversations"
          ? filter.sources
          : scope.filter.sources
        : origin === "conversations"
          ? scope.filter.sources
          : filter.sources;
    const nextScope: ViewScope = {
      preset,
      filter: { ...filter, sources: nextSources },
    };
    if (scopesEqual(scope, nextScope)) {
      continue;
    }
    changed = true;
    next[view] = nextScope;
  }
  return changed ? next : scopes;
}

export function filtersEqual(left: Filter, right: Filter): boolean {
  return (
    left.from === right.from &&
    left.to === right.to &&
    sameItems(left.sources, right.sources) &&
    sameItems(left.models, right.models) &&
    sameItems(left.projects, right.projects) &&
    sameItems(left.providers, right.providers)
  );
}

export function scopesEqual(left: ViewScope, right: ViewScope): boolean {
  return left.preset === right.preset && filtersEqual(left.filter, right.filter);
}

export function stripLocationHash(raw: string): string {
  return raw.replace(/^#/, "");
}

export function replaceLocationHash(next: string): void {
  if (stripLocationHash(window.location.hash) !== next) {
    window.history.replaceState(null, "", `#${next}`);
  }
}

export function hashForConversation(source: string, sessionId: string): string {
  return `conversations/${encodeURIComponent(source)}/${encodeURIComponent(sessionId)}`;
}

export function parseConversationFocus(raw: string): ConversationFocus | null {
  const hash = stripLocationHash(raw);
  if (!hash.startsWith("conversations/")) {
    return null;
  }
  const rest = hash.slice("conversations/".length);
  const slash = rest.indexOf("/");
  if (slash <= 0 || slash === rest.length - 1) {
    return null;
  }
  try {
    const source = decodeURIComponent(rest.slice(0, slash));
    const session_id = decodeURIComponent(rest.slice(slash + 1));
    if (!source || !session_id) {
      return null;
    }
    return { source, session_id };
  } catch {
    return null;
  }
}

export function conversationFocusToRestore(
  live: ConversationFocus | null,
  hash: string,
): ConversationFocus | null {
  return live ?? parseConversationFocus(hash);
}

export function hashForWorktime(day: string): string {
  return `worktime/${day}`;
}

export function parseWorktimeDay(raw: string): string | null {
  const hash = stripLocationHash(raw);
  if (!hash.startsWith("worktime/")) {
    return null;
  }
  const day = hash.slice("worktime/".length);
  return parseDateValue(day) ? day : null;
}

/** 热力图格子点击：合法日历日才写成工作时间线 hash。 */
export function worktimeHashForDay(day: string): string | null {
  return parseDateValue(day) ? hashForWorktime(day) : null;
}

export function hashBelongsToView(raw: string, view: View): boolean {
  const hash = stripLocationHash(raw);
  if (view === "settings") {
    return hash === "settings" || hash.startsWith("settings-");
  }
  if (view === "conversations") {
    return hash === "conversations" || hash === "sessions" || hash.startsWith("conversations/");
  }
  if (view === "worktime") {
    return hash === "worktime" || hash.startsWith("worktime/");
  }
  return hash === view;
}

export function parseViewHash(raw: string): View {
  const hash = stripLocationHash(raw);
  if (hash === "sessions" || hash === "conversations" || hash.startsWith("conversations/")) {
    return "conversations";
  }
  if (hash === "source") {
    return "application";
  }
  if (hash === "settings" || hash.startsWith("settings-")) {
    return "settings";
  }
  if (hash === "worktime" || hash.startsWith("worktime/")) {
    return "worktime";
  }
  return views.find((item) => item === hash) ?? "overview";
}

export function viewFromHash(): View {
  return parseViewHash(window.location.hash);
}

export function viewStamp(
  view: View,
  filter: Filter,
  preset: string,
  grain: Grain,
  epoch: number,
): string {
  const grainSensitive = view === "overview" || view === "trend" || view === "application";
  return JSON.stringify({
    epoch,
    preset,
    from: filter.from,
    to: filter.to,
    sources: filter.sources,
    models: filter.models,
    projects: filter.projects,
    providers: filter.providers,
    grain: grainSensitive ? grain : "",
  });
}

/** 拉概览时已经带上了趋势、模型、项目，切到这些页不应再打一轮查询。 */
export function viewsWarmedBy(view: View): View[] {
  if (view === "overview") {
    return ["overview", "trend", "model", "project"];
  }
  return [view];
}

/** 本次查询会覆盖这些视图正在展示的共享数据集。 */
export function viewsInvalidatedBy(view: View): View[] {
  switch (view) {
    case "overview":
      return ["trend", "model", "project"];
    case "trend":
    case "model":
    case "project":
      return ["overview"];
    default:
      return [];
  }
}

/**
 * 按本次筛选用过的视图校正缓存戳：
 * - 本次查询筛选用过的视图标为新鲜
 * - 筛选用过且一致的预热页也可以复用
 * - 共享数据被不同筛选覆盖的兄弟页必须失效
 */
export function reconcileLoadedStamps(
  loaded: Partial<Record<View, string>>,
  target: View,
  used: ViewScope,
  scopes: Record<View, ViewScope>,
  grain: Grain,
  epoch: number,
): Partial<Record<View, string>> {
  const next = { ...loaded };
  for (const view of viewsWarmedBy(target)) {
    const scope = view === target ? used : scopes[view];
    if (scopesEqual(scope, used)) {
      next[view] = viewStamp(view, scope.filter, scope.preset, grain, epoch);
    }
  }
  for (const view of viewsInvalidatedBy(target)) {
    if (!scopesEqual(scopes[view], used)) {
      delete next[view];
    }
  }
  return next;
}

export function isViewFresh(
  loaded: Partial<Record<View, string>>,
  view: View,
  filter: Filter,
  preset: string,
  grain: Grain,
  epoch: number,
): boolean {
  return loaded[view] === viewStamp(view, filter, preset, grain, epoch);
}

/** 读出 viewStamp 写入的 JSON，忽略颗粒度。坏戳当从未落过。 */
function readStampScope(raw: string | undefined): {
  epoch: number;
  preset: string;
  filter: Filter;
} | null {
  if (!raw) {
    return null;
  }
  try {
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value)) {
      return null;
    }
    if (typeof value.epoch !== "number" || typeof value.preset !== "string") {
      return null;
    }
    if (value.from !== null && typeof value.from !== "string") {
      return null;
    }
    if (value.to !== null && typeof value.to !== "string") {
      return null;
    }
    if (
      !isStringArray(value.sources) ||
      !isStringArray(value.models) ||
      !isStringArray(value.projects) ||
      !isStringArray(value.providers)
    ) {
      return null;
    }
    return {
      epoch: value.epoch,
      preset: value.preset,
      filter: {
        from: value.from,
        to: value.to,
        sources: value.sources,
        models: value.models,
        projects: value.projects,
        providers: value.providers,
      },
    };
  } catch {
    return null;
  }
}

/** 已按同一筛选、预设、数据 epoch 落过戳（忽略颗粒度）。 */
export function isViewStampedForScope(
  loaded: Partial<Record<View, string>>,
  view: View,
  filter: Filter,
  preset: string,
  epoch: number,
): boolean {
  const scope = readStampScope(loaded[view]);
  return (
    scope !== null &&
    scope.epoch === epoch &&
    scope.preset === preset &&
    filtersEqual(scope.filter, filter)
  );
}
