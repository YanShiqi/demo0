CREATE TABLE shop_orders (
    id CHAR(26) PRIMARY KEY NOT NULL,
    user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    product_id VARCHAR(64) NOT NULL,
    product_name VARCHAR(80) NOT NULL,
    product_description VARCHAR(500) NOT NULL,
    icon_file VARCHAR(128) NOT NULL,
    fulfillment_type VARCHAR(32) NOT NULL,
    price_paid BIGINT NOT NULL CHECK (price_paid > 0),
    valid_days BIGINT CHECK (valid_days IS NULL OR valid_days > 0),
    purchase_key CHAR(26) NOT NULL UNIQUE,
    created_at VARCHAR(40) NOT NULL
);

CREATE TABLE redemption_vouchers (
    id CHAR(26) PRIMARY KEY NOT NULL,
    order_id CHAR(26) NOT NULL UNIQUE REFERENCES shop_orders(id) ON DELETE RESTRICT,
    token_hash CHAR(64) NOT NULL UNIQUE,
    token_mask VARCHAR(80) NOT NULL,
    status VARCHAR(20) NOT NULL CHECK (status IN ('active', 'redeemed', 'cancelled')),
    expires_at VARCHAR(40),
    redeemed_at VARCHAR(40),
    redeemed_by_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    redemption_note VARCHAR(200) NOT NULL DEFAULT '',
    cancelled_at VARCHAR(40),
    cancelled_by_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    cancellation_reason VARCHAR(200) NOT NULL DEFAULT '',
    created_at VARCHAR(40) NOT NULL
);

CREATE TABLE voucher_audit_logs (
    id CHAR(26) PRIMARY KEY NOT NULL,
    voucher_id CHAR(26) NOT NULL REFERENCES redemption_vouchers(id) ON DELETE RESTRICT,
    event_type VARCHAR(20) NOT NULL CHECK (event_type IN ('created', 'redeemed', 'cancelled')),
    actor_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    note VARCHAR(200) NOT NULL DEFAULT '',
    created_at VARCHAR(40) NOT NULL
);

CREATE INDEX shop_orders_user_created_idx ON shop_orders(user_id, created_at);
CREATE INDEX redemption_vouchers_status_expiry_idx ON redemption_vouchers(status, expires_at);
CREATE INDEX voucher_audit_voucher_created_idx ON voucher_audit_logs(voucher_id, created_at);
