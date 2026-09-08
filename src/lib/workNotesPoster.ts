import type { WorkNotesPosterViewModel } from "../workNotes/posterTypes";
import type { WorkNotesDto, WorkNotesEntry, WorkNotesRangeKind } from "../types";
import { formatCompact, projectLabel } from "./format";
import { periodRangeLabel } from "./reportCopy";

function rangePhrases(kind: WorkNotesRangeKind): { kicker: string; tokenUnit: string } {
  if (kind === "today") {
    return { kicker: "码表 · 今日纪要", tokenUnit: "今日 token" };
  }
  if (kind === "this_month") {
    return { kicker: "码表 · 本月纪要", tokenUnit: "本月 token" };
  }
  if (kind === "custom") {
    return { kicker: "码表 · 区间纪要", tokenUnit: "区间 token" };
  }
  return { kicker: "码表 · 本周纪要", tokenUnit: "本周 token" };
}

function entryProject(project: string): string | null {
  const trimmed = project.trim();
  if (!trimmed) {
    return null;
  }
  const label = projectLabel(trimmed);
  return label === "未标注" ? null : label;
}

function mapEntry(entry: WorkNotesEntry) {
  return {
    title: entry.title,
    detail: entry.detail,
    project: entryProject(entry.project),
  };
}

export function toWorkNotesPosterViewModel(dto: WorkNotesDto): WorkNotesPosterViewModel | null {
  if (!dto.has_data) {
    return null;
  }
  const phrases = rangePhrases(dto.range_kind);
  return {
    kicker: phrases.kicker,
    rangeLabel: periodRangeLabel(dto.start_date, dto.end_date),
    headline: dto.headline,
    closing: dto.closing,
    entries: dto.entries.map(mapEntry),
    metrics: [
      { id: "sessions", label: "会话", value: formatCompact(dto.session_count) },
      { id: "projects", label: "项目", value: formatCompact(dto.project_count) },
      { id: "active_days", label: "活跃天数", value: formatCompact(dto.active_days) },
      { id: "tokens", label: phrases.tokenUnit, value: formatCompact(dto.total_tokens) },
    ],
    skippedLabel: dto.skipped_sparse > 0 ? `已略过 ${dto.skipped_sparse} 个零星会话` : null,
  };
}
