import type { ReactNode } from "react";
import { GeneratedFilterSelect, type GeneratedFilter } from "./GeneratedFilter";
import { panelClass } from "./ui/card";
import { cn } from "../lib/cn";

/** 成员/项目页会话列表区块：标题 + glass 工具栏筛选 + 内容。 */
export function SessionListSection({
  title,
  generated,
  onGeneratedChange,
  children,
  footer,
}: {
  title: ReactNode;
  generated: GeneratedFilter;
  onGeneratedChange: (value: GeneratedFilter) => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <section className={cn(panelClass, "p-4")}>
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-medium">{title}</h2>
        <div className="glass flex w-fit max-w-full flex-wrap items-center rounded-2xl p-1.5">
          <GeneratedFilterSelect value={generated} onChange={onGeneratedChange} />
        </div>
      </div>
      {children}
      {footer}
    </section>
  );
}
