import { lazy, type ComponentType, type LazyExoticComponent } from "react";

function namedLazy<P extends object>(
  factory: () => Promise<Record<string, ComponentType<P>>>,
  exportName: string,
): LazyExoticComponent<ComponentType<P>> {
  return lazy(async () => {
    const module = await factory();
    const component = module[exportName] as ComponentType<P> | undefined;
    if (!component) {
      throw new Error(`lazyViews: export "${exportName}" not found`);
    }
    return { default: component };
  });
}

export const LazyOverview = namedLazy(() => import("../components/Overview"), "Overview");
export const LazyTrend = namedLazy(() => import("../components/Trend"), "Trend");
export const LazyApplicationAnalytics = namedLazy(
  () => import("../components/ApplicationAnalytics"),
  "ApplicationAnalytics",
);
export const LazyBreakdown = namedLazy(() => import("../components/Breakdown"), "Breakdown");
export const LazyConversations = namedLazy(
  () => import("../components/Conversations"),
  "Conversations",
);
export const LazySettings = namedLazy(() => import("../components/Settings"), "Settings");
export const LazyCursorAccountUsagePanel = namedLazy(
  () => import("../components/CursorAccountUsagePanel"),
  "CursorAccountUsagePanel",
);
export const LazyCursorPanel = namedLazy(() => import("../components/CursorPanel"), "CursorPanel");
export const LazyCursorSessionPanel = namedLazy(
  () => import("../components/CursorSessionPanel"),
  "CursorSessionPanel",
);
export const LazyGlobalInstructionPanel = namedLazy(
  () => import("../components/GlobalInstructionPanel"),
  "GlobalInstructionPanel",
);
export const LazyWorkTimeline = namedLazy(
  () => import("../components/WorkTimeline"),
  "WorkTimeline",
);
export const LazyWorkNotes = namedLazy(
  () => import("../components/WorkNotesPanel"),
  "WorkNotesPanel",
);
