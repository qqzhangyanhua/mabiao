import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type {
  AccountView,
  BehaviorReport,
  DeviceView,
  SummaryResponse,
  UsageComposition,
  UsageRecord,
} from "./api/types";

const TOKENS = { input: 1, output: 1, cache_read: 0, cache_creation: 0, reasoning: 0, total: 2 };

const PARTS = { input: 10, output: 4, cache_read: 6, cache_creation: 1, reasoning: 1 };

const COMPOSITION: UsageComposition = {
  totals: PARTS,
  by_model: [{ key: "native-model", ...PARTS }, { key: "missing-model", ...PARTS }],
  by_source: [{ key: "codex", ...PARTS }],
  by_project: [],
  unpriced_models: [{ model: "missing-model", count: 1 }],
};

const BEHAVIOR: BehaviorReport = {
  tool_calls: 4,
  tool_failures: 1,
  other_errors: 1,
  failure_rate: 0.25,
  tools: [{ name: "Bash", calls: 1, failures: 1 }],
  tools_truncated: false,
};

const SUMMARY: SummaryResponse = {
  totals: {
    record_count: 2,
    total_tokens: 12,
    cost_snapshot_total: 1,
    unified_cost_total: 3,
    unified_unpriced_count: 1,
  },
  by_day: [],
  by_account: [],
  by_source: [],
  by_model: [],
  by_project: [],
};

const RECORD: UsageRecord = {
  id: 9,
  account_id: 2,
  account: "alice",
  occurred_at: "2026-03-02T08:00:00Z",
  source: "codex",
  model: "missing-model",
  provider: "openai",
  project: "mabiao",
  project_id: 1,
  session_id: "s-1",
  session_pk: 41,
  tokens: { ...TOKENS },
  native_cost: null,
  cost_snapshot: 1.5,
  pricing_source: "unpriced",
  unified_cost: null,
  unified_pricing_source: "unpriced",
  unified_cost_source: "none",
};

const DEVICES: DeviceView[] = [
  {
    device_id: "secret-device-id",
    device_name: "旧笔记本",
    first_seen_at: "2026-01-01T00:00:00Z",
    last_seen_at: "2026-03-01T00:00:00Z",
  },
  {
    device_id: "other-secret",
    device_name: "新台式机",
    first_seen_at: "2026-02-01T00:00:00Z",
    last_seen_at: "2026-03-05T00:00:00Z",
  },
];

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

function stub(role: "admin" | "member") {
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
      if (url.startsWith("/api/v1/usage/composition")) return Promise.resolve(json(200, COMPOSITION));
      if (url.startsWith("/api/v1/usage/summary")) return Promise.resolve(json(200, SUMMARY));
      if (url.startsWith("/api/v1/usage")) {
        return Promise.resolve(json(200, { records: [RECORD], totals: SUMMARY.totals }));
      }
      if (url.startsWith("/api/v1/sessions/behavior")) return Promise.resolve(json(200, BEHAVIOR));
      if (url.startsWith("/api/v1/sessions")) return Promise.resolve(json(200, { sessions: [], total: 0 }));
      if (/\/api\/v1\/accounts\/\d+\/devices$/.test(url)) return Promise.resolve(json(200, DEVICES));
      if (url.startsWith("/api/v1/work-timeline")) {
        return Promise.resolve(
          json(200, {
            days: [],
            segment_count: 0,
            total_tokens: 0,
            cost_snapshot: 0,
            unified_cost: 0,
            truncated: false,
          }),
        );
      }
      if (url === "/api/v1/admin/accounts") return Promise.resolve(json(200, ACCOUNTS));
      if (url === "/api/v1/admin/coverage") return Promise.resolve(json(200, []));
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
});

describe("消耗明细、设备与行为", () => {
  it("filters unpriced usage from the overview and links a row to its session", async () => {
    const requests = stub("admin");
    render(<App />);
    signIn();
    expect(await screen.findByRole("heading", { name: "行为指标" })).toBeTruthy();
    expect(await screen.findByText("25%")).toBeTruthy();
    expect(screen.getByText("Bash")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "消耗明细" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "1 条未计入" }));
    await waitFor(() =>
      expect(requests.some((r) => r.includes("/api/v1/usage?") && r.includes("unified_cost_source=none"))).toBe(
        true,
      ),
    );
    const pricing = screen.getByRole("radiogroup", { name: "定价来源" });
    expect(within(pricing).getByRole("radio", { name: "未定价" }).getAttribute("aria-checked")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "missing-model · 1" }));
    await waitFor(() =>
      expect(
        requests.some(
          (r) => r.includes("unified_cost_source=none") && r.includes("model=missing-model"),
        ),
      ).toBe(true),
    );
    const link = await screen.findByRole("link", { name: /2026-03-02/ });
    expect(link.getAttribute("href")).toContain("#/session/41");
    expect(screen.getByRole("link", { name: "去价目" }).getAttribute("href")).toContain("#/pricing");
  });

  it("lists a member's devices by last seen and hides the device id", async () => {
    const requests = stub("member");
    render(<App />);
    signIn();
    expect(await screen.findByRole("heading", { name: "设备" })).toBeTruthy();
    expect(await screen.findByText("新台式机")).toBeTruthy();
    const names = screen.getAllByRole("row").map((row) => row.textContent ?? "");
    const desktop = names.findIndex((text) => text.includes("新台式机"));
    const laptop = names.findIndex((text) => text.includes("旧笔记本"));
    expect(desktop).toBeGreaterThan(-1);
    expect(laptop).toBeGreaterThan(desktop);
    expect(screen.queryByText("secret-device-id")).toBeNull();
    expect(screen.queryByRole("link", { name: "去价目" })).toBeNull();
    expect(requests.some((r) => r.includes("/api/v1/accounts/2/devices"))).toBe(true);
    expect(screen.getByRole("heading", { name: "行为指标" })).toBeTruthy();
    expect(screen.getByText("Bash")).toBeTruthy();
    expect(screen.queryByText("这段时间没有工具调用")).toBeNull();
  });
});
