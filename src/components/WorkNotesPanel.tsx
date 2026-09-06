import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { formatCompact, formatTokens, humanStatus } from "../lib/format";
import {
  clampWorkNotesCustomRange,
  thisWeekStartDate,
  todayDateValue,
  workNotesCustomPickerBounds,
  workNotesRangeCopy,
  workNotesRangePayload,
} from "../lib/workNotesRange";
import {
  engineSelectLabel,
  installedWorkNoteEngines,
  loadWorkNotesPreference,
  resolveWorkNotesEngine,
  saveWorkNotesPreference,
  type WorkNotesPreference,
} from "../lib/workNotesPreference";
import type { WorkNotesDto, WorkNotesPreviewDto, WorkNotesRangeKind } from "../types";
import { EmptyState } from "./EmptyState";
import { KpiCard } from "./Kpi";
import { LoadingOverlay } from "./LoadingOverlay";
import { Button } from "./ui/Button";
import { DatePicker } from "./ui/DatePicker";
import { Segmented } from "./ui/Segmented";
import { Select } from "./ui/Select";

const RANGE_OPTIONS = [
  { value: "this_week" as const, label: "本周" },
  { value: "this_month" as const, label: "本月" },
  { value: "custom" as const, label: "区间" },
];

export function WorkNotesPanel() {
  const [rangeKind, setRangeKind] = useState<WorkNotesRangeKind>("this_week");
  const [customFrom, setCustomFrom] = useState(() => thisWeekStartDate());
  const [customTo, setCustomTo] = useState(() => todayDateValue());
  const [preview, setPreview] = useState<WorkNotesPreviewDto | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(true);
  const [dto, setDto] = useState<WorkNotesDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pendingConfirm, setPendingConfirm] = useState(false);
  const [preference, setPreference] = useState(loadWorkNotesPreference);

  const range = useMemo(
    () => workNotesRangePayload(rangeKind, customFrom, customTo),
    [customFrom, customTo, rangeKind],
  );
  const copy = workNotesRangeCopy(rangeKind);
  const customBounds = workNotesCustomPickerBounds(customFrom, customTo);
  const installed = useMemo(
    () => installedWorkNoteEngines(preference.detected),
    [preference.detected],
  );
  const engineId = resolveWorkNotesEngine(preference.engineId, installed);
  const model = engineId ? (preference.models[engineId] ?? "") : "";
  const selectedEngine = installed.find((engine) => engine.id === engineId) ?? null;
  const generateDisabled =
    busy ||
    previewLoading ||
    previewError !== null ||
    preview == null ||
    preview.gate === "rejected" ||
    preview.session_count === 0 ||
    engineId == null;

  function persist(next: WorkNotesPreference) {
    saveWorkNotesPreference(next);
    setPreference(next);
  }

  useEffect(() => {
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切区间时先置 loading，避免沿用上一档会话数
    setPreviewLoading(true);
    setPreviewError(null);
    void invoke<WorkNotesPreviewDto>("preview_work_notes", { range })
      .then((next) => {
        if (!cancelled) {
          setPreview(next);
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setPreview(null);
          setPreviewError(humanStatus(caught));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setPreviewLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [range]);

  function resetGenerated() {
    setPendingConfirm(false);
    setDto(null);
    setError(null);
  }

  function generate() {
    if (engineId == null) {
      return;
    }
    if (preview?.gate === "confirm" && !pendingConfirm) {
      setPendingConfirm(true);
      return;
    }
    persist({ ...preference, engineId });
    setBusy(true);
    setError(null);
    void invoke<WorkNotesDto>("build_work_notes", {
      range,
      engine_id: engineId,
      model: model.trim() === "" ? null : model.trim(),
      confirmed: preview?.gate === "confirm",
    })
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

  const emptyHint =
    preview && preview.skipped_sparse > 0
      ? `已略过 ${preview.skipped_sparse} 个零星会话`
      : copy.emptyHint;

  return (
    <LoadingOverlay active={busy} label="正在生成工作纪要…">
      <div className="work-notes">
        <div className="work-notes-head">
          <div className="work-notes-range">
            <Segmented
              value={rangeKind}
              options={RANGE_OPTIONS}
              ariaLabel="工作纪要区间"
              disabled={busy}
              onChange={(next) => {
                if (next === "custom") {
                  const seeded = clampWorkNotesCustomRange(
                    preview?.start_date ?? thisWeekStartDate(),
                    preview?.end_date ?? todayDateValue(),
                  );
                  setCustomFrom(seeded.from);
                  setCustomTo(seeded.to);
                }
                setRangeKind(next);
                resetGenerated();
              }}
            />
            {rangeKind === "custom" ? (
              <div className="work-notes-custom-range">
                <DatePicker
                  ariaLabel="区间起始日"
                  value={customFrom}
                  min={customBounds.fromMin}
                  max={customBounds.fromMax}
                  disabled={busy || previewLoading}
                  onChange={(day) => {
                    const nextRange = clampWorkNotesCustomRange(day, customTo, new Date(), "from");
                    setCustomFrom(nextRange.from);
                    setCustomTo(nextRange.to);
                    resetGenerated();
                  }}
                />
                <span>至</span>
                <DatePicker
                  ariaLabel="区间结束日"
                  value={customTo}
                  min={customBounds.toMin}
                  max={customBounds.toMax}
                  disabled={busy || previewLoading}
                  onChange={(day) => {
                    const nextRange = clampWorkNotesCustomRange(customFrom, day, new Date(), "to");
                    setCustomFrom(nextRange.from);
                    setCustomTo(nextRange.to);
                    resetGenerated();
                  }}
                />
              </div>
            ) : null}
          </div>
          {installed.length > 0 && engineId ? (
            <div className="work-notes-engine">
              <Select
                ariaLabel="纪要引擎"
                value={engineId}
                options={installed.map((engine) => ({
                  value: engine.id,
                  label: engineSelectLabel(engine),
                }))}
                disabled={busy}
                align="left"
                onChange={(next) => {
                  persist({ ...preference, engineId: next });
                  resetGenerated();
                }}
              />
              <label className="work-notes-model">
                <span>模型</span>
                <input
                  value={model}
                  placeholder="默认"
                  aria-label="纪要模型"
                  disabled={busy}
                  onChange={(event) => {
                    persist({
                      ...preference,
                      engineId,
                      models: { ...preference.models, [engineId]: event.target.value },
                    });
                  }}
                />
              </label>
            </div>
          ) : null}
          <Button variant="accent" disabled={generateDisabled} onClick={generate}>
            {pendingConfirm ? "确认生成" : "生成"}
          </Button>
        </div>
        <p className="work-notes-help">
          {selectedEngine
            ? selectedEngine.writes_session_dir
              ? "以只读、禁用工具的方式运行。会在你的会话记录里留下一条。"
              : "以只读、禁用工具的方式运行。"
            : copy.help}
        </p>
        {previewError ? (
          <EmptyState icon="alertTriangle" tone="warn" title="无法读取区间" hint={previewError} />
        ) : null}
        {previewLoading ? <p className="work-notes-scale">正在统计会话数…</p> : null}
        {!previewLoading && preview && !previewError ? (
          <p className="work-notes-scale">
            {preview.start_date} 至 {preview.end_date}，有 {preview.session_count} 个会话
            {preview.skipped_sparse > 0 ? `，已略过 ${preview.skipped_sparse} 个零星会话` : ""}
          </p>
        ) : null}
        {!previewLoading && preview?.message ? (
          <p
            className={
              preview.gate === "rejected" ? "work-notes-gate-reject" : "work-notes-gate-confirm"
            }
          >
            {preview.message}
          </p>
        ) : null}
        {error ? (
          <EmptyState icon="alertTriangle" tone="warn" title="生成失败" hint={error} />
        ) : null}
        {!previewLoading &&
        !error &&
        !previewError &&
        preview &&
        preview.session_count === 0 &&
        !dto ? (
          <EmptyState icon="notes" title="这段时间没有可总结的会话" hint={emptyHint} />
        ) : null}
        {preference.detected.length === 0 ? (
          <EmptyState
            icon="notes"
            title="还没有检测本机纪要引擎"
            hint="请到设置 → 数据里点「检测」。选项只列出本机已安装的 CLI。"
          />
        ) : null}
        {preference.detected.length > 0 && installed.length === 0 ? (
          <EmptyState
            icon="notes"
            title="本机没有可用的纪要引擎"
            hint="设置里已经检测过，但没有找到 Codex 或 Claude。装好后再点「检测」。"
          />
        ) : null}
        {!previewLoading &&
        !error &&
        !previewError &&
        !dto &&
        preview &&
        preview.session_count > 0 &&
        preview.gate !== "rejected" &&
        installed.length > 0 ? (
          <EmptyState icon="notes" title="还没有生成工作纪要" hint={copy.emptyHint} />
        ) : null}
        {dto && !dto.has_data ? (
          <EmptyState
            icon="notes"
            title="这段时间没有可总结的会话"
            hint={
              dto.skipped_sparse > 0 ? `已略过 ${dto.skipped_sparse} 个零星会话` : copy.emptyHint
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
        <KpiCard icon="chat" tone="purple" label="会话" value={formatCompact(dto.session_count)} />
        <KpiCard icon="project" tone="cyan" label="项目" value={formatCompact(dto.project_count)} />
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
