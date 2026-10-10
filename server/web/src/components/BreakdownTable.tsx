import { useState } from "react";
import type { Breakdown } from "../api/types";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";

const COLLAPSED_ROWS = 10;

interface Props {
  title: string;
  rows: Breakdown[];
  mode: CostMode;
  /** 给每行名称生成链接；返回 `undefined` 表示不可点。 */
  linkOf?: (row: Breakdown) => string | undefined;
}

export function BreakdownTable({ title, rows, mode, linkOf }: Props) {
  const [expanded, setExpanded] = useState(false);
  // 服务端按统一价排序；切到快照口径时按快照重排，榜单才和看到的数字一致。
  const sorted = [...rows].sort(
    (a, b) => costOf(b, mode) - costOf(a, mode) || b.total_tokens - a.total_tokens,
  );
  const shown = expanded ? sorted : sorted.slice(0, COLLAPSED_ROWS);
  const total = sorted.reduce((sum, row) => sum + costOf(row, mode), 0);

  return (
    <section className="rounded border border-slate-200 bg-white p-4">
      <h3 className="mb-3 text-sm font-medium text-slate-700">{title}</h3>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-slate-500">这段时间没有数据</p>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-200 text-left text-xs text-slate-500">
              <th className="py-1 font-normal">名称</th>
              <th className="py-1 text-right font-normal">记录</th>
              <th className="py-1 text-right font-normal">Token</th>
              <th className="py-1 text-right font-normal">费用</th>
              <th className="w-24 py-1 pl-3 font-normal">占比</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((row) => {
              const cost = costOf(row, mode);
              const share = total > 0 ? cost / total : 0;
              const href = linkOf?.(row);
              return (
                <tr key={row.key} className="border-b border-slate-100 last:border-0">
                  <td className="max-w-56 truncate py-1.5 pr-2" title={row.label}>
                    {href ? (
                      <a href={href} className="text-blue-700 hover:underline">
                        {row.label}
                      </a>
                    ) : (
                      row.label
                    )}
                  </td>
                  <td className="py-1.5 text-right tabular-nums">{row.record_count}</td>
                  <td className="py-1.5 text-right tabular-nums">
                    {formatTokens(row.total_tokens)}
                  </td>
                  <td className="py-1.5 text-right tabular-nums">
                    {formatCost(cost)}
                    {mode === "unified" && row.unpriced_count > 0 && (
                      <span
                        className="ml-1 text-amber-600"
                        title={`${row.unpriced_count} 条未定价，费用偏低`}
                      >
                        *
                      </span>
                    )}
                  </td>
                  <td className="py-1.5 pl-3">
                    <div className="h-2 rounded bg-slate-100">
                      <div
                        className="h-2 rounded bg-blue-400"
                        style={{ width: `${(share * 100).toFixed(1)}%` }}
                      />
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {rows.length > COLLAPSED_ROWS && (
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="mt-2 text-sm text-blue-700 hover:underline"
        >
          {expanded ? "收起" : `显示全部 ${rows.length} 项`}
        </button>
      )}
    </section>
  );
}
