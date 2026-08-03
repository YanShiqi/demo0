ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE password_reset_audit_logs (
    id CHAR(26) PRIMARY KEY,
    actor_user_id CHAR(26) NOT NULL REFERENCES users(id),
    target_user_id CHAR(26) NOT NULL REFERENCES users(id),
    created_at VARCHAR(40) NOT NULL
);

