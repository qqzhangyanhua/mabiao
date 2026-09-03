import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { copyReportImage } from "../lib/copyReportImage";
import { consumeEscape } from "../lib/escapeShortcut";
import { humanStatus } from "../lib/format";
import { periodRangeLabel, toPosterViewModel } from "../lib/reportCopy";
import { capturePoster } from "../report/capturePoster";
import { ReportPoster } from "../report/ReportPoster";
import type { ReportDto } from "../types";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

type CopyStatus = { tone: "ok" | "error"; text: string };

export function ReportDialog({ onClose }: { onClose: () => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const posterRef = useRef<HTMLElement>(null);
  const copyRun = useRef(0);
  const copyingRef = useRef(false);
  const [offset, setOffset] = useState(0);
  const [dto, setDto] = useState<ReportDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [copying, setCopying] = useState(false);
  const [copyStatus, setCopyStatus] = useState<CopyStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    copyRun.current += 1;
    copyingRef.current = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切周期时先置 loading，避免沿用上一周海报
    setLoading(true);
    setError(null);
    setCopying(false);
    setCopyStatus(null);
    void invoke<ReportDto>("get_report", { period: { kind: "week", offset } })
      .then((next) => {
        if (!cancelled) {
          setDto(next);
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(humanStatus(caught));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [offset]);

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

  const rangeLabel = dto ? periodRangeLabel(dto.start_date, dto.end_date) : "正在解析周期…";
  const poster = dto && !loading ? toPosterViewModel(dto) : null;
  const canCopy = Boolean(poster) && !copying;

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
            <h2 id="report-dialog-title">周报</h2>
            <Button variant="icon" onClick={onClose} aria-label="关闭周报">
              <Icon name="close" size={15} />
            </Button>
          </header>
          <p className="report-dialog-range">{rangeLabel}</p>
          <p className="muted">最近一个已经结束的完整自然周。当前进行中的一周不可选。</p>
          <div className="report-dialog-period-nav">
            <Button
              size="sm"
              onClick={() => setOffset((value) => value + 1)}
              disabled={loading || copying}
            >
              更早一周
            </Button>
            <Button
              size="sm"
              onClick={() => setOffset((value) => Math.max(0, value - 1))}
              disabled={loading || copying || offset === 0}
            >
              更近一周
            </Button>
          </div>
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
          {loading ? (
            <div className="report-dialog-status">
              <Spinner size={22} />
              <span>正在生成周报…</span>
            </div>
          ) : null}
          {!loading && error ? (
            <EmptyState icon="alertTriangle" tone="warn" title="周报加载失败" hint={error} />
          ) : null}
          {!loading && !error && dto && !dto.has_data ? (
            <EmptyState
              icon="calendar"
              title="这个周期没有消耗记录"
              hint="不会生成空海报。可以往前切到更早的一周。"
            />
          ) : null}
          {!loading && !error && poster ? (
            <ReportPoster data={poster} posterRef={posterRef} />
          ) : null}
        </section>
      </div>
    </div>
  );
}
