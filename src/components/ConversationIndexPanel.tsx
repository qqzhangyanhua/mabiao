import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { humanStatus } from "../lib/format";
import type { ConversationIndexProgressDto } from "../types";

const POLL_MS = 2000;

function formatIndexBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

export function ConversationIndexPanel() {
  const [progress, setProgress] = useState<ConversationIndexProgressDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;

    async function load() {
      try {
        const next = await invoke<ConversationIndexProgressDto>("get_conversation_index_progress");
        if (cancelled) {
          return;
        }
        setProgress(next);
        setError(null);
        if (next.total > 0 && next.indexed < next.total) {
          timer = window.setTimeout(() => {
            void load();
          }, POLL_MS);
        }
      } catch (err: unknown) {
        if (cancelled) {
          return;
        }
        setError(humanStatus(err));
      }
    }

    void load();
    return () => {
      cancelled = true;
      if (timer !== undefined) {
        window.clearTimeout(timer);
      }
    };
  }, []);

  const indexed = progress?.indexed ?? 0;
  const total = progress?.total ?? 0;
  const percent = total === 0 ? 100 : Math.min(100, (indexed / total) * 100);
  const complete = total === 0 || indexed >= total;

  return (
    <section className="panel" id="settings-conversation-index">
      <div className="panel-head">
        <div>
          <h2>对话索引</h2>
          <p className="panel-note">
            按会话在后台补建事件索引。未就绪的会话仍可打开，只是会走整份解析。搜索会读本机会话正文，数据仍留在本机，不进备份、不上传。
          </p>
        </div>
      </div>
      {error ? (
        <p className="panel-note snapshot-error" role="alert">
          {error}
        </p>
      ) : (
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-copy">
              <h3>{complete ? "已就绪" : "补建中"}</h3>
              <p>
                已索引 {indexed} / {total}
                {progress && (progress.index_bytes ?? 0) > 0
                  ? ` · 对话索引占用 ${formatIndexBytes(progress.index_bytes ?? 0)}`
                  : complete
                    ? null
                    : " · 对话索引占用待就绪后统计"}
              </p>
            </div>
            <div className="budget-bar" aria-hidden="true">
              <i className="budget-bar-fill" style={{ width: `${percent}%` }} />
            </div>
          </div>
        </div>
      )}
    </section>
  );
}