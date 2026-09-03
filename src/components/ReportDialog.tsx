import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { consumeEscape } from "../lib/escapeShortcut";
import { humanStatus } from "../lib/format";
import { periodRangeLabel, toPosterViewModel } from "../lib/reportCopy";
import { ReportPoster } from "../report/ReportPoster";
import type { ReportDto } from "../types";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

export function ReportDialog({ onClose }: { onClose: () => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const [offset, setOffset] = useState(0);
  const [dto, setDto] = useState<ReportDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切周期时先置 loading，避免沿用上一周海报
    setLoading(true);
    setError(null);
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
            <Button size="sm" onClick={() => setOffset((value) => value + 1)} disabled={loading}>
              更早一周
            </Button>
            <Button
              size="sm"
              onClick={() => setOffset((value) => Math.max(0, value - 1))}
              disabled={loading || offset === 0}
            >
              更近一周
            </Button>
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
          {!loading && !error && poster ? <ReportPoster data={poster} /> : null}
        </section>
      </div>
    </div>
  );
}
