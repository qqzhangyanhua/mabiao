import type { ReactNode } from "react";
import { Icon } from "../icons";
import { KpiCard, Spark } from "./Kpi";

type Delta = { text: string; tone: "up" | "down" | "flat" } | null;

export function OverviewKpiSection({
  tokenValue,
  tokenDelta,
  sessionValue,
  sessionDelta,
  costValue,
  costDelta,
  costTitle,
  costDetail,
  costActionLabel,
  onCostClick,
  dailyValue,
  dailyDelta,
  spark,
  costSpark,
  live,
}: {
  tokenValue: string;
  tokenDelta: Delta;
  sessionValue: string;
  sessionDelta: Delta;
  costValue: string;
  costDelta: Delta | null;
  costTitle: string | undefined;
  costDetail: ReactNode;
  costActionLabel: string | undefined;
  onCostClick: (() => void) | undefined;
  dailyValue: string;
  dailyDelta: Delta;
  spark: number[];
  costSpark: number[];
  live: boolean;
}) {
  return (
    <section className="kpi-row">
      <KpiCard
        icon="tokens"
        tone="purple"
        label="总 Token 使用量"
        value={tokenValue}
        delta={tokenDelta}
        spark={spark}
      />
      <KpiCard
        icon="chat"
        tone="cyan"
        label="总会话数"
        value={sessionValue}
        delta={sessionDelta}
        spark={spark}
      />
      <KpiCard
        icon="cost"
        tone="orange"
        label="总费用估算"
        value={costValue}
        delta={costDelta}
        spark={costSpark}
        title={costTitle}
        detail={costDetail}
        actionLabel={costActionLabel}
        onClick={onCostClick}
      />
      <KpiCard
        icon="daily"
        tone="blue"
        label="日均 Token 使用量"
        value={dailyValue}
        delta={dailyDelta}
        spark={spark}
        live={live}
        radar
      />
    </section>
  );
}

export function OverviewStatusBar({
  costValue,
  costSourceLine,
  unpriced,
  cacheRead,
  cacheCreation,
  reasoning,
  cacheHitRateLabel,
  rate,
  spark,
  sparkColor,
  updatedAt,
}: {
  costValue: string;
  costSourceLine: string | null;
  unpriced: boolean;
  cacheRead: string;
  cacheCreation: string;
  reasoning: string;
  cacheHitRateLabel: string;
  rate: string;
  spark: number[];
  sparkColor: string;
  updatedAt: string;
}) {
  return (
    <footer className="status-bar">
      <div className="stat-block">
        <span className="muted">费用（估算）</span>
        <strong>{costValue}</strong>
        <em>{costSourceLine ?? (unpriced ? "部分模型单价未配置" : "已按单价核算")}</em>
      </div>
      <div className="stat-block">
        <span className="muted">缓存 / 推理</span>
        <strong>
          {cacheRead} / {cacheCreation} / {reasoning}
        </strong>
        <em>读 / 写 / 推理 · 命中率 {cacheHitRateLabel}（近似口径）</em>
      </div>
      <div className="stat-block">
        <span className="muted">Token 速率（估算）</span>
        <strong>
          {rate} <small>/min</small>
        </strong>
        <Spark values={spark} color={sparkColor} />
      </div>
      <div className="stat-block last">
        <span className="muted">
          <Icon name="clock" size={13} /> 数据更新时间
        </span>
        <strong className="clock">{updatedAt}</strong>
      </div>
    </footer>
  );
}
