import { describe, expect, it } from "vitest";
import { SESSION_ENTRY_COPY, conversationFocusFromSession } from "./sessionEntryCopy";

describe("SESSION_ENTRY_COPY — no-body and same-conversation affordances", () => {
  it("states Cursor 会话 page has no conversation body", () => {
    expect(SESSION_ENTRY_COPY.cursorSessionsBanner).toContain("没有对话正文");
    expect(SESSION_ENTRY_COPY.cursorSessionsBanner).toContain("对话记录");
    expect(SESSION_ENTRY_COPY.cursorSessionsEmptyTitle).toContain("Cursor 会话");
    expect(SESSION_ENTRY_COPY.cursorSessionsEmptyHint).toContain("没有对话正文");
    expect(SESSION_ENTRY_COPY.cursorSessionsTableNote).toContain("对话记录");
  });

  it("points rows and timeline bars at the same 对话记录", () => {
    expect(SESSION_ENTRY_COPY.openConversationRow).toBe("打开对话记录");
    expect(SESSION_ENTRY_COPY.workTimelineBanner).toContain("对话记录");
    expect(SESSION_ENTRY_COPY.conversationCatalogNote).toContain("Cursor 会话");
    expect(SESSION_ENTRY_COPY.conversationCatalogNote).toContain("不含正文");
    expect(SESSION_ENTRY_COPY.behaviorTabNote).toContain("Cursor 会话");
    expect(SESSION_ENTRY_COPY.behaviorTabNote).toContain("完整事件");
  });
});

describe("conversationFocusFromSession", () => {
  it("maps a Cursor session row to conversation focus for one-click open", () => {
    expect(conversationFocusFromSession({ id: "abc", source: "cursor_agent" })).toEqual({
      source: "cursor_agent",
      session_id: "abc",
    });
  });

  it("clears focus when opening the catalog without a session", () => {
    expect(conversationFocusFromSession()).toBeNull();
  });
});
