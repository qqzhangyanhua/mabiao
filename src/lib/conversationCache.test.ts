import { describe, expect, it } from "vitest";
import {
  conversationKey,
  pinnedConversationKeys,
  pruneConversationDetails,
  touchConversationOrder,
  type ConversationCacheChild,
} from "./conversationCache";

const children: Record<string, ConversationCacheChild[]> = {
  root: [
    { relationship_id: "r1", key: "child1" },
    { relationship_id: "r2", key: "child2" },
    { relationship_id: "r3", key: null },
  ],
  child1: [{ relationship_id: "r4", key: "grandchild" }],
};
const childrenOf = (key: string) => children[key] ?? [];

describe("conversationKey", () => {
  it("用不可见分隔符拼接，避免来源名里的字符撞车", () => {
    expect(conversationKey({ source: "claude", session_id: "abc" })).not.toBe(
      conversationKey({ source: "claude:abc", session_id: "" }),
    );
  });
});

describe("pinnedConversationKeys", () => {
  it("导航栈上的会话总是保留", () => {
    expect(
      pinnedConversationKeys({
        rootKeys: ["root", "child2"],
        expandedRelationshipIds: [],
        childrenOf,
      }),
    ).toEqual(["root", "child2"]);
  });

  it("沿已展开的关系递归保留子会话", () => {
    expect(
      pinnedConversationKeys({
        rootKeys: ["root"],
        expandedRelationshipIds: ["r1", "r4"],
        childrenOf,
      }),
    ).toEqual(["root", "child1", "grandchild"]);
  });

  it("未展开的关系与无法解析的子会话不占名额", () => {
    expect(
      pinnedConversationKeys({
        rootKeys: ["root"],
        expandedRelationshipIds: ["r3"],
        childrenOf,
      }),
    ).toEqual(["root"]);
  });

  it("关系成环也能停下来", () => {
    expect(
      pinnedConversationKeys({
        rootKeys: ["a"],
        expandedRelationshipIds: ["loop"],
        childrenOf: () => [{ relationship_id: "loop", key: "a" }],
      }),
    ).toEqual(["a"]);
  });
});

describe("pruneConversationDetails", () => {
  const details = { a: 1, b: 2, c: 3, d: 4 };

  it("钉住的条目不受上限影响", () => {
    const result = pruneConversationDetails({
      details,
      order: ["a", "b", "c", "d"],
      pinned: ["a", "b", "c"],
      limit: 1,
    });
    expect(Object.keys(result.details).sort()).toEqual(["a", "b", "c"]);
  });

  it("剩余名额留给最近使用的", () => {
    const result = pruneConversationDetails({
      details,
      order: ["a", "b", "c", "d"],
      pinned: ["a"],
      limit: 3,
    });
    expect(Object.keys(result.details).sort()).toEqual(["a", "c", "d"]);
    expect(result.order).toEqual(["a", "c", "d"]);
  });

  it("已经不在缓存里的键不会被写回顺序表", () => {
    const result = pruneConversationDetails({
      details: { a: 1 },
      order: ["gone", "a"],
      pinned: ["missing"],
      limit: 4,
    });
    expect(result.details).toEqual({ a: 1 });
    expect(result.order).toEqual(["a"]);
  });
});

describe("touchConversationOrder", () => {
  it("把键移到最后而不是重复追加", () => {
    expect(touchConversationOrder(["a", "b", "c"], "b")).toEqual(["a", "c", "b"]);
    expect(touchConversationOrder(["a"], "z")).toEqual(["a", "z"]);
  });
});
