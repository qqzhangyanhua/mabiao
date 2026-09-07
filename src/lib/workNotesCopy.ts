import { formatTokens, formatUsdAmount } from "./format";
import { projectLabel } from "./format";
import type { WorkNotesDto, WorkNotesPreviewDto } from "../types";

export function formatEstimatedSecs(secs: number): string {
  if (secs <= 0) {
    return "约 0 秒";
  }
  if (secs < 60) {
    return `约 ${secs} 秒`;
  }
  const minutes = Math.max(1, Math.round(secs / 60));
  return `约 ${minutes} 分钟`;
}

export function workNotesEstimateCopy(preview: WorkNotesPreviewDto): string | null {
  if (preview.session_count <= 0) {
    return null;
  }
  const cost =
    preview.estimated_unpriced || preview.estimated_cost == null
      ? "费用未定价"
      : `约 ${formatUsdAmount(preview.estimated_cost)}`;
  return `预计 ${preview.estimated_calls} 次调用、${formatEstimatedSecs(preview.estimated_secs)}、${cost}（约 ${formatTokens(preview.estimated_input_tokens)} token）`;
}

export function workNotesUsageCopy(dto: WorkNotesDto): string | null {
  if (dto.actual_unpriced && dto.actual_input_tokens === 0 && dto.actual_output_tokens === 0) {
    return "本次引擎没有回吐用量";
  }
  const tokens = `实际 ${formatTokens(dto.actual_input_tokens)} 输入 / ${formatTokens(dto.actual_output_tokens)} 输出 token`;
  const cost =
    dto.actual_unpriced || dto.actual_cost == null ? "费用未定价" : formatUsdAmount(dto.actual_cost);
  return `${tokens}，${cost}`;
}

/**
 * 把工作纪要序列化为可读纯文字，供「复制文字」按钮使用。
 * 格式：标题 → 日期 → 各条目（标题 + 说明 + 项目）→ 收尾语。
 */
export function workNotesPlainText(dto: WorkNotesDto): string {
  const lines: string[] = [];

  if (dto.headline) {
    lines.push(dto.headline);
  }
  lines.push(`${dto.start_date} 至 ${dto.end_date}`);
  lines.push("");

  dto.entries.forEach((entry, index) => {
    const num = String(index + 1).padStart(2, "0");
    lines.push(`${num}  ${entry.title}`);
    if (entry.detail) {
      lines.push(`    ${entry.detail}`);
    }
    const proj = entry.project?.trim();
    if (proj) {
      const label = projectLabel(proj);
      if (label !== "未标注") {
        lines.push(`    📁 ${label}`);
      }
    }
    lines.push("");
  });

  if (dto.closing) {
    lines.push(dto.closing);
    lines.push("");
  }

  if (dto.skipped_sparse > 0) {
    lines.push(`（已略过 ${dto.skipped_sparse} 个零星会话）`);
  }

  return lines.join("\n").trimEnd();
}
