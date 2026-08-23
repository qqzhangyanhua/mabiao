import { describe, expect, it } from "vitest";
import { conversationKey } from "./conversationCache";

describe("conversationKey", () => {
  it("用不可见分隔符拼接，避免来源名里的字符撞车", () => {
    expect(conversationKey({ source: "claude", session_id: "abc" })).not.toBe(
      conversationKey({ source: "claude:abc", session_id: "" }),
    );
  });
});
