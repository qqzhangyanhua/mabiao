import type { UsageRecord } from "../api/types";
import { addSplit, emptySplit, type TokenSplit } from "./tokens";

function fromRecord(record: UsageRecord): TokenSplit {
  return {
    input: record.tokens.input,
    output: record.tokens.output,
    cache_read: record.tokens.cache_read,
    cache_creation: record.tokens.cache_creation,
    reasoning: record.tokens.reasoning,
  };
}

export function totalSplit(records: UsageRecord[]): TokenSplit {
  return records.reduce((sum, record) => addSplit(sum, fromRecord(record)), emptySplit());
}

export function splitsBy(records: UsageRecord[], keyOf: (record: UsageRecord) => string): Map<string, TokenSplit> {
  const map = new Map<string, TokenSplit>();
  for (const record of records) {
    const key = keyOf(record);
    map.set(key, addSplit(map.get(key) ?? emptySplit(), fromRecord(record)));
  }
  return map;
}

/** 模型按 `key`（模型名），项目按 `id`。 */
export function splitForRow(
  splits: Map<string, TokenSplit> | undefined,
  row: { key: string; id: number | null },
): TokenSplit | undefined {
  if (!splits) return undefined;
  return splits.get(row.key) ?? (row.id === null ? undefined : splits.get(String(row.id)));
}
