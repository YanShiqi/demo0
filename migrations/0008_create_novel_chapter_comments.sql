CREATE TABLE novel_chapter_comments (
    id CHAR(26) PRIMARY KEY,
    chapter_id CHAR(26) NOT NULL REFERENCES novel_chapters(id) ON DELETE CASCADE,
    author_user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at VARCHAR(40) NOT NULL,
    deleted_at VARCHAR(40)
);

CREATE INDEX novel_chapter_comments_chapter_created_idx ON novel_chapter_comments(chapter_id, created_at);
CREATE INDEX novel_chapter_comments_deleted_at_idx ON novel_chapter_comments(deleted_at);
