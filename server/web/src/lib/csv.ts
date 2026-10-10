export type CsvCell = string | number | null | undefined;

// Excel 会把以这些字符开头的单元格当公式执行；成员名、会话标题都是用户可控的文本。
const FORMULA_LEAD = /^[=+\-@\t\r]/;

function cell(value: CsvCell): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : "";
  const text = FORMULA_LEAD.test(value) ? `'${value}` : value;
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** 数字原样输出，字符串做公式转义。带 BOM，Excel 才不会把中文读成乱码。 */
export function toCsv(rows: CsvCell[][]): string {
  return "\uFEFF" + rows.map((row) => row.map(cell).join(",")).join("\r\n") + "\r\n";
}

export function downloadCsv(filename: string, rows: CsvCell[][]): void {
  const blob = new Blob([toCsv(rows)], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}
