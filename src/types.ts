export type Filter = {
  from: string | null;
  to: string | null;
  sources: string[];
  models: string[];
  projects: string[];
  providers: string[];
};

export type OverviewDto = {
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  reasoning_tokens: number;
  session_count: number;
  cost: number | null;
  unpriced: boolean;
};

export type BurnRateDto = {
  tokens_per_minute: number;
  cost_per_hour: number | null;
};

export type ProjectionDto = {
  total_tokens: number;
  cost: number | null;
};

export type BillingWindowDto = {
  source: string;
  application: string;
  start: string;
  end: string;
  last_activity: string;
  is_active: boolean;
  elapsed_minutes: number;
  remaining_minutes: number | null;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  reasoning_tokens: number;
  session_count: number;
  cost: number | null;
  unpriced: boolean;
  burn: BurnRateDto | null;
  projection: ProjectionDto | null;
};

export type WeeklyWindowDto = {
  source: string;
  application: string;
  window_days: number;
  start: string;
  end: string;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  reasoning_tokens: number;
  session_count: number;
  cost: number | null;
  unpriced: boolean;
  daily_average_tokens: number;
  daily_average_cost: number | null;
};

export type BillingWindowsDto = {
  now: string;
  window_hours: number;
  current: BillingWindowDto[];
  recent: BillingWindowDto[];
  weekly_window_days: number;
  weekly: WeeklyWindowDto[];
};

export type OfficialQuotaFreshness = "official" | "stale" | "unavailable";

export type OfficialQuotaWindow = {
  kind: string;
  label: string;
  used_percent: number | null;
  resets_at: string | null;
  /** 金额口径。充值制的自定义提供商给的是钱不是百分比，两种口径可以并存。 */
  used_amount: number | null;
  limit_amount: number | null;
  /** ISO 4217 代码，例如 USD。取不到时为 null，只显示数字。 */
  currency: string | null;
};

export type OfficialQuotaRow = {
  provider: string;
  application: string;
  windows: OfficialQuotaWindow[];
  freshness: OfficialQuotaFreshness;
  captured_at: string | null;
  error: string | null;
};

export type OfficialQuotaConfig = {
  alerts_enabled: boolean;
  /** 主窗口「配置显示」里关掉的官方额度账号，托盘额度面板复用同一份配置。 */
  hidden_providers: string[];
};

export type OfficialQuotaDto = {
  rows: OfficialQuotaRow[];
  alerts_enabled: boolean;
  stale_after_minutes: number;
  /** 本机没检测到登录态、因而没出现在 rows 里的账号（provider id）。 */
  undetected: string[];
  /** 与 OfficialQuotaConfig.hidden_providers 一致，供本地状态对齐用。 */
  hidden_providers: string[];
};

export type OfficialQuotaHookDto = {
  settings_path: string;
  command: string;
  snippet: string;
  already_configured: boolean;
  conflict: boolean;
  conflict_command: string | null;
};

export type InstructionLoadStatus =
  | "loaded"
  | "present_unloaded"
  | "locally_invisible"
  | "not_created";

export type InstructionEvidence = "verified" | "inferred" | "no_mechanism";

export type InstructionEntryKind = "file" | "directory";

export type GlobalInstructionFile = {
  kind: InstructionEntryKind;
  display_path: string;
  abs_path: string;
  byte_size: number;
  modified_at: string | null;
  load_status: InstructionLoadStatus;
  evidence: InstructionEvidence;
  content: string;
  error: string | null;
  note: string | null;
  action: string | null;
  editable: boolean;
};

export type GlobalInstructionSourceRow = {
  source: string;
  application: string;
  files: GlobalInstructionFile[];
};

export type InstructionCheckupKind =
  | "empty"
  | "present_unloaded"
  | "override_shields"
  | "near_limit"
  | "over_limit"
  | "orphan_memories"
  | "auto_memory";

export type InstructionCheckupSeverity = "low" | "medium" | "high" | "critical";

export type InstructionCheckupFinding = {
  kind: InstructionCheckupKind;
  severity: InstructionCheckupSeverity;
  source: string;
  application: string;
  display_path: string;
  problem: string;
  consequence: string;
};

export type InstructionOverlapHint = {
  keyword: string;
  global_application: string;
  global_display_path: string;
  global_snippet: string;
  project_display_path: string;
  project_snippet: string;
};

export type InstructionInvestment = {
  source: string;
  application: string;
  loaded_bytes: number;
  modified_at: string | null;
  total_tokens: number;
};

export type InstructionImbalance = {
  source: string;
  application: string;
  note: string;
};

export type ClaudeAutoMemoryFile = {
  name: string;
  abs_path: string;
  byte_size: number;
  modified_at: string | null;
  content: string;
};

export type ClaudeAutoMemoryRepo = {
  repo: string;
  display_path: string;
  abs_path: string;
  byte_size: number;
  modified_at: string | null;
  files: ClaudeAutoMemoryFile[];
};

export type GlobalInstructionDto = {
  sources: GlobalInstructionSourceRow[];
  findings: InstructionCheckupFinding[];
  selected_project: string | null;
  projects: string[];
  hints: InstructionOverlapHint[];
  investments: InstructionInvestment[];
  imbalances: InstructionImbalance[];
  claude_memories: ClaudeAutoMemoryRepo[];
};

export type WriteUserFileRequest = {
  abs_path: string;
  content: string;
  expected_mtime: string | null;
};

export type WriteUserFileResult = {
  modified_at: string | null;
  byte_size: number;
};

export type BudgetConfig = {
  monthly_usd: number | null;
};

export type BudgetStatusDto = {
  monthly_budget: number | null;
  month: string;
  days_elapsed: number;
  days_in_month: number;
  month_to_date_cost: number;
  unpriced: boolean;
  projected_month_cost: number | null;
  percent_used: number | null;
  percent_projected: number | null;
  thresholds: number[];
};

export type Grain = "hour" | "day" | "week" | "month";

export type View =
  | "overview"
  | "trend"
  | "application"
  | "model"
  | "provider"
  | "project"
  | "conversations"
  | "cursor"
  | "cursor-sessions"
  | "worktime"
  | "instructions"
  | "settings";

export type ConversationFocus = {
  source: string;
  session_id: string;
};

export type SeriesPoint = {
  bucket: string;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  reasoning_tokens: number;
  cost: number | null;
};

export type NamedAmount = {
  name: string;
  total_tokens: number;
  share: number;
  cost: number | null;
  unpriced: boolean;
};

export type EfficiencyMetrics = {
  total_tokens: number;
  session_count: number;
  cache_hit_rate: number | null;
  average_session_tokens: number | null;
  reasoning_share: number | null;
};

export type ApplicationEfficiency = {
  source: string;
  application: string;
  metrics: EfficiencyMetrics;
};

export type ApplicationTrendPoint = {
  bucket: string;
  total_tokens: number;
  values: Record<string, number>;
};

export type ProjectApplicationRow = {
  project: string;
  total_tokens: number;
  values: Record<string, number>;
};

export type ApplicationAnalyticsDto = {
  summary: EfficiencyMetrics;
  by_application: ApplicationEfficiency[];
  trend: ApplicationTrendPoint[];
  projects: ProjectApplicationRow[];
};

export type SessionRow = {
  session_id: string;
  source: string;
  project: string;
  model: string;
  total_tokens: number;
  started_at: string;
  ended_at: string;
  source_file: string;
  cost: number | null;
  unpriced: boolean;
};

export type SortDir = "asc" | "desc";

export type ConversationQuery = {
  search?: string | null;
  page?: number;
  page_size?: number;
  sources?: string[];
  projects?: string[];
};

export type ConversationSessionRow = {
  source: string;
  session_id: string;
  title: string;
  project: string;
  model: string;
  started_at: string;
  ended_at: string;
  source_file: string;
  source_files: string[];
  capabilities: string[];
  support_status: string;
  file_available: boolean;
  total_tokens: number;
  cost: number | null;
  unpriced: boolean;
};

export type ConversationPage = {
  rows: ConversationSessionRow[];
  total: number;
};

export type ConversationMessage = {
  role: string;
  occurred_at: string;
  text: string;
};

export type ConversationEventKind =
  | "message"
  | "plan"
  | "tool_call"
  | "tool_result"
  | "model_change"
  | "error"
  | "system_status"
  | "unadapted";

export type ConversationEventActor = "user" | "assistant" | "tool";

export type ConversationEventCapabilityStatus =
  | "complete"
  | "missing_timestamp"
  | "unadapted"
  | "unadapted_missing_timestamp";

export type ConversationEventContentStatus = "complete" | "deferred";

export type ConversationAttachmentKind = "image" | "file";

export type ConversationAttachmentStatus = "available" | "missing" | "embedded" | "unsupported";

export type ConversationAttachment = {
  id: string;
  kind: ConversationAttachmentKind;
  name: string;
  original_path: string;
  media_type: string;
  size_bytes: number | null;
  status: ConversationAttachmentStatus;
};

export type ConversationEvent = {
  event_id: string;
  sequence: number;
  source_file: string;
  source_sequence: number;
  kind: ConversationEventKind;
  occurred_at: string | null;
  actor: ConversationEventActor | null;
  name: string | null;
  text: string | null;
  details: unknown;
  attachments: ConversationAttachment[];
  capability_status: ConversationEventCapabilityStatus;
  content_status: ConversationEventContentStatus;
};

export type ConversationEventContentDto = {
  event_id: string;
  text: string | null;
  details: unknown;
};

export type ConversationAttachmentContentDto = {
  attachment: ConversationAttachment;
  data_url: string;
};

export type ConversationUsagePage = {
  rows: ConversationUsageRecord[];
  total: number;
};

export type ConversationUsageRecord = {
  occurred_at: string;
  source: string;
  model: string;
  provider: string;
  project: string;
  session_id: string;
  source_file: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  reasoning_tokens: number;
  total_tokens: number;
  native_cost: number | null;
};

export type ConversationAgentLinkStatus =
  | "linked"
  | "missing_source"
  | "unresolved"
  | "conflict"
  | "cycle";

export type ConversationAgentCapabilityStatus = "complete" | "partial" | "unavailable";

export type ConversationAgentLink = {
  relationship_id: string;
  session_id: string | null;
  launch_event_id: string | null;
  status: ConversationAgentLinkStatus;
  session: ConversationSessionRow | null;
};

export type ConversationAgentRelations = {
  capability_status: ConversationAgentCapabilityStatus;
  parent: ConversationAgentLink | null;
  children: ConversationAgentLink[];
};

export type ConversationEventAnchor =
  | { type: "first" }
  | { type: "last" }
  | { type: "before"; sequence: number }
  | { type: "after"; sequence: number };

export type ConversationEventPage = {
  events: ConversationEvent[];
  has_more_before: boolean;
  has_more_after: boolean;
};

export interface ConversationDetailDto {
  revision: string;
  session: ConversationSessionRow;
  event_count: number;
  usage_record_count: number;
  agent_relations: ConversationAgentRelations;
  cursor_behavior?: CursorSessionDetailDto | null;
}

export type ConversationDetailStateDto = {
  revision: string;
  changed: boolean;
  file_available: boolean;
};

export type ConversationIndexProgressDto = {
  indexed: number;
  total: number;
};

export type CostSource = "native" | "user" | "snapshot" | "none";

export type PriceOrigin = "user" | "snapshot";

export type PriceEntry = {
  model: string;
  provider: string | null;
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
  origin?: PriceOrigin;
};

export type PriceTable = {
  prices: PriceEntry[];
};

export type PriceSnapshotMeta = {
  as_of: string;
  source: string;
  count: number;
  bundled: boolean;
};

export type CodeVolumeCommit = {
  commit_hash: string;
  branch: string;
  scored_at: string;
  commit_message: string;
  lines_added: number;
  lines_deleted: number;
  composer_lines_added: number;
  composer_lines_deleted: number;
  human_lines_added: number;
  human_lines_deleted: number;
  tab_lines_added: number;
  tab_lines_deleted: number;
  ai_percentage: number | null;
};

export type CodeVolumeDailyPoint = {
  bucket: string;
  lines_added: number;
  lines_deleted: number;
  composer_lines_added: number;
  tab_lines_added: number;
  human_lines_added: number;
};

export type CodeVolumeBranchRow = {
  name: string;
  commit_count: number;
  lines_added: number;
  composer_lines_added: number;
};

export type CodeVolumeSummary = {
  commit_count: number;
  lines_added: number;
  lines_deleted: number;
  net_lines: number;
  composer_lines_added: number;
  composer_lines_deleted: number;
  human_lines_added: number;
  human_lines_deleted: number;
  tab_lines_added: number;
  tab_lines_deleted: number;
  ai_percentage: number | null;
  total_cost: number | null;
  cost_unpriced: boolean;
  cost_per_thousand_ai_lines: number | null;
  daily: CodeVolumeDailyPoint[];
  by_branch: CodeVolumeBranchRow[];
  commits: CodeVolumeCommit[];
};

export type CursorAccountUsageDto = {
  as_of: string | null;
  event_count: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  daily: SeriesPoint[];
  by_model: NamedAmount[];
  headless_tokens: number;
  interactive_tokens: number;
  headless_share: number | null;
};

export type CursorSessionSortKey =
  | "session"
  | "project"
  | "model"
  | "turns"
  | "errors"
  | "tools"
  | "files"
  | "time";

export type CursorSessionListRow = {
  session_id: string;
  project: string;
  turn_count: number;
  success_count: number;
  error_count: number;
  aborted_count: number;
  user_prompt_count: number;
  subagent_count: number;
  models: string[];
  sources: string[];
  tool_call_count: number;
  first_seen_at: string | null;
  last_seen_at: string | null;
  files_touched: number;
  source_file: string;
};

export type CursorSessionProjectRow = {
  name: string;
  session_count: number;
  turn_count: number;
  error_count: number;
  files_touched: number;
  last_seen_at: string | null;
};

export type CursorSessionDailyPoint = {
  bucket: string;
  session_count: number;
  turn_count: number;
};

export type CursorSessionModelRow = {
  name: string;
  session_count: number;
};

export type CursorSessionToolRow = {
  name: string;
  call_count: number;
};

export type CursorSessionSourceRow = {
  name: string;
  session_count: number;
};

export type CursorSessionExtensionRow = {
  name: string;
  file_count: number;
};

export type CursorSessionSummaryDto = {
  as_of: string | null;
  session_count: number;
  turn_count: number;
  aborted_count: number;
  user_prompt_count: number;
  subagent_count: number;
  error_rate: number | null;
  average_turns: number | null;
  average_tools_per_turn: number | null;
  write_read_ratio: number | null;
  active_project_count: number;
  by_project: CursorSessionProjectRow[];
  by_model: CursorSessionModelRow[];
  by_source: CursorSessionSourceRow[];
  by_extension: CursorSessionExtensionRow[];
  top_tools: CursorSessionToolRow[];
  tool_groups: CursorSessionToolRow[];
  daily: CursorSessionDailyPoint[];
};

export type CursorSessionQuery = {
  search?: string | null;
  project?: string | null;
  sortBy?: CursorSessionSortKey | null;
  sortDir?: SortDir | null;
  page?: number;
  pageSize?: number;
};

export type CursorSessionPage = {
  rows: CursorSessionListRow[];
  total: number;
};

export type CursorSessionHashFile = {
  path: string;
  extension: string;
  source: string;
};

export type CursorSessionDetailDto = {
  session: CursorSessionListRow;
  tools: CursorSessionToolRow[];
  hash_files: CursorSessionHashFile[];
  read_paths: string[];
  write_paths: string[];
  transcript_missing: boolean;
};

export type CursorAccountEventQuery = {
  page?: number | null;
  pageSize?: number | null;
  sortDir?: SortDir | null;
};

export type CursorAccountEventRow = {
  occurred_at: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  is_headless: boolean;
};

export type CursorAccountEventPage = {
  rows: CursorAccountEventRow[];
  total: number;
};

/** 工作时间线里的一根横条：一条会话按当天本地日历日裁剪后的区间。
 * total_tokens 只统计该会话落在这天的消耗记录；Cursor 本机会话无记录时为 0。 */
export type WorkSegment = {
  session_id: string;
  source: string;
  project: string;
  model: string;
  start: string;
  end: string;
  total_tokens: number;
};

export type WorkTimelineDto = {
  day: string;
  total_tokens: number;
  segment_count: number;
  turn_count: number;
  ai_exec_minutes: number;
  peak_parallel: number;
  parallel_intensity: number | null;
  segments: WorkSegment[];
};

export type FilterOptions = {
  sources: string[];
  models: string[];
  projects: string[];
  providers: string[];
};

export type IngestIssue = {
  source: string;
  path: string;
  message: string;
  event_type?: string | null;
  line?: number | null;
};

export type SourceIngestReport = {
  source: string;
  detected: boolean;
  files_seen: number;
  files_parsed: number;
  files_skipped: number;
  files_failed: number;
  records_written: number;
  records_removed: number;
  records_archived: number;
};

export type IngestReport = {
  files_seen: number;
  files_parsed: number;
  files_skipped: number;
  files_failed: number;
  records_written: number;
  records_removed: number;
  records_archived: number;
  partial_success: boolean;
  issues: IngestIssue[];
  conversation_issues: IngestIssue[];
  sources: SourceIngestReport[];
};

export type SourceDiagnostic = {
  source: string;
  application: string;
  detected: boolean;
  root_path: string;
  cached_files: number;
  record_count: number;
  total_tokens: number;
  coverage: string;
  archived_record_count: number;
};
