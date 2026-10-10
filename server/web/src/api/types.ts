// 与 `server/src/api.rs` 的响应形状一一对应。字段 snake_case，时间都是 RFC 3339 字符串。

export type Role = "admin" | "member";

export interface LoginResponse {
  token: string;
  expires_at: string;
  role: Role;
  account: string;
}

export interface AccountView {
  id: number;
  account: string;
  role: Role;
  active: boolean;
  created_at: string;
  deactivated_at: string | null;
}

export interface CoverageView {
  account_id: number;
  account: string;
  active: boolean;
  device_count: number;
  last_push_at: string | null;
  /** `YYYY-MM-DD`（UTC）。 */
  covered_through: string | null;
}

export interface UsageTotals {
  record_count: number;
  total_tokens: number;
  cost_snapshot_total: number;
  unified_cost_total: number;
  unified_unpriced_count: number;
}

export interface Breakdown {
  key: string;
  label: string;
  id: number | null;
  record_count: number;
  total_tokens: number;
  cost_snapshot: number;
  unified_cost: number;
  unpriced_count: number;
}

export interface SummaryResponse {
  totals: UsageTotals;
  by_day: Breakdown[];
  by_account: Breakdown[];
  by_source: Breakdown[];
  by_model: Breakdown[];
  by_project: Breakdown[];
}

export interface SessionListItem {
  id: number;
  account_id: number;
  account: string;
  device_name: string;
  source: string;
  session_id: string;
  title: string;
  project: string;
  project_id: number | null;
  project_name: string | null;
  model: string;
  started_at: string | null;
  ended_at: string | null;
  event_count: number;
  generated_by_work_notes: boolean;
  pushed_at: string;
}

export interface SessionListResponse {
  sessions: SessionListItem[];
  total: number;
}

/** 服务端统一错误体（`push_protocol::ApiError`）。 */
export interface ApiErrorBody {
  code: string;
  message: string;
}
