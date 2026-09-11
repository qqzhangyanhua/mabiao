import { workNotesUsageCopy } from "../lib/workNotesCopy";
import type { WorkNotesDto } from "../types";

export function WorkNotesProgress({
  status,
}: {
  status: { done: number; total: number; current_title: string };
}) {
  const pct = status.total > 0 ? Math.round((status.done / status.total) * 100) : 0;
  return (
    <div className="work-notes-progress-box" role="status" aria-live="polite">
      <div className="work-notes-progress-header">
        <span className="work-notes-progress-title-meta">正在总结会话…</span>
        <span className="work-notes-progress-count">
          已完成 <strong>{status.done}</strong> / {status.total}
          <span className="work-notes-progress-pct">（{pct}%）</span>
        </span>
      </div>
      <div className="work-notes-progress-track">
        <div
          className="work-notes-progress-bar"
          style={{ width: `${status.total > 0 ? Math.min(100, Math.max(3, pct)) : 3}%` }}
        />
      </div>
      {status.current_title ? (
        <p className="work-notes-progress-current">
          <span className="work-notes-progress-dot" aria-hidden="true" />
          <span className="work-notes-progress-current-label">当前正在总结：</span>
          <span className="work-notes-progress-current-title">{status.current_title}</span>
        </p>
      ) : null}
    </div>
  );
}

export function WorkNotesFailures({ dto }: { dto: WorkNotesDto }) {
  return (
    <div className="work-notes-failures">
      <p className="work-notes-failures-title">有 {dto.failed_count} 个会话总结失败</p>
      {dto.failures.map((failure, index) => (
        <pre key={`${failure.title}-${index}`} className="work-notes-stderr">
          {failure.title ? `${failure.title}\n` : ""}
          {failure.error}
        </pre>
      ))}
    </div>
  );
}

export function WorkNotesUsage({ dto }: { dto: WorkNotesDto }) {
  const usage = workNotesUsageCopy(dto);
  if (!usage) {
    return null;
  }
  return <p className="work-notes-usage">{usage}</p>;
}
