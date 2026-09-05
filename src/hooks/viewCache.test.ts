import { describe, expect, it } from "vitest";
import type { Filter, View } from "../types";
import {
  emptyViewScope,
  filtersEqual,
  hashBelongsToView,
  hashForConversation,
  hashForWorktime,
  initialViewScopes,
  isViewFresh,
  isViewStampedForScope,
  parseConversationFocus,
  parseViewHash,
  parseWorktimeDay,
  conversationFocusToRestore,
  reconcileLoadedStamps,
  syncSharedFilters,
  viewStamp,
  views,
  viewsInvalidatedBy,
  viewsWarmedBy,
  worktimeHashForDay,
} from "./viewCache";

const filter: Filter = {
  from: null,
  to: null,
  sources: [],
  models: [],
  projects: [],
  providers: [],
};

const ranged: Filter = { ...filter, from: "2026-08-01", to: "2026-08-07" };

describe("emptyViewScope", () => {
  it("defaults to the last 7 days instead of all history", () => {
    const scope = emptyViewScope();
    expect(scope.preset).toBe("7");
    expect(scope.filter.from).toBeTruthy();
    expect(scope.filter.to).toBeTruthy();
    expect(Date.parse(scope.filter.to!) - Date.parse(scope.filter.from!)).toBe(
      7 * 24 * 3600 * 1000,
    );
  });

  it("gives every view the same initial window", () => {
    const scopes = initialViewScopes();
    for (const view of views) {
      expect(scopes[view].preset).toBe("7");
      expect(scopes[view].filter.from).toBe(scopes.overview.filter.from);
      expect(scopes[view].filter.to).toBe(scopes.overview.filter.to);
    }
  });
});

describe("parseViewHash", () => {
  it("maps known view hashes", () => {
    expect(parseViewHash("#sessions")).toBe("conversations");
    expect(parseViewHash("#conversations")).toBe("conversations");
    expect(parseViewHash("source")).toBe("application");
    expect(parseViewHash("#instructions")).toBe("instructions");
  });

  it("keeps settings panel anchors on the settings view", () => {
    expect(parseViewHash("#settings")).toBe("settings");
    expect(parseViewHash("#settings-budget")).toBe("settings");
    expect(parseViewHash("settings-diagnostics")).toBe("settings");
    expect(parseViewHash("#settings-appearance")).toBe("settings");
    expect(parseViewHash("#settings-unpriced")).toBe("settings");
  });

  it("keeps conversation session and worktime day hashes on their views", () => {
    expect(parseViewHash("#conversations/claude/abc")).toBe("conversations");
    expect(parseViewHash("conversations/cursor_agent/sess%2F1")).toBe("conversations");
    expect(parseViewHash("#worktime/2026-08-22")).toBe("worktime");
    expect(parseViewHash("worktime/2026-08-22")).toBe("worktime");
  });

  it("falls back to overview for unknown hashes", () => {
    expect(parseViewHash("#nope")).toBe("overview");
  });
});

describe("conversation and worktime hashes", () => {
  it("round-trips a conversation session", () => {
    const hash = hashForConversation("claude", "sess-1");
    expect(hash).toBe("conversations/claude/sess-1");
    expect(parseConversationFocus(`#${hash}`)).toEqual({
      source: "claude",
      session_id: "sess-1",
    });
  });

  it("encodes slashes in session ids", () => {
    const hash = hashForConversation("cursor_agent", "a/b");
    expect(hash).toBe("conversations/cursor_agent/a%2Fb");
    expect(parseConversationFocus(hash)).toEqual({
      source: "cursor_agent",
      session_id: "a/b",
    });
  });

  it("ignores incomplete conversation hashes", () => {
    expect(parseConversationFocus("#conversations")).toBeNull();
    expect(parseConversationFocus("#conversations/claude")).toBeNull();
    expect(parseConversationFocus("#conversations/claude/")).toBeNull();
  });

  it("round-trips a worktime day", () => {
    expect(hashForWorktime("2026-08-22")).toBe("worktime/2026-08-22");
    expect(parseWorktimeDay("#worktime/2026-08-22")).toBe("2026-08-22");
    expect(parseWorktimeDay("#worktime")).toBeNull();
    expect(parseWorktimeDay("#worktime/2026-13-01")).toBeNull();
  });

  it("maps a heatmap day to a worktime hash and rejects invalid dates", () => {
    expect(worktimeHashForDay("2026-08-22")).toBe("worktime/2026-08-22");
    expect(worktimeHashForDay("2026-13-01")).toBeNull();
    expect(worktimeHashForDay("nope")).toBeNull();
  });

  it("treats nested hashes as belonging to the current view", () => {
    expect(hashBelongsToView("#conversations/claude/abc", "conversations")).toBe(true);
    expect(hashBelongsToView("#worktime/2026-08-22", "worktime")).toBe(true);
    expect(hashBelongsToView("#settings-budget", "settings")).toBe(true);
    expect(hashBelongsToView("#overview", "conversations")).toBe(false);
  });

  it("restores a conversation session from hash after live focus was consumed", () => {
    expect(conversationFocusToRestore(null, "#conversations/claude/abc")).toEqual({
      source: "claude",
      session_id: "abc",
    });
  });
});

describe("viewStamp", () => {
  it("keeps model/provider/project stamps stable across grain changes", () => {
    const day = viewStamp("model", filter, "all", "day", 1);
    const week = viewStamp("model", filter, "all", "week", 1);
    expect(day).toBe(week);
    expect(viewStamp("trend", filter, "all", "day", 1)).not.toBe(
      viewStamp("trend", filter, "all", "week", 1),
    );
  });

  it("invalidates every view when ingest epoch bumps", () => {
    const before = viewStamp("trend", filter, "all", "day", 1);
    const after = viewStamp("trend", filter, "all", "day", 2);
    expect(before).not.toBe(after);
  });
});

describe("filtersEqual", () => {
  it("treats the same membership as equal regardless of order", () => {
    expect(
      filtersEqual({ ...filter, projects: ["a", "b"] }, { ...filter, projects: ["b", "a"] }),
    ).toBe(true);
    expect(filtersEqual(filter, ranged)).toBe(false);
  });
});

describe("syncSharedFilters", () => {
  it("copies shared filters to every view and usage sources to non-conversation views", () => {
    const scopes = initialViewScopes();
    scopes.overview = {
      filter: {
        ...filter,
        from: "2026-08-01",
        to: "2026-08-07",
        models: ["gpt-5"],
        providers: ["openai"],
      },
      preset: "7",
    };
    scopes.trend = {
      filter: { ...filter, sources: ["old"], projects: ["/old"], models: ["old-model"] },
      preset: "all",
    };

    const next = syncSharedFilters(
      scopes,
      {
        ...filter,
        from: "2026-08-01",
        to: "2026-08-07",
        models: ["gpt-5"],
        providers: ["openai"],
        sources: ["claude"],
        projects: ["/workspace/app"],
      },
      "7",
      "overview",
    );

    expect(next.overview.filter).toEqual({
      ...filter,
      from: "2026-08-01",
      to: "2026-08-07",
      models: ["gpt-5"],
      providers: ["openai"],
      sources: ["claude"],
      projects: ["/workspace/app"],
    });
    expect(next.overview.preset).toBe("7");
    expect(next.trend.filter).toEqual({
      ...filter,
      from: "2026-08-01",
      to: "2026-08-07",
      models: ["gpt-5"],
      providers: ["openai"],
      sources: ["claude"],
      projects: ["/workspace/app"],
    });
    expect(next.trend.preset).toBe("7");
    expect(next.conversations.filter.sources).toEqual([]);
    expect(next.conversations.filter.projects).toEqual(["/workspace/app"]);
    expect(next.conversations.filter.models).toEqual(["gpt-5"]);
    expect(next.conversations.filter.providers).toEqual(["openai"]);
    expect(next.conversations.preset).toBe("7");
  });

  it("keeps usage sources unchanged when the conversation source filter changes", () => {
    const scopes = initialViewScopes();
    scopes.overview = {
      filter: { ...filter, sources: ["codex"], models: ["old-model"] },
      preset: "all",
    };

    const next = syncSharedFilters(
      scopes,
      {
        ...filter,
        sources: ["cursor_agent"],
        projects: ["/workspace/app"],
        models: ["gpt-5"],
        providers: ["openai"],
      },
      "7",
      "conversations",
    );

    expect(next.conversations.filter.sources).toEqual(["cursor_agent"]);
    expect(next.conversations.filter.projects).toEqual(["/workspace/app"]);
    expect(next.conversations.filter.models).toEqual(["gpt-5"]);
    expect(next.conversations.preset).toBe("7");
    expect(next.overview.filter.sources).toEqual(["codex"]);
    expect(next.overview.filter.projects).toEqual(["/workspace/app"]);
    expect(next.overview.filter.models).toEqual(["gpt-5"]);
    expect(next.overview.filter.providers).toEqual(["openai"]);
    expect(next.overview.preset).toBe("7");
  });

  it("returns the same object when shared filters already match", () => {
    const scopes = initialViewScopes();
    expect(
      syncSharedFilters(scopes, scopes.overview.filter, scopes.overview.preset, "overview"),
    ).toBe(scopes);
  });
});

describe("viewsWarmedBy", () => {
  it("marks trend/model/project warm after overview", () => {
    expect(viewsWarmedBy("overview")).toEqual(["overview", "trend", "model", "project"]);
    expect(viewsWarmedBy("trend")).toEqual(["trend"]);
  });
});

describe("viewsInvalidatedBy", () => {
  it("invalidates shared datasets written by the current view", () => {
    expect(viewsInvalidatedBy("overview")).toEqual(["trend", "model", "project"]);
    expect(viewsInvalidatedBy("conversations")).toEqual([]);
    expect(viewsInvalidatedBy("trend")).toEqual(["overview"]);
  });
});

describe("reconcileLoadedStamps", () => {
  it("warms sibling views only when their filters still match", () => {
    const scopes = initialViewScopes();
    const used = scopes.overview;
    const loaded = reconcileLoadedStamps({}, "overview", used, scopes, "day", 1);

    expect(loaded.overview).toBe(viewStamp("overview", used.filter, used.preset, "day", 1));
    expect(loaded.trend).toBe(viewStamp("trend", used.filter, used.preset, "day", 1));
    expect(loaded.project).toBe(viewStamp("project", used.filter, used.preset, "day", 1));
  });

  it("does not leak one view's filter into another view's cache stamp", () => {
    const scopes = initialViewScopes();
    scopes.project = { filter: ranged, preset: "7" };
    const used = emptyViewScope();
    const loaded = reconcileLoadedStamps(
      {
        project: viewStamp("project", ranged, "7", "day", 1),
      },
      "overview",
      used,
      scopes,
      "day",
      1,
    );

    expect(loaded.overview).toBe(viewStamp("overview", used.filter, used.preset, "day", 1));
    expect(loaded.project).toBeUndefined();
    expect(isViewFresh(loaded, "project", ranged, "7", "day", 1)).toBe(false);
  });

  it("invalidates overview when a sibling overwrites shared data with a different filter", () => {
    const scopes = initialViewScopes();
    const overviewScope = scopes.overview;
    const projectScope = { filter: ranged, preset: "7" };
    scopes.project = projectScope;
    const afterOverview = reconcileLoadedStamps({}, "overview", overviewScope, scopes, "day", 1);
    const afterProject = reconcileLoadedStamps(
      afterOverview,
      "project",
      projectScope,
      scopes,
      "day",
      1,
    );

    expect(afterProject.project).toBe(viewStamp("project", ranged, "7", "day", 1));
    expect(afterProject.overview).toBeUndefined();
  });
});

describe("isViewFresh", () => {
  it("hits after overview warm and misses after filter change", () => {
    const loaded: Partial<Record<View, string>> = {};
    for (const view of viewsWarmedBy("overview")) {
      loaded[view] = viewStamp(view, filter, "all", "day", 1);
    }
    expect(isViewFresh(loaded, "trend", filter, "all", "day", 1)).toBe(true);
    expect(isViewFresh(loaded, "model", filter, "all", "week", 1)).toBe(true);
    expect(isViewFresh(loaded, "trend", { ...filter, from: "2026-08-01" }, "all", "day", 1)).toBe(
      false,
    );
  });
});

describe("isViewStampedForScope", () => {
  it("returns false when the view has never been stamped", () => {
    expect(isViewStampedForScope({}, "overview", filter, "all", 1)).toBe(false);
  });

  it("matches the same filter, preset, and epoch across grains", () => {
    const loaded = { overview: viewStamp("overview", filter, "all", "day", 1) };
    expect(isViewStampedForScope(loaded, "overview", filter, "all", 1)).toBe(true);
    expect(isViewFresh(loaded, "overview", filter, "all", "week", 1)).toBe(false);
  });

  it("returns false when the data epoch differs", () => {
    const loaded = { overview: viewStamp("overview", filter, "all", "day", 1) };
    expect(isViewStampedForScope(loaded, "overview", filter, "all", 2)).toBe(false);
  });

  it("returns false when the preset differs", () => {
    const loaded = { overview: viewStamp("overview", filter, "7", "day", 1) };
    expect(isViewStampedForScope(loaded, "overview", filter, "all", 1)).toBe(false);
  });

  it("treats filter set membership as equal regardless of order", () => {
    const stamped = {
      ...filter,
      sources: ["claude", "codex"],
      models: ["opus", "gpt"],
      projects: ["/a", "/b"],
      providers: ["anthropic", "openai"],
    };
    const shuffled = {
      ...filter,
      sources: ["codex", "claude"],
      models: ["gpt", "opus"],
      projects: ["/b", "/a"],
      providers: ["openai", "anthropic"],
    };
    const loaded = { overview: viewStamp("overview", stamped, "7", "day", 1) };
    expect(isViewStampedForScope(loaded, "overview", shuffled, "7", 1)).toBe(true);
  });

  it("matches an empty filter", () => {
    const empty: Filter = {
      from: null,
      to: null,
      sources: [],
      models: [],
      projects: [],
      providers: [],
    };
    const loaded = { overview: viewStamp("overview", empty, "all", "week", 3) };
    expect(isViewStampedForScope(loaded, "overview", empty, "all", 3)).toBe(true);
  });
});
