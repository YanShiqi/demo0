CREATE TABLE public_messages (
    id CHAR(26) PRIMARY KEY,
    author_user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body VARCHAR(1200) NOT NULL,
    created_at VARCHAR(40) NOT NULL,
    created_at_epoch BIGINT NOT NULL,
    deleted_at VARCHAR(40)
);

CREATE INDEX public_messages_author_user_id_idx ON public_messages(author_user_id);
CREATE INDEX public_messages_created_at_epoch_idx ON public_messages(created_at_epoch);
CREATE INDEX public_messages_deleted_at_idx ON public_messages(deleted_at);
