import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { AccountView, CoverageView, SummaryResponse } from "./api/types";

const EMPTY_SUMMARY: SummaryResponse = {
  totals: {
    record_count: 3,
    total_tokens: 5000,
    cost_snapshot_total: 9,
    unified_cost_total: 0.01,
    unified_unpriced_count: 1,
  },
  by_day: [],
  by_account: [],
  by_source: [],
  by_model: [],
  by_project: [],
};

const ACCOUNTS: AccountView[] = [
  { id: 1, account: "root", role: "admin", active: true, created_at: "", deactivated_at: null },
  { id: 2, account: "alice", role: "member", active: true, created_at: "", deactivated_at: null },
  { id: 3, account: "carol", role: "member", active: true, created_at: "", deactivated_at: null },
];

const TODAY = new Date().toISOString().slice(0, 10);

const COVERAGE: CoverageView[] = [
  {
    account_id: 1,
    account: "root",
    active: true,
    device_count: 0,
    last_push_at: null,
    covered_through: null,
  },
  {
    account_id: 2,
    account: "alice",
    active: true,
    device_count: 1,
    last_push_at: "2026-03-01T10:00:00Z",
    covered_through: TODAY,
  },
  {
    account_id: 3,
    account: "carol",
    active: true,
    device_count: 1,
    last_push_at: "2026-01-02T10:00:00Z",
    covered_through: "2026-01-01",
  },
];

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

/** 按路径回桩；记录下所有请求，供断言「发了什么」。 */
function stubServer(role: "admin" | "member") {
  const requests: string[] = [];
  const me = role === "admin" ? ACCOUNTS[0] : ACCOUNTS[1];
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url =
        typeof input === "string" ? input : input instanceof URL ? input.pathname : input.url;
      requests.push(`${init?.method ?? "GET"} ${url}`);
      if (url === "/api/v1/login") {
        const body = JSON.parse(init?.body as string) as { password: string };
        return Promise.resolve(
          body.password === "good"
            ? json(200, { token: "tok", expires_at: "", role, account: me?.account })
            : json(401, { code: "invalid_credentials", message: "账号或密码错误" }),
        );
      }
      if (url === "/api/v1/me") return Promise.resolve(json(200, me));
      if (url.startsWith("/api/v1/usage/summary")) return Promise.resolve(json(200, EMPTY_SUMMARY));
      if (url.startsWith("/api/v1/sessions"))
        return Promise.resolve(json(200, { sessions: [], total: 0 }));
      if (url === "/api/v1/admin/accounts") return Promise.resolve(json(200, ACCOUNTS));
      if (url === "/api/v1/admin/coverage") return Promise.resolve(json(200, COVERAGE));
      return Promise.resolve(json(404, { code: "not_found", message: "没有这个接口" }));
    }),
  );
  return requests;
}

function signIn(password = "good") {
  fireEvent.change(screen.getByLabelText("账号"), { target: { value: "someone" } });
  fireEvent.change(screen.getByLabelText("密码"), { target: { value: password } });
  fireEvent.click(screen.getByRole("button", { name: "登录" }));
}

beforeEach(() => {
  sessionStorage.clear();
  window.location.hash = "";
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("登录", () => {
  it("shows the server's message when the password is wrong and stays on the login page", async () => {
    stubServer("admin");
    render(<App />);
    signIn("bad");
    expect((await screen.findByRole("alert")).textContent).toBe("账号或密码错误");
    expect(screen.queryByText("退出")).toBeNull();
    expect(sessionStorage.getItem("mabiao.token")).toBeNull();
  });

  it("an admin lands on the team overview with admin navigation", async () => {
    const requests = stubServer("admin");
    render(<App />);
    signIn();
    expect(await screen.findByRole("heading", { name: "团队总览" })).toBeTruthy();
    expect(screen.getByRole("link", { name: "成员" })).toBeTruthy();
    expect(
      requests.some((r) => r.startsWith("GET /api/v1/usage/summary?") && !r.includes("account_id")),
    ).toBe(true);
    expect(await screen.findByText(/未计入/)).toBeTruthy();
  });

  it("a member lands on their own page and sees no team navigation", async () => {
    const requests = stubServer("member");
    render(<App />);
    signIn();
    expect(await screen.findByRole("heading", { name: /alice/ })).toBeTruthy();
    expect(screen.queryByRole("link", { name: "团队总览" })).toBeNull();
    expect(screen.queryByRole("link", { name: "成员" })).toBeNull();
    expect(requests.some((r) => r.includes("/usage/summary") && r.includes("account_id=2"))).toBe(
      true,
    );
  });

  it("a member who types the admin URL is sent back to their own page", async () => {
    const requests = stubServer("member");
    window.location.hash = "#/members";
    render(<App />);
    signIn();
    expect(await screen.findByRole("heading", { name: /alice/ })).toBeTruthy();
    expect(requests.some((r) => r.includes("/admin/"))).toBe(false);
  });

  it("logging out returns to the login page and drops the token", async () => {
    stubServer("admin");
    render(<App />);
    signIn();
    fireEvent.click(await screen.findByRole("button", { name: "退出" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "登录" })).toBeTruthy());
    expect(sessionStorage.getItem("mabiao.token")).toBeNull();
  });
});

describe("成员管理", () => {
  it("shows last push and coverage per member, flagging someone who never pushed", async () => {
    stubServer("admin");
    window.location.hash = "#/members";
    render(<App />);
    signIn();
    expect(await screen.findByText(TODAY)).toBeTruthy();
    expect(screen.getAllByText("从未推送").length).toBe(1);
    expect(screen.getByText("2026-01-01（有缺口）")).toBeTruthy();
  });

  it("creates a member through the admin API and reloads the list", async () => {
    const requests = stubServer("admin");
    window.location.hash = "#/members";
    render(<App />);
    signIn();
    await screen.findByText(TODAY);
    fireEvent.change(screen.getByLabelText("新成员账号"), { target: { value: "bob" } });
    fireEvent.change(screen.getByLabelText("初始密码"), { target: { value: "a-long-password" } });
    fireEvent.click(screen.getByRole("button", { name: "创建成员" }));
    await waitFor(() => expect(requests).toContain("POST /api/v1/admin/accounts"));
    await waitFor(() =>
      expect(requests.filter((r) => r === "GET /api/v1/admin/accounts").length).toBeGreaterThan(1),
    );
  });
});
