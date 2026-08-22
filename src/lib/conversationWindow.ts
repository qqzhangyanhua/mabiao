/**
 * 会话时间线的渲染窗口。
 *
 * 一条长会话可以有上万条事件，每条还要过一遍 markdown 渲染；全量挂进 DOM 会让 webview
 * 的常驻内存和会话长度成正比。窗口固定锚在末尾（最新事件），用「已隐藏多少条」而不是
 * 「渲染多少条」来描述，这样轮询追加新事件时窗口起点不动，不会把用户正在看的内容顶走。
 */

export const CONVERSATION_WINDOW_INITIAL = 60;
export const CONVERSATION_WINDOW_STEP = 60;

export function initialConversationHiddenCount(
  total: number,
  initial: number = CONVERSATION_WINDOW_INITIAL,
): number {
  return Math.max(0, Math.trunc(total) - Math.max(1, Math.trunc(initial)));
}

export function revealEarlierConversationEvents(
  hiddenCount: number,
  step: number = CONVERSATION_WINDOW_STEP,
): number {
  return Math.max(0, Math.trunc(hiddenCount) - Math.max(1, Math.trunc(step)));
}

export function conversationWindowSlice<T>(
  events: readonly T[],
  hiddenCount: number,
): readonly T[] {
  const start = Math.min(Math.max(0, Math.trunc(hiddenCount)), events.length);
  return start === 0 ? events : events.slice(start);
}
