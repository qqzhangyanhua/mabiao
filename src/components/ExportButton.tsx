import { useState } from "react";
import { Icon } from "../icons";
import { exportCcusageDailyJson, exportCsv, exportJson } from "../lib/exportFile";
import type { Filter } from "../types";
import { Spinner } from "./Spinner";

type ExportFormat = "csv" | "json" | "ccusage";
type ExportRows = (string | number)[][];

const FORMATS: { format: ExportFormat; label: string }[] = [
  { format: "csv", label: "CSV" },
  { format: "json", label: "JSON" },
];

export function ExportButton({
  label = "导出",
  filename,
  headers,
  rows,
  getRows,
  ccusageFilter,
  disabled: disabledProp,
}: {
  label?: string;
  /** 不带扩展名的文件基础名，例如「会话列表」 */
  filename: string;
  headers: string[];
  /** 静态数据（已经在内存中的全部行）。与 `getRows` 二选一。 */
  rows?: ExportRows;
  /** 懒加载数据源，用于服务端分页场景：点击时才拉取完整结果集。 */
  getRows?: () => Promise<ExportRows>;
  /** 传入当前用量筛选时，多一档与 `ccusage daily --json` 同构的导出。 */
  ccusageFilter?: Filter;
  disabled?: boolean;
}) {
  const [status, setStatus] = useState<string | null>(null);
  const [busyFormat, setBusyFormat] = useState<ExportFormat | null>(null);

  async function handleExport(format: ExportFormat) {
    setBusyFormat(format);
    setStatus(null);
    try {
      if (format === "ccusage") {
        if (!ccusageFilter) {
          setStatus("没有可导出的数据");
          return;
        }
        const saved = await exportCcusageDailyJson(`${filename}-ccusage-daily.json`, ccusageFilter);
        setStatus(saved ? "已导出" : null);
        return;
      }
      const data = getRows ? await getRows() : (rows ?? []);
      if (data.length === 0) {
        setStatus("没有可导出的数据");
        return;
      }
      const saved =
        format === "csv"
          ? await exportCsv(`${filename}.csv`, headers, data)
          : await exportJson(`${filename}.json`, headers, data);
      setStatus(saved ? "已导出" : null);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "导出失败");
    } finally {
      setBusyFormat(null);
    }
  }

  const staticEmpty = !getRows && (rows?.length ?? 0) === 0;
  const tableDisabled = (disabledProp ?? staticEmpty) || busyFormat !== null;
  const ccusageDisabled = (disabledProp ?? false) || busyFormat !== null || !ccusageFilter;
  const formats = ccusageFilter
    ? [...FORMATS, { format: "ccusage" as const, label: "ccusage" }]
    : FORMATS;

  return (
    <span className="export-action">
      <span className="export-group" role="group" aria-label={`${label}选项`}>
        <span className="export-group-icon" aria-hidden>
          {busyFormat ? <Spinner size={12} /> : <Icon name="download" size={13} />}
        </span>
        {formats.map(({ format, label: formatLabel }) => (
          <button
            key={format}
            type="button"
            className={`export-btn export-btn-${format}${busyFormat === format ? " is-busy" : ""}`}
            disabled={format === "ccusage" ? ccusageDisabled : tableDisabled}
            aria-label={`${label} ${formatLabel}`}
            aria-busy={busyFormat === format}
            onClick={() => handleExport(format)}
          >
            {formatLabel}
          </button>
        ))}
      </span>
      {status ? <span className="export-status">{status}</span> : null}
    </span>
  );
}
