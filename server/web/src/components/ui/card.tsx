import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

/** 放长表格、长列表的卡片：较不透明，不模糊。 */
export const panelClass = "panel rounded-2xl text-card-foreground";

/** 指标卡、图表卡等小面积卡片：毛玻璃。 */
export const glassClass = "glass rounded-2xl text-card-foreground";

export const cardPadClass = "p-4";

export function Card({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn(panelClass, className)} {...props} />;
}
