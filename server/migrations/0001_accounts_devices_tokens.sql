-- 远程账号、设备、登录 token（ADR 0026）。
-- 一个部署对应一个团队；账号只停用不删除，推送来的数据归属它。

CREATE TABLE remote_accounts (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account        TEXT        NOT NULL,
    password_hash  TEXT        NOT NULL,
    role           TEXT        NOT NULL CHECK (role IN ('admin', 'member')),
    active         BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    deactivated_at TIMESTAMPTZ,
    CHECK (active = (deactivated_at IS NULL))
);

-- 账号名不分大小写唯一，登录也按小写查。
CREATE UNIQUE INDEX remote_accounts_account_lower_idx ON remote_accounts (lower(account));

-- 每台桌面端首次登录生成设备 ID，随推送带上；同一账号下按设备 ID 唯一。
CREATE TABLE devices (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id    BIGINT      NOT NULL REFERENCES remote_accounts (id),
    device_id     TEXT        NOT NULL,
    device_name   TEXT        NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, device_id)
);

-- 只存 token 的 SHA-256，库泄露时拿不到可用的 token。
CREATE TABLE login_tokens (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id BIGINT      NOT NULL REFERENCES remote_accounts (id),
    token_hash BYTEA       NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX login_tokens_account_idx ON login_tokens (account_id);
CREATE INDEX login_tokens_expires_idx ON login_tokens (expires_at);
