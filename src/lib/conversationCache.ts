/**
 * 会话身份键。详情 DTO 只含元数据，时间线按页自取，不再缓存整条会话正文。
 */

export function conversationKey(session: { source: string; session_id: string }): string {
  return `${session.source}\u{1f}${session.session_id}`;
}
