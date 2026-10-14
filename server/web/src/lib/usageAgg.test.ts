import { describe, expect, it } from "vitest";
import type { UsageRecord } from "../api/types";
import { splitForRow, splitsBy, totalSplit } from "./usageAgg";

function record(partial: Partial<UsageRecord> & Pick<UsageRecord, "id" | "model">): UsageRecord {
  return {
    account_id: 1,
    account: "alice",
    occurred_at: "2026-03-01T00:00:00Z",
    source: "codex",
    provider: "openai",
    project: "/w/alpha",
    project_id: 11,
    session_id: "s",
    session_pk: null,
    tokens: { input: 10, output: 20, cache_read: 5, cache_creation: 0, reasoning: 0, total: 35 },
    native_cost: null,
    cost_snapshot: 1,
    pricing_source: "snapshot",
    unified_cost: 1,
    unified_pricing_source: "snapshot",
    unified_cost_source: "snapshot",
    ...partial,
  };
}

describe("totalSplit / splitsBy", () => {
  const rows = [
    record({ id: 1, model: "a", tokens: { input: 10, output: 0, cache_read: 0, cache_creation: 0, reasoning: 0, total: 10 } }),
    record({ id: 2, model: "b", tokens: { input: 0, output: 4, cache_read: 6, cache_creation: 0, reasoning: 0, total: 10 } }),
    record({ id: 3, model: "a", tokens: { input: 5, output: 0, cache_read: 0, cache_creation: 0, reasoning: 1, total: 6 } }),
  ];

  it("sums every record", () => {
    expect(totalSplit(rows)).toEqual({
      input: 15,
      output: 4,
      cache_read: 6,
      cache_creation: 0,
      reasoning: 1,
    });
  });

  it("groups by the caller key", () => {
    const byModel = splitsBy(rows, (row) => row.model);
    expect(byModel.get("a")?.input).toBe(15);
    expect(byModel.get("b")?.cache_read).toBe(6);
  });
});

describe("splitForRow", () => {
  it("prefers the breakdown key then falls back to id", () => {
    const splits = new Map([
      ["11", { input: 3, output: 0, cache_read: 0, cache_creation: 0, reasoning: 0 }],
    ]);
    expect(splitForRow(splits, { key: "git:alpha", id: 11 })?.input).toBe(3);
    expect(splitForRow(splits, { key: "missing", id: 9 })).toBeUndefined();
  });
});
