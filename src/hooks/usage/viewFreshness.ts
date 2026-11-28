import { rangeFromPreset } from "../../lib/format";
import { views } from "../../lib/viewHash";
import type { Filter, Grain, View } from "../../types";
import { emptyFilter } from "./constants";

export type ViewScope = {
  filter: Filter;
  preset: string;
};

export type FreshnessRevisions = {
  data: number;
  pricing: number;
  cursor: number;
};

export type ViewDependency = {
  grain: boolean;
  pricing: boolean;
  cursor: boolean;
};

export type ViewFreshnessEntry = {
  scope: ViewScope;
  grain: Grain;
  revisions: FreshnessRevisions;
};

export type ViewFreshnessMap = Partial<Record<View, ViewFreshnessEntry>>;

export const DEFAULT_RANGE_PRESET = "7";

export const LOCAL_ONLY_VIEWS: ReadonlySet<View> = new Set([
  "conversations",
  "work-notes",
  "cursor",
  "cursor-sessions",
  "worktime",
  "instructions",
  "settings",
]);

export const CURSOR_OVERLAY_VIEWS: readonly View[] = ["overview", "trend", "application", "project"];
export const CURSOR_OVERLAY_VIEW_SET: ReadonlySet<View> = new Set(CURSOR_OVERLAY_VIEWS);

export const VIEW_DEPENDENCIES: Record<View, ViewDependency> = {
  overview: { grain: true, pricing: true, cursor: true },
  trend: { grain: true, pricing: true, cursor: true },
  application: { grain: true, pricing: false, cursor: true },
  model: { grain: false, pricing: true, cursor: false },
  provider: { grain: false, pricing: true, cursor: false },
  project: { grain: false, pricing: true, cursor: true },
  conversations: { grain: false, pricing: false, cursor: false },
  "work-notes": { grain: false, pricing: false, cursor: false },
  cursor: { grain: false, pricing: true, cursor: true },
  "cursor-sessions": { grain: false, pricing: false, cursor: false },
  worktime: { grain: false, pricing: false, cursor: false },
  instructions: { grain: false, pricing: false, cursor: false },
  settings: { grain: false, pricing: true, cursor: false },
};

function sameItems(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  const other = new Set(right);
  return left.every((item) => other.has(item));
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

export function cloneScope(scope: ViewScope): ViewScope {
  return {
    preset: scope.preset,
    filter: {
      ...scope.filter,
      sources: [...scope.filter.sources],
      models: [...scope.filter.models],
      projects: [...scope.filter.projects],
      providers: [...scope.filter.providers],
    },
  };
}

export function createFreshnessEntry(
  scope: ViewScope,
  grain: Grain,
  revisions: FreshnessRevisions,
): ViewFreshnessEntry {
  return {
    scope: cloneScope(scope),
    grain,
    revisions: { ...revisions },
  };
}

export function emptyViewScope(preset = DEFAULT_RANGE_PRESET): ViewScope {
  return {
    filter: {
      ...emptyFilter,
      ...rangeFromPreset(preset),
      sources: [],
      models: [],
      projects: [],
      providers: [],
    },
    preset,
  };
}

export function initialViewScopes(preset = DEFAULT_RANGE_PRESET): Record<View, ViewScope> {
  const seed = emptyViewScope(preset);
  const scopes = {} as Record<View, ViewScope>;
  for (const view of views) {
    scopes[view] = cloneScope(seed);
  }
  return scopes;
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

export function isEntryFresh(
  entry: ViewFreshnessEntry | undefined,
  view: View,
  scope: ViewScope,
  grain: Grain,
  revisions: FreshnessRevisions,
  options?: { ignoreGrain?: boolean },
): boolean {
  if (!entry) {
    return false;
  }
  if (entry.revisions.data !== revisions.data) {
    return false;
  }
  if (!scopesEqual(entry.scope, scope)) {
    return false;
  }
  const deps = VIEW_DEPENDENCIES[view];
  if (!options?.ignoreGrain && deps.grain && entry.grain !== grain) {
    return false;
  }
  if (deps.pricing && entry.revisions.pricing !== revisions.pricing) {
    return false;
  }
  if (deps.cursor && entry.revisions.cursor !== revisions.cursor) {
    return false;
  }
  return true;
}

export function stampHydratedViews(
  freshness: ViewFreshnessMap,
  hydratedViews: ReadonlySet<View>,
  viewScopes: Record<View, ViewScope>,
  target: View,
  used: ViewScope,
  grain: Grain,
  capturedRevisions: FreshnessRevisions,
): { freshness: ViewFreshnessMap; hydratedViews: Set<View> } {
  const nextFreshness: ViewFreshnessMap = { ...freshness };
  const nextHydrated = new Set(hydratedViews);
  nextHydrated.add(target);
  for (const warmed of viewsWarmedBy(target)) {
    const scope = warmed === target ? used : viewScopes[warmed];
    if (scopesEqual(scope, used)) {
      nextFreshness[warmed] = createFreshnessEntry(scope, grain, capturedRevisions);
      nextHydrated.add(warmed);
    }
  }
  for (const invalidated of viewsInvalidatedBy(target)) {
    if (!scopesEqual(viewScopes[invalidated], used)) {
      delete nextFreshness[invalidated];
    }
  }
  return { freshness: nextFreshness, hydratedViews: nextHydrated };
}

export function evictCursorOverlayViews(
  freshness: ViewFreshnessMap,
  hydratedViews: ReadonlySet<View>,
  activeView: View,
): { freshness: ViewFreshnessMap; hydratedViews: Set<View> } {
  const nextFreshness: ViewFreshnessMap = { ...freshness };
  const nextHydrated = new Set(hydratedViews);
  for (const view of [...CURSOR_OVERLAY_VIEWS, "cursor" as const]) {
    delete nextFreshness[view];
    if (view !== activeView) {
      nextHydrated.delete(view);
    }
  }
  return { freshness: nextFreshness, hydratedViews: nextHydrated };
}
