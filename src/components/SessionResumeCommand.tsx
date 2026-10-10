import { useState, type MouseEvent } from "react";
import { Icon } from "../icons";
import { sessionResumeHint } from "../lib/sessionResume";
import { Button } from "./ui/Button";

export function SessionResumeCommand({
  source,
  sessionId,
  cwd,
}: {
  source: string;
  sessionId: string;
  cwd?: string | null;
}) {
  const hint = sessionResumeHint(source, sessionId, { cwd });
  const [copied, setCopied] = useState(false);

  async function copyCommand(event: MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    if (!hint.command) {
      return;
    }
    try {
      await navigator.clipboard.writeText(hint.command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className="session-resume">
      <div className="session-resume-label">
        <span>恢复会话</span>
        {hint.command ? <em>{hint.hint}</em> : null}
      </div>
      {hint.command ? (
        <div className="session-resume-cmd">
          <code title={hint.command}>{hint.command}</code>
          <Button
            variant="icon"
            className={copied ? "table-icon-btn is-copied" : "table-icon-btn"}
            onClick={copyCommand}
            title={copied ? "已复制" : "复制恢复命令"}
            aria-label={copied ? "已复制恢复命令" : "复制恢复命令"}
          >
            <Icon name={copied ? "check" : "copy"} size={12} />
          </Button>
        </div>
      ) : (
        <p className="session-resume-empty muted">{hint.hint}</p>
      )}
    </div>
  );
}
