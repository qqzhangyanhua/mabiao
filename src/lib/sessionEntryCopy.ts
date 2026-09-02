import type { ConversationFocus } from "../types";

export const SESSION_ENTRY_COPY = {
  cursorSessionsBanner:
    "本页只有跨会话行为聚合（轮次、工具、失败率），没有对话正文。点下行可打开「对话记录」里同一条 Cursor Agent 会话。",
  cursorSessionsEmptyTitle: "暂无 Cursor 会话数据",
  cursorSessionsEmptyHint:
    "本页只有行为聚合，没有对话正文。请确认本机已有 Cursor Agent 对话，并已启用自动刷新或手动刷新；正文请到「对话记录」查看。",
  cursorSessionsTableNote: "行为数字在此；点行打开对话记录看正文。",
  openConversationRow: "打开对话记录",
  openConversationRowTitle: "打开对话记录中的同一条会话",
  workTimelineBanner: "点时间轴横条可打开「对话记录」里的同一条会话。",
  conversationCatalogNote:
    "全来源共用此目录（含 Cursor Agent）。搜索可命中标题与已索引正文；正文留在本机，不进备份。侧栏「Cursor → 会话」是同一批 Cursor 会话的跨会话聚合，不含正文。",
  behaviorTabNote: "本页签是同一条 Cursor 会话的行为聚合；正文在「完整事件」。",
} as const;

export function conversationFocusFromSession(session?: {
  id: string;
  source: string;
}): ConversationFocus | null {
  return session ? { source: session.source, session_id: session.id } : null;
}
