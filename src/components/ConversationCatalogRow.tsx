import { Icon } from "../icons";
import {
  capabilityLabel,
  conversationSourceLabel,
  conversationFileUnavailableLabel,
  conversationRangeLabel,
  conversationRangeTitle,
  conversationStatusLabel,
  conversationWorkNotesSnippet,
  conversationWorkNotesSummaries,
  workNotesGeneratedLabel,
} from "../lib/conversationDisplay";
import { formatCost, formatTokens, projectLabel } from "../lib/format";
import { HighlightedSnippet } from "../lib/highlightMatch";
import { SourceLabel } from "./SourceIcon";
import type { ConversationCatalogRowProps } from "./type";
import { Button } from "./ui/Button";

export function ConversationCatalogRow({
  row,
  maxTotal,
  searching = false,
  highlightQuery = "",
  onOpen,
  onSummarize,
  onViewSummary,
}: ConversationCatalogRowProps) {
  const rangeTitle = conversationRangeTitle(row);
  const workNotesSnippet = conversationWorkNotesSnippet(row);
  const hasSummary = conversationWorkNotesSummaries(row).length > 0;
  return (
    <tr
      className="clickable"
      tabIndex={0}
      aria-label={`打开对话：${row.title}`}
      onClick={() => onOpen(row)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpen(row);
        }
      }}
    >
      <td title={row.title}>
        <div className="conversation-title-cell">
          <strong>{row.title}</strong>
          <span className="mono">{row.session_id}</span>
          {workNotesSnippet ? (
            <span className="conversation-work-notes-snippet" title={workNotesSnippet}>
              {workNotesSnippet}
            </span>
          ) : null}
          {row.match_field === "body" && row.match_snippet ? (
            <HighlightedSnippet
              className="conversation-match-snippet"
              text={row.match_snippet}
              query={highlightQuery}
            />
          ) : null}
        </div>
      </td>
      <td>
        <SourceLabel source={row.source} fallback={conversationSourceLabel(row.source)} />
      </td>
      <td title={row.project}>{projectLabel(row.project)}</td>
      <td>{row.model || "未标注"}</td>
      <td>
        <span className="cell-bar">
          <i style={{ width: `${(row.total_tokens / maxTotal) * 100}%` }} />
        </span>
        <span className="cell-bar-label">{formatTokens(row.total_tokens)}</span>
      </td>
      <td title={row.unpriced ? "部分轮次单价未配置" : undefined}>
        {row.total_tokens === 0 && row.cost == null ? "—" : formatCost(row.cost, row.unpriced)}
        {row.unpriced && (row.total_tokens > 0 || row.cost != null) ? (
          <span className="muted"> *</span>
        ) : null}
      </td>
      <td title={rangeTitle || undefined}>{conversationRangeLabel(row)}</td>
      <td>
        <div className="conversation-capabilities">
          {row.capabilities.length > 0 ? (
            row.capabilities.map((capability) => (
              <span key={capability}>{capabilityLabel(capability)}</span>
            ))
          ) : (
            <span>仅元数据</span>
          )}
        </div>
      </td>
      <td>
        <div className="conversation-row-statuses">
          <span className={`conversation-status status-${row.support_status}`}>
            {conversationStatusLabel(row.support_status)}
          </span>
          {row.generated_by_work_notes ? (
            <span className="conversation-status status-generated">{workNotesGeneratedLabel()}</span>
          ) : null}
          {row.file_available ? null : (
            <span className="conversation-file-unavailable">
              <Icon name="alertTriangle" size={12} />
              {conversationFileUnavailableLabel(row.source)}
            </span>
          )}
          {searching && row.event_index_ready === false ? (
            <span className="conversation-index-pending">正文索引未就绪，当前只搜标题</span>
          ) : null}
          {hasSummary ? (
            <Button
              variant="text"
              onClick={(event) => {
                event.stopPropagation();
                onViewSummary(row);
              }}
              onKeyDown={(event) => event.stopPropagation()}
            >
              查看摘要
            </Button>
          ) : null}
          {row.generated_by_work_notes ? null : (
            <Button
              variant="text"
              onClick={(event) => {
                event.stopPropagation();
                onSummarize(row);
              }}
              onKeyDown={(event) => event.stopPropagation()}
            >
              生成摘要
            </Button>
          )}
        </div>
      </td>
    </tr>
  );
}
