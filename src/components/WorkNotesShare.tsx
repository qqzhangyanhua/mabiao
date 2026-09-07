import { useRef, useState } from "react";
import { Icon } from "../icons";
import { copyReportImage } from "../lib/copyReportImage";
import { humanStatus } from "../lib/format";
import { toWorkNotesPosterViewModel } from "../lib/workNotesPoster";
import {
  loadWorkNotesPosterPreference,
  saveWorkNotesPosterPreference,
} from "../lib/workNotesPosterPreference";
import { workNotesPlainText } from "../lib/workNotesCopy";
import { capturePoster } from "../report/capturePoster";
import type { WorkNotesDto } from "../types";
import { WorkNotesPoster } from "../workNotes/WorkNotesPoster";
import type { WorkNotesPosterStyleId } from "../workNotes/posterStyleRegistry";
import { ReportPreviewFrame } from "./ReportPreviewFrame";
import { WorkNotesPosterStyles } from "./WorkNotesPosterStyles";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";

type CopyStatus = { tone: "ok" | "error"; text: string };
export type WorkNotesViewMode = "fit" | "scroll";

const VIEW_MODE_OPTIONS = [
  { value: "fit" as const, label: "适应全貌" },
  { value: "scroll" as const, label: "100% 阅读" },
];

export function WorkNotesShare({ dto }: { dto: WorkNotesDto }) {
  const posterRef = useRef<HTMLElement>(null);
  const copyRun = useRef(0);
  const copyingRef = useRef(false);
  const [styleId, setStyleId] = useState<WorkNotesPosterStyleId>(
    () => loadWorkNotesPosterPreference().posterStyleId,
  );
  const [viewMode, setViewMode] = useState<WorkNotesViewMode>("fit");
  const [copying, setCopying] = useState(false);
  const [copyStatus, setCopyStatus] = useState<CopyStatus | null>(null);
  const [textCopyStatus, setTextCopyStatus] = useState<CopyStatus | null>(null);
  const poster = toWorkNotesPosterViewModel(dto);

  if (!poster) {
    return null;
  }

  function selectStyle(nextStyleId: WorkNotesPosterStyleId) {
    if (copyingRef.current) {
      return;
    }
    setStyleId(nextStyleId);
    setCopyStatus(null);
    saveWorkNotesPosterPreference({ posterStyleId: nextStyleId });
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
      setCopyStatus({ tone: "ok", text: "已复制图片" });
      window.setTimeout(() => {
        setCopyStatus((prev) => (prev?.tone === "ok" ? null : prev));
      }, 2500);
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

  async function copyText() {
    try {
      const text = workNotesPlainText(dto);
      await navigator.clipboard.writeText(text);
      setTextCopyStatus({ tone: "ok", text: "已复制文字" });
      window.setTimeout(() => {
        setTextCopyStatus((prev) => (prev?.tone === "ok" ? null : prev));
      }, 2500);
    } catch (caught: unknown) {
      setTextCopyStatus({ tone: "error", text: humanStatus(caught) });
    }
  }

  return (
    <div className="work-notes-share">
      <div className="work-notes-share-bar">
        <WorkNotesPosterStyles
          selectedStyleId={styleId}
          disabled={copying}
          onSelect={selectStyle}
        />
        <div className="work-notes-share-bar-actions">
          <Segmented
            value={viewMode}
            options={VIEW_MODE_OPTIONS}
            ariaLabel="纪要展示模式"
            onChange={setViewMode}
          />
          <div className="work-notes-copy">
            <Button
              variant="accent"
              disabled={copying}
              onClick={() => {
                if (copyingRef.current) {
                  return;
                }
                void copyPoster();
              }}
            >
              <Icon name={copyStatus?.tone === "ok" && !copying ? "check" : "copy"} size={14} />
              {copying ? "正在复制…" : copyStatus?.tone === "ok" ? "已复制图片" : "复制图片"}
            </Button>
            {copyStatus && copyStatus.tone === "error" ? (
              <p className="work-notes-copy-status is-error" role="alert">
                {copyStatus.text}
              </p>
            ) : null}
          </div>
          <div className="work-notes-copy">
            <Button
              onClick={() => {
                void copyText();
              }}
            >
              <Icon
                name={textCopyStatus?.tone === "ok" ? "check" : "copy"}
                size={14}
              />
              {textCopyStatus?.tone === "ok" ? "已复制文字" : "复制文字"}
            </Button>
            {textCopyStatus && textCopyStatus.tone === "error" ? (
              <p className="work-notes-copy-status is-error" role="alert">
                {textCopyStatus.text}
              </p>
            ) : null}
          </div>
        </div>
      </div>
      <div className={`work-notes-preview is-${viewMode}`}>
        {viewMode === "fit" ? (
          <ReportPreviewFrame>
            <WorkNotesPoster data={poster} posterRef={posterRef} styleId={styleId} />
          </ReportPreviewFrame>
        ) : (
          <div className="work-notes-scroll-stage">
            <WorkNotesPoster data={poster} posterRef={posterRef} styleId={styleId} />
          </div>
        )}
      </div>
    </div>
  );
}
