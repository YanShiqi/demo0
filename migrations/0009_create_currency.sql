ALTER TABLE users ADD COLUMN currency_balance BIGINT NOT NULL DEFAULT 0;

CREATE TABLE currency_logs (
    id CHAR(26) PRIMARY KEY NOT NULL,
    operation_id CHAR(26) NOT NULL,
    user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    amount_delta BIGINT NOT NULL,
    balance_after BIGINT NOT NULL,
    reason VARCHAR(64) NOT NULL,
    operator_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    related_id VARCHAR(160),
    idempotency_key VARCHAR(160) NOT NULL UNIQUE,
    note VARCHAR(200) NOT NULL DEFAULT '',
    created_at VARCHAR(40) NOT NULL,
    CHECK (amount_delta <> 0),
    CHECK (balance_after >= 0)
);

CREATE INDEX currency_logs_user_created_idx
    ON currency_logs(user_id, created_at);

CREATE INDEX currency_logs_operation_idx
    ON currency_logs(operation_id);
