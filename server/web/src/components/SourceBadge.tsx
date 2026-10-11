import type { ReactNode } from "react";
import { cn } from "../lib/cn";
import { sourceTone } from "../lib/chart";
import { sourceLabel } from "../lib/source";

const pillClass =
  "inline-flex max-w-full items-center truncate rounded-full border px-2 py-0.5 text-xs font-medium";

export function MetaPill({
  href,
  children,
  className,
}: {
  href?: string;
  children: ReactNode;
  className?: string;
}) {
  const merged = cn(
    pillClass,
    "border-border/80 bg-muted/70 text-foreground",
    href && "hover:border-primary/40 hover:text-primary",
    className,
  );
  if (href) {
    return (
      <a href={href} className={merged}>
        {children}
      </a>
    );
  }
  return <span className={merged}>{children}</span>;
}

export function SourceBadge({ source, className }: { source: string; className?: string }) {
  const tone = sourceTone(source);
  return (
    <span
      className={cn(pillClass, className)}
      style={{
        color: tone,
        background: `color-mix(in srgb, ${tone} 14%, transparent)`,
        borderColor: `color-mix(in srgb, ${tone} 30%, transparent)`,
      }}
    >
      {sourceLabel(source)}
    </span>
  );
}
