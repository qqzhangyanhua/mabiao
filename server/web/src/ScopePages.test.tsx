import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type {
  AccountView,
  BehaviorReport,
  SummaryResponse,
  UsageComposition,
} from "./api/types";

const PARTS = { input: 10, output: 4, cache_read: 6, cache_creation: 1, reasoning: 1 };

const COMPOSITION: UsageComposition = {
  totals: PARTS,
  by_model: [{ key: "gpt-5", ...PARTS }],
  by_source: [{ key: "codex", ...PARTS }],
  by_project: [{ key: "11", ...PARTS }],
  unpriced_models: [],
};

const BEHAVIOR: BehaviorReport = {
  tool_calls: 2,
  tool_failures: 0,
  other_errors: 0,
  failure_rate: 0,
  tools: [{ name: "Bash", calls: 2, failures: 0 }],
  tools_truncated: false,
};

const SUMMARY: SummaryResponse = {
  totals: {
    record_count: 1,
    total_tokens: 22,
    cost_snapshot_total: 1,
    unified_cost_total: 1,
    unified_unpriced_count: 0,
  },
  by_day: [],
  by_account: [],
  by_source: [
    {
      key: "codex",
      label: "codex",
      id: null,
      record_count: 1,
      total_tokens: 22,
      cost_snapshot: 1,
      unified_cost: 1,
      unpriced_count: 0,
      session_count: 1,
    },
  ],
  by_model: [],
  by_project: [
    {
      key: "git:github.com/team/alpha",
      label: "alpha",
      id: 11,
      record_count: 1,
      total_tokens: 22,
      cost_snapshot: 1,
      unified_cost: 1,
      unpriced_count: 0,
      session_count: 0,
    },
  ],
};

const ACCOUNTS: AccountView[] = [
  { id: 1, account: "root", role: "admin", active: true, created_at: "", deactivated_at: null },
  { id: 2, account: "alice", role: "member", active: true, created_at: "", deactivated_at: null },
];

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

interface StubOptions {
  role?: "admin" | "member";
  projectError?: { status: number; code: string; message: string };
  summaryError?: { status: number; code: string; message: string };
  devicesError?: { status: number; code: string; message: string };
}

function stubServer(options: StubOptions = {}) {
  const role = options.role ?? "admin";
  const requests: string[] = [];
  const me = role === "admin" ? ACCOUNTS[0] : ACCOUNTS[1];
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url =
        typeof input === "string" ? input : input instanceof URL ? input.pathname : input.url;
      requests.push(`${init?.method ?? "GET"} ${url}`);
      if (url === "/api/v1/login") {
        return Promise.resolve(json(200, { token: "tok", expires_at: "", role, account: me?.account }));
      }
      if (url === "/api/v1/me") return Promise.resolve(json(200, me));
      if (url.startsWith("/api/v1/projects/")) {
        if (options.projectError) {
          return Promise.resolve(
            json(options.projectError.status, {
              code: options.projectError.code,
              message: options.projectError.message,
            }),
          );
        }
        return Promise.resolve(
          json(200, {
            project: {
              id: 11,
              name: "alpha",
              key: "git:github.com/team/alpha",
              git_remote: "github.com/team/alpha",
              created_at: "2026-03-01T00:00:00Z",
            },
            summary: SUMMARY,
          }),
        );
      }
      if (url.startsWith("/api/v1/usage/composition")) return Promise.resolve(json(200, COMPOSITION));
      if (url.startsWith("/api/v1/usage/summary")) {
        if (options.summaryError) {
          return Promise.resolve(
            json(options.summaryError.status, {
              code: options.summaryError.code,
              message: options.summaryError.message,
            }),
          );
        }
        return Promise.resolve(json(200, SUMMARY));
      }
      if (url.startsWith("/api/v1/usage")) {
        return Promise.resolve(json(200, { records: [], totals: SUMMARY.totals }));
      }
      if (url.startsWith("/api/v1/sessions/behavior")) return Promise.resolve(json(200, BEHAVIOR));
      if (url.startsWith("/api/v1/sessions")) return Promise.resolve(json(200, { sessions: [], total: 0 }));
      if (/\/api\/v1\/accounts\/\d+\/devices$/.test(url)) {
        if (options.devicesError) {
          return Promise.resolve(
            json(options.devicesError.status, {
              code: options.devicesError.code,
              message: options.devicesError.message,
            }),
          );
        }
        return Promise.resolve(json(200, []));
      }
      if (url === "/api/v1/admin/accounts") return Promise.resolve(json(200, ACCOUNTS));
      if (url === "/api/v1/admin/projects") return Promise.resolve(json(200, []));
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

describe("洞察拆解与空闲态", () => {
  it("ProjectsPage fetches composition but never requests /api/v1/sessions/behavior", async () => {
    const requests = stubServer({ role: "admin" });
    window.location.hash = "#/projects";
    render(<App />);
    signIn();

    expect(await screen.findByRole("link", { name: /alpha/ })).toBeTruthy();
    await waitFor(() =>
      expect(requests.some((r) => r.includes("/api/v1/usage/composition"))).toBe(true),
    );
    expect(requests.some((r) => r.includes("/api/v1/sessions/behavior"))).toBe(false);
  });

  it("SourcesPage fetches composition and usage but never requests /api/v1/sessions/behavior", async () => {
    const requests = stubServer({ role: "admin" });
    window.location.hash = "#/sources";
    render(<App />);
    signIn();

    expect(await screen.findByRole("heading", { name: "Agent 工具" })).toBeTruthy();
    expect(await screen.findByRole("heading", { name: "消耗明细" })).toBeTruthy();
    await waitFor(() =>
      expect(requests.some((r) => r.includes("/api/v1/usage/composition"))).toBe(true),
    );
    expect(requests.some((r) => r.includes("/api/v1/sessions/behavior"))).toBe(false);
  });

  it("incomplete custom date range stays idle without Promise.reject or data fetches", async () => {
    const rejectSpy = vi.spyOn(Promise, "reject");
    const requests = stubServer({ role: "admin" });
    window.location.hash = "#/overview?range=custom&from=2026-03-10&to=2026-03-01";
    render(<App />);
    signIn();

    expect(await screen.findByText("请选择完整的起止日期（结束不早于开始）")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "行为指标" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "消耗明细" })).toBeNull();
    expect(requests.some((r) => r.includes("/api/v1/usage"))).toBe(false);
    expect(requests.some((r) => r.includes("/api/v1/sessions"))).toBe(false);
    expect(rejectSpy).not.toHaveBeenCalled();
  });
});

describe("页面级错误短路", () => {
  it("ProjectPage 404 renders a single error without subordinate panels or extra requests", async () => {
    const requests = stubServer({
      role: "admin",
      projectError: { status: 404, code: "not_found", message: "项目不存在" },
    });
    window.location.hash = "#/project/999";
    render(<App />);
    signIn();

    expect((await screen.findByRole("alert")).textContent).toContain("项目不存在");
    expect(screen.queryByRole("heading", { name: "行为指标" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "消耗明细" })).toBeNull();
    expect(screen.queryByText(/会话/)).toBeNull();
    expect(screen.queryByText("项目管理（管理员）")).toBeNull();
    expect(requests.some((r) => r.includes("/api/v1/usage"))).toBe(false);
    expect(requests.some((r) => r.includes("/api/v1/sessions"))).toBe(false);
    expect(requests.some((r) => r.includes("/api/v1/admin/projects"))).toBe(false);
  });

  it("OverviewPage summary error hides BehaviorPanel and UsageSection", async () => {
    stubServer({
      role: "admin",
      summaryError: { status: 500, code: "internal", message: "汇总查询失败" },
    });
    window.location.hash = "#/overview";
    render(<App />);
    signIn();

    expect((await screen.findByRole("alert")).textContent).toContain("汇总查询失败");
    expect(screen.queryByRole("heading", { name: "行为指标" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "消耗明细" })).toBeNull();
  });

  it("MemberPage scope error hides DeviceList, BehaviorPanel, UsageSection, and SessionList", async () => {
    stubServer({
      role: "admin",
      summaryError: { status: 403, code: "forbidden", message: "只能查看自己的数据" },
    });
    window.location.hash = "#/member/2";
    render(<App />);
    signIn();

    expect((await screen.findByRole("alert")).textContent).toContain("只能查看自己的数据");
    expect(screen.queryByRole("heading", { name: "设备" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "行为指标" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "消耗明细" })).toBeNull();
    expect(screen.queryByRole("heading", { name: /会话/ })).toBeNull();
  });

  it("MemberPage keeps a localized devices error inside DeviceList when summary succeeds", async () => {
    stubServer({
      role: "member",
      devicesError: { status: 500, code: "internal", message: "设备列表暂时不可用" },
    });
    window.location.hash = "#/member/2";
    render(<App />);
    signIn();

    expect(await screen.findByRole("heading", { name: "行为指标" })).toBeTruthy();
    expect(await screen.findByRole("heading", { name: "设备" })).toBeTruthy();
    expect((await screen.findByRole("alert")).textContent).toContain("设备列表暂时不可用");
  });

  it("SourcesPage summary error hides UsageSection", async () => {
    stubServer({
      role: "admin",
      summaryError: { status: 500, code: "internal", message: "来源汇总失败" },
    });
    window.location.hash = "#/sources";
    render(<App />);
    signIn();

    expect((await screen.findByRole("alert")).textContent).toContain("来源汇总失败");
    expect(screen.queryByRole("heading", { name: "消耗明细" })).toBeNull();
  });
});
