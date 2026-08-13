CREATE TABLE weekly_check_ins (
    id CHAR(26) PRIMARY KEY NOT NULL,
    user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    week_start VARCHAR(10) NOT NULL,
    reward_amount BIGINT NOT NULL,
    created_at VARCHAR(40) NOT NULL,
    UNIQUE(user_id, week_start),
    CHECK (reward_amount > 0)
);

CREATE INDEX weekly_check_ins_user_week_idx
    ON weekly_check_ins(user_id, week_start);
