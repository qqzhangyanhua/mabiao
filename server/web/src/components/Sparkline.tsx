import { useId } from "react";
import { sparkPoints } from "../lib/chart";

const W = 120;
const H = 32;

/** 装饰性小趋势线；数值已在卡片正文里，读屏跳过。少于 2 个点不画。 */
export function Sparkline({
  values,
  color,
  height = H,
}: {
  values: number[];
  color: string;
  height?: number;
}) {
  const gradient = `spark-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  if (values.length < 2) return null;
  const line = sparkPoints(values, W, H);
  return (
    <svg
      aria-hidden="true"
      width="100%"
      height={height}
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio="none"
      className="pointer-events-none block"
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
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
