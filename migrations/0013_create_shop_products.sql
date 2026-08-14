CREATE TABLE shop_products (
    id VARCHAR(64) PRIMARY KEY NOT NULL,
    name VARCHAR(80) NOT NULL,
    description VARCHAR(500) NOT NULL,
    icon_storage_name VARCHAR(128) NOT NULL,
    icon_media_type VARCHAR(32) NOT NULL,
    price BIGINT NOT NULL CHECK (price > 0),
    valid_days BIGINT CHECK (valid_days IS NULL OR valid_days > 0),
    max_active_per_user BIGINT NOT NULL CHECK (max_active_per_user > 0),
    total_limit BIGINT CHECK (total_limit IS NULL OR total_limit > 0),
    sold_count BIGINT NOT NULL DEFAULT 0 CHECK (sold_count >= 0),
    enabled BOOLEAN NOT NULL,
    sort_order BIGINT NOT NULL,
    created_by_user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    updated_by_user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at VARCHAR(40) NOT NULL,
    updated_at VARCHAR(40) NOT NULL,
    CHECK (total_limit IS NULL OR total_limit >= sold_count)
);

CREATE TABLE shop_product_audit_logs (
    id CHAR(26) PRIMARY KEY NOT NULL,
    product_id VARCHAR(64) NOT NULL,
    action VARCHAR(20) NOT NULL CHECK (action IN ('created', 'updated', 'enabled', 'disabled', 'deleted')),
    actor_user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    before_snapshot TEXT NOT NULL DEFAULT '',
    after_snapshot TEXT NOT NULL DEFAULT '',
    created_at VARCHAR(40) NOT NULL
);

CREATE INDEX shop_products_enabled_sort_idx ON shop_products(enabled, sort_order, id);
CREATE INDEX shop_product_audit_product_created_idx ON shop_product_audit_logs(product_id, created_at, id);
