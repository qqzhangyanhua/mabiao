import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { formatBytes, formatClock, humanStatus } from "../lib/format";
import type {
  GlobalInstructionDto,
  GlobalInstructionFile,
  GlobalInstructionSourceRow,
  InstructionEvidence,
  InstructionLoadStatus,
} from "../types";
import { EmptyState } from "./EmptyState";
import {
  canEditInstruction,
  canOpenInstruction,
  idleSourceLabel,
  isIdleSource,
  showsEvidenceBadge,
  showsLoadBadge,
} from "../lib/instructionAccess";
import { InstructionCheckup } from "./InstructionCheckup";
import { InstructionClaudeMemory } from "./InstructionClaudeMemory";
import { InstructionEditor } from "./InstructionEditor";
import { InstructionInsight } from "./InstructionInsight";
import { InstructionOverlap } from "./InstructionOverlap";
import { SourceLabel } from "./SourceIcon";
import { Button } from "./ui/Button";

const STATUS_LABEL: Record<InstructionLoadStatus, string> = {
  loaded: "已加载",
  present_unloaded: "存在但未被加载",
  locally_invisible: "本地不可见",
  not_created: "未创建",
};

const EVIDENCE_LABEL: Record<InstructionEvidence, string> = {
  verified: "已验证",
  inferred: "推测",
  no_mechanism: "无机制",
};

export function GlobalInstructionPanel() {
  const [data, setData] = useState<GlobalInstructionDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [openIdle, setOpenIdle] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const draftsRef = useRef(drafts);
  const selectedProjectRef = useRef<string | null>(null);
  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);

  const load = useCallback((force = false, project?: string | null) => {
    if (!force && Object.keys(draftsRef.current).length > 0 && project === undefined) {
      return;
    }
    setBusy(true);
    setError(null);
    const nextProject = project === undefined ? selectedProjectRef.current : project;
    invoke<GlobalInstructionDto>("get_global_instructions", { project: nextProject })
      .then((next) => {
        setData(next);
        selectedProjectRef.current = next.selected_project;
        if (force) {
          setDrafts({});
        }
      })
      .catch((err: unknown) => {
        setError(humanStatus(err));
      })
      .finally(() => {
        setBusy(false);
      });
  }, []);

  useEffect(() => {
    load();
    function onFocus() {
      load();
    }
    function onVisibility() {
      if (document.visibilityState === "visible") {
        load();
      }
    }
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [load]);

  const files = data?.sources.flatMap((row) => row.files) ?? [];

  async function openCursorSettings() {
    setActionError(null);
    try {
      await invoke("open_cursor_instruction_settings");
    } catch (err: unknown) {
      setActionError(humanStatus(err));
    }
  }

  async function openExternal(absPath: string) {
    setActionError(null);
    try {
      await invoke("open_global_instruction", { abs_path: absPath });
    } catch (err: unknown) {
      setActionError(humanStatus(err));
    }
  }

  return (
    <article className="panel instruction-panel">
      <div className="panel-head">
        <div>
          <h2>全局指令</h2>
          <p className="muted">每次进入或切回应用时重新读盘，不缓存。</p>
        </div>
        <Button type="button" variant="ghost" disabled={busy} onClick={() => load(true)}>
          重新读取
        </Button>
      </div>
      {error ? <EmptyState tone="warn" title="读取失败" hint={error} /> : null}
      {actionError ? <EmptyState tone="warn" title="无法打开" hint={actionError} /> : null}
      {data ? <InstructionCheckup findings={data.findings} /> : null}
      {data ? <InstructionClaudeMemory repos={data.claude_memories} /> : null}
      {data ? (
        <InstructionInsight investments={data.investments} imbalances={data.imbalances} />
      ) : null}
      {data ? (
        <InstructionOverlap
          selectedProject={data.selected_project}
          projects={data.projects}
          hints={data.hints}
          onProjectChange={(project) => load(false, project)}
        />
      ) : null}
      {!error && !files.length && !busy ? (
        <EmptyState
          title="尚未发现全局指令"
          hint="已覆盖全部已支持来源。未创建与无机制不是同一回事。"
        />
      ) : null}
      {data
        ? data.sources
            .filter((row) => !isIdleSource(row))
            .map((row) => (
              <SourceFiles
                key={row.source}
                row={row}
                drafts={drafts}
                openPath={openPath}
                onToggle={(id) => setOpenPath((current) => (current === id ? null : id))}
                onDraft={(id, value) => setDrafts((current) => ({ ...current, [id]: value }))}
                onSaved={(id) => {
                  setDrafts((current) => {
                    const next = { ...current };
                    delete next[id];
                    return next;
                  });
                  load(true);
                }}
                onCursorSettings={openCursorSettings}
                onOpenExternal={(path) => void openExternal(path)}
              />
            ))
        : null}
      {data && data.sources.some(isIdleSource) ? (
        <section className="instruction-idle">
          <div>
            <h3>未创建 / 无机制</h3>
            <p className="muted">没有已加载或被屏蔽的指令。展开后仍可查看路径或创建白名单内的文件。</p>
          </div>
          {data.sources.filter(isIdleSource).map((row) => {
            const open = openIdle === row.source;
            return (
              <div className="instruction-idle-source" key={row.source}>
                <button
                  type="button"
                  className="instruction-idle-head"
                  onClick={() =>
                    setOpenIdle((current) => (current === row.source ? null : row.source))
                  }
                >
                  <SourceLabel source={row.source} fallback={row.application} />
                  <em className="instruction-evidence">{idleSourceLabel(row)}</em>
                </button>
                {open ? (
                  <FileList
                    row={row}
                    drafts={drafts}
                    openPath={openPath}
                    onToggle={(id) => setOpenPath((current) => (current === id ? null : id))}
                    onDraft={(id, value) =>
                      setDrafts((current) => ({ ...current, [id]: value }))
                    }
                    onSaved={(id) => {
                      setDrafts((current) => {
                        const next = { ...current };
                        delete next[id];
                        return next;
                      });
                      load(true);
                    }}
                    onCursorSettings={openCursorSettings}
                    onOpenExternal={(path) => void openExternal(path)}
                  />
                ) : null}
              </div>
            );
          })}
        </section>
      ) : null}
    </article>
  );
}

function fileId(source: string, file: GlobalInstructionFile): string {
  return `${source}:${file.display_path}`;
}

function SourceFiles({
  row,
  drafts,
  openPath,
  onToggle,
  onDraft,
  onSaved,
  onCursorSettings,
  onOpenExternal,
}: FileListProps) {
  return (
    <section className="instruction-source">
      <h3>
        <SourceLabel source={row.source} fallback={row.application} />
      </h3>
      <FileList
        row={row}
        drafts={drafts}
        openPath={openPath}
        onToggle={onToggle}
        onDraft={onDraft}
        onSaved={onSaved}
        onCursorSettings={onCursorSettings}
        onOpenExternal={onOpenExternal}
      />
    </section>
  );
}

type FileListProps = {
  row: GlobalInstructionSourceRow;
  drafts: Record<string, string>;
  openPath: string | null;
  onToggle: (id: string) => void;
  onDraft: (id: string, value: string) => void;
  onSaved: (id: string) => void;
  onCursorSettings: () => void;
  onOpenExternal: (path: string) => void;
};

function FileList({
  row,
  drafts,
  openPath,
  onToggle,
  onDraft,
  onSaved,
  onCursorSettings,
  onOpenExternal,
}: FileListProps) {
  return (
    <ul className="instruction-list">
      {row.files.map((file) => {
        const id = fileId(row.source, file);
        return (
          <InstructionRow
            key={id}
            file={file}
            draft={drafts[id] ?? file.content}
            open={openPath === id}
            onToggle={() => onToggle(id)}
            onDraft={(value) => onDraft(id, value)}
            onSaved={() => onSaved(id)}
            onCursorSettings={onCursorSettings}
            onOpenExternal={() => onOpenExternal(file.abs_path)}
          />
        );
      })}
    </ul>
  );
}

function InstructionRow({
  file,
  draft,
  open,
  onToggle,
  onDraft,
  onSaved,
  onCursorSettings,
  onOpenExternal,
}: {
  file: GlobalInstructionFile;
  draft: string;
  open: boolean;
  onToggle: () => void;
  onDraft: (value: string) => void;
  onSaved: () => void;
  onCursorSettings: () => void;
  onOpenExternal: () => void;
}) {
  return (
    <li className={`instruction-row status-${file.load_status} evidence-${file.evidence}`}>
      <div className="instruction-row-bar">
        <button type="button" className="instruction-row-head" onClick={onToggle}>
          <div className="instruction-row-title">
            <strong>{file.display_path}</strong>
            {showsLoadBadge(file) || showsEvidenceBadge(file) ? (
              <span className="instruction-badges">
                {showsLoadBadge(file) ? (
                  <em className="instruction-status">{STATUS_LABEL[file.load_status]}</em>
                ) : null}
                {showsEvidenceBadge(file) ? (
                  <em className="instruction-evidence">{EVIDENCE_LABEL[file.evidence]}</em>
                ) : null}
              </span>
            ) : null}
          </div>
          <div className="instruction-row-meta">
            <span>{file.kind === "directory" ? "目录" : formatBytes(file.byte_size)}</span>
            <span>{formatClock(file.modified_at)}</span>
          </div>
          {file.note ? <p className="instruction-note">{file.note}</p> : null}
        </button>
        {canOpenInstruction(file) ? (
          <Button type="button" variant="ghost" onClick={onOpenExternal}>
            在外部打开
          </Button>
        ) : null}
      </div>
      {open ? (
        <div className="instruction-body">
          {file.error ? <p className="instruction-error">{file.error}</p> : null}
          {file.action === "cursor_settings" ? (
            <Button type="button" variant="ghost" onClick={onCursorSettings}>
              在 Cursor 中打开设置
            </Button>
          ) : null}
          {file.load_status === "locally_invisible" ? (
            <p className="muted">内容在账号服务端，本机无法展示。</p>
          ) : null}
          {file.evidence === "no_mechanism" ? (
            <p className="muted">该来源没有用户级全局指令机制，不必按路径去创建文件。</p>
          ) : null}
          {canEditInstruction(file) ? (
            <InstructionEditor file={file} draft={draft} onDraft={onDraft} onSaved={onSaved} />
          ) : null}
        </div>
      ) : null}
    </li>
  );
}
