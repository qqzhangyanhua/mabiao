-- 团队价目与统一费用（ADR 0026「统一价目」）。
-- 管理员维护的 model（+ provider）单价，单位与桌面端价目一致：每 token 的价格。
CREATE TABLE team_prices (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    model          TEXT             NOT NULL,
    -- 空表示按 model 兜底；非空要 model 与 provider 都对上才算精确命中。
    provider       TEXT,
    input          DOUBLE PRECISION NOT NULL CHECK (input >= 0),
    output         DOUBLE PRECISION NOT NULL CHECK (output >= 0),
    cache_read     DOUBLE PRECISION NOT NULL CHECK (cache_read >= 0),
    cache_creation DOUBLE PRECISION NOT NULL CHECK (cache_creation >= 0),
    updated_by     BIGINT           REFERENCES remote_accounts (id),
    created_at     TIMESTAMPTZ      NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ      NOT NULL DEFAULT now()
);
-- 匹配与 `pricing` crate 一致：model、provider 都不分大小写。
CREATE UNIQUE INDEX team_prices_key ON team_prices (lower(model), lower(coalesce(provider, '')));

-- 统一费用：用团队价目按同一优先级重算的结果，与客户端的 `cost_snapshot` / `pricing_source`
-- 并存，不覆盖它们。三列全空表示「还没算过」（旧数据、迁移前入库），由启动回填或手动重算补上。
ALTER TABLE usage_records
    ADD COLUMN unified_cost DOUBLE PRECISION,
    ADD COLUMN unified_pricing_source TEXT
        CHECK (unified_pricing_source IN ('native', 'exact', 'fallback', 'unpriced')),
    -- 钱来自谁：来源自带 / 团队价目 / 内置 LiteLLM 快照 / 未定价。
    ADD COLUMN unified_cost_source TEXT
        CHECK (unified_cost_source IN ('native', 'team', 'snapshot', 'none'));
CREATE INDEX usage_records_uncomputed_idx ON usage_records (id) WHERE unified_pricing_source IS NULL;
CREATE INDEX usage_records_model_idx ON usage_records (lower(model));
