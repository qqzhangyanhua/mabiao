import { Icon } from "../icons";
import {
  capabilityLabel,
  conversationSourceLabel,
  conversationFileUnavailableLabel,
  conversationRangeLabel,
  conversationRangeTitle,
  conversationStatusLabel,
} from "../lib/conversationDisplay";
import { formatCost, formatTokens, projectLabel } from "../lib/format";
import { SourceLabel } from "./SourceIcon";
import type { ConversationCatalogRowProps } from "./type";

export function ConversationCatalogRow({ row, maxTotal, onOpen }: ConversationCatalogRowProps) {
  const rangeTitle = conversationRangeTitle(row);
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
          {row.file_available ? null : (
            <span className="conversation-file-unavailable">
              <Icon name="alertTriangle" size={12} />
              {conversationFileUnavailableLabel(row.source)}
            </span>
          )}
        </div>
      </td>
    </tr>
  );
}
