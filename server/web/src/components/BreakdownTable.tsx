import { useState } from "react";
import type { Breakdown } from "../api/types";
import { sliceColor } from "../lib/chart";
import { cn } from "../lib/cn";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import { panelClass } from "./ui/card";

const COLLAPSED_ROWS = 10;

interface Props {
  title: string;
  rows: Breakdown[];
  mode: CostMode;
  /** 给每行名称生成链接；返回 `undefined` 表示不可点。 */
  linkOf?: (row: Breakdown) => string | undefined;
}

function percent(share: number): string {
  if (share <= 0) return "0%";
  return `${(share * 100).toFixed(share < 0.1 ? 1 : 0)}%`;
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
            title={`${part.label} ${percent(part.value / total)}`}
            style={{ width: `${(part.value / total) * 100}%`, background: part.color }}
          />
        ))}
    </div>
  );
}

function Row({
  row,
  mode,
  share,
  color,
  href,
}: {
  row: Breakdown;
  mode: CostMode;
  share: number;
  color: string;
  href: string | undefined;
}) {
  const cost = costOf(row, mode);
  return (
    <tr className="border-b border-border/70 last:border-0">
      <td className="max-w-56 truncate py-1.5 pr-2" title={row.label}>
        {href ? (
          <a href={href} className="text-primary hover:underline">
            {row.label}
          </a>
        ) : (
          row.label
        )}
      </td>
      <td className="py-1.5 text-right tabular-nums">{row.record_count}</td>
      <td className="py-1.5 text-right tabular-nums">{formatTokens(row.total_tokens)}</td>
      <td className="py-1.5 text-right tabular-nums">
        {formatCost(cost)}
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
              style={{ width: `${(share * 100).toFixed(1)}%`, background: color }}
            />
          </div>
          <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">
            {percent(share)}
          </span>
        </div>
      </td>
    </tr>
  );
}

export function BreakdownTable({ title, rows, mode, linkOf }: Props) {
  const [expanded, setExpanded] = useState(false);
  // 服务端按统一价排序；切到快照口径时按快照重排，榜单才和看到的数字一致。
  const sorted = [...rows].sort(
    (a, b) => costOf(b, mode) - costOf(a, mode) || b.total_tokens - a.total_tokens,
  );
  const shown = expanded ? sorted : sorted.slice(0, COLLAPSED_ROWS);
  const costTotal = sorted.reduce((sum, row) => sum + costOf(row, mode), 0);
  const weight = (row: Breakdown) => (costTotal > 0 ? costOf(row, mode) : row.total_tokens);
  const total = sorted.reduce((sum, row) => sum + weight(row), 0);
  const colorOf = new Map(sorted.map((row, index) => [row.key, sliceColor(row.key, index)]));
  const lead = sorted[0];

  return (
    <section className={cn(panelClass, "p-4")}>
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h3 className="text-sm font-medium">{title}</h3>
        <span className="truncate text-xs text-muted-foreground">
          {rows.length === 0
            ? "0 项"
            : lead && total > 0
              ? `${rows.length} 项 · ${lead.label} ${percent(weight(lead) / total)}`
              : `${rows.length} 项`}
        </span>
      </div>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <>
          <Composition rows={sorted} weight={weight} colorOf={colorOf} />
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th className="py-1 font-normal">名称</th>
                <th className="py-1 text-right font-normal">记录</th>
                <th className="py-1 text-right font-normal">Token</th>
                <th className="py-1 text-right font-normal">费用</th>
                <th className="w-28 py-1 pl-3 font-normal">占比</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((row) => (
                <Row
                  key={row.key}
                  row={row}
                  mode={mode}
                  share={total > 0 ? weight(row) / total : 0}
                  color={colorOf.get(row.key) ?? "var(--chart-1)"}
                  href={linkOf?.(row)}
                />
              ))}
            </tbody>
          </table>
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
