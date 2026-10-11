import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

export const panelClass =
  "rounded-xl border border-border bg-card text-card-foreground shadow-[0_1px_2px_var(--shadow-soft)]";

export function Card({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn(panelClass, className)} {...props} />;
}
