import type { Breakdown } from "../api/types";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import type { TokenSplit } from "../lib/tokens";
import { splitForRow } from "../lib/usageAgg";
import { TokenComposition } from "./TokenComposition";
import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";

export function DimensionCards({
  title,
  rows,
  mode,
  splits,
  hrefOf,
  labelOf,
}: {
  title: string;
  rows: Breakdown[];
  mode: CostMode;
  splits?: Map<string, TokenSplit>;
  hrefOf?: (row: Breakdown) => string | undefined;
  labelOf?: (row: Breakdown) => string;
}) {
  return (
    <section>
      <h3 className="mb-3 text-sm font-medium">{title}</h3>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {rows.slice(0, 6).map((row) => {
            const href = hrefOf?.(row);
            const label = labelOf?.(row) ?? row.label;
            const split = splitForRow(splits, row);
            const inner = (
              <>
                <div className="flex items-baseline justify-between gap-2">
                  <span className="truncate font-medium">{label}</span>
                  <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
                    {formatCost(costOf(row, mode))}
                  </span>
                </div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {formatTokens(row.total_tokens)} token · {row.record_count} 条
                </div>
                {split && (
                  <div className="mt-3">
                    <TokenComposition split={split} />
                  </div>
                )}
              </>
            );
            return href ? (
              <a key={row.key} href={href} className={cn(panelClass, "p-4 hover:border-primary/40")}>
                {inner}
              </a>
            ) : (
              <div key={row.key} className={cn(panelClass, "p-4")}>
                {inner}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
