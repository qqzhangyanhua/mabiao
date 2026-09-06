import { useRef, useState } from "react";
import { Icon } from "../icons";
import { copyReportImage } from "../lib/copyReportImage";
import { humanStatus } from "../lib/format";
import { toWorkNotesPosterViewModel } from "../lib/workNotesPoster";
import {
  loadWorkNotesPosterPreference,
  saveWorkNotesPosterPreference,
} from "../lib/workNotesPosterPreference";
import { capturePoster } from "../report/capturePoster";
import type { WorkNotesDto } from "../types";
import { WorkNotesPoster } from "../workNotes/WorkNotesPoster";
import type { WorkNotesPosterStyleId } from "../workNotes/posterStyleRegistry";
import { ReportPreviewFrame } from "./ReportPreviewFrame";
import { WorkNotesPosterStyles } from "./WorkNotesPosterStyles";
import { Button } from "./ui/Button";

type CopyStatus = { tone: "ok" | "error"; text: string };

export function WorkNotesShare({ dto }: { dto: WorkNotesDto }) {
  const posterRef = useRef<HTMLElement>(null);
  const copyRun = useRef(0);
  const copyingRef = useRef(false);
  const [styleId, setStyleId] = useState<WorkNotesPosterStyleId>(
    () => loadWorkNotesPosterPreference().posterStyleId,
  );
  const [copying, setCopying] = useState(false);
  const [copyStatus, setCopyStatus] = useState<CopyStatus | null>(null);
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
    <div className="work-notes-share">
      <div className="work-notes-share-bar">
        <WorkNotesPosterStyles
          selectedStyleId={styleId}
          disabled={copying}
          onSelect={selectStyle}
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
            {copying ? "正在复制…" : "复制图片"}
          </Button>
          {copyStatus ? (
            <p
              className={`work-notes-copy-status is-${copyStatus.tone}`}
              role={copyStatus.tone === "error" ? "alert" : "status"}
            >
              {copyStatus.text}
            </p>
          ) : null}
        </div>
      </div>
      <div className="work-notes-preview">
        <ReportPreviewFrame>
          <WorkNotesPoster data={poster} posterRef={posterRef} styleId={styleId} />
        </ReportPreviewFrame>
      </div>
    </div>
  );
}
