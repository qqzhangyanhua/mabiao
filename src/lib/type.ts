import type { SeriesPoint } from "../types";

export type SettingsTabId =
  "general" | "sources" | "display" | "budget" | "backup" | "cursor" | "pricing";

export type SettingsTab = {
  id: SettingsTabId;
  label: string;
  anchors: readonly string[];
};

export type { OfficialQuotaProviderId } from "./overviewLayout";

export type SourceIconId =
  | "claude"
  | "codex"
  | "grok"
  | "gemini"
  | "kimi"
  | "qwen"
  | "copilot"
  | "opencode"
  | "factory"
  | "pi"
  | "omp"
  | "dsh"
  | "cursor"
  | "unknown";

export type TrendStats = {
  totalTokens: number;
  hasCost: boolean;
  totalCost: number;
  bucketAvg: number;
  peak: SeriesPoint | null;
  sparkTokens: number[];
  sparkCost: number[];
  maxTotal: number;
};

export type TrendTableRow = {
  point: SeriesPoint;
  chronologicalIndex: number;
  shareOfTotal: number;
  periodDelta: number | null;
};
