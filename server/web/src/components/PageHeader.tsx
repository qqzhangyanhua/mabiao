import type { ReactNode } from "react";
import { IconChip } from "./ui/icon-chip";

export function PageHeader({
  title,
  subtitle,
  extra,
  icon,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  extra?: ReactNode;
  icon?: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="flex min-w-0 items-center gap-3.5">
        {icon ? <IconChip size="lg">{icon}</IconChip> : null}
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h1>
          {subtitle ? <p className="mt-1 text-sm text-muted-foreground">{subtitle}</p> : null}
        </div>
      </div>
      {extra}
    </div>
  );
}
