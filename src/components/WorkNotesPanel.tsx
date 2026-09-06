import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { formatCompact, formatTokens, humanStatus } from "../lib/format";
import type { WorkNotesDto, WorkNotesRangeKind } from "../types";
import { EmptyState } from "./EmptyState";
import { KpiCard } from "./Kpi";
import { LoadingOverlay } from "./LoadingOverlay";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";

const RANGE_OPTIONS = [{ value: "this_week" as const, label: "本周" }];

export function WorkNotesPanel() {
  const [rangeKind, setRangeKind] = useState<WorkNotesRangeKind>("this_week");
  const [dto, setDto] = useState<WorkNotesDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function generate() {
    setBusy(true);
    setError(null);
    void invoke<WorkNotesDto>("build_work_notes", { range: { kind: rangeKind } })
      .then((next) => {
        setDto(next);
      })
      .catch((caught: unknown) => {
        setDto(null);
        setError(humanStatus(caught));
      })
      .finally(() => {
        setBusy(false);
      });
  }

  return (
    <LoadingOverlay active={busy} label="正在生成工作纪要…">
      <div className="work-notes">
        <div className="work-notes-head">
          <Segmented
            value={rangeKind}
            options={RANGE_OPTIONS}
            ariaLabel="工作纪要区间"
            disabled={busy}
            onChange={setRangeKind}
          />
          <Button variant="accent" disabled={busy} onClick={generate}>
            生成
          </Button>
        </div>
        {error ? (
          <EmptyState icon="alertTriangle" tone="warn" title="生成失败" hint={error} />
        ) : null}
        {!error && !dto ? (
          <EmptyState
            icon="notes"
            title="还没有生成工作纪要"
            hint="选「本周」后点生成。本周一到此刻的对话会交给本机 Codex 总结。"
          />
        ) : null}
        {dto && !dto.has_data ? (
          <EmptyState
            icon="notes"
            title="这段时间没有可总结的会话"
            hint={
              dto.skipped_sparse > 0
                ? `已略过 ${dto.skipped_sparse} 个零星会话`
                : "换一段时间再试，或先去对话记录确认有正文。"
            }
          />
        ) : null}
        {dto?.has_data ? <WorkNotesResult dto={dto} /> : null}
      </div>
    </LoadingOverlay>
  );
}

function WorkNotesResult({ dto }: { dto: WorkNotesDto }) {
  return (
    <div className="work-notes-result">
      <div className="kpi-row work-notes-kpis">
        <KpiCard
          icon="chat"
          tone="purple"
          label="会话"
          value={formatCompact(dto.session_count)}
        />
        <KpiCard
          icon="project"
          tone="cyan"
          label="项目"
          value={formatCompact(dto.project_count)}
        />
        <KpiCard
          icon="calendar"
          tone="orange"
          label="活跃天数"
          value={formatCompact(dto.active_days)}
        />
        <KpiCard
          icon="tokens"
          tone="blue"
          label="区间 Token"
          value={formatTokens(dto.total_tokens)}
        />
      </div>
      {dto.headline ? <h2 className="work-notes-headline">{dto.headline}</h2> : null}
      <ol className="work-notes-entries">
        {dto.entries.map((entry, index) => (
          <li key={`${entry.title}-${index}`} className="work-notes-entry">
            <div className="work-notes-entry-title">{entry.title}</div>
            <div className="work-notes-entry-detail">{entry.detail}</div>
            {entry.project ? <div className="work-notes-entry-project">{entry.project}</div> : null}
          </li>
        ))}
      </ol>
      {dto.closing ? <p className="work-notes-closing">{dto.closing}</p> : null}
      {dto.skipped_sparse > 0 ? (
        <p className="work-notes-skipped">已略过 {dto.skipped_sparse} 个零星会话</p>
      ) : null}
    </div>
  );
}
