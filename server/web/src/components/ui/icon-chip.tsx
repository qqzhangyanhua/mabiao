import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

export type ChipTone = "brand" | "violet" | "blue" | "cyan" | "amber" | "emerald" | "rose";

const TONE: Record<ChipTone, string> = {
  brand: "bg-brand text-primary-foreground shadow-[0_6px_16px_-8px_var(--primary)]",
  violet: "bg-violet-500/15 text-violet-600 dark:text-violet-300",
  blue: "bg-blue-500/15 text-blue-600 dark:text-blue-300",
  cyan: "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300",
  amber: "bg-amber-500/15 text-amber-600 dark:text-amber-300",
  emerald: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300",
  rose: "bg-rose-500/15 text-rose-600 dark:text-rose-300",
};

export const CHIP_CYCLE: ChipTone[] = ["violet", "blue", "emerald", "cyan", "amber", "rose"];

export function IconChip({
  children,
  tone = "brand",
  size = "md",
  className,
}: {
  children: ReactNode;
  tone?: ChipTone;
  size?: "sm" | "md" | "lg";
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "grid shrink-0 place-items-center",
        size === "sm" && "size-7 rounded-lg",
        size === "md" && "size-9 rounded-xl",
        size === "lg" && "size-11 rounded-2xl",
        TONE[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
