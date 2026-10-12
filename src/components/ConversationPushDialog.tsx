import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../icons";
import { consumeEscape } from "../lib/escapeShortcut";
import { humanStatus } from "../lib/format";
import {
  defaultPushPreset,
  formatBytes,
  issueKeys,
  pushHistoryLine,
  pushOutcomeSummary,
  pushPresetOptions,
  pushRangeFor,
  type PushPreset,
} from "../lib/pushRange";
import type {
  Filter,
  PushHistoryEntry,
  PushOutcome,
  PushPreviewDto,
  PushProgress,
  PushRunInput,
  PushSessionIssue,
  RemoteServerDto,
} from "../types";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";

type Phase = "pick" | "running" | "done";

function IssueList({ title, issues }: { title: string; issues: PushSessionIssue[] }) {
  if (issues.length === 0) {
    return null;
  }
  return (
    <div className="conversation-push-issues">
      <h4>
        {title}（{issues.length}）
      </h4>
      <ul>
        {issues.map((issue) => (
          <li key={`${issue.source}-${issue.session_id}`}>
            <span className="conversation-push-issue-title" title={issue.title}>
              {issue.title || issue.session_id}
            </span>
            <span className="muted">{issue.reason}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

const SLOW_PREVIEW_SECONDS = 3;

// 挂载即开始计时，卸载即归零：每次重新统计都会重新挂载。
function PreviewingStatus() {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const startedAt = Date.now();
    const timer = window.setInterval(
      () => setSeconds(Math.floor((Date.now() - startedAt) / 1000)),
      1000,
    );
    return () => window.clearInterval(timer);
  }, []);
  return (
    <div className="conversation-push-previewing" role="status">
      <div className="conversation-push-previewing-line">
        <span className="spinner" style={{ width: 14, height: 14 }} aria-hidden="true" />
        <span>
          正在统计要推的内容…
          {seconds >= 1 ? <span className="muted">（已用 {seconds} 秒）</span> : null}
        </span>
      </div>
      <progress aria-label="统计进度" />
      {seconds >= SLOW_PREVIEW_SECONDS ? (
        <span className="muted conversation-summary-hint">
          会话较多或时间范围较大时需要读取本机全部会话文件，可能要等一会儿，没有卡住。
          也可以换个更短的时间范围。
        </span>
      ) : null}
    </div>
  );
}

export function ConversationPushDialog({
  filter,
  onClose,
  onOpenSettings,
}: {
  filter: Filter;
  onClose: () => void;
  onOpenSettings: () => void;
}) {
  const titleId = "conversation-push-dialog-title";
  const dialogRef = useRef<HTMLDivElement>(null);
  const phaseRef = useRef<Phase>("pick");
  const onCloseRef = useRef(onClose);
  const [server, setServer] = useState<RemoteServerDto | null>(null);
  const [preset, setPreset] = useState<PushPreset>(() => defaultPushPreset(filter));
  const [previewResult, setPreviewResult] = useState<{
    key: string;
    data: PushPreviewDto | null;
    error: string | null;
  } | null>(null);
  const [phase, setPhase] = useState<Phase>("pick");
  const [progress, setProgress] = useState<PushProgress | null>(null);
  const [outcome, setOutcome] = useState<PushOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<PushHistoryEntry[]>([]);
  const [force, setForce] = useState(false);
  const loggedIn = server?.state === "logged_in";
  // 区间只在换预设或筛选内容真的变了才重算（父组件每次刷新 filter 引用都可能是新的，
  // 按引用重算会带着新的 now 反复触发预览），预览与真推用同一份。
  const filterKey = JSON.stringify(filter);
  const range = useMemo(
    () => pushRangeFor(preset, JSON.parse(filterKey) as Filter, new Date()),
    [preset, filterKey],
  );
  const rangeKey = JSON.stringify(range);
  const currentPreview = previewResult?.key === rangeKey ? previewResult : null;
  const preview = currentPreview?.data ?? null;
  const previewing = loggedIn && phase === "pick" && currentPreview === null;
  const needsLoginAgain = outcome?.login_required === true;
  const nothingNew = preview !== null && preview.sessions + preview.usage_records === 0;
  const hasPushedBefore =
    preview !== null && preview.unchanged_sessions + preview.usage_already_pushed > 0;
  // 没有新内容时，只有勾了「强制重推」且以前推过东西才有得发。
  const canSend =
    loggedIn && !previewing && preview !== null && (!nothingNew || (force && hasPushedBefore));

  useEffect(() => {
    phaseRef.current = phase;
    onCloseRef.current = onClose;
  });

  useEffect(() => {
    let cancelled = false;
    void invoke<RemoteServerDto>("get_remote_server")
      .then((loaded) => {
        if (!cancelled) setServer(loaded);
      })
      .catch((caught: unknown) => {
        if (!cancelled) setError(humanStatus(caught));
      });
    void invoke<PushHistoryEntry[]>("get_push_history")
      .then((entries) => {
        if (!cancelled) setHistory(entries);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!loggedIn || phase !== "pick") {
      return;
    }
    let cancelled = false;
    void invoke<PushPreviewDto>("preview_push", { range })
      .then((data) => {
        if (!cancelled) setPreviewResult({ key: rangeKey, data, error: null });
      })
      .catch((caught: unknown) => {
        if (!cancelled) setPreviewResult({ key: rangeKey, data: null, error: humanStatus(caught) });
      });
    return () => {
      cancelled = true;
    };
    // range 与 rangeKey 同源：只在区间真的变了才重新预览。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loggedIn, phase, rangeKey]);

  function run(input: PushRunInput) {
    setPhase("running");
    setProgress(null);
    setOutcome(null);
    setError(null);
    let unlisten: (() => void) | undefined;
    let finished = false;
    void listen<PushProgress>("push-progress", (event) => setProgress(event.payload)).then(
      (stop) => {
        if (finished) stop();
        else unlisten = stop;
      },
    );
    void invoke<PushOutcome>("run_push", { input })
      .then((result) => {
        setOutcome(result);
        setPhase("done");
        void invoke<PushHistoryEntry[]>("get_push_history")
          .then(setHistory)
          .catch(() => undefined);
        if (result.login_required) {
          void invoke<RemoteServerDto>("get_remote_server")
            .then(setServer)
            .catch(() => undefined);
        }
      })
      .catch((caught: unknown) => {
        setError(humanStatus(caught));
        setPhase("pick");
      })
      .finally(() => {
        finished = true;
        unlisten?.();
      });
  }

  function send() {
    run({ range, only: [], include_usage: true, force });
  }

  function retry() {
    if (!outcome) return;
    run({
      range,
      only: issueKeys(outcome.failed),
      include_usage: outcome.usage_error !== null,
      force: false,
    });
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
    focusable().find((control) => !control.classList.contains("icon-btn"))?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (consumeEscape(event)) {
        if (phaseRef.current !== "running") onCloseRef.current();
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

  const running = phase === "running";

  return (
    <div
      className="conversation-summary-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget && !running) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="conversation-summary-dialog conversation-push-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header>
          <div>
            <h3 id={titleId}>推送到远程服务</h3>
            {server && loggedIn ? (
              <p className="muted conversation-push-account">
                {server.account} · {server.base_url}
              </p>
            ) : null}
          </div>
          <Button variant="icon" onClick={onClose} disabled={running} aria-label="关闭推送">
            <Icon name="close" size={15} />
          </Button>
        </header>

        {server && !loggedIn ? (
          <div className="conversation-summary-empty">
            <p className="conversation-summary-help">
              {server.notice ?? "还没有登录远程服务。登录后才能推送。"}
            </p>
            <Button
              variant="accent"
              onClick={() => {
                onClose();
                onOpenSettings();
              }}
            >
              去设置页登录
            </Button>
          </div>
        ) : null}

        {loggedIn && phase !== "done" ? (
          <>
            <div className="conversation-summary-field">
              <span>时间范围</span>
              <Segmented
                ariaLabel="推送时间范围"
                value={preset}
                options={pushPresetOptions(filter)}
                disabled={running}
                onChange={setPreset}
              />
            </div>
            <p className="conversation-summary-hint">
              {filter.sources.length > 0
                ? `只推 ${filter.sources.join("、")}（沿用顶栏来源筛选）。`
                : "推全部来源。"}
              与区间有重叠的会话整场推送，不切开。
            </p>
            <label className="conversation-summary-hint">
              <input
                type="checkbox"
                checked={force}
                disabled={running}
                onChange={(event) => setForce(event.target.checked)}
              />{" "}
              强制重推（忽略已推送记录，比如服务端删过会话）
            </label>
            {previewing ? <PreviewingStatus /> : null}
            {preview ? (
              <div className="conversation-push-preview">
                <dl>
                  <div>
                    <dt>会话</dt>
                    <dd>{preview.sessions}</dd>
                  </div>
                  <div>
                    <dt>事件</dt>
                    <dd>{preview.events}</dd>
                  </div>
                  <div>
                    <dt>消耗记录</dt>
                    <dd>{preview.usage_records}</dd>
                  </div>
                  <div>
                    <dt>预计大小</dt>
                    <dd>{formatBytes(preview.estimated_bytes)}</dd>
                  </div>
                  <div>
                    <dt>将打码</dt>
                    <dd>{preview.redactions} 处</dd>
                  </div>
                </dl>
                {hasPushedBefore ? (
                  <p className="conversation-summary-hint">
                    另有 {preview.unchanged_sessions} 场会话、{preview.usage_already_pushed} 条消耗记录
                    以前已推送且没有变化，
                    {force ? "已勾选强制重推，会再推一遍" : "这次不会重复推送"}。
                  </p>
                ) : null}
                {nothingNew && !force ? (
                  <p className="conversation-summary-hint">没有新内容需要推送。</p>
                ) : null}
                <p className="conversation-push-notice" role="note">
                  {preview.notice}。打码只认常见密钥格式，没被规则命中的内容仍会推出去。
                </p>
                {preview.usage_skipped_invalid_time > 0 ? (
                  <p className="conversation-summary-hint">
                    有 {preview.usage_skipped_invalid_time} 条消耗记录时间无法识别，不会推送。
                  </p>
                ) : null}
                <IssueList title="本机读不全，将跳过" issues={preview.skipped} />
              </div>
            ) : null}
          </>
        ) : null}

        {running ? (
          <div className="conversation-push-progress" role="status">
            <progress
              value={progress ? progress.done : undefined}
              max={progress ? Math.max(progress.total, 1) : undefined}
              aria-label="推送进度"
            />
            <span className="muted">
              {progress ? `${progress.done} / ${progress.total}` : "准备中…"}
              {progress?.title ? ` · ${progress.title}` : ""}
            </span>
          </div>
        ) : null}

        {phase === "done" && outcome ? (
          <div className="conversation-push-result">
            <p className="conversation-summary-hint" role="status">
              {pushOutcomeSummary(outcome)}
            </p>
            {needsLoginAgain ? (
              <p className="conversation-summary-error" role="alert">
                登录已失效，剩下的会话没有推送。请到设置页重新登录后再试。
              </p>
            ) : null}
            {outcome.usage_error && !needsLoginAgain ? (
              <p className="conversation-summary-error" role="alert">
                消耗记录推送失败：{outcome.usage_error}
              </p>
            ) : null}
            <IssueList title="失败，可重试" issues={outcome.failed} />
            <IssueList title="跳过（本机读不全，重试也不会好）" issues={outcome.skipped} />
          </div>
        ) : null}

        {error ?? currentPreview?.error ? (
          <p className="conversation-summary-error" role="alert">
            {error ?? currentPreview?.error}
          </p>
        ) : null}

        {history.length > 0 && phase !== "running" ? (
          <details className="conversation-push-history">
            <summary>推送历史（{history.length}）</summary>
            <ul>
              {history.slice(0, 10).map((entry) => (
                <li key={`${entry.at}-${entry.sessions_succeeded}-${entry.usage_inserted}`}>
                  <span>{new Date(entry.at).toLocaleString()}</span>
                  <span className="muted">{pushHistoryLine(entry)}</span>
                </li>
              ))}
            </ul>
          </details>
        ) : null}

        <footer>
          <div className="conversation-summary-actions">
            <Button disabled={running} onClick={onClose}>
              {phase === "done" ? "完成" : "取消"}
            </Button>
            {phase === "done" && needsLoginAgain ? (
              <Button
                variant="accent"
                onClick={() => {
                  onClose();
                  onOpenSettings();
                }}
              >
                去设置页登录
              </Button>
            ) : null}
            {phase === "done" && !needsLoginAgain && (outcome?.failed.length || outcome?.usage_error) ? (
              <Button variant="accent" onClick={retry}>
                重试
              </Button>
            ) : null}
            {phase !== "done" && loggedIn ? (
              <Button variant="accent" disabled={!canSend || running} onClick={send}>
                推送
              </Button>
            ) : null}
          </div>
        </footer>
      </div>
    </div>
  );
}
