import { describe, expect, it, vi } from "vitest";
import { ApiError, createApi, toQueryString } from "./client";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function setup(respond: () => Response | Promise<Response>, token: string | null = "tok") {
  const fetchFn = vi.fn((_input: RequestInfo | URL, _init?: RequestInit) =>
    Promise.resolve(respond()),
  );
  const onUnauthorized = vi.fn();
  const api = createApi({ getToken: () => token, onUnauthorized, fetchFn });
  return { api, fetchFn, onUnauthorized };
}

describe("toQueryString", () => {
  it("skips undefined values and encodes the rest", () => {
    expect(toQueryString({ a: 1, b: undefined, c: "x y" })).toBe("?a=1&c=x+y");
    expect(toQueryString({})).toBe("");
  });
});

describe("createApi", () => {
  it("sends the bearer token on authenticated calls", async () => {
    const { api, fetchFn } = setup(() => jsonResponse(200, []));
    await api.accounts();
    const [, init] = fetchFn.mock.calls[0]!;
    expect((init?.headers as Record<string, string>).authorization).toBe("Bearer tok");
  });

  it("logs in without a token and with the protocol version", async () => {
    const { api, fetchFn } = setup(() =>
      jsonResponse(200, { token: "t", expires_at: "x", role: "admin", account: "root" }),
    );
    await api.login("root", "pw");
    const [, init] = fetchFn.mock.calls[0]!;
    expect((init?.headers as Record<string, string>).authorization).toBeUndefined();
    expect(JSON.parse(init?.body as string)).toEqual({
      protocol_version: 1,
      account: "root",
      password: "pw",
    });
  });

  it("builds the summary query from scope and timezone", async () => {
    const { api, fetchFn } = setup(() => jsonResponse(200, {}));
    await api.summary({ accountId: 3, from: "2026-03-01T00:00:00.000Z" }, 480);
    expect(fetchFn.mock.calls[0]![0]).toBe(
      "/api/v1/usage/summary?account_id=3&from=2026-03-01T00%3A00%3A00.000Z&tz_offset_minutes=480",
    );
  });

  it("turns the server error body into an ApiError with the Chinese message", async () => {
    const { api } = setup(() =>
      jsonResponse(403, { code: "forbidden", message: "只能查看自己的数据" }),
    );
    await expect(api.accounts()).rejects.toMatchObject({
      status: 403,
      code: "forbidden",
      message: "只能查看自己的数据",
    });
  });

  it("falls back to the status code when the error body is not JSON", async () => {
    const { api } = setup(() => new Response("<html>bad gateway</html>", { status: 502 }));
    const error = await api.accounts().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).message).toContain("502");
  });

  it("signals logout on 401 for authenticated calls only", async () => {
    const expired = setup(() =>
      jsonResponse(401, { code: "token_expired", message: "登录已过期" }),
    );
    await expect(expired.api.me()).rejects.toBeInstanceOf(ApiError);
    expect(expired.onUnauthorized).toHaveBeenCalledOnce();

    const wrongPassword = setup(() =>
      jsonResponse(401, { code: "invalid_credentials", message: "账号或密码错误" }),
    );
    await expect(wrongPassword.api.login("a", "b")).rejects.toMatchObject({
      code: "invalid_credentials",
    });
    expect(wrongPassword.onUnauthorized).not.toHaveBeenCalled();
  });

  it("reports a network failure in plain words", async () => {
    const fetchFn = vi.fn(() => Promise.reject(new TypeError("Failed to fetch")));
    const api = createApi({ getToken: () => "t", onUnauthorized: vi.fn(), fetchFn });
    await expect(api.me()).rejects.toMatchObject({ status: 0, code: "network" });
  });
});
