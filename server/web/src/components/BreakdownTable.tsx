import { useState } from "react";
import type { Breakdown } from "../api/types";
import { sliceColor } from "../lib/chart";
import { cn } from "../lib/cn";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatPercent, formatTokens } from "../lib/format";
import { panelClass } from "./ui/card";
import { numCellClass, tableClass, thClass, theadRowClass, trClass } from "./ui/table";

const COLLAPSED_ROWS = 10;

interface Props {
  title: string;
  rows: Breakdown[];
  mode: CostMode;
  linkOf?: (row: Breakdown) => string | undefined;
  labelOf?: (row: Breakdown) => string;
}

function Composition({
  rows,
  weight,
  colorOf,
}: {
  rows: Breakdown[];
  weight: (row: Breakdown) => number;
  colorOf: Map<string, string>;
}) {
  const total = rows.reduce((sum, row) => sum + weight(row), 0);
  if (total <= 0) return null;
  const top = rows.slice(0, 6);
  const rest = total - top.reduce((sum, row) => sum + weight(row), 0);
  const parts = [
    ...top.map((row) => ({
      key: row.key,
      label: row.label,
      value: weight(row),
      color: colorOf.get(row.key) ?? "var(--chart-1)",
    })),
    ...(rest > 0
      ? [{ key: "__rest", label: "其他", value: rest, color: "var(--muted-foreground)" }]
      : []),
  ];
  return (
    <div className="mb-3 flex h-2 overflow-hidden rounded-full bg-muted" role="img" aria-label="占比">
      {parts
        .filter((part) => part.value > 0)
        .map((part) => (
          <div
            key={part.key}
            title={`${part.label} ${formatPercent(part.value / total)}`}
            style={{ width: `${(part.value / total) * 100}%`, background: part.color }}
          />
        ))}
    </div>
  );
}

export function BreakdownTable({ title, rows, mode, linkOf, labelOf }: Props) {
  const [expanded, setExpanded] = useState(false);
  const sorted = [...rows].sort(
    (a, b) => costOf(b, mode) - costOf(a, mode) || b.total_tokens - a.total_tokens,
  );
  const shown = expanded ? sorted : sorted.slice(0, COLLAPSED_ROWS);
  const costTotal = sorted.reduce((sum, row) => sum + costOf(row, mode), 0);
  const weight = (row: Breakdown) => (costTotal > 0 ? costOf(row, mode) : row.total_tokens);
  const total = sorted.reduce((sum, row) => sum + weight(row), 0);
  const colorOf = new Map(sorted.map((row, index) => [row.key, sliceColor(row.key, index)]));
  const lead = sorted[0];
  const nameOf = (row: Breakdown) => labelOf?.(row) ?? row.label;

  return (
    <section className={cn(panelClass, "p-4")}>
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h3 className="text-sm font-medium">{title}</h3>
        <span className="truncate text-xs text-muted-foreground">
          {rows.length === 0
            ? "0 项"
            : lead && total > 0
              ? `${rows.length} 项 · ${nameOf(lead)} ${formatPercent(weight(lead) / total)}`
              : `${rows.length} 项`}
        </span>
      </div>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <>
          <Composition rows={sorted} weight={weight} colorOf={colorOf} />
          <div className="overflow-x-auto">
            <table className={tableClass}>
              <caption className="sr-only">{title}</caption>
              <thead>
                <tr className={theadRowClass}>
                  <th scope="col" className={`py-1 ${thClass}`}>
                    名称
                  </th>
                  <th scope="col" className={`py-1 ${numCellClass} ${thClass}`}>
                    记录
                  </th>
                  <th scope="col" className={`py-1 ${numCellClass} ${thClass}`}>
                    Token
                  </th>
                  <th scope="col" className={`py-1 ${numCellClass} ${thClass}`}>
                    费用
                  </th>
                  <th scope="col" className={`w-28 py-1 pl-3 ${thClass}`}>
                    占比
                  </th>
                </tr>
              </thead>
              <tbody>
                {shown.map((row) => {
                  const share = total > 0 ? weight(row) / total : 0;
                  const href = linkOf?.(row);
                  const label = nameOf(row);
                  return (
                    <tr key={row.key} className={trClass}>
                      <th scope="row" className="max-w-56 truncate py-1.5 pr-2 font-normal" title={label}>
                        {href ? (
                          <a href={href} className="text-primary hover:underline">
                            {label}
                          </a>
                        ) : (
                          label
                        )}
                      </th>
                      <td className={`py-1.5 ${numCellClass}`}>{row.record_count}</td>
                      <td className={`py-1.5 ${numCellClass}`}>{formatTokens(row.total_tokens)}</td>
                      <td className={`py-1.5 ${numCellClass}`}>
                        {formatCost(costOf(row, mode))}
                        {mode === "unified" && row.unpriced_count > 0 && (
                          <span className="ml-1 text-warning" title={`${row.unpriced_count} 条未定价，费用偏低`}>
                            *
                          </span>
                        )}
                      </td>
                      <td className="py-1.5 pl-3">
                        <div className="flex items-center gap-2">
                          <div className="h-1.5 flex-1 rounded-full bg-muted">
                            <div
                              className="h-1.5 rounded-full"
                              style={{
                                width: `${(share * 100).toFixed(1)}%`,
                                background: colorOf.get(row.key) ?? "var(--chart-1)",
                              }}
                            />
                          </div>
                          <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">
                            {formatPercent(share)}
                          </span>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </>
      )}
      {rows.length > COLLAPSED_ROWS && (
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="mt-2 text-sm text-primary hover:underline"
        >
          {expanded ? "收起" : `显示全部 ${rows.length} 项`}
        </button>
      )}
    </section>
  );
}
