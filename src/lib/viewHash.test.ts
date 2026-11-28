import { describe, expect, it } from "vitest";
import {
  conversationFocusToRestore,
  hashBelongsToView,
  hashForConversation,
  hashForWorktime,
  parseConversationFocus,
  parseViewHash,
  parseWorktimeDay,
  stripLocationHash,
  views,
  worktimeHashForDay,
} from "./viewHash";

describe("parseViewHash", () => {
  it("maps known view hashes and legacy aliases", () => {
    expect(parseViewHash("#sessions")).toBe("conversations");
    expect(parseViewHash("#conversations")).toBe("conversations");
    expect(parseViewHash("source")).toBe("application");
    expect(parseViewHash("#instructions")).toBe("instructions");
    expect(stripLocationHash("#overview")).toBe("overview");
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

  it("falls back to overview for unknown hashes and registers all 13 views", () => {
    expect(parseViewHash("#nope")).toBe("overview");
    expect(views).toHaveLength(13);
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

  it("ignores incomplete or malformed conversation hashes", () => {
    expect(parseConversationFocus("#conversations")).toBeNull();
    expect(parseConversationFocus("#conversations/claude")).toBeNull();
    expect(parseConversationFocus("#conversations/claude/")).toBeNull();
    expect(parseConversationFocus("#conversations/claude/%E0%A4%A")).toBeNull();
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
    expect(
      conversationFocusToRestore({ source: "codex", session_id: "live-1" }, "#conversations/claude/abc"),
    ).toEqual({
      source: "codex",
      session_id: "live-1",
    });
  });
});
