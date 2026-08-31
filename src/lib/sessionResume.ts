/** 把会话 ID 收成可直接粘贴到终端的参数。 */
export function shellArg(value: string): string {
  if (/^[A-Za-z0-9._:/=+-]+$/.test(value)) {
    return value;
  }
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export type SessionResumeHint = {
  command: string | null;
  hint: string;
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
};

const MISSING_ID_HINT = "缺少会话 ID，无法生成恢复命令";
const UNSUPPORTED_HINT = "该来源暂无公开的 CLI 恢复命令，可复制会话 ID";

export function sessionResumeHint(source: string, sessionId: string): SessionResumeHint {
  const id = sessionId.trim();
  if (!id) {
    return { command: null, hint: MISSING_ID_HINT };
  }
  const template = RESUME_TEMPLATES[source];
  if (!template) {
    return { command: null, hint: UNSUPPORTED_HINT };
  }
  return {
    command: template.command(shellArg(id)),
    hint: template.hint,
  };
}
