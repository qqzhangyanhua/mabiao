import { formatTokens, formatUsdAmount } from "./format";
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
