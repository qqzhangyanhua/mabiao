import type { ReactNode } from "react";
import { Icon, type IconName } from "../icons";

/**
 * 统一的空状态展示组件，用于表格、列表、面板等场景，
 * 保证图标 + 文案 + 提示的视觉规范一致。
 */
export function EmptyState({
  icon = "inbox",
  title,
  hint,
  tone = "muted",
  compact = false,
  className,
  action,
}: {
  icon?: IconName;
  title: string;
  hint?: string;
  tone?: "muted" | "warn";
  compact?: boolean;
  className?: string;
  action?: ReactNode;
}) {
  const classes = ["empty-state"];
  if (compact) classes.push("empty-state-compact");
  if (tone === "warn") classes.push("empty-state-warn");
  if (className) classes.push(className);

  return (
    <div className={classes.join(" ")}>
      <Icon name={icon} size={compact ? 18 : 26} className="empty-state-icon" />
      <div className="empty-state-title">{title}</div>
      {hint ? <div className="empty-state-hint">{hint}</div> : null}
      {action ? <div className="empty-state-action">{action}</div> : null}
    </div>
  );
}
