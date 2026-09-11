import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { consumeEscape } from "../lib/escapeShortcut";
import { conversationSourceLabel } from "../lib/conversationDisplay";
import { formatTokens, humanStatus, projectLabel, relativeTime } from "../lib/format";
import { formatEstimatedSecs } from "../lib/workNotesCopy";
import {
  allSessionKeys,
  selectedSessionRefs,
  workNotesSessionKey,
} from "../lib/workNotesSessions";
import type {
  WorkNotesPreviewDto,
  WorkNotesRange,
  WorkNotesSessionChoice,
  WorkNotesSessionRef,
} from "../types";
import { Button } from "./ui/Button";

export function WorkNotesSessionPicker({
  sessions,
  range,
  engineId,
  model,
  extraInstructions,
  initialPreview,
  startError,
  onClose,
  onConfirm,
}: {
  sessions: WorkNotesSessionChoice[];
  range: WorkNotesRange;
  engineId: string;
  model: string;
  extraInstructions: string;
  initialPreview: WorkNotesPreviewDto;
  startError: string | null;
  onClose: () => void;
  onConfirm: (sessions: WorkNotesSessionRef[], confirmed: boolean) => void;
}) {
  const titleId = "work-notes-session-picker-title";
  const dialogRef = useRef<HTMLDivElement>(null);
  const [keys, setKeys] = useState(() => allSessionKeys(sessions));
  const [subsetPreview, setSubsetPreview] = useState<{
    key: string;
    preview: WorkNotesPreviewDto;
  } | null>(null);
  const [subsetError, setSubsetError] = useState<{ key: string; message: string } | null>(null);
  const [pendingConfirm, setPendingConfirm] = useState(false);
  const refs = selectedSessionRefs(sessions, keys);
  const subsetKey = refs.map(workNotesSessionKey).join("\n");
  const isSubset = refs.length > 0 && refs.length < sessions.length;
  const estimate =
    isSubset && subsetPreview?.key === subsetKey ? subsetPreview.preview : initialPreview;
  const estimateLoading = isSubset && subsetPreview?.key !== subsetKey && subsetError?.key !== subsetKey;
  const estimateError = isSubset && subsetError?.key === subsetKey ? subsetError.message : null;

  useEffect(() => {
    if (!isSubset) {
      return;
    }
    const key = subsetKey;
    const selected = selectedSessionRefs(sessions, keys);
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void invoke<WorkNotesPreviewDto>("preview_work_notes", {
        range,
        engineId,
        model: model.trim() === "" ? null : model.trim(),
        extraInstructions,
        sessions: selected,
      })
        .then((next) => {
          if (!cancelled) {
            setSubsetPreview({ key, preview: next });
            setPendingConfirm(false);
          }
        })
        .catch((caught: unknown) => {
          if (!cancelled) {
            setSubsetError({ key, message: humanStatus(caught) });
          }
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [engineId, extraInstructions, isSubset, keys, model, range, sessions, subsetKey]);

  useEffect(() => {
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
    const initial =
      focusable().find((control) => !control.classList.contains("icon-btn")) ?? focusable()[0];
    initial?.focus();

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

  const selectedCount = keys.size;
  const empty = selectedCount === 0;
  const gate = empty ? "ok" : estimate.gate;
  const confirmDisabled = empty || gate === "rejected" || estimateLoading || estimateError !== null;

  function toggle(key: string) {
    setKeys((current) => {
      const next = new Set(current);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
    setPendingConfirm(false);
  }

  function confirm() {
    if (confirmDisabled) {
      return;
    }
    if (gate === "confirm" && !pendingConfirm) {
      setPendingConfirm(true);
      return;
    }
    onConfirm(selectedSessionRefs(sessions, keys), gate === "confirm");
  }

  return (
    <div
      className="work-notes-picker-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="work-notes-picker"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header className="work-notes-picker-head">
          <div>
            <h3 id={titleId}>选择要总结的会话</h3>
            <p className="work-notes-picker-count">
              已选 {selectedCount} / {sessions.length}
            </p>
          </div>
          <Button variant="icon" onClick={onClose} aria-label="关闭会话选择">
            <Icon name="close" size={15} />
          </Button>
        </header>
        <div className="work-notes-picker-toolbar">
          <Button variant="text" onClick={() => setKeys(allSessionKeys(sessions))}>
            全选
          </Button>
          <Button variant="text" onClick={() => setKeys(new Set())}>
            全不选
          </Button>
        </div>
        <ul className="work-notes-picker-list">
          {sessions.map((session) => {
            const key = workNotesSessionKey(session);
            const checked = keys.has(key);
            const title = session.title.trim() || "未命名会话";
            return (
              <li key={key}>
                <label className={checked ? "work-notes-picker-row is-on" : "work-notes-picker-row"}>
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggle(key)}
                    aria-label={title}
                  />
                  <span className="work-notes-picker-copy">
                    <span className="work-notes-picker-title" title={title}>
                      {title}
                      {session.cached ? (
                        <span className="work-notes-picker-cached">已有摘要</span>
                      ) : null}
                    </span>
                    <span className="work-notes-picker-meta">
                      {conversationSourceLabel(session.source)}
                      {" · "}
                      {projectLabel(session.project)}
                      {" · "}
                      {formatTokens(session.total_tokens)} tok
                      {session.started_at ? ` · ${relativeTime(session.started_at)}` : ""}
                    </span>
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
        {empty ? <p className="work-notes-picker-hint">请至少选择一个会话</p> : null}
        {!empty && estimateLoading ? <p className="work-notes-picker-hint">正在按所选会话重算预估…</p> : null}
        {!empty && !estimateLoading && estimateError ? (
          <p className="work-notes-picker-error" role="alert">
            {estimateError}
          </p>
        ) : null}
        {!empty && !estimateLoading && !estimateError ? (
          <p className="work-notes-picker-estimate">
            预计 {estimate.estimated_calls} 次调用 · {formatEstimatedSecs(estimate.estimated_secs)}
            {estimate.message ? ` · ${estimate.message}` : ""}
          </p>
        ) : null}
        {startError ? (
          <p className="work-notes-picker-error" role="alert">
            {startError}
          </p>
        ) : null}
        <footer>
          <Button onClick={onClose}>取消</Button>
          <Button variant="accent" disabled={confirmDisabled} onClick={confirm}>
            {pendingConfirm ? "确认生成" : `生成 ${selectedCount} 个会话`}
          </Button>
        </footer>
      </div>
    </div>
  );
}
