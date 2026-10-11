import type { Breakdown } from "../api/types";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import { TokenComposition } from "./TokenComposition";
import type { TokenSplit } from "../lib/tokens";
import { splitForRow } from "../lib/usageAgg";
import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";

export function ModelRanking({
  title,
  rows,
  mode,
  splits,
  labelOf,
}: {
  title: string;
  rows: Breakdown[];
  mode: CostMode;
  splits?: Map<string, TokenSplit>;
  labelOf?: (row: Breakdown) => string;
}) {
  return (
    <section className={cn(panelClass, "p-4")}>
      <h3 className="mb-3 text-sm font-medium">{title}</h3>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <ol className="space-y-3">
          {rows.slice(0, 8).map((row) => {
            const split = splitForRow(splits, row);
            const label = labelOf?.(row) ?? row.label;
            return (
              <li key={row.key}>
                <div className="flex items-baseline justify-between gap-2 text-sm">
                  <span className="truncate font-medium">{label}</span>
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {formatTokens(row.total_tokens)} · {formatCost(costOf(row, mode))}
                  </span>
                </div>
                {split && (
                  <div className="mt-1.5">
                    <TokenComposition split={split} />
                  </div>
                )}
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
