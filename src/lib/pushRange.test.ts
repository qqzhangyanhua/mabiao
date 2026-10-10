import { describe, expect, it } from "vitest";
import type { Filter, PushOutcome } from "../types";
import {
  defaultPushPreset,
  formatBytes,
  issueKeys,
  pushOutcomeSummary,
  pushPresetOptions,
  pushRangeFor,
} from "./pushRange";

const EMPTY: Filter = {
  from: null,
  to: null,
  sources: [],
  models: [],
  projects: [],
  providers: [],
};

function outcome(patch: Partial<PushOutcome> = {}): PushOutcome {
  return {
    sessions_succeeded: 3,
    failed: [],
    skipped: [],
    usage_inserted: 10,
    usage_duplicates: 2,
    usage_skipped_invalid_time: 0,
    usage_error: null,
    login_required: false,
    ...patch,
  };
}

describe("pushRangeFor", () => {
  const now = new Date(2026, 8, 10, 15, 30, 45);

  it("builds yesterday as a whole local day without milliseconds", () => {
    const range = pushRangeFor("yesterday", EMPTY, now);
    expect(range.from).toBe(new Date(2026, 8, 9, 0, 0, 0).toISOString().replace(".000Z", "Z"));
    expect(range.to).toBe(new Date(2026, 8, 9, 23, 59, 59).toISOString().replace(".000Z", "Z"));
    expect(range.from).not.toMatch(/\.\d{3}Z$/);
  });

  it("builds rolling windows ending now", () => {
    const week = pushRangeFor("7d", EMPTY, now);
    expect(week.to).toBe(now.toISOString().replace(/\.\d{3}Z$/, "Z"));
    expect(new Date(week.to!).getTime() - new Date(week.from!).getTime()).toBe(7 * 24 * 3600 * 1000);
    const month = pushRangeFor("30d", EMPTY, now);
    expect(new Date(month.to!).getTime() - new Date(month.from!).getTime()).toBe(
      30 * 24 * 3600 * 1000,
    );
  });

  it("carries the topbar sources and range through", () => {
    const filter = { ...EMPTY, from: "2026-09-01T00:00:00Z", to: "2026-09-02T00:00:00Z", sources: ["codex"] };
    expect(pushRangeFor("filter", filter, now)).toEqual({
      from: "2026-09-01T00:00:00Z",
      to: "2026-09-02T00:00:00Z",
      sources: ["codex"],
    });
    expect(pushRangeFor("7d", filter, now).sources).toEqual(["codex"]);
  });
});

describe("push presets", () => {
  it("offers the topbar range only when one is set, and defaults to it", () => {
    expect(pushPresetOptions(EMPTY).map((option) => option.value)).toEqual([
      "yesterday",
      "7d",
      "30d",
    ]);
    expect(defaultPushPreset(EMPTY)).toBe("7d");
    const ranged = { ...EMPTY, from: "2026-09-01T00:00:00Z" };
    expect(pushPresetOptions(ranged)[0].value).toBe("filter");
    expect(defaultPushPreset(ranged)).toBe("filter");
  });
});

describe("push summaries", () => {
  it("summarises a clean run", () => {
    expect(pushOutcomeSummary(outcome())).toBe(
      "已推送 3 场会话，消耗记录新增 10 条，已存在 2 条",
    );
  });

  it("names failures, skips and a usage error", () => {
    const issue = { source: "codex", session_id: "s1", title: "t", reason: "r" };
    const text = pushOutcomeSummary(
      outcome({ failed: [issue], skipped: [issue, issue], usage_error: "boom" }),
    );
    expect(text).toBe("已推送 3 场会话，1 场失败，2 场跳过，消耗记录失败");
  });

  it("turns issues into retry keys", () => {
    expect(issueKeys([{ source: "codex", session_id: "s1", title: "t", reason: "r" }])).toEqual([
      { source: "codex", session_id: "s1" },
    ]);
  });

  it("formats sizes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
  });
});
