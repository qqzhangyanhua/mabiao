import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

export const panelClass =
  "rounded-lg border border-border bg-card text-card-foreground shadow-[var(--shadow-card)]";

export const cardPadClass = "p-4";

export function Card({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn(panelClass, className)} {...props} />;
}
