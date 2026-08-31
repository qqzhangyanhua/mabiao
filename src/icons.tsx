import type { ReactElement } from "react";

export type IconName =
  | "overview"
  | "trend"
  | "source"
  | "model"
  | "provider"
  | "project"
  | "sessions"
  | "cursor"
  | "settings"
  | "instruction"
  | "refresh"
  | "calendar"
  | "filter"
  | "tokens"
  | "chat"
  | "cost"
  | "daily"
  | "clock"
  | "sun"
  | "moon"
  | "monitor"
  | "download"
  | "chevron"
  | "check"
  | "close"
  | "search"
  | "copy"
  | "inbox"
  | "alertTriangle";

const strokes: Record<IconName, ReactElement> = {
  overview: (
    <>
      <rect x="3" y="3" width="8" height="8" rx="1.5" />
      <rect x="13" y="3" width="8" height="5" rx="1.5" />
      <rect x="13" y="10" width="8" height="11" rx="1.5" />
      <rect x="3" y="13" width="8" height="8" rx="1.5" />
    </>
  ),
  trend: (
    <>
      <path d="M4 19V5" />
      <path d="M4 19h16" />
      <path d="M7 14l4-5 3 3 5-7" />
    </>
  ),
  source: (
    <>
      <path d="M12 3 4 7.5 12 12l8-4.5L12 3Z" />
      <path d="M4 12.5 12 17l8-4.5" />
      <path d="M4 16.5 12 21l8-4.5" />
    </>
  ),
  model: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <path d="M9 9h.01M15 9h.01M9 15h.01M15 15h.01M12 12h.01" />
    </>
  ),
  provider: (
    <>
      <circle cx="6" cy="12" r="2.2" />
      <circle cx="18" cy="7" r="2.2" />
      <circle cx="18" cy="17" r="2.2" />
      <path d="M8 11.2 16 8.2M8 12.8l8 4" />
    </>
  ),
  project: (
    <>
      <path d="M4 8.5 12 4l8 4.5v7L12 20l-8-4.5v-7Z" />
      <path d="M12 12 4.4 7.6M12 12l7.6-4.4M12 12v8" />
    </>
  ),
  sessions: (
    <>
      <path d="M5 6h14v10H8l-3 3V6Z" />
      <path d="M8 10h8M8 13h5" />
    </>
  ),
  cursor: (
    <>
      <path d="M8 5h8l3 5-3 5H8L5 10l3-5Z" />
      <path d="M9 15v4l3-2 3 2v-4" />
    </>
  ),
  instruction: (
    <>
      <path d="M7 4h10v16H7z" />
      <path d="M10 8h4M10 12h4M10 16h2" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 4v2.2M12 17.8V20M4.9 7.1l1.6 1.6M17.5 15.3l1.6 1.6M4 12h2.2M17.8 12H20M4.9 16.9l1.6-1.6M17.5 8.7l1.6-1.6" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 12a8 8 0 1 1-2.3-5.6" />
      <path d="M20 5v5h-5" />
    </>
  ),
  calendar: (
    <>
      <rect x="4" y="5" width="16" height="15" rx="2" />
      <path d="M8 3v4M16 3v4M4 10h16" />
    </>
  ),
  filter: (
    <>
      <path d="M4 6h16l-6 7v5l-4 2v-7L4 6Z" />
    </>
  ),
  tokens: (
    <>
      <path d="M12 3 5 7v10l7 4 7-4V7l-7-4Z" />
      <path d="M12 12 5.4 7.6M12 12l6.6-4.4M12 12v9" />
    </>
  ),
  chat: (
    <>
      <path d="M6 7h12v8H9l-3 3V7Z" />
    </>
  ),
  cost: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 7v10M9.5 9.5c.6-1 2.8-1.4 3.6 0 .7 1.2-.3 1.9-1.6 2.3-1.4.4-2.5 1-1.8 2.3.8 1.4 3 1 3.7 0" />
    </>
  ),
  daily: (
    <>
      <path d="M4 18V8M9 18V5M14 18v-7M19 18V9" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 8v5l3 2" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2.5M12 19.5V22M4.2 4.2l1.8 1.8M18 18l1.8 1.8M2 12h2.5M19.5 12H22M4.2 19.8l1.8-1.8M18 6l1.8-1.8" />
    </>
  ),
  moon: (
    <>
      <path d="M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5Z" />
    </>
  ),
  monitor: (
    <>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M8 20h8M12 16v4" />
    </>
  ),
  download: (
    <>
      <path d="M12 3v12M7.5 10.5 12 15l4.5-4.5" />
      <path d="M4 18h16" />
    </>
  ),
  chevron: (
    <>
      <path d="M15 6 9 12l6 6" />
    </>
  ),
  check: (
    <>
      <path d="M5 12.5 9.5 17 19 7.5" />
    </>
  ),
  close: (
    <>
      <path d="M6 6l12 12" />
      <path d="M18 6 6 18" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M16 16.5 20.5 21" />
    </>
  ),
  copy: (
    <>
      <rect x="8" y="8" width="11" height="13" rx="1.6" />
      <path d="M6 16V5.6A1.6 1.6 0 0 1 7.6 4H16" />
    </>
  ),
  inbox: (
    <>
      <path d="M4 12h4l1.8 3h4.4l1.8-3h4" />
      <path d="M6.5 5h11l2.5 7v7a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 19v-7l3-7Z" />
    </>
  ),
  alertTriangle: (
    <>
      <path d="M12 4 3 20h18L12 4Z" />
      <path d="M12 10v4M12 17h.01" />
    </>
  ),
};

export function Icon({
  name,
  size = 16,
  className,
}: {
  name: IconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden
    >
      {strokes[name]}
    </svg>
  );
}

export const sourceTone: Record<string, string> = {
  claude: "tone-claude",
  codex: "tone-codex",
  pi: "tone-pi",
  omp: "tone-omp",
  opencode: "tone-open",
  kimi: "tone-kimi",
  grok: "tone-grok",
  gemini: "tone-gemini",
  qwen: "tone-qwen",
  factory: "tone-factory",
  droid: "tone-factory",
  dsh: "tone-dsh",
  cursor: "tone-cursor",
  cursor_agent: "tone-cursor",
  copilot: "tone-copilot",
  antigravity: "tone-gemini",
};
