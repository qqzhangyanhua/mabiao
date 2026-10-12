import { useId } from "react";
import { sparkPoints } from "../lib/chart";

const W = 88;
const H = 30;

/** 装饰性小趋势线；数值已在卡片正文里，读屏跳过。少于 2 个点不画。 */
export function Sparkline({ values, color }: { values: number[]; color: string }) {
  const gradient = `spark-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  if (values.length < 2) return null;
  const line = sparkPoints(values, W, H);
  return (
    <svg
      aria-hidden="true"
      width={W}
      height={H}
      viewBox={`0 0 ${W} ${H}`}
      className="pointer-events-none overflow-visible"
    >
      <defs>
        <linearGradient id={gradient} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={0.28} />
          <stop offset="100%" stopColor={color} stopOpacity={0} />
        </linearGradient>
      </defs>
      <polygon points={`2,${H} ${line} ${W - 2},${H}`} fill={`url(#${gradient})`} />
      <polyline
        points={line}
        fill="none"
        stroke={color}
        strokeWidth={1.75}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}
