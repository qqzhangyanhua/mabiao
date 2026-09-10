import { formatBytes, formatClock, formatCompact } from "../lib/format";
import type { InstructionImbalance, InstructionInvestment } from "../types";
import { SourceLabel } from "./SourceIcon";

export function InstructionInsight({
  investments,
  imbalances,
}: {
  investments: InstructionInvestment[];
  imbalances: InstructionImbalance[];
}) {
  return (
    <section className="panel instruction-insight">
      <div className="panel-head">
        <div>
          <h2>投入与用量</h2>
          <p className="panel-note">对照已加载指令的字节数和本机用量。久未修改不是问题。</p>
        </div>
      </div>
      <ul className="instruction-insight-list">
        {investments.map((row) => (
          <li key={row.source} className="instruction-insight-row">
            <strong>
              <SourceLabel source={row.source} fallback={row.application} size={14} />
            </strong>
            <span>{formatBytes(row.loaded_bytes)}</span>
            <span>{formatClock(row.modified_at)}</span>
            <span>{formatCompact(row.total_tokens)} tok</span>
          </li>
        ))}
      </ul>
      {imbalances.length > 0 ? (
        <ul className="instruction-insight-imbalances">
          {imbalances.map((item) => (
            <li key={item.source}>{item.note}</li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
