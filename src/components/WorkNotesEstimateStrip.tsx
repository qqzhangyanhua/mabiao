import { formatTokens, formatUsdAmount } from "../lib/format";
import { formatEstimatedSecs } from "../lib/workNotesCopy";
import type { WorkNotesPreviewDto } from "../types";

export function WorkNotesEstimateStrip({ preview }: { preview: WorkNotesPreviewDto }) {
  const listed = preview.sessions ?? [];
  if (listed.length === 0) {
    return (
      <p className="work-notes-scale">
        {preview.start_date} 至 {preview.end_date}，无会话
        {preview.skipped_sparse > 0 ? `（已略过 ${preview.skipped_sparse} 个零星会话）` : ""}
      </p>
    );
  }
  return (
    <div className="work-notes-estimate-strip">
      <div className="work-notes-estimate-item">
        <span className="work-notes-estimate-label">区间范围</span>
        <span className="work-notes-estimate-value">
          {preview.start_date} 至 {preview.end_date}
        </span>
      </div>
      <div className="work-notes-estimate-item">
        <span className="work-notes-estimate-label">可总结会话</span>
        <span className="work-notes-estimate-value">
          {listed.length} 个
          {preview.skipped_sparse > 0 ? (
            <span className="work-notes-estimate-sub">（略过 {preview.skipped_sparse} 零星）</span>
          ) : null}
        </span>
      </div>
      <div className="work-notes-estimate-item">
        <span className="work-notes-estimate-label">全选预计耗时</span>
        <span className="work-notes-estimate-value">
          {formatEstimatedSecs(preview.estimated_secs)} · {preview.estimated_calls} 次调用
        </span>
      </div>
      <div className="work-notes-estimate-item">
        <span className="work-notes-estimate-label">全选预计消耗</span>
        <span className="work-notes-estimate-value">
          {preview.estimated_unpriced || preview.estimated_cost == null
            ? "费用未定价"
            : `约 ${formatUsdAmount(preview.estimated_cost)}`}{" "}
          <span className="work-notes-estimate-sub">
            （约 {formatTokens(preview.estimated_input_tokens)} tok）
          </span>
        </span>
      </div>
    </div>
  );
}
