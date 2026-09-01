import { useEffect, useRef, useState } from "react";
import { consumeEscape } from "../lib/escapeShortcut";

/** 管理可关闭浮层：点击外部或按 Escape 时收起。 */
export function useDismissible(initialOpen = false) {
  const [open, setOpen] = useState(initialOpen);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    function onDocClick(event: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    function onKeyDown(event: KeyboardEvent) {
      if (!consumeEscape(event)) {
        return;
      }
      setOpen(false);
      rootRef.current?.querySelector<HTMLElement>("button")?.focus();
    }
    document.addEventListener("mousedown", onDocClick);
    // 捕获阶段消费 Escape，避免对话详情或全局清筛选同时响应。
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  return { open, setOpen, rootRef };
}
