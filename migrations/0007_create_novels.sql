CREATE TABLE novels (
    id CHAR(26) PRIMARY KEY,
    title VARCHAR(60) NOT NULL,
    title_key VARCHAR(128) NOT NULL UNIQUE,
    created_at VARCHAR(40) NOT NULL,
    updated_at VARCHAR(40) NOT NULL,
    deleted_at VARCHAR(40)
);

CREATE TABLE novel_chapters (
    id CHAR(26) PRIMARY KEY,
    novel_id CHAR(26) NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    title VARCHAR(80) NOT NULL,
    chapter_number INTEGER NOT NULL,
    markdown TEXT NOT NULL,
    created_at VARCHAR(40) NOT NULL,
    updated_at VARCHAR(40) NOT NULL,
    deleted_at VARCHAR(40),
    UNIQUE (novel_id, chapter_number)
);

CREATE INDEX novels_deleted_at_updated_at_idx ON novels(deleted_at, updated_at);
CREATE INDEX novel_chapters_novel_id_number_idx ON novel_chapters(novel_id, chapter_number);
CREATE INDEX novel_chapters_deleted_at_created_at_idx ON novel_chapters(deleted_at, created_at);

