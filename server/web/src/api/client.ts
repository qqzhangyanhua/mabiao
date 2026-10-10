import type {
  AccountView,
  AdminProject,
  ApiErrorBody,
  CoverageView,
  LoginResponse,
  MergeProjectResponse,
  Project,
  ProjectDetail,
  SessionDetail,
  SessionListResponse,
  SummaryResponse,
} from "./types";

/** 与 `crates/push-protocol` 的 `PROTOCOL_VERSION` 一致；登录请求要带。 */
const PROTOCOL_VERSION = 1;

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export interface ScopeParams {
  accountId?: number;
  projectId?: number;
  /** RFC 3339，含。 */
  from?: string;
  /** RFC 3339，不含。 */
  to?: string;
}

export function toQueryString(
  params: Record<string, string | number | boolean | undefined>,
): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) search.set(key, String(value));
  }
  const text = search.toString();
  return text ? `?${text}` : "";
}

export interface Api {
  login(account: string, password: string): Promise<LoginResponse>;
  me(): Promise<AccountView>;
  accounts(): Promise<AccountView[]>;
  createMember(account: string, password: string): Promise<AccountView>;
  deactivate(accountId: number): Promise<AccountView>;
  coverage(): Promise<CoverageView[]>;
  summary(scope: ScopeParams, tzOffsetMinutes: number): Promise<SummaryResponse>;
  /** `generatedByWorkNotes`：`true` 只看「码表生成」的，`false` 排除它们，不传不过滤。 */
  sessions(
    scope: ScopeParams,
    limit: number,
    offset: number,
    generatedByWorkNotes?: boolean,
  ): Promise<SessionListResponse>;
  sessionDetail(id: number): Promise<SessionDetail>;
  deleteSession(id: number): Promise<void>;
  project(id: number, scope: ScopeParams, tzOffsetMinutes: number): Promise<ProjectDetail>;
  adminProjects(): Promise<AdminProject[]>;
  renameProject(id: number, name: string): Promise<Project>;
  mergeProject(id: number, intoProjectId: number): Promise<MergeProjectResponse>;
}

export interface ApiOptions {
  getToken: () => string | null;
  /** 服务端明确说 token 无效（401）时调用，用来回到登录页。登录请求本身的 401 不触发。 */
  onUnauthorized: () => void;
  fetchFn?: typeof fetch;
}

async function errorOf(response: Response): Promise<ApiError> {
  let body: Partial<ApiErrorBody> = {};
  try {
    body = (await response.json()) as Partial<ApiErrorBody>;
  } catch {
    // 不是 JSON（例如反向代理的错误页）：用状态码兜底。
  }
  return new ApiError(
    response.status,
    body.code ?? "unknown",
    body.message ?? `请求失败（HTTP ${response.status}）`,
  );
}

export function createApi(options: ApiOptions): Api {
  const doFetch = options.fetchFn ?? ((...args) => fetch(...args));

  async function request<T>(
    method: string,
    path: string,
    body?: unknown,
    authenticated = true,
  ): Promise<T> {
    const headers: Record<string, string> = {};
    const token = options.getToken();
    if (authenticated && token) headers.authorization = `Bearer ${token}`;
    if (body !== undefined) headers["content-type"] = "application/json";
    let response: Response;
    try {
      response = await doFetch(path, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      throw new ApiError(0, "network", "连不上服务，请检查网络后重试");
    }
    if (!response.ok) {
      const error = await errorOf(response);
      if (response.status === 401 && authenticated) options.onUnauthorized();
      throw error;
    }
    // 删除成功是 204，没有响应体。
    if (response.status === 204) return undefined as T;
    return (await response.json()) as T;
  }

  const scopeQuery = (scope: ScopeParams) => ({
    account_id: scope.accountId,
    project_id: scope.projectId,
    from: scope.from,
    to: scope.to,
  });

  return {
    login: (account, password) =>
      request(
        "POST",
        "/api/v1/login",
        { protocol_version: PROTOCOL_VERSION, account, password },
        false,
      ),
    me: () => request("GET", "/api/v1/me"),
    accounts: () => request("GET", "/api/v1/admin/accounts"),
    createMember: (account, password) =>
      request("POST", "/api/v1/admin/accounts", { account, password }),
    deactivate: (accountId) => request("POST", `/api/v1/admin/accounts/${accountId}/deactivate`),
    coverage: () => request("GET", "/api/v1/admin/coverage"),
    summary: (scope, tzOffsetMinutes) =>
      request(
        "GET",
        `/api/v1/usage/summary${toQueryString({ ...scopeQuery(scope), tz_offset_minutes: tzOffsetMinutes })}`,
      ),
    sessions: (scope, limit, offset, generatedByWorkNotes) =>
      request(
        "GET",
        `/api/v1/sessions${toQueryString({
          ...scopeQuery(scope),
          limit,
          offset,
          generated_by_work_notes: generatedByWorkNotes,
        })}`,
      ),
    sessionDetail: (id) => request("GET", `/api/v1/sessions/${id}`),
    deleteSession: (id) => request("DELETE", `/api/v1/sessions/${id}`),
    project: (id, scope, tzOffsetMinutes) =>
      request(
        "GET",
        `/api/v1/projects/${id}${toQueryString({ ...scopeQuery(scope), tz_offset_minutes: tzOffsetMinutes })}`,
      ),
    adminProjects: () => request("GET", "/api/v1/admin/projects"),
    renameProject: (id, name) => request("PUT", `/api/v1/admin/projects/${id}`, { name }),
    mergeProject: (id, intoProjectId) =>
      request("POST", `/api/v1/admin/projects/${id}/merge`, { into_project_id: intoProjectId }),
  };
}
