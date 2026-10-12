import { ArrowRight, FolderKanban, Users } from "lucide-react";
import type { Breakdown } from "../api/types";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import type { TokenSplit } from "../lib/tokens";
import { splitForRow } from "../lib/usageAgg";
import { TokenComposition } from "./TokenComposition";
import { cn } from "../lib/cn";
import { glassClass } from "./ui/card";
import { CHIP_CYCLE, IconChip } from "./ui/icon-chip";

const innerClass =
  "block rounded-xl border border-border bg-card/70 p-3.5 hover:border-primary/40 hover:bg-card";

export function DimensionCards({
  title,
  rows,
  mode,
  splits,
  hrefOf,
  labelOf,
  moreHref,
  kind = "project",
}: {
  title: string;
  rows: Breakdown[];
  mode: CostMode;
  splits?: Map<string, TokenSplit>;
  hrefOf?: (row: Breakdown) => string | undefined;
  labelOf?: (row: Breakdown) => string;
  moreHref?: string;
  kind?: "project" | "member";
}) {
  const Icon = kind === "member" ? Users : FolderKanban;
  return (
    <section className={cn(glassClass, "p-5")}>
      <div className="mb-4 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2.5">
          <IconChip tone="violet" size="sm">
            <Icon size={15} />
          </IconChip>
          <h3 className="text-[15px] font-semibold">{title}</h3>
        </div>
        {moreHref && (
          <a
            href={moreHref}
            className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-primary"
          >
            查看全部
            <ArrowRight size={13} aria-hidden="true" />
          </a>
        )}
      </div>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {rows.slice(0, 6).map((row, index) => {
            const href = hrefOf?.(row);
            const label = labelOf?.(row) ?? row.label;
            const split = splitForRow(splits, row);
            const inner = (
              <>
                <div className="flex items-center gap-2.5">
                  <IconChip tone={CHIP_CYCLE[index % CHIP_CYCLE.length]} size="sm">
                    <Icon size={14} />
                  </IconChip>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline justify-between gap-2">
                      <span className="truncate text-sm font-semibold">{label}</span>
                      <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
                        {formatCost(costOf(row, mode))}
                      </span>
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {formatTokens(row.total_tokens)} token · {row.record_count} 条
                    </div>
                  </div>
                </div>
                {split && (
                  <div className="mt-3">
                    <TokenComposition split={split} />
                  </div>
                )}
              </>
            );
            return href ? (
              <a key={row.key} href={href} className={innerClass}>
                {inner}
              </a>
            ) : (
              <div key={row.key} className={innerClass}>
                {inner}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
