import type { InstructionCheckupFinding, InstructionCheckupSeverity } from "../types";
import { SourceLabel } from "./SourceIcon";

const SEVERITY_LABEL: Record<InstructionCheckupSeverity, string> = {
  critical: "严重",
  high: "高",
  medium: "中",
  low: "低",
};

export function InstructionCheckup({ findings }: { findings: InstructionCheckupFinding[] }) {
  return (
    <section className="instruction-checkup">
      <h3>体检</h3>
      {findings.length === 0 ? (
        <p className="instruction-checkup-ok">未发现静默失效</p>
      ) : (
        <ul className="instruction-checkup-list">
          {findings.map((finding) => (
            <li
              key={`${finding.kind}:${finding.source}:${finding.display_path}`}
              className={`instruction-checkup-item severity-${finding.severity}`}
            >
              <div className="instruction-checkup-head">
                <em className="instruction-checkup-severity">{SEVERITY_LABEL[finding.severity]}</em>
                <strong>
                  <SourceLabel source={finding.source} fallback={finding.application} size={14} />
                </strong>
                {finding.display_path ? <span>{finding.display_path}</span> : null}
              </div>
              <p className="instruction-checkup-problem">{finding.problem}</p>
              <p className="instruction-checkup-consequence">{finding.consequence}</p>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
