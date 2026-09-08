import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../icons";
import { consumeEscape } from "../lib/escapeShortcut";
import { humanStatus } from "../lib/format";
import {
  engineSelectLabel,
  installedWorkNoteEngines,
  loadWorkNotesPreference,
  resolveWorkNotesEngine,
  saveWorkNotesPreference,
  type WorkNotesPreference,
} from "../lib/workNotesPreference";
import type {
  ConversationSessionRow,
  DetectedEngine,
  WorkNotesSessionSummary,
} from "../types";
import { Button } from "./ui/Button";
import { Select } from "./ui/Select";

export function ConversationSummaryDialog({
  session,
  onClose,
  onGenerated,
}: {
  session: ConversationSessionRow;
  onClose: () => void;
  onGenerated: (summaries: WorkNotesSessionSummary[]) => void;
}) {
  const titleId = "conversation-summary-dialog-title";
  const dialogRef = useRef<HTMLDivElement>(null);
  const busyRef = useRef(false);
  const onCloseRef = useRef(onClose);
  const [preference, setPreference] = useState(loadWorkNotesPreference);
  const [busy, setBusy] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const installed = useMemo(
    () => installedWorkNoteEngines(preference.detected),
    [preference.detected],
  );
  const engineId = resolveWorkNotesEngine(preference.engineId, installed);
  const model = engineId ? (preference.models[engineId] ?? "") : "";
  const selected = installed.find((engine) => engine.id === engineId) ?? null;
  const generateDisabled = busy || detecting || engineId == null;

  useEffect(() => {
    busyRef.current = busy;
    onCloseRef.current = onClose;
  });

  function persist(next: WorkNotesPreference) {
    saveWorkNotesPreference(next);
    setPreference(next);
  }

  function detect() {
    setDetecting(true);
    setError(null);
    void invoke<DetectedEngine[]>("detect_work_note_engines")
      .then((detected) => {
        persist({ ...preference, detected });
      })
      .catch((caught: unknown) => {
        setError(humanStatus(caught));
      })
      .finally(() => {
        setDetecting(false);
      });
  }

  function generate() {
    if (engineId == null) {
      return;
    }
    persist({ ...preference, engineId });
    setBusy(true);
    setError(null);
    void invoke<WorkNotesSessionSummary[]>("summarize_conversation_session", {
      source: session.source,
      sessionId: session.session_id,
      engineId,
      model: model.trim() === "" ? null : model.trim(),
    })
      .then((summaries) => {
        onGenerated(summaries);
      })
      .catch((caught: unknown) => {
        setBusy(false);
        setError(humanStatus(caught));
      });
  }

  function stop() {
    void invoke("cancel_work_notes").catch((caught: unknown) => {
      setError(humanStatus(caught));
    });
  }

  function requestClose() {
    if (busyRef.current) {
      void invoke("cancel_work_notes");
    }
    onClose();
  }

  useEffect(() => {
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), select, input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
    const initial =
      focusable().find((control) => !control.classList.contains("icon-btn")) ?? focusable()[0];
    initial?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (consumeEscape(event)) {
        if (busyRef.current) {
          void invoke("cancel_work_notes");
        }
        onCloseRef.current();
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
  }, []);

  return (
    <div
      className="conversation-summary-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget && !busy) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="conversation-summary-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header>
          <div>
            <h3 id={titleId}>生成摘要</h3>
            <p className="conversation-summary-session" title={session.title}>
              {session.title}
            </p>
          </div>
          <Button variant="icon" onClick={requestClose} aria-label="关闭生成摘要">
            <Icon name="close" size={15} />
          </Button>
        </header>
        {installed.length > 0 && engineId ? (
          <div className="conversation-summary-fields">
            <div className="conversation-summary-field">
              <div className="conversation-summary-field-head">
                <span>引擎</span>
                <Button variant="text" disabled={detecting || busy} onClick={detect}>
                  {detecting ? "正在检测…" : "检测"}
                </Button>
              </div>
              <Select
                ariaLabel="纪要引擎"
                value={engineId}
                options={installed.map((engine) => ({
                  value: engine.id,
                  label: engineSelectLabel(engine),
                }))}
                disabled={busy}
                align="left"
                onChange={(next) => persist({ ...preference, engineId: next })}
              />
            </div>
            <label className="conversation-summary-field">
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
        ) : (
          <div className="conversation-summary-empty">
            <p className="conversation-summary-help">
              {preference.detected.length === 0
                ? "还没有检测过纪要引擎。点「检测」会跑 which 和 --version。"
                : "本机没有可用的纪要引擎。"}
            </p>
            <Button disabled={detecting || busy} onClick={detect}>
              {detecting ? "正在检测…" : "检测"}
            </Button>
          </div>
        )}
        {selected ? (
          <p className="conversation-summary-hint">
            {selected.writes_session_dir
              ? "以只读、禁用工具的方式运行。会在你的会话记录里留下一条。"
              : "以只读、禁用工具的方式运行。"}
          </p>
        ) : null}
        {busy ? (
          <p className="conversation-summary-hint" role="status">
            正在生成…
          </p>
        ) : null}
        {error ? (
          <p className="conversation-summary-error" role="alert">
            {error}
          </p>
        ) : null}
        <footer>
          <div className="conversation-summary-actions">
            <Button disabled={busy} onClick={onClose}>
              取消
            </Button>
            {busy ? (
              <Button variant="danger" onClick={stop}>
                停止
              </Button>
            ) : (
              <Button variant="accent" disabled={generateDisabled} onClick={generate}>
                生成
              </Button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
