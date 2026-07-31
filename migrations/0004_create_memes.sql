CREATE TABLE memes (
    id CHAR(26) PRIMARY KEY,
    author_user_id CHAR(26) NOT NULL REFERENCES users(id),
    storage_name VARCHAR(96) NOT NULL UNIQUE,
    media_type VARCHAR(32) NOT NULL,
    title VARCHAR(120) NOT NULL,
    status VARCHAR(20) NOT NULL CHECK (status IN ('pending', 'approved', 'deleted')),
    created_at VARCHAR(40) NOT NULL,
    created_at_epoch BIGINT NOT NULL,
    reviewed_at VARCHAR(40),
    reviewed_by CHAR(26) REFERENCES users(id)
);

CREATE INDEX memes_status_created_idx ON memes(status, created_at_epoch, id);
CREATE INDEX memes_author_user_id_idx ON memes(author_user_id);

CREATE TABLE meme_tags (
    id CHAR(26) PRIMARY KEY,
    name VARCHAR(80) NOT NULL,
    name_key VARCHAR(160) NOT NULL UNIQUE
);

CREATE TABLE meme_tag_links (
    meme_id CHAR(26) NOT NULL REFERENCES memes(id) ON DELETE CASCADE,
    tag_id CHAR(26) NOT NULL REFERENCES meme_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (meme_id, tag_id)
);

CREATE INDEX meme_tag_links_tag_id_idx ON meme_tag_links(tag_id);
