import { invoke } from "@tauri-apps/api/core";
import type { Filter } from "../types";

function escapeCsvCell(value: string | number): string {
  const text = String(value);
  if (/[",\n]/.test(text)) {
    return `"${text.replace(/"/g, '""')}"`;
  }
  return text;
}

export function buildCsv(headers: string[], rows: (string | number)[][]): string {
  return [headers, ...rows].map((row) => row.map(escapeCsvCell).join(",")).join("\r\n");
}

function buildJson(headers: string[], rows: (string | number)[][]): string {
  const records = rows.map((row) =>
    Object.fromEntries(headers.map((header, index) => [header, row[index] ?? ""])),
  );
  return JSON.stringify(records, null, 2);
}

/**
 * 弹出原生保存对话框，把内容写入用户选择的 CSV 文件。
 * 返回 `false` 表示用户取消保存。
 */
export async function exportCsv(
  defaultName: string,
  headers: string[],
  rows: (string | number)[][],
): Promise<boolean> {
  const content = buildCsv(headers, rows);
  return invoke<boolean>("export_csv", { defaultName, content });
}

/**
 * 弹出原生保存对话框，把内容写入用户选择的 JSON 文件（数组套对象，key 为表头）。
 * 返回 `false` 表示用户取消保存。
 */
export async function exportJson(
  defaultName: string,
  headers: string[],
  rows: (string | number)[][],
): Promise<boolean> {
  const content = buildJson(headers, rows);
  return invoke<boolean>("export_json", { defaultName, content });
}

/**
 * 由 Rust 按当前筛选生成与 `ccusage daily --json` 同构的 JSON，再走现有保存对话框。
 * webview 不聚合。
 */
export async function exportCcusageDailyJson(
  defaultName: string,
  filter: Filter,
): Promise<boolean> {
  const content = await invoke<string>("get_ccusage_daily_json", { filter });
  return invoke<boolean>("export_json", { defaultName, content });
}

/**
 * 弹出原生保存对话框，把图表截图（PNG data URL）写入用户选择的文件。
 * 返回 `false` 表示用户取消保存。
 */
export async function exportImage(defaultName: string, dataUrl: string): Promise<boolean> {
  const base64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  return invoke<boolean>("export_image", { defaultName, base64 });
}
