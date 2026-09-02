import { describe, expect, it } from "vitest";
import type { ConversationSessionRow } from "../types";
import {
  conversationCatalogEmptyCopy,
  conversationCatalogItems,
  conversationIndexIncomplete,
} from "./conversationCatalogItems";

function row(
  session_id: string,
  match_field?: ConversationSessionRow["match_field"],
): ConversationSessionRow {
  return {
    source: "codex",
    session_id,
    title: session_id,
    project: "/p",
    model: "m",
    started_at: "",
    ended_at: "",
    source_file: "",
    source_files: [],
    capabilities: [],
    support_status: "experimental",
    file_available: true,
    total_tokens: 0,
    cost: null,
    unpriced: false,
    match_field,
  };
}

describe("conversationCatalogItems", () => {
  it("inserts section headings when searching mixed hits", () => {
    const items = conversationCatalogItems(
      [row("a", "title"), row("b", "body")],
      true,
    );
    expect(items.map((item) => item.type === "heading" ? item.field : item.row.session_id)).toEqual([
      "title",
      "a",
      "body",
      "b",
    ]);
  });

  it("skips headings when not searching", () => {
    expect(conversationCatalogItems([row("a", "title")], false)).toEqual([
      { type: "row", row: row("a", "title") },
    ]);
  });
});

describe("conversationCatalogEmptyCopy", () => {
  it("does not tell users to shorten a query that already cannot search bodies", () => {
    expect(
      conversationCatalogEmptyCopy({
        searching: true,
        query: "权",
        indexIncomplete: false,
      }).hint,
    ).toContain("只搜标题");
    expect(
      conversationCatalogEmptyCopy({
        searching: true,
        query: "authentication",
        indexIncomplete: false,
      }).hint,
    ).toBe("没有匹配的标题或已索引正文。");
  });
});

describe("conversationIndexIncomplete", () => {
  it("is true only while some indexable sessions remain", () => {
    expect(conversationIndexIncomplete({ indexed: 1, total: 2 })).toBe(true);
    expect(conversationIndexIncomplete({ indexed: 2, total: 2 })).toBe(false);
    expect(conversationIndexIncomplete(null)).toBe(false);
  });
});
