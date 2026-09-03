import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { copyReportImage } from "../lib/copyReportImage";
import { consumeEscape } from "../lib/escapeShortcut";
import { humanStatus } from "../lib/format";
import { firstEligibleQuotaRow, quotaCardEmptyCopy, toQuotaCardViewModel } from "../lib/quotaCard";
import { periodRangeLabel, toPosterViewModel } from "../lib/reportCopy";
import { capturePoster } from "../report/capturePoster";
import { QuotaPoster } from "../report/QuotaPoster";
import { ReportPoster } from "../report/ReportPoster";
import type { OfficialQuotaDto, ReportDto } from "../types";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";

type CopyStatus = { tone: "ok" | "error"; text: string };
type CardKind = "week" | "quota";

const CARD_KIND_OPTIONS = [
  { value: "week", label: "周报" },
  { value: "quota", label: "额度" },
] as const;

export function ReportDialog({ onClose }: { onClose: () => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const posterRef = useRef<HTMLElement>(null);
  const copyRun = useRef(0);
  const copyingRef = useRef(false);
  const [kind, setKind] = useState<CardKind>("week");
  const [offset, setOffset] = useState(0);
  const [weekDto, setWeekDto] = useState<ReportDto | null>(null);
  const [weekError, setWeekError] = useState<string | null>(null);
  const [weekLoading, setWeekLoading] = useState(true);
  const [quotaDto, setQuotaDto] = useState<OfficialQuotaDto | null>(null);
  const [quotaNowMs, setQuotaNowMs] = useState(() => Date.now());
  const [quotaError, setQuotaError] = useState<string | null>(null);
  const [quotaLoading, setQuotaLoading] = useState(false);
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
    void invoke<ReportDto>("get_report", { period: { kind: "week", offset } })
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
  }, [offset]);

  useEffect(() => {
    if (kind !== "quota" || quotaDto !== null) {
      return;
    }
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切到额度时先置 loading，避免空态闪一下
    setQuotaLoading(true);
    setQuotaError(null);
    void invoke<OfficialQuotaDto>("get_official_quota")
      .then((next) => {
        if (!cancelled) {
          setQuotaDto(next);
          setQuotaNowMs(Date.now());
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setQuotaError(humanStatus(caught));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setQuotaLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [kind, quotaDto]);

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
  const weekPoster = kind === "week" && weekDto && !weekLoading ? toPosterViewModel(weekDto) : null;
  const quotaRow =
    kind === "quota" && quotaDto && !quotaLoading ? firstEligibleQuotaRow(quotaDto) : null;
  const quotaPoster = quotaRow ? toQuotaCardViewModel(quotaRow, quotaNowMs) : null;
  const canCopy = Boolean(kind === "week" ? weekPoster : quotaPoster) && !copying;

  function selectKind(next: CardKind) {
    if (copying) {
      return;
    }
    setKind(next);
    setCopyStatus(null);
    if (next === "quota" && quotaDto === null) {
      setQuotaLoading(true);
    }
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
          <div className="report-dialog-kind">
            <Segmented
              value={kind}
              options={CARD_KIND_OPTIONS}
              disabled={copying}
              ariaLabel="卡片类型"
              onChange={selectKind}
            />
          </div>
          {kind === "week" ? (
            <>
              <p className="report-dialog-range">{rangeLabel}</p>
              <p className="muted">最近一个已经结束的完整自然周。当前进行中的一周不可选。</p>
              <div className="report-dialog-period-nav">
                <Button
                  size="sm"
                  onClick={() => setOffset((value) => value + 1)}
                  disabled={weekLoading || copying}
                >
                  更早一周
                </Button>
                <Button
                  size="sm"
                  onClick={() => setOffset((value) => Math.max(0, value - 1))}
                  disabled={weekLoading || copying || offset === 0}
                >
                  更近一周
                </Button>
              </div>
            </>
          ) : quotaRow ? (
            <>
              <p className="report-dialog-range">{quotaRow.application}</p>
              {quotaRow.plan ? <p className="muted">{quotaRow.plan}</p> : null}
            </>
          ) : (
            <p className="muted">只使用可见且已有官方额度快照的账号。</p>
          )}
          <div className="report-dialog-copy">
            <Button variant="accent" onClick={() => void copyPoster()} disabled={!canCopy}>
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
          {kind === "week" ? (
            <>
              {weekLoading ? (
                <div className="report-dialog-status">
                  <Spinner size={22} />
                  <span>正在生成周报…</span>
                </div>
              ) : null}
              {!weekLoading && weekError ? (
                <EmptyState
                  icon="alertTriangle"
                  tone="warn"
                  title="周报加载失败"
                  hint={weekError}
                />
              ) : null}
              {!weekLoading && !weekError && weekDto && !weekDto.has_data ? (
                <EmptyState
                  icon="calendar"
                  title="这个周期没有消耗记录"
                  hint="不会生成空海报。可以往前切到更早的一周。"
                />
              ) : null}
              {!weekLoading && !weekError && weekPoster ? (
                <ReportPoster data={weekPoster} posterRef={posterRef} />
              ) : null}
            </>
          ) : (
            <>
              {quotaLoading ? (
                <div className="report-dialog-status">
                  <Spinner size={22} />
                  <span>正在读取官方额度…</span>
                </div>
              ) : null}
              {!quotaLoading && quotaError ? (
                <EmptyState
                  icon="alertTriangle"
                  tone="warn"
                  title="额度加载失败"
                  hint={quotaError}
                />
              ) : null}
              {!quotaLoading && !quotaError && quotaDto && !quotaPoster ? (
                <EmptyState icon="clock" {...quotaCardEmptyCopy(quotaDto)} />
              ) : null}
              {!quotaLoading && !quotaError && quotaPoster ? (
                <QuotaPoster data={quotaPoster} posterRef={posterRef} />
              ) : null}
            </>
          )}
        </section>
      </div>
    </div>
  );
}
