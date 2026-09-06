import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { humanStatus } from "../lib/format";
import {
  loadWorkNotesPreference,
  saveWorkNotesPreference,
  workNotesEngineLabel,
} from "../lib/workNotesPreference";
import type { DetectedEngine } from "../types";
import { Button } from "./ui/Button";

export function WorkNotesEnginePanel() {
  const [preference, setPreference] = useState(loadWorkNotesPreference);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function detect() {
    setBusy(true);
    setError(null);
    void invoke<DetectedEngine[]>("detect_work_note_engines")
      .then((detected) => {
        const next = { ...preference, detected };
        saveWorkNotesPreference(next);
        setPreference(next);
      })
      .catch((caught: unknown) => {
        setError(humanStatus(caught));
      })
      .finally(() => {
        setBusy(false);
      });
  }

  return (
    <section className="panel" id="settings-work-notes-engines">
      <div className="panel-head">
        <div>
          <h2>纪要引擎</h2>
          <p className="panel-note">
            手动检测本机已装的 CLI 及其版本。启动时不会自动探测。工作纪要只列出检测为已安装的引擎。
          </p>
        </div>
        <Button disabled={busy} onClick={detect}>
          {busy ? "正在检测…" : "检测"}
        </Button>
      </div>
      {error ? (
        <p className="panel-note snapshot-error" role="alert">
          {error}
        </p>
      ) : null}
      {preference.detected.length === 0 ? (
        <p className="panel-note">还没有检测过。点「检测」会跑 which 和 --version。</p>
      ) : (
        <div className="settings-rows">
          {preference.detected.map((engine) => (
            <div className="settings-row" key={engine.id}>
              <div className="settings-row-copy">
                <h3>{workNotesEngineLabel(engine.id)}</h3>
                <p>
                  {engine.program}
                  {engine.writes_session_dir ? " · 会在你的会话记录里留下一条" : ""}
                </p>
              </div>
              {engine.installed ? (
                <span className="health-state ok">{engine.version ?? "已安装"}</span>
              ) : (
                <span className="health-state">未安装</span>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
