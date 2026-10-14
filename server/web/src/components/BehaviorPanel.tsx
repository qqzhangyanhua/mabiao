import type { AsyncState } from "../hooks";
import type { BehaviorReport } from "../api/types";
import { formatPercent, formatTokens } from "../lib/format";
import { cn } from "../lib/cn";
import { ErrorNote, Loading } from "./Feedback";
import { panelClass } from "./ui/card";
import { numCellClass, tableClass, thClass, theadRowClass, trClass } from "./ui/table";

export function BehaviorPanel({ report }: { report: AsyncState<BehaviorReport> }) {
  return (
    <section className={cn(panelClass, "p-4")}>
      <h2 className="text-sm font-medium">行为指标</h2>
      <p className="mt-1 text-xs text-muted-foreground">
        只统计已推送会话里的语义事件。失败率是工具失败除以工具调用，不是桌面端 Cursor 会话的失败率。
      </p>
      {report.error ? (
        <ErrorNote error={report.error} onRetry={report.reload} />
      ) : report.loading || !report.data ? (
        <Loading />
      ) : (
        <>
          <dl className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-4">
            <Stat label="失败率" value={report.data.failure_rate === null ? "—" : formatPercent(report.data.failure_rate)} />
            <Stat label="工具调用" value={formatTokens(report.data.tool_calls)} />
            <Stat label="工具失败" value={formatTokens(report.data.tool_failures)} />
            <Stat label="其它错误" value={formatTokens(report.data.other_errors)} />
          </dl>
          {report.data.tools.length === 0 ? (
            <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有工具调用</p>
          ) : (
            <div className="mt-3 overflow-x-auto">
              <table className={tableClass}>
                <caption className="sr-only">工具分布</caption>
                <thead>
                  <tr className={theadRowClass}>
                    <th scope="col" className={`px-3 py-2 ${thClass}`}>工具</th>
                    <th scope="col" className={`px-3 py-2 ${numCellClass} ${thClass}`}>调用</th>
                    <th scope="col" className={`px-3 py-2 ${numCellClass} ${thClass}`}>失败</th>
                  </tr>
                </thead>
                <tbody>
                  {report.data.tools.map((tool) => (
                    <tr key={tool.name} className={trClass}>
                      <th scope="row" className="px-3 py-2 text-left font-normal">{tool.name}</th>
                      <td className={`px-3 py-2 ${numCellClass}`}>{tool.calls}</td>
                      <td className={`px-3 py-2 ${numCellClass}`}>{tool.failures}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {report.data.tools_truncated && (
                <p className="mt-2 text-xs text-muted-foreground">只列出调用最多的 30 个工具，上面的合计是全量。</p>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-lg font-medium tabular-nums">{value}</dd>
    </div>
  );
}
