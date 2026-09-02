import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { humanStatus } from "../lib/format";
import { SourceLabel } from "./SourceIcon";
import { Button } from "./ui/Button";

type ScanPathRow = {
  source: string;
  application: string;
  env_var: string;
  override_roots: string[];
  env_roots: string[];
  default_roots: string[];
  effective_scan_dirs: string[];
  join_leaf: string;
  active: string;
  note: string;
};

type ScanPathPanelDto = {
  rows: ScanPathRow[];
};

function layerLabel(active: string): string {
  if (active === "ui") {
    return "设置页";
  }
  if (active === "env") {
    return "环境变量";
  }
  return "默认";
}

function joinRoots(roots: string[]): string {
  return roots.join(", ");
}

function splitRoots(raw: string): string[] {
  return raw
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function ScanPathPanel({
  operationBusy,
  onSaved,
}: {
  operationBusy: boolean;
  onSaved: () => void;
}) {
  const [rows, setRows] = useState<ScanPathRow[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<"idle" | "load" | "save" | "pick">("load");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<ScanPathPanelDto>("get_scan_path_config")
      .then((panel) => {
        setRows(panel.rows);
        setDrafts(Object.fromEntries(panel.rows.map((row) => [row.source, joinRoots(row.override_roots)])));
        setError(null);
      })
      .catch((cause: unknown) => setError(humanStatus(cause)))
      .finally(() => setBusy("idle"));
  }, []);

  const dirty = useMemo(() => {
    return rows.some((row) => (drafts[row.source] ?? "") !== joinRoots(row.override_roots));
  }, [drafts, rows]);

  function applyPanel(panel: ScanPathPanelDto) {
    setRows(panel.rows);
    setDrafts(Object.fromEntries(panel.rows.map((row) => [row.source, joinRoots(row.override_roots)])));
  }

  async function save() {
    setBusy("save");
    setError(null);
    try {
      const overrides: Record<string, string[]> = {};
      for (const row of rows) {
        overrides[row.source] = splitRoots(drafts[row.source] ?? "");
      }
      applyPanel(await invoke<ScanPathPanelDto>("save_scan_path_config", { overrides }));
      onSaved();
    } catch (cause: unknown) {
      setError(humanStatus(cause));
    } finally {
      setBusy("idle");
    }
  }

  async function browse(source: string) {
    setBusy("pick");
    setError(null);
    try {
      const picked = await invoke<string | null>("pick_directory", {
        title: "选择扫描根目录",
      });
      if (!picked) {
        return;
      }
      setDrafts((current) => {
        const previous = (current[source] ?? "").trim();
        return {
          ...current,
          [source]: previous ? `${previous}, ${picked}` : picked,
        };
      });
    } catch (cause: unknown) {
      setError(humanStatus(cause));
    } finally {
      setBusy("idle");
    }
  }

  const locked = busy !== "idle" || operationBusy;

  return (
    <section className="panel" id="settings-scan-paths">
      <div className="panel-head">
        <div>
          <h2>扫描路径</h2>
          <p className="panel-note">
            从 Dock 打开时读不到终端里 export 的环境变量。这里填绝对路径，优先生效；留空则仍用环境变量或默认位置。
            填写方式与环境变量相同：根目录、逗号分隔多个目录，保存后整体替换默认路径。改路径后旧目录里的记录会归档。
          </p>
        </div>
        <div className="row-actions">
          <Button variant="accent" disabled={locked || !dirty} onClick={() => void save()}>
            {busy === "save" ? "正在保存…" : "保存"}
          </Button>
        </div>
      </div>
      {error ? (
        <p className="panel-note snapshot-error" role="alert">
          {error}
        </p>
      ) : null}
      <div className="scan-path-list">
        {rows.map((row) => (
          <div className="scan-path-row" key={row.source}>
            <div className="scan-path-head">
              <strong>
                <SourceLabel source={row.source} fallback={row.application} size={14} />
              </strong>
              <span className="scan-path-env">
                <code>{row.env_var}</code>
                <em>{layerLabel(row.active)}</em>
              </span>
            </div>
            <div className="scan-path-input-row">
              <input
                value={drafts[row.source] ?? ""}
                onChange={(event) =>
                  setDrafts((current) => ({ ...current, [row.source]: event.target.value }))
                }
                placeholder={joinRoots(row.default_roots) || "默认位置，留空不覆盖"}
                aria-label={`${row.application} 扫描根目录`}
                disabled={locked}
                spellCheck={false}
              />
              <Button size="sm" disabled={locked} onClick={() => void browse(row.source)}>
                浏览
              </Button>
              <Button
                size="sm"
                disabled={locked || !(drafts[row.source] ?? "").trim()}
                onClick={() => setDrafts((current) => ({ ...current, [row.source]: "" }))}
              >
                清除
              </Button>
            </div>
            <p className="scan-path-meta">
              {row.join_leaf
                ? `与 ${row.env_var} 相同，填根目录；实际扫描会再拼 ${row.join_leaf}。`
                : `与 ${row.env_var} 相同，填实际扫描目录。`}
              {row.note ? ` ${row.note}` : ""}
              {` 当前扫描：${joinRoots(row.effective_scan_dirs) || "—"}`}
              {row.env_roots.length > 0 ? ` 环境变量：${joinRoots(row.env_roots)}` : ""}
            </p>
          </div>
        ))}
        {rows.length === 0 && busy === "load" ? (
          <p className="panel-note">正在读取扫描路径…</p>
        ) : null}
      </div>
    </section>
  );
}
