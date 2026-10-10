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

type ResumeTemplate = {
  command: (sessionId: string) => string;
  hint: string;
};

const DEFAULT_HINT = "在对应项目目录下执行，可直接粘贴到终端";

const RESUME_TEMPLATES: Record<string, ResumeTemplate> = {
  claude: {
    command: (id) => `claude --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  codex: {
    command: (id) => `codex resume ${id}`,
    hint: DEFAULT_HINT,
  },
  copilot: {
    command: (id) => `copilot --resume=${id}`,
    hint: DEFAULT_HINT,
  },
  cursor_agent: {
    command: (id) => `cursor-agent --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  factory: {
    command: (id) => `droid --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  gemini: {
    command: (id) => `gemini --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  grok: {
    command: (id) => `grok --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  kimi: {
    command: (id) => `kimi --session ${id}`,
    hint: DEFAULT_HINT,
  },
  opencode: {
    command: (id) => `opencode --session ${id}`,
    hint: DEFAULT_HINT,
  },
  pi: {
    command: (id) => `pi --session ${id}`,
    hint: DEFAULT_HINT,
  },
  omp: {
    command: (id) => `omp --session ${id}`,
    hint: DEFAULT_HINT,
  },
  qwen: {
    command: (id) => `qwen --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  qoder: {
    command: (id) => `qodercli --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  qoder_cn: {
    command: (id) => `qoderclicn --resume ${id}`,
    hint: DEFAULT_HINT,
  },
  cline: {
    command: (id) => `cline --id ${id}`,
    hint: DEFAULT_HINT,
  },
};

const MISSING_ID_HINT = "缺少会话 ID，无法生成恢复命令";
/** 对话记录来源里没有公开 CLI 恢复命令的：dsh、workbuddy、zcode、alma。 */
const UNSUPPORTED_HINT = "该来源暂无公开的 CLI 恢复命令，可复制会话 ID";

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
  const template = RESUME_TEMPLATES[source];
  if (!template) {
    return { command: null, hint: UNSUPPORTED_HINT };
  }
  const command = prefixResumeCommand(
    template.command(quoteUnix(id)),
    options.cwd,
    options.windows ?? detectWindows(),
  );
  return {
    command,
    hint: template.hint,
  };
}
