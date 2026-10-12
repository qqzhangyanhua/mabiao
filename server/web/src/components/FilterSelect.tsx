import { ChevronDown } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn";

/** 玻璃工具栏里的下拉：左侧图标 + 标签，右侧自绘箭头，原生 select 保证键盘与读屏行为。 */
export function FilterSelect({
  icon,
  label,
  className,
  children,
  ...props
}: ComponentProps<"select"> & { icon: ReactNode; label: string }) {
  return (
    <label className="flex items-center gap-2 rounded-xl bg-muted/50 py-1 pr-1 pl-3 text-sm">
      <span aria-hidden="true" className="text-muted-foreground">
        {icon}
      </span>
      <span className="text-muted-foreground">{label}</span>
      <span className="relative">
        <select
          aria-label={label}
          className={cn(
            "h-8 max-w-48 appearance-none rounded-lg border border-border bg-card pr-8 pl-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring",
            className,
          )}
          {...props}
        >
          {children}
        </select>
        <ChevronDown
          aria-hidden="true"
          size={14}
          className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-muted-foreground"
        />
      </span>
    </label>
  );
}
