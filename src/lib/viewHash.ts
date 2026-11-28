import type { ConversationFocus, View } from "../types";
import { parseDateValue } from "./calendar";

export const views: View[] = [
  "overview",
  "trend",
  "application",
  "model",
  "provider",
  "project",
  "conversations",
  "work-notes",
  "cursor",
  "cursor-sessions",
  "worktime",
  "instructions",
  "settings",
];

export function stripLocationHash(raw: string): string {
  return raw.replace(/^#/, "");
}

export function replaceLocationHash(next: string): void {
  if (stripLocationHash(window.location.hash) !== next) {
    window.history.replaceState(null, "", `#${next}`);
  }
}

export function hashForConversation(source: string, sessionId: string): string {
  return `conversations/${encodeURIComponent(source)}/${encodeURIComponent(sessionId)}`;
}

export function parseConversationFocus(raw: string): ConversationFocus | null {
  const hash = stripLocationHash(raw);
  if (!hash.startsWith("conversations/")) {
    return null;
  }
  const rest = hash.slice("conversations/".length);
  const slash = rest.indexOf("/");
  if (slash <= 0 || slash === rest.length - 1) {
    return null;
  }
  try {
    const source = decodeURIComponent(rest.slice(0, slash));
    const session_id = decodeURIComponent(rest.slice(slash + 1));
    if (!source || !session_id) {
      return null;
    }
    return { source, session_id };
  } catch {
    return null;
  }
}

export function conversationFocusToRestore(
  live: ConversationFocus | null,
  hash: string,
): ConversationFocus | null {
  return live ?? parseConversationFocus(hash);
}

export function hashForWorktime(day: string): string {
  return `worktime/${day}`;
}

export function parseWorktimeDay(raw: string): string | null {
  const hash = stripLocationHash(raw);
  if (!hash.startsWith("worktime/")) {
    return null;
  }
  const day = hash.slice("worktime/".length);
  return parseDateValue(day) ? day : null;
}

/** 热力图格子点击：合法日历日才写成工作时间线 hash。 */
export function worktimeHashForDay(day: string): string | null {
  return parseDateValue(day) ? hashForWorktime(day) : null;
}

export function hashBelongsToView(raw: string, view: View): boolean {
  const hash = stripLocationHash(raw);
  if (view === "settings") {
    return hash === "settings" || hash.startsWith("settings-");
  }
  if (view === "conversations") {
    return hash === "conversations" || hash === "sessions" || hash.startsWith("conversations/");
  }
  if (view === "worktime") {
    return hash === "worktime" || hash.startsWith("worktime/");
  }
  return hash === view;
}

export function parseViewHash(raw: string): View {
  const hash = stripLocationHash(raw);
  if (hash === "sessions" || hash === "conversations" || hash.startsWith("conversations/")) {
    return "conversations";
  }
  if (hash === "source") {
    return "application";
  }
  if (hash === "settings" || hash.startsWith("settings-")) {
    return "settings";
  }
  if (hash === "worktime" || hash.startsWith("worktime/")) {
    return "worktime";
  }
  return views.find((item) => item === hash) ?? "overview";
}

export function viewFromHash(): View {
  return parseViewHash(window.location.hash);
}
