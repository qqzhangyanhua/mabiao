import { useEffect, useRef } from "react";
import { conversationWorkNotesSummaries } from "../lib/conversationDisplay";
import { consumeEscape } from "../lib/escapeShortcut";
import { workNotesEngineLabel } from "../lib/workNotesPreference";
import type { ConversationSessionRow } from "../types";
import { Button } from "./ui/Button";

export function ConversationSummaryViewDialog({
  session,
  onClose,
}: {
  session: ConversationSessionRow;
  onClose: () => void;
}) {
  const titleId = "conversation-summary-view-title";
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const summaries = conversationWorkNotesSummaries(session);

  useEffect(() => {
    onCloseRef.current = onClose;
  });

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
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="conversation-summary-dialog is-view"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header>
          <div>
            <h3 id={titleId}>工作纪要摘要</h3>
            <p className="conversation-summary-session" title={session.title}>
              {session.title}
            </p>
          </div>
        </header>
        <div className="conversation-summary-body">
          {summaries.length === 0 ? (
            <p className="conversation-summary-help">这条会话还没有摘要。</p>
          ) : (
            summaries.map((item) => (
              <div
                className="conversation-work-notes-item"
                key={`${item.engine}:${item.model}:${item.created_at}`}
              >
                <p>{item.summary}</p>
                <span>
                  {workNotesEngineLabel(item.engine)}
                  {item.model.trim() ? ` · ${item.model}` : ""}
                </span>
              </div>
            ))
          )}
        </div>
        <footer>
          <div className="conversation-summary-actions">
            <Button onClick={onClose}>关闭</Button>
          </div>
        </footer>
      </div>
    </div>
  );
}
