import type { TokenParts, UsageComposition } from "../api/types";
import { cacheHitRate, type TokenSplit } from "./tokens";

export function asSplit(parts: TokenParts): TokenSplit {
  return {
    input: parts.input,
    output: parts.output,
    cache_read: parts.cache_read,
    cache_creation: parts.cache_creation,
    reasoning: parts.reasoning,
  };
}

export function compositionSplits(data: UsageComposition | undefined): {
  modelSplits: Map<string, TokenSplit> | undefined;
  sourceSplits: Map<string, TokenSplit> | undefined;
  projectSplits: Map<string, TokenSplit> | undefined;
  cacheHit: number | null;
} {
  if (!data) {
    return { modelSplits: undefined, sourceSplits: undefined, projectSplits: undefined, cacheHit: null };
  }
  const map = (rows: UsageComposition["by_model"]) => new Map(rows.map((row) => [row.key, asSplit(row)]));
  return {
    modelSplits: map(data.by_model),
    sourceSplits: map(data.by_source),
    projectSplits: map(data.by_project),
    cacheHit: cacheHitRate(asSplit(data.totals)),
  };
}
