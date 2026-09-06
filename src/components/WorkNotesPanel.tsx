import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { humanStatus } from "../lib/format";
import { useWorkNotesProgress } from "../lib/useWorkNotesProgress";
import { workNotesEstimateCopy, workNotesUsageCopy } from "../lib/workNotesCopy";
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
import { WorkNotesHistory } from "./WorkNotesHistory";
import { WorkNotesShare } from "./WorkNotesShare";
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
  const [startError, setStartError] = useState<string | null>(null);
  const [pendingConfirm, setPendingConfirm] = useState(false);
  const [extraInstructions, setExtraInstructions] = useState("");
  const [preference, setPreference] = useState(loadWorkNotesPreference);
  const [pollRev, setPollRev] = useState(0);
  const progress = useWorkNotesProgress(pollRev);
  const running = progress?.status === "running";

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
    running ||
    previewLoading ||
    previewError !== null ||
    preview == null ||
    preview.gate === "rejected" ||
    preview.session_count === 0 ||
    engineId == null;
  const estimateCopy = preview ? workNotesEstimateCopy(preview) : null;
  const rawDto = progress?.status === "done" ? progress.result : null;
  const progressDto =
    rawDto &&
    preview &&
    rawDto.start_date === preview.start_date &&
    rawDto.end_date === preview.end_date
      ? rawDto
      : null;
  const dto = progressDto ?? preview?.cached ?? null;

  function persist(next: WorkNotesPreference) {
    saveWorkNotesPreference(next);
    setPreference(next);
  }

  useEffect(() => {
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切区间时先置 loading，避免沿用上一档会话数
    setPreviewLoading(true);
    setPreviewError(null);
    void invoke<WorkNotesPreviewDto>("preview_work_notes", {
      range,
      engineId,
      model: model.trim() === "" ? null : model.trim(),
    })
      .then((next) => {
        if (!cancelled) {
          setPreview(next);
          if (next.cached) {
            setExtraInstructions(next.cached.extra_instructions);
          }
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
  }, [engineId, model, range]);

  function resetGenerated() {
    setPendingConfirm(false);
    setStartError(null);
    setExtraInstructions("");
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
    setStartError(null);
    void invoke("start_work_notes", {
      range,
      engineId,
      model: model.trim() === "" ? null : model.trim(),
      extraInstructions,
      confirmed: preview?.gate === "confirm",
    })
      .then(() => {
        setPollRev((value) => value + 1);
      })
      .catch((caught: unknown) => {
        setStartError(humanStatus(caught));
      });
  }

  function stop() {
    void invoke("cancel_work_notes")
      .then(() => {
        setPollRev((value) => value + 1);
      })
      .catch((caught: unknown) => {
        setStartError(humanStatus(caught));
      });
  }

  const emptyHint =
    preview && preview.skipped_sparse > 0
      ? `已略过 ${preview.skipped_sparse} 个零星会话`
      : copy.emptyHint;
  const jobError = progress?.status === "error" ? progress.error : startError;
  const cancelled = progress?.status === "cancelled";

  return (
    <div className="work-notes">
      <div className="work-notes-head">
        <div className="work-notes-range">
          <Segmented
            value={rangeKind}
            options={RANGE_OPTIONS}
            ariaLabel="工作纪要区间"
            disabled={running}
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
                disabled={running || previewLoading}
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
                disabled={running || previewLoading}
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
              disabled={running}
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
                disabled={running}
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
        <div className="work-notes-actions">
          {running ? (
            <Button variant="danger" onClick={stop}>
              停止
            </Button>
          ) : (
            <Button variant="accent" disabled={generateDisabled} onClick={generate}>
              {pendingConfirm ? "确认生成" : cancelled ? "继续生成" : "生成"}
            </Button>
          )}
        </div>
      </div>
      <p className="work-notes-help">
        {selectedEngine
          ? selectedEngine.writes_session_dir
            ? "以只读、禁用工具的方式运行。会在你的会话记录里留下一条。"
            : "以只读、禁用工具的方式运行。"
          : copy.help}
      </p>
      <label className="work-notes-extra">
        <span className="work-notes-extra-label">补充指令</span>
        <textarea
          value={extraInstructions}
          onChange={(event) => setExtraInstructions(event.target.value)}
          placeholder="例如：我是后端，重点讲架构改动，别提 CSS"
          rows={3}
          disabled={running}
          aria-label="补充指令"
        />
      </label>
      {previewError ? (
        <EmptyState icon="alertTriangle" tone="warn" title="无法读取区间" hint={previewError} />
      ) : null}
      {previewLoading ? <p className="work-notes-scale">正在统计会话数…</p> : null}
      {!previewLoading && preview && !previewError ? (
        <p className="work-notes-scale">
          {preview.start_date} 至 {preview.end_date}，有 {preview.session_count} 个会话
          {preview.skipped_sparse > 0 ? `，已略过 ${preview.skipped_sparse} 个零星会话` : ""}
          {estimateCopy ? `。${estimateCopy}` : ""}
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
      {running && progress ? (
        <p className="work-notes-progress" role="status" aria-live="polite">
          已完成 {progress.done} / 共 {progress.total}
          {progress.current_title ? `，正在总结：${progress.current_title}` : ""}
        </p>
      ) : null}
      {cancelled ? (
        <p className="work-notes-progress">
          已停止。已完成的会话摘要还在，再次生成不会重复调用。
        </p>
      ) : null}
      {jobError ? (
        <EmptyState icon="alertTriangle" tone="warn" title="生成失败" hint={jobError} />
      ) : null}
      {dto && dto.failures.length > 0 ? <WorkNotesFailures dto={dto} /> : null}
      {!previewLoading &&
      !jobError &&
      !previewError &&
      preview &&
      preview.session_count === 0 &&
      !dto &&
      !running ? (
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
          hint="设置里已经检测过，但没有找到 Codex、Claude、Grok 或 Cursor Agent。装好后再点「检测」。"
        />
      ) : null}
      {!previewLoading &&
      !jobError &&
      !previewError &&
      !dto &&
      !running &&
      !cancelled &&
      preview &&
      preview.session_count > 0 &&
      preview.gate !== "rejected" &&
      installed.length > 0 ? (
        <EmptyState icon="notes" title="还没有生成工作纪要" hint={copy.emptyHint} />
      ) : null}
      {dto && !dto.has_data && dto.failed_count === 0 ? (
        <EmptyState
          icon="notes"
          title="这段时间没有可总结的会话"
          hint={
            dto.skipped_sparse > 0 ? `已略过 ${dto.skipped_sparse} 个零星会话` : copy.emptyHint
          }
        />
      ) : null}
      {dto?.has_data ? <WorkNotesShare dto={dto} /> : null}
      {dto ? <WorkNotesUsage dto={dto} /> : null}
      <WorkNotesHistory currentEngineId={engineId} />
    </div>
  );
}

function WorkNotesFailures({ dto }: { dto: WorkNotesDto }) {
  return (
    <div className="work-notes-failures">
      <p className="work-notes-failures-title">有 {dto.failed_count} 个会话总结失败</p>
      {dto.failures.map((failure, index) => (
        <pre key={`${failure.title}-${index}`} className="work-notes-stderr">
          {failure.title ? `${failure.title}\n` : ""}
          {failure.error}
        </pre>
      ))}
    </div>
  );
}

function WorkNotesUsage({ dto }: { dto: WorkNotesDto }) {
  const usage = workNotesUsageCopy(dto);
  if (!usage) {
    return null;
  }
  return <p className="work-notes-usage">{usage}</p>;
}
