import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { copyReportImage } from "../lib/copyReportImage";
import { consumeEscape } from "../lib/escapeShortcut";
import { humanStatus } from "../lib/format";
import { periodRangeLabel, periodStatusCopy, toPosterViewModel } from "../lib/reportCopy";
import {
  clampCustomRange,
  customPickerBounds,
  lastCompletedWeekEnd,
  latestSelectableDate,
  periodEndFromOffset,
  periodOffsetFromDate,
  periodStartFromOffset,
  reportPeriodPayload,
  weekStartFromOffset,
} from "../lib/reportPeriod";
import { loadSharePreference, saveSharePreference } from "../lib/sharePreference";
import { capturePoster } from "../report/capturePoster";
import { ReportPoster } from "../report/ReportPoster";
import type { ReportPosterStyleId } from "../report/posterStyleRegistry";
import type { ReportDto, ReportPeriodKind } from "../types";
import { EmptyState } from "./EmptyState";
import { SharePosterStyles } from "./SharePosterStyles";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";
import { DatePicker } from "./ui/DatePicker";
import { Segmented } from "./ui/Segmented";

type CopyStatus = { tone: "ok" | "error"; text: string };

const PERIOD_KIND_OPTIONS = [
  { value: "week", label: "一周" },
  { value: "month", label: "一个月" },
  { value: "custom", label: "区间" },
] as const;

export function ReportDialog({ onClose }: { onClose: () => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const posterRef = useRef<HTMLElement>(null);
  const copyRun = useRef(0);
  const copyingRef = useRef(false);
  const [openedWith] = useState(loadSharePreference);
  const [posterStyleId, setPosterStyleId] = useState<ReportPosterStyleId>(openedWith.posterStyleId);
  const [periodKind, setPeriodKind] = useState<ReportPeriodKind>("week");
  const [offset, setOffset] = useState(0);
  const [customFrom, setCustomFrom] = useState(() => weekStartFromOffset(0));
  const [customTo, setCustomTo] = useState(() => lastCompletedWeekEnd());
  const [weekDto, setWeekDto] = useState<ReportDto | null>(null);
  const [weekError, setWeekError] = useState<string | null>(null);
  const [weekLoading, setWeekLoading] = useState(true);
  const [copying, setCopying] = useState(false);
  const [copyStatus, setCopyStatus] = useState<CopyStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    copyRun.current += 1;
    copyingRef.current = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切周期时先置 loading，避免沿用上一周海报
    setWeekLoading(true);
    setWeekError(null);
    setCopying(false);
    setCopyStatus(null);
    void invoke<ReportDto>("get_report", {
      period: reportPeriodPayload(periodKind, offset, customFrom, customTo),
    })
      .then((next) => {
        if (!cancelled) {
          setWeekDto(next);
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setWeekError(humanStatus(caught));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setWeekLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [customFrom, customTo, offset, periodKind]);

  useEffect(() => {
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
    focusable()[0]?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (consumeEscape(event)) {
        onClose();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      const controls = focusable();
      if (controls.length === 0) {
        event.preventDefault();
        dialog?.focus();
        return;
      }
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      previousFocus?.focus();
    };
  }, [onClose]);

  const rangeLabel = weekDto
    ? periodRangeLabel(weekDto.start_date, weekDto.end_date)
    : "正在解析周期…";
  const periodStatus = periodStatusCopy(periodKind);
  const customBounds = customPickerBounds(customFrom, customTo);
  const weekPoster = weekDto && !weekLoading ? toPosterViewModel(weekDto) : null;
  const canCopy = Boolean(weekPoster) && !copying;

  function persistPreference(nextStyleId: ReportPosterStyleId = posterStyleId) {
    saveSharePreference({ posterStyleId: nextStyleId });
  }

  function selectPosterStyle(nextStyleId: ReportPosterStyleId) {
    if (copyingRef.current) {
      return;
    }
    setPosterStyleId(nextStyleId);
    setCopyStatus(null);
    persistPreference(nextStyleId);
  }

  async function copyPoster() {
    const node = posterRef.current;
    if (!node) {
      setCopyStatus({ tone: "error", text: "找不到海报节点" });
      return;
    }
    if (copyingRef.current) {
      return;
    }
    copyingRef.current = true;
    const run = ++copyRun.current;
    setCopying(true);
    setCopyStatus(null);
    try {
      const dataUrl = await capturePoster(node);
      if (run !== copyRun.current) {
        return;
      }
      await copyReportImage(dataUrl);
      if (run !== copyRun.current) {
        return;
      }
      setCopyStatus({ tone: "ok", text: "已复制，可以去聊天窗口粘贴了" });
    } catch (caught: unknown) {
      if (run !== copyRun.current) {
        return;
      }
      setCopyStatus({ tone: "error", text: humanStatus(caught) });
    } finally {
      if (run === copyRun.current) {
        copyingRef.current = false;
        setCopying(false);
      }
    }
  }

  return (
    <div
      className="report-dialog-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="report-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="report-dialog-title"
        tabIndex={-1}
      >
        <aside className="report-dialog-params">
          <header className="report-dialog-params-head">
            <h2 id="report-dialog-title">分享</h2>
            <Button variant="icon" onClick={onClose} aria-label="关闭分享">
              <Icon name="close" size={15} />
            </Button>
          </header>
          <p className="report-dialog-range">{rangeLabel}</p>
          <p className="muted">{periodStatus.help}</p>
          <div className="report-dialog-kind">
            <Segmented
              value={periodKind}
              options={PERIOD_KIND_OPTIONS}
              disabled={copying || weekLoading}
              ariaLabel="报告周期"
              onChange={(next) => {
                if (copyingRef.current) {
                  return;
                }
                if (next === "custom") {
                  const seedKind = periodKind === "custom" ? "week" : periodKind;
                  const nextRange = clampCustomRange(
                    weekDto?.start_date ?? periodStartFromOffset(seedKind, offset),
                    weekDto?.end_date ?? periodEndFromOffset(seedKind, offset),
                  );
                  setCustomFrom(nextRange.from);
                  setCustomTo(nextRange.to);
                } else {
                  setOffset(0);
                }
                setPeriodKind(next);
              }}
            />
          </div>
          {periodKind === "custom" ? (
            <div className="report-dialog-custom-range">
              <DatePicker
                ariaLabel="区间起始日"
                value={customFrom}
                min={customBounds.fromMin}
                max={customBounds.fromMax}
                disabled={weekLoading || copying}
                onChange={(day) => {
                  if (copyingRef.current) {
                    return;
                  }
                  const nextRange = clampCustomRange(day, customTo, new Date(), "from");
                  setCustomFrom(nextRange.from);
                  setCustomTo(nextRange.to);
                }}
              />
              <span>至</span>
              <DatePicker
                ariaLabel="区间结束日"
                value={customTo}
                min={customBounds.toMin}
                max={customBounds.toMax}
                disabled={weekLoading || copying}
                onChange={(day) => {
                  if (copyingRef.current) {
                    return;
                  }
                  const nextRange = clampCustomRange(customFrom, day, new Date(), "to");
                  setCustomFrom(nextRange.from);
                  setCustomTo(nextRange.to);
                }}
              />
            </div>
          ) : (
            <>
              <DatePicker
                ariaLabel={periodKind === "month" ? "选择月报日期" : "选择周报日期"}
                value={periodStartFromOffset(periodKind, offset)}
                max={latestSelectableDate(periodKind)}
                disabled={weekLoading || copying}
                onChange={(day) => {
                  if (copyingRef.current) {
                    return;
                  }
                  setOffset(periodOffsetFromDate(periodKind, day));
                }}
              />
              <div className="report-dialog-period-nav">
                <Button
                  size="sm"
                  onClick={() => {
                    if (copyingRef.current) {
                      return;
                    }
                    setOffset((value) => value + 1);
                  }}
                  disabled={weekLoading || copying}
                >
                  {periodKind === "month" ? "更早一月" : "更早一周"}
                </Button>
                <Button
                  size="sm"
                  onClick={() => {
                    if (copyingRef.current) {
                      return;
                    }
                    setOffset((value) => Math.max(0, value - 1));
                  }}
                  disabled={weekLoading || copying || offset === 0}
                >
                  {periodKind === "month" ? "更近一月" : "更近一周"}
                </Button>
              </div>
            </>
          )}
          <SharePosterStyles
            selectedStyleId={posterStyleId}
            disabled={copying}
            onSelect={selectPosterStyle}
          />
          <div className="report-dialog-copy">
            <Button
              variant="accent"
              onClick={() => {
                if (copyingRef.current) {
                  return;
                }
                void copyPoster();
              }}
              disabled={!canCopy}
            >
              <Icon name={copyStatus?.tone === "ok" && !copying ? "check" : "copy"} size={14} />
              {copying ? "正在复制…" : "复制图片"}
            </Button>
            {copyStatus ? (
              <p
                className={`report-dialog-copy-status is-${copyStatus.tone}`}
                role={copyStatus.tone === "error" ? "alert" : "status"}
              >
                {copyStatus.text}
              </p>
            ) : null}
          </div>
        </aside>
        <section className="report-dialog-preview" aria-live="polite">
          {weekLoading ? (
            <div className="report-dialog-status">
              <Spinner size={22} />
              <span>{periodStatus.loading}</span>
            </div>
          ) : null}
          {!weekLoading && weekError ? (
            <EmptyState
              icon="alertTriangle"
              tone="warn"
              title={periodStatus.failed}
              hint={weekError}
            />
          ) : null}
          {!weekLoading && !weekError && weekDto && !weekDto.has_data ? (
            <EmptyState
              icon="calendar"
              title="这个周期没有消耗记录"
              hint={periodStatus.emptyHint}
            />
          ) : null}
          {!weekLoading && !weekError && weekPoster ? (
            <ReportPoster data={weekPoster} posterRef={posterRef} styleId={posterStyleId} />
          ) : null}
        </section>
      </div>
    </div>
  );
}
