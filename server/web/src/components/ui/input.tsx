import { ChevronDown } from "lucide-react";
import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

/** 表单字段标签，与 ProjectAdmin 等页面对齐。 */
export function FieldLabel({ className, ...props }: ComponentProps<"span">) {
  return <span className={cn("text-xs text-muted-foreground", className)} {...props} />;
}

const fieldClass =
  "h-9 rounded-lg border border-input bg-card px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50";

/** 原生 select 共用样式（FilterSelect、NativeSelect）。 */
export const nativeSelectClass =
  "appearance-none rounded-lg border border-input bg-card pl-3 pr-8 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50";

export function Input({ className, ...props }: ComponentProps<"input">) {
  return <input className={cn("w-full", fieldClass, className)} {...props} />;
}

export function NativeSelect({ className, ...props }: ComponentProps<"select">) {
  return (
    <span className={cn("relative inline-block max-w-full", className)}>
      <select className={cn("h-9 w-full", nativeSelectClass)} {...props} />
      <ChevronDown
        aria-hidden="true"
        size={14}
        className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-muted-foreground"
      />
    </span>
  );
}
