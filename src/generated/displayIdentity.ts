// 由展示身份快照测试生成。不要手改。
// 重新生成：
// MABIAO_UPDATE_DISPLAY_IDENTITY=1 cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact

/** 来源 slug，顺序与 `Source::ALL` 一致。 */
export const SOURCE_SLUGS = [
  "codex",
  "claude",
  "pi",
  "omp",
  "opencode",
  "kimi",
  "dsh",
  "gemini",
  "grok",
  "qwen",
  "factory",
  "cursor_agent",
  "copilot",
  "hermes",
  "agy",
  "qoder",
  "qoder_cn",
  "cline",
  "workbuddy",
  "zcode",
  "alma",
] as const;

/** 官方额度提供方 slug，顺序与 `OfficialQuotaProvider::ALL` 一致。 */
export const OFFICIAL_QUOTA_PROVIDER_SLUGS = [
  "claude",
  "codex",
  "cursor",
  "grok",
  "droid",
  "antigravity",
  "opencode",
  "copilot",
  "devin",
] as const;

/** 品牌 ID，顺序与 `Brand::ALL` 一致。 */
export const BRAND_IDS = [
  "claude",
  "codex",
  "grok",
  "gemini",
  "kimi",
  "qwen",
  "copilot",
  "opencode",
  "factory",
  "pi",
  "omp",
  "dsh",
  "cursor",
  "hermes",
  "agy",
  "qoder",
  "cline",
  "workbuddy",
  "zcode",
  "alma",
] as const;

/** 按 slug 索引的显示名。重名 slug 以来源 `application_name` 为准。 */
export const DISPLAY_NAMES = {
  codex: "Codex",
  claude: "Claude Code",
  pi: "Pi",
  omp: "OMP",
  opencode: "OpenCode",
  kimi: "Kimi CLI",
  dsh: "DeepSeek Harness",
  gemini: "Gemini CLI",
  grok: "Grok CLI",
  qwen: "Qwen Code",
  factory: "Droid",
  cursor_agent: "Cursor Agent",
  copilot: "GitHub Copilot CLI",
  hermes: "Hermes",
  agy: "Antigravity",
  qoder: "Qoder",
  qoder_cn: "Qoder CN",
  cline: "Cline",
  workbuddy: "WorkBuddy",
  zcode: "ZCode",
  alma: "Alma",
  cursor: "Cursor",
  droid: "Droid",
  antigravity: "Antigravity",
  devin: "Devin",
} as const;

/** 按 slug 索引的品牌。重名 slug 以来源为准；没有品牌时为 null。 */
export const BRAND_BY_SLUG = {
  codex: "codex",
  claude: "claude",
  pi: "pi",
  omp: "omp",
  opencode: "opencode",
  kimi: "kimi",
  dsh: "dsh",
  gemini: "gemini",
  grok: "grok",
  qwen: "qwen",
  factory: "factory",
  cursor_agent: "cursor",
  copilot: "copilot",
  hermes: "hermes",
  agy: "agy",
  qoder: "qoder",
  qoder_cn: "qoder",
  cline: "cline",
  workbuddy: "workbuddy",
  zcode: "zcode",
  alma: "alma",
  cursor: "cursor",
  droid: "factory",
  antigravity: "agy",
  devin: null,
} as const;

export type UsageSource = (typeof SOURCE_SLUGS)[number];
export type BrandId = (typeof BRAND_IDS)[number];
