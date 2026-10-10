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

export type EventKind =
  | "message"
  | "plan"
  | "tool_call"
  | "tool_result"
  | "model_change"
  | "error"
  | "system_status"
  | "unadapted";

export type EventActor = "user" | "assistant" | "tool";

/** 一条语义事件；`text` 与 `details` 推送前已按内置规则打码。 */
export interface SessionEvent {
  event_id: string;
  sequence: number;
  source_file: string;
  source_sequence: number;
  kind: EventKind;
  occurred_at: string | null;
  actor: EventActor | null;
  name: string | null;
  text: string | null;
  details?: unknown;
}

/** 证据层级。`on_disk_possible` 是磁盘现状，不是会话当时的状态。 */
export type ContextLayer = "injected" | "observed" | "on_disk_possible";

export interface ContextItem {
  layer: ContextLayer;
  kind: string;
  id: string;
  label: string;
  path?: string;
  load_mode?: string;
  injection_status?: string;
  char_count?: number;
  is_noise?: boolean;
  is_unused_install?: boolean;
  /** 注入原文，只有 `injected` 层且源快照还在时才有。 */
  content?: string;
}

export interface ContextManifest {
  items: ContextItem[];
  /** 为 false 时没有会话当时的注入快照（Cursor 按当前磁盘重建）。 */
  has_injected_snapshot: boolean;
  /** 源快照已被清理，条目只是度量，没有原文。 */
  from_cache?: boolean;
  volume_is_estimate?: boolean;
}

export interface SessionUsage {
  totals: UsageTotals;
  by_model: Breakdown[];
}

export interface SessionDetail {
  session: SessionListItem;
  source_files: string[];
  redaction_count: number;
  events: SessionEvent[];
  context_manifest: ContextManifest | null;
  usage: SessionUsage;
}

export interface Project {
  id: number;
  name: string;
  key: string;
  /** 归一后的 remote（`host/owner/repo`）；目录兜底的项目为空。 */
  git_remote: string | null;
  created_at: string;
}

export interface ProjectDetail {
  project: Project;
  summary: SummaryResponse;
}

export interface AdminProject extends Project {
  session_count: number;
  usage_record_count: number;
  /** 已经合并进它的项目个数。 */
  merged_count: number;
}

export interface MergeProjectResponse {
  project: Project;
  sessions_moved: number;
  usage_records_moved: number;
}

/** 服务端统一错误体（`push_protocol::ApiError`）。 */
export interface ApiErrorBody {
  code: string;
  message: string;
}
