CREATE TABLE users (
    id CHAR(26) PRIMARY KEY,
    username VARCHAR(32) NOT NULL,
    username_key VARCHAR(32) NOT NULL UNIQUE,
    nickname VARCHAR(32) NOT NULL,
    nickname_key VARCHAR(128) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(20) NOT NULL CHECK (role IN ('user', 'admin', 'super_admin')),
    avatar_storage_name VARCHAR(96),
    avatar_media_type VARCHAR(32),
    created_at VARCHAR(40) NOT NULL,
    updated_at VARCHAR(40) NOT NULL
);

CREATE TABLE web_sessions (
    token_hash CHAR(64) PRIMARY KEY,
    user_id CHAR(26) REFERENCES users(id) ON DELETE CASCADE,
    csrf_token VARCHAR(64) NOT NULL,
    expires_at BIGINT NOT NULL,
    created_at VARCHAR(40) NOT NULL
);

CREATE INDEX web_sessions_user_id_idx ON web_sessions(user_id);
CREATE INDEX web_sessions_expires_at_idx ON web_sessions(expires_at);

CREATE TABLE role_audit_logs (
    id CHAR(26) PRIMARY KEY,
    actor_user_id CHAR(26) NOT NULL REFERENCES users(id),
    target_user_id CHAR(26) NOT NULL REFERENCES users(id),
    old_role VARCHAR(20) NOT NULL,
    new_role VARCHAR(20) NOT NULL,
    created_at VARCHAR(40) NOT NULL
);
