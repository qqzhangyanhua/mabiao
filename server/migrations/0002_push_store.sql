-- 推送入库（ADR 0026「推送什么」「身份、凭证、幂等」「项目归并」）。
-- 首版不自动删除任何数据；删除只有成员删自己的会话、管理员删任意会话两条手动路径。

-- 团队共享的项目实体。key 是归并键：git remote 归一后的 `git:host/owner/repo`，
-- 没有 remote 时按目录名兜底 `dir:<小写目录名>`。
CREATE TABLE projects (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key        TEXT        NOT NULL UNIQUE,
    name       TEXT        NOT NULL,
    -- 已去掉凭证与协议差异的 remote（`host/owner/repo`），没有则为空。原始 URL 不入库：它可能带 token。
    git_remote TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 保留各设备上的原始路径：同一仓库在不同机器上路径不同。
-- 同一路径后来带上 remote 重新推送时，会被改指到 git 项目。
CREATE TABLE project_paths (
    device_pk  BIGINT      NOT NULL REFERENCES devices (id),
    path       TEXT        NOT NULL,
    project_id BIGINT      NOT NULL REFERENCES projects (id),
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_pk, path)
);
CREATE INDEX project_paths_project_idx ON project_paths (project_id);

-- 一场会话一行，按（账号, 设备, 来源, session_id）整场覆盖。
CREATE TABLE sessions (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id    BIGINT      NOT NULL REFERENCES remote_accounts (id),
    device_pk     BIGINT      NOT NULL REFERENCES devices (id),
    source        TEXT        NOT NULL,
    session_id    TEXT        NOT NULL,
    title         TEXT        NOT NULL,
    project_path  TEXT        NOT NULL,
    project_id    BIGINT      REFERENCES projects (id),
    model         TEXT        NOT NULL,
    -- 客户端上报的是字符串；不是 RFC 3339 时存空，不拒收整场会话。
    started_at    TIMESTAMPTZ,
    ended_at      TIMESTAMPTZ,
    source_files  JSONB       NOT NULL,
    generated_by_work_notes BOOLEAN NOT NULL,
    redaction_count INTEGER   NOT NULL,
    event_count   INTEGER     NOT NULL,
    events        JSONB       NOT NULL,
    context_manifest JSONB,
    first_pushed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    pushed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, device_pk, source, session_id)
);
CREATE INDEX sessions_account_ended_idx ON sessions (account_id, ended_at);
CREATE INDEX sessions_project_idx ON sessions (project_id);
CREATE INDEX sessions_device_path_idx ON sessions (device_pk, project_path);

-- 消耗记录按（账号, 设备, 指纹）去重。费用快照与定价来源原样保存，不改写；
-- 团队价目的重算结果以后单独存，不覆盖这两列。
CREATE TABLE usage_records (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id    BIGINT      NOT NULL REFERENCES remote_accounts (id),
    device_pk     BIGINT      NOT NULL REFERENCES devices (id),
    fingerprint   TEXT        NOT NULL,
    occurred_at   TIMESTAMPTZ NOT NULL,
    source        TEXT        NOT NULL,
    model         TEXT        NOT NULL,
    provider      TEXT        NOT NULL,
    project_path  TEXT        NOT NULL,
    project_id    BIGINT      REFERENCES projects (id),
    session_id    TEXT        NOT NULL,
    source_file   TEXT        NOT NULL,
    input_tokens          BIGINT NOT NULL,
    output_tokens         BIGINT NOT NULL,
    cache_read_tokens     BIGINT NOT NULL,
    cache_creation_tokens BIGINT NOT NULL,
    reasoning_tokens      BIGINT NOT NULL,
    total_tokens          BIGINT NOT NULL,
    native_cost   DOUBLE PRECISION,
    cost_snapshot DOUBLE PRECISION,
    pricing_source TEXT       NOT NULL CHECK (pricing_source IN ('native', 'exact', 'fallback', 'unpriced')),
    pushed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, device_pk, fingerprint)
);
CREATE INDEX usage_records_account_time_idx ON usage_records (account_id, occurred_at);
CREATE INDEX usage_records_project_idx ON usage_records (project_id);
CREATE INDEX usage_records_device_path_idx ON usage_records (device_pk, project_path);
