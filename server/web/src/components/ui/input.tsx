import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

const fieldClass =
  "h-9 rounded-lg border border-input bg-card px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50";

export function Input({ className, ...props }: ComponentProps<"input">) {
  return <input className={cn("w-full", fieldClass, className)} {...props} />;
}

export function NativeSelect({ className, ...props }: ComponentProps<"select">) {
  return <select className={cn("w-full", fieldClass, className)} {...props} />;
}
