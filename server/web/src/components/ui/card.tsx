import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

export const panelClass =
  "rounded-xl border border-border bg-[linear-gradient(180deg,var(--panel-grad-1),var(--panel-grad-2))] text-card-foreground shadow-[0_10px_30px_var(--shadow-soft)]";

export function Card({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn(panelClass, className)} {...props} />;
}
