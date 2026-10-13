import { RESUME_TEMPLATES } from "../generated/displayIdentity";

/** 把会话 ID 或路径收成可直接粘贴到 Unix shell 的参数。 */
export function shellArg(value: string): string {
  return quoteUnix(value);
}

function quoteUnix(value: string): string {
  if (/^[A-Za-z0-9._:/=+-]+$/.test(value)) {
    return value;
  }
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** PowerShell `-LiteralPath`：单引号内把 `'` 写成 `''`。 */
export function powershellLiteral(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

export type SessionResumeHint = {
  command: string | null;
  hint: string;
};

export type SessionResumeOptions = {
  cwd?: string | null;
  windows?: boolean;
};

const DEFAULT_HINT = "在对应项目目录下执行，可直接粘贴到终端";
const MISSING_ID_HINT = "缺少会话 ID，无法生成恢复命令";
const UNSUPPORTED_HINT = "该来源暂无公开的 CLI 恢复命令，可复制会话 ID";

function isResumeSlug(source: string): source is keyof typeof RESUME_TEMPLATES {
  return Object.hasOwn(RESUME_TEMPLATES, source);
}

export function prefersWindowsShell(windows = detectWindows()): boolean {
  return windows;
}

function detectWindows(): boolean {
  if (typeof navigator === "undefined") {
    return false;
  }
  return /Windows/i.test(navigator.userAgent);
}

export function prefixResumeCommand(
  command: string,
  cwd: string | null | undefined,
  windows = false,
): string {
  const folder = cwd?.trim() ?? "";
  if (!folder) {
    return command;
  }
  if (windows) {
    return `Set-Location -LiteralPath ${powershellLiteral(folder)}; ${command}`;
  }
  return `cd ${quoteUnix(folder)} && ${command}`;
}

export function sessionResumeHint(
  source: string,
  sessionId: string,
  options: SessionResumeOptions = {},
): SessionResumeHint {
  const id = sessionId.trim();
  if (!id) {
    return { command: null, hint: MISSING_ID_HINT };
  }
  const template = isResumeSlug(source) ? RESUME_TEMPLATES[source] : null;
  if (!template) {
    return { command: null, hint: UNSUPPORTED_HINT };
  }
  const command = prefixResumeCommand(
    template.replace("{id}", quoteUnix(id)),
    options.cwd,
    options.windows ?? detectWindows(),
  );
  return {
    command,
    hint: DEFAULT_HINT,
  };
}
