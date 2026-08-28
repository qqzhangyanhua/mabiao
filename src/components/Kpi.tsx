import type { ReactNode } from "react";
import { Icon, type IconName } from "../icons";

export type KpiTone = "purple" | "cyan" | "orange" | "blue";

export function toneColor(tone: KpiTone): string {
  if (tone === "cyan") return "#22d3ee";
  if (tone === "orange") return "#f59e0b";
  if (tone === "blue") return "#60a5fa";
  return "#8b6cff";
}

export function KpiCard({
  icon,
  tone,
  label,
  value,
  delta,
  spark,
  live,
  radar,
  hint,
  actionLabel,
  onClick,
}: {
  icon: IconName;
  tone: KpiTone;
  label: string;
  value: string;
  delta?: { text: string; tone: "up" | "down" | "flat" } | null;
  spark?: number[];
  live?: boolean;
  radar?: boolean;
  hint?: string;
  actionLabel?: string;
  onClick?: () => void;
}) {
  const interactive = onClick != null;
  const body = (
    <>
      {radar ? <div className="radar" /> : null}
      <div className="kpi-top">
        <span className="kpi-ico">
          <Icon name={icon} size={16} />
        </span>
        <span className="kpi-label">{label}</span>
        {live ? (
          <span className="live-tag">
            <i /> 实时
          </span>
        ) : null}
      </div>
      <div className="kpi-value">{value}</div>
      {hint ? <p className="kpi-hint">{hint}</p> : null}
      <div className="kpi-foot">
        {actionLabel ? (
          <span className="kpi-action">
            {actionLabel}
            <Icon name="chevron" size={12} className="flip" />
          </span>
        ) : delta ? (
          <span className={`delta ${delta.tone}`}>{delta.text}</span>
        ) : (
          <span />
        )}
        {spark ? <Spark values={spark} color={toneColor(tone)} /> : null}
      </div>
    </>
  );
  if (interactive) {
    return (
      <button
        type="button"
        className={`kpi tone-${tone} is-clickable`}
        aria-label={[label, value, hint, actionLabel].filter(Boolean).join("，")}
        onClick={onClick}
      >
        {body}
      </button>
    );
  }
  return <article className={`kpi tone-${tone}`}>{body}</article>;
}

export function Spark({ values, color }: { values: number[]; color: string }) {
  if (values.length < 2) {
    return <svg className="spark" viewBox="0 0 120 36" />;
  }
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const w = 120;
  const h = 36;
  const pts = values.map((v, i) => {
    const x = (i / (values.length - 1)) * w;
    const y = h - ((v - min) / span) * (h - 6) - 3;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const line = pts.join(" ");
  return (
    <svg className="spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none">
      <polygon points={`0,${h} ${line} ${w},${h}`} fill={color} opacity="0.16" />
      <polyline points={line} fill="none" stroke={color} strokeWidth="1.8" />
    </svg>
  );
}

export function LegendRow({
  color,
  label,
  value,
  extra,
  icon,
  onClick,
}: {
  color: string;
  label: string;
  value: string;
  extra?: string;
  icon?: ReactNode;
  onClick?: () => void;
}) {
  const className = icon ? "legend-row has-icon" : "legend-row";
  const content = (
    <>
      <i style={{ background: color }} />
      {icon}
      <span className="legend-label">{label}</span>
      <span className="legend-nums">
        <strong>{value}</strong>
        {extra ? <em>{extra}</em> : null}
      </span>
    </>
  );
  if (onClick) {
    return (
      <button type="button" className={`${className} is-clickable`} onClick={onClick}>
        {content}
      </button>
    );
  }
  return <div className={className}>{content}</div>;
}
