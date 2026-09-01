type ShortcutEvent = {
  key: string;
  preventDefault: () => void;
  stopPropagation: () => void;
};

function consume(event: ShortcutEvent): true {
  event.preventDefault();
  event.stopPropagation();
  return true;
}

/** Esc 分层：浮层（捕获）> 对话详情 > 清筛选。前两层调用此函数消费事件。 */
export function consumeEscape(event: ShortcutEvent): boolean {
  return event.key === "Escape" ? consume(event) : false;
}

/** 对话详情吞掉 R，避免全局全盘摄取。 */
export function consumeRefreshShortcut(event: ShortcutEvent): boolean {
  return event.key === "r" || event.key === "R" ? consume(event) : false;
}
