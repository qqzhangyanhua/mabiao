import { useEffect } from "react";
import { viewForShortcutKey } from "../lib/nav";
import type { View } from "../types";

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable;
}

export function useKeyboardShortcuts({
  onNavigate,
  onRefresh,
  onClearFilters,
}: {
  onNavigate: (view: View) => void;
  onRefresh: () => void;
  onClearFilters: () => void;
}): void {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if (isTypingTarget(event.target)) {
        return;
      }
      if (event.key === "r" || event.key === "R") {
        event.preventDefault();
        onRefresh();
        return;
      }
      if (event.key === "Escape") {
        onClearFilters();
        return;
      }
      const next = viewForShortcutKey(event.key);
      if (next) {
        event.preventDefault();
        onNavigate(next);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onNavigate, onRefresh, onClearFilters]);
}
