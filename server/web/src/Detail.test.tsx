import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type {
  AdminProject,
  ProjectDetail,
  SessionDetail,
  SessionListItem,
  SummaryResponse,
} from "./api/types";

const TOTALS = {
  record_count: 2,
  total_tokens: 4000,
  cost_snapshot_total: 12,
  unified_cost_total: 0.008,
  unified_unpriced_count: 0,
};

const SUMMARY: SummaryResponse = {
  totals: TOTALS,
  by_day: [],
  by_account: [
    {
      key: "alice",
      label: "alice",
      id: 2,
      record_count: 1,
      total_tokens: 1000,
      cost_snapshot: 1,
      unified_cost: 0.002,
      unpriced_count: 0,
    },
    {
      key: "bob",
      label: "bob",
      id: 3,
      record_count: 1,
      total_tokens: 3000,
      cost_snapshot: 1,
      unified_cost: 0.006,
      unpriced_count: 0,
    },
  ],
  by_source: [],
  by_model: [],
  by_project: [
    {
      key: "git:github.com/team/alpha",
      label: "alpha",
      id: 11,
      record_count: 2,
      total_tokens: 4000,
      cost_snapshot: 2,
      unified_cost: 0.008,
      unpriced_count: 0,
    },
  ],
};

const LISTED: SessionListItem = {
  id: 31,
  account_id: 2,
  account: "alice",
  device_name: "办公室",
  source: "codex",
  session_id: "s-1",
  title: "修一个 bug",
  project: "/w/alpha",
  project_id: 11,
  project_name: "alpha",
  model: "team-model",
  started_at: "2026-03-01T09:00:00Z",
  ended_at: "2026-03-01T10:00:00Z",
  event_count: 2,
  generated_by_work_notes: false,
  pushed_at: "2026-03-01T10:05:00Z",
};

const DETAIL: SessionDetail = {
  session: LISTED,
  source_files: ["/s/a.jsonl"],
  redaction_count: 1,
  events: [
    {
      event_id: "e0",
      sequence: 0,
      source_file: "/s/a.jsonl",
      source_sequence: 0,
      kind: "message",
      occurred_at: null,
      actor: "user",
      name: null,
      text: "请帮我改一下",
    },
    {
      event_id: "e1",
      sequence: 1,
      source_file: "/s/a.jsonl",
      source_sequence: 1,
      kind: "tool_call",
      occurred_at: null,
      actor: "assistant",
      name: "shell",
      text: "ls -la",
    },
  ],
  context_manifest: {
    has_injected_snapshot: true,
    items: [
      { layer: "injected", kind: "instruction", id: "i", label: "AGENTS.md", content: "注入原文" },
      { layer: "on_disk_possible", kind: "instruction", id: "d", label: "CLAUDE.md" },
    ],
  },
  usage: { totals: TOTALS, by_model: [] },
};

const PROJECT: ProjectDetail = {
  project: {
    id: 11,
    name: "alpha",
    key: "git:github.com/team/alpha",
    git_remote: "github.com/team/alpha",
    created_at: "2026-03-01T00:00:00Z",
  },
  summary: SUMMARY,
};

const ADMIN_PROJECTS: AdminProject[] = [
  { ...PROJECT.project, session_count: 3, usage_record_count: 2, merged_count: 0 },
  {
    id: 12,
    name: "alpha-copy",
    key: "dir:alpha",
    git_remote: null,
    created_at: "2026-03-01T00:00:00Z",
    session_count: 1,
    usage_record_count: 0,
    merged_count: 0,
  },
];

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function stubServer(role: "admin" | "member") {
  const requests: string[] = [];
  const me = { id: role === "admin" ? 1 : 2, account: role === "admin" ? "root" : "alice", role };
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.pathname : input.url;
      const method = init?.method ?? "GET";
      requests.push(`${method} ${url}`);
      if (url === "/api/v1/login") {
        return Promise.resolve(json(200, { token: "tok", expires_at: "", role, account: me.account }));
      }
      if (url === "/api/v1/me") return Promise.resolve(json(200, me));
      if (url === "/api/v1/sessions/31" && method === "DELETE")
        return Promise.resolve(new Response(null, { status: 204 }));
      if (url === "/api/v1/sessions/31") return Promise.resolve(json(200, DETAIL));
      if (url.startsWith("/api/v1/sessions?"))
        return Promise.resolve(json(200, { sessions: [LISTED], total: 1 }));
      if (url.startsWith("/api/v1/projects/11")) return Promise.resolve(json(200, PROJECT));
      if (url === "/api/v1/admin/projects") return Promise.resolve(json(200, ADMIN_PROJECTS));
      if (url === "/api/v1/admin/projects/11" && method === "PUT")
        return Promise.resolve(json(200, { ...PROJECT.project, name: "阿尔法" }));
      if (url === "/api/v1/admin/projects/11/merge")
        return Promise.resolve(
          json(200, { project: ADMIN_PROJECTS[1], sessions_moved: 3, usage_records_moved: 2 }),
        );
      if (url.startsWith("/api/v1/usage/summary")) return Promise.resolve(json(200, SUMMARY));
      return Promise.resolve(json(404, { code: "not_found", message: "没有这个接口" }));
    }),
  );
  return requests;
}

function signIn() {
  fireEvent.change(screen.getByLabelText("账号"), { target: { value: "someone" } });
  fireEvent.change(screen.getByLabelText("密码"), { target: { value: "good" } });
  fireEvent.click(screen.getByRole("button", { name: "登录" }));
}

beforeEach(() => {
  sessionStorage.clear();
  window.location.hash = "";
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("会话详情", () => {
  it("shows events and keeps injected, on-disk tiers apart in the context manifest", async () => {
    stubServer("member");
    window.location.hash = "#/session/31";
    render(<App />);
    signIn();

    expect(await screen.findByRole("heading", { name: "修一个 bug" })).toBeTruthy();
    expect(screen.getByText("请帮我改一下")).toBeTruthy();
    expect(screen.getByText("已注入（原文）")).toBeTruthy();
    expect(screen.getByText("磁盘可能生效（非当时注入）")).toBeTruthy();
    // CLAUDE.md 只是磁盘现状，不能出现在「已注入」那一组里。
    const injected = screen.getByText("已注入（原文）").closest("section")!;
    expect(within(injected).getByText("AGENTS.md")).toBeTruthy();
    expect(within(injected).queryByText("CLAUDE.md")).toBeNull();
  });

  it("filters events down to tool calls", async () => {
    stubServer("member");
    window.location.hash = "#/session/31";
    render(<App />);
    signIn();
    await screen.findByText("请帮我改一下");

    fireEvent.click(screen.getByRole("button", { name: "工具调用" }));

    expect(screen.queryByText("请帮我改一下")).toBeNull();
    expect(screen.getByText("ls -la")).toBeTruthy();
  });

  it("deletes the session after confirmation and goes back to the owner's page", async () => {
    const requests = stubServer("member");
    vi.spyOn(window, "confirm").mockReturnValue(true);
    window.location.hash = "#/session/31";
    render(<App />);
    signIn();

    fireEvent.click(await screen.findByRole("button", { name: "删除会话" }));

    await waitFor(() => expect(requests).toContain("DELETE /api/v1/sessions/31"));
    await waitFor(() => expect(window.location.hash).toBe("#/member/2"));
  });

  it("does not delete when the confirmation is declined", async () => {
    const requests = stubServer("member");
    vi.spyOn(window, "confirm").mockReturnValue(false);
    window.location.hash = "#/session/31";
    render(<App />);
    signIn();

    fireEvent.click(await screen.findByRole("button", { name: "删除会话" }));

    expect(requests.some((r) => r.startsWith("DELETE"))).toBe(false);
  });
});

describe("项目页", () => {
  it("shows who spent what on the project, with member links for admins", async () => {
    stubServer("admin");
    window.location.hash = "#/project/11";
    render(<App />);
    signIn();

    expect(await screen.findByRole("heading", { name: "alpha" })).toBeTruthy();
    const bob = await screen.findByRole("link", { name: "bob" });
    expect(bob.getAttribute("href")).toBe("#/member/3");
  });

  it("offers rename and merge to admins and calls the admin API", async () => {
    const requests = stubServer("admin");
    vi.spyOn(window, "confirm").mockReturnValue(true);
    window.location.hash = "#/project/11";
    render(<App />);
    signIn();

    const nameInput = await screen.findByLabelText("项目名");
    fireEvent.change(nameInput, { target: { value: "阿尔法" } });
    fireEvent.click(screen.getByRole("button", { name: "改名" }));
    await waitFor(() => expect(requests).toContain("PUT /api/v1/admin/projects/11"));

    const select = await screen.findByLabelText("合并进");
    await screen.findByRole("option", { name: /alpha-copy/ });
    fireEvent.change(select, { target: { value: "12" } });
    fireEvent.click(screen.getByRole("button", { name: "合并" }));
    await waitFor(() => expect(requests).toContain("POST /api/v1/admin/projects/11/merge"));
    await waitFor(() => expect(window.location.hash).toBe("#/project/12"));
  });

  it("members see the project but no management panel and never call admin endpoints", async () => {
    const requests = stubServer("member");
    window.location.hash = "#/project/11";
    render(<App />);
    signIn();

    expect(await screen.findByRole("heading", { name: "alpha" })).toBeTruthy();
    await screen.findByText(/只统计你自己的用量/);
    expect(screen.queryByText("项目管理（管理员）")).toBeNull();
    expect(requests.some((r) => r.includes("/admin/"))).toBe(false);
  });

  it("the project list links each project to its page", async () => {
    stubServer("admin");
    window.location.hash = "#/projects";
    render(<App />);
    signIn();

    const link = await screen.findByRole("link", { name: "alpha" });
    expect(link.getAttribute("href")).toBe("#/project/11");
  });
});
