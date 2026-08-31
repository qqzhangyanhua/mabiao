import { describe, expect, it } from "vitest";
import { sessionResumeHint, shellArg } from "./sessionResume";

describe("shellArg", () => {
  it("leaves safe identifiers unquoted", () => {
    expect(shellArg("7f9f9a2e-1b3c-4c7a-9b0e-abcdef012345")).toBe(
      "7f9f9a2e-1b3c-4c7a-9b0e-abcdef012345",
    );
    expect(shellArg("ses_abc123")).toBe("ses_abc123");
  });

  it("single-quotes values that need a shell-safe argument", () => {
    expect(shellArg("auth refactor")).toBe("'auth refactor'");
    expect(shellArg("it's-me")).toBe("'it'\\''s-me'");
  });
});

describe("sessionResumeHint", () => {
  it("builds source-specific resume commands", () => {
    expect(sessionResumeHint("codex", "abc-123").command).toBe("codex resume abc-123");
    expect(sessionResumeHint("claude", "auth-refactor").command).toBe(
      "claude --resume auth-refactor",
    );
    expect(sessionResumeHint("pi", "sess01").command).toBe("pi --session sess01");
    expect(sessionResumeHint("omp", "sess01").command).toBe("omp --session sess01");
    expect(sessionResumeHint("opencode", "ses_abc123").command).toBe(
      "opencode --session ses_abc123",
    );
    expect(sessionResumeHint("kimi", "wire-1").command).toBe("kimi --session wire-1");
    expect(sessionResumeHint("gemini", "a1b2c3d4").command).toBe("gemini --resume a1b2c3d4");
    expect(sessionResumeHint("grok", "g1").command).toBe("grok --resume g1");
    expect(sessionResumeHint("qwen", "q1").command).toBe("qwen --resume q1");
    expect(sessionResumeHint("factory", "droid-1").command).toBe("droid --resume droid-1");
    expect(sessionResumeHint("cursor_agent", "cur-1").command).toBe(
      "cursor-agent --resume cur-1",
    );
    expect(sessionResumeHint("copilot", "cp-1").command).toBe("copilot --resume=cp-1");
  });

  it("quotes session ids that are not shell-safe", () => {
    expect(sessionResumeHint("codex", "my session").command).toBe("codex resume 'my session'");
    expect(sessionResumeHint("copilot", "my session").command).toBe(
      "copilot --resume='my session'",
    );
  });

  it("returns a hint without command when the source has no CLI resume", () => {
    expect(sessionResumeHint("dsh", "sess-1")).toEqual({
      command: null,
      hint: "该来源暂无公开的 CLI 恢复命令，可复制会话 ID",
    });
    expect(sessionResumeHint("unknown", "sess-1").command).toBeNull();
  });

  it("returns a missing-id hint when the session id is blank", () => {
    expect(sessionResumeHint("codex", "  ")).toEqual({
      command: null,
      hint: "缺少会话 ID，无法生成恢复命令",
    });
  });
});
