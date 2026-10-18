import type { ConversationIndexProgressDto, ConversationSessionRow } from "../types";

export const BODY_SEARCH_MIN_CHARS = 3;
export const CATALOG_PAGE_SIZE = 20;

export type ConversationCatalogItem =
  | { type: "heading"; field: "title" | "body" }
  | { type: "row"; row: ConversationSessionRow };

export function conversationCatalogItems(
  rows: ConversationSessionRow[],
  searching: boolean,
): ConversationCatalogItem[] {
  if (!searching) {
    return rows.map((row) => ({ type: "row" as const, row }));
  }
  const items: ConversationCatalogItem[] = [];
  let lastField: "title" | "body" | null = null;
  for (const row of rows) {
    const field = row.match_field ?? null;
    if (field && field !== lastField) {
      items.push({ type: "heading", field });
      lastField = field;
    }
    items.push({ type: "row", row });
  }
  return items;
}

export function conversationIndexIncomplete(
  progress: ConversationIndexProgressDto | null,
): boolean {
  return progress != null && progress.total > 0 && progress.indexed < progress.total;
}

/** 本机不存会话正文，对话目录按设计为空。 */
export const BODYLESS_CONVERSATION_SOURCES = ["aider", "warp", "windsurf"] as const;

export function isBodylessConversationFilter(sources: string[]): boolean {
  return (
    sources.length > 0 &&
    sources.every((source) =>
      (BODYLESS_CONVERSATION_SOURCES as readonly string[]).includes(source),
    )
  );
}

export function conversationCatalogEmptyCopy({
  searching,
  query,
  indexIncomplete,
  sources = [],
}: {
  searching: boolean;
  query: string;
  indexIncomplete: boolean;
  sources?: string[];
}): { title: string; hint: string } {
  if (!searching && isBodylessConversationFilter(sources)) {
    return {
      title: "该来源没有本机会话正文",
      hint: "Aider / Warp / Windsurf 只记消耗，本机不存对话正文，目录为空是预期。",
    };
  }
  if (!searching) {
    return {
      title: "当前条件下暂无对话记录",
      hint: "请确认本机已有会话文件，并执行一次刷新。Cursor 与其它来源共用此目录。",
    };
  }
  if (indexIncomplete) {
    return {
      title: "没有标题或正文命中",
      hint: "正文索引仍在补建，未就绪会话目前只搜标题。",
    };
  }
  if ([...query.trim()].length < BODY_SEARCH_MIN_CHARS) {
    return {
      title: "没有标题命中",
      hint: "两字及以下只搜标题。换更长的关键字可搜正文。",
    };
  }
  return {
    title: "没有标题或正文命中",
    hint: "没有匹配的标题或已索引正文。",
  };
}
