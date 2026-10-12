import { Boxes } from "lucide-react";
import type { Breakdown } from "../api/types";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import { TokenComposition } from "./TokenComposition";
import type { TokenSplit } from "../lib/tokens";
import { splitForRow } from "../lib/usageAgg";
import { cn } from "../lib/cn";
import { glassClass } from "./ui/card";
import { IconChip } from "./ui/icon-chip";

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
    <section className={cn(glassClass, "h-full p-5")}>
      <div className="mb-4 flex items-center gap-2.5">
        <IconChip tone="blue" size="sm">
          <Boxes size={15} />
        </IconChip>
        <h3 className="text-[15px] font-semibold">{title}</h3>
      </div>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <ol className="space-y-4">
          {rows.slice(0, 8).map((row) => {
            const split = splitForRow(splits, row);
            const label = labelOf?.(row) ?? row.label;
            return (
              <li key={row.key}>
                <div className="flex items-baseline justify-between gap-2 text-sm">
                  <span className="truncate font-semibold">{label}</span>
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {formatTokens(row.total_tokens)} · {formatCost(costOf(row, mode))}
                  </span>
                </div>
                {split && (
                  <div className="mt-2">
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
