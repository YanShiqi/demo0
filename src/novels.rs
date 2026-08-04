use sqlx::{FromRow, SqlitePool};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::{config::NovelConfig, error::AppError};

#[derive(Clone, Debug, FromRow)]
pub struct NovelRow {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct NovelChapterRow {
    pub id: String,
    pub novel_id: String,
    pub title: String,
    pub chapter_number: i64,
    pub markdown: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct NovelChapterPreviewRow {
    pub novel_id: String,
    pub novel_title: String,
    pub chapter_id: String,
    pub chapter_title: String,
    pub chapter_number: i64,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct NovelWithChapters {
    pub novel: NovelRow,
    pub chapters: Vec<NovelChapterRow>,
}

pub fn validate_novel_title(
    title: &str,
    config: &NovelConfig,
) -> Result<(String, String), AppError> {
    validate_title(title, config.max_title_length, "小说标题")
}

pub fn validate_chapter_title(title: &str, config: &NovelConfig) -> Result<String, AppError> {
    validate_title(title, config.max_chapter_title_length, "章节标题").map(|(title, _)| title)
}

pub fn validate_chapter_upload(
    file_name: Option<&str>,
    bytes: &[u8],
    config: &NovelConfig,
) -> Result<String, AppError> {
    if bytes.is_empty() || bytes.len() > config.chapter_max_upload_bytes {
        return Err(AppError::BadRequest(format!(
            "章节 Markdown 文件必须小于 {} KiB",
            config.chapter_max_upload_bytes / 1024
        )));
    }
    let file_name = file_name.unwrap_or_default().to_lowercase();
    if !file_name.ends_with(".md") {
        return Err(AppError::BadRequest("章节文件必须使用 .md 后缀".to_owned()));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| AppError::BadRequest("章节 Markdown 必须是 UTF-8 文本".to_owned()))
}

pub async fn create_novel(
    pool: &SqlitePool,
    title: &str,
    config: &NovelConfig,
) -> Result<String, AppError> {
    let (title, title_key) = validate_novel_title(title, config)?;
    let now = now_string()?;
    let id = Ulid::new().to_string();
    let result = sqlx::query(
        "INSERT INTO novels (id, title, title_key, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&title)
    .bind(&title_key)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await;

    if let Err(error) = result {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            return Err(AppError::BadRequest("小说名称已存在".to_owned()));
        }
        return Err(error.into());
    }
    Ok(id)
}

pub async fn create_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    title: &str,
    markdown: &str,
    config: &NovelConfig,
) -> Result<String, AppError> {
    let title = validate_chapter_title(title, config)?;
    let now = now_string()?;
    let id = Ulid::new().to_string();
    let mut transaction = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM novels WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_one(&mut *transaction)
    .await?;
    if exists == 0 {
        return Err(AppError::NotFound);
    }
    // 章节号按上传顺序递增，包含已软删除章节，避免删除后复用编号造成阅读链接含义变化。
    let current_max = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(chapter_number) FROM novel_chapters WHERE novel_id = ?",
    )
    .bind(novel_id)
    .fetch_one(&mut *transaction)
    .await?
    .unwrap_or_default();
    let chapter_number = current_max + 1;
    sqlx::query(
        "INSERT INTO novel_chapters (id, novel_id, title, chapter_number, markdown, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(novel_id)
    .bind(&title)
    .bind(chapter_number)
    .bind(markdown)
    .bind(&now)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE novels SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(novel_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(id)
}

pub async fn list_recent_chapters(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<NovelChapterPreviewRow>, AppError> {
    Ok(sqlx::query_as::<_, NovelChapterPreviewRow>(
        "SELECT novels.id AS novel_id, novels.title AS novel_title, novel_chapters.id AS chapter_id, novel_chapters.title AS chapter_title, novel_chapters.chapter_number, novel_chapters.updated_at FROM novel_chapters JOIN novels ON novels.id = novel_chapters.novel_id WHERE novels.deleted_at IS NULL AND novel_chapters.deleted_at IS NULL ORDER BY novel_chapters.updated_at DESC, novel_chapters.id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn list_novels(pool: &SqlitePool) -> Result<Vec<NovelRow>, AppError> {
    Ok(sqlx::query_as::<_, NovelRow>(
        "SELECT id, title, created_at, updated_at FROM novels WHERE deleted_at IS NULL ORDER BY updated_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_novel(pool: &SqlitePool, novel_id: &str) -> Result<NovelRow, AppError> {
    sqlx::query_as::<_, NovelRow>(
        "SELECT id, title, created_at, updated_at FROM novels WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn list_chapters(
    pool: &SqlitePool,
    novel_id: &str,
) -> Result<Vec<NovelChapterRow>, AppError> {
    Ok(sqlx::query_as::<_, NovelChapterRow>(
        "SELECT id, novel_id, title, chapter_number, markdown, created_at, updated_at FROM novel_chapters WHERE novel_id = ? AND deleted_at IS NULL ORDER BY chapter_number ASC, id ASC",
    )
    .bind(novel_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    chapter_id: &str,
) -> Result<NovelChapterRow, AppError> {
    let _ = get_novel(pool, novel_id).await?;
    sqlx::query_as::<_, NovelChapterRow>(
        "SELECT id, novel_id, title, chapter_number, markdown, created_at, updated_at FROM novel_chapters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(chapter_id)
    .bind(novel_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn list_novels_with_chapters(
    pool: &SqlitePool,
) -> Result<Vec<NovelWithChapters>, AppError> {
    let novels = list_novels(pool).await?;
    let mut result = Vec::with_capacity(novels.len());
    for novel in novels {
        let chapters = list_chapters(pool, &novel.id).await?;
        result.push(NovelWithChapters { novel, chapters });
    }
    Ok(result)
}

pub async fn soft_delete_novel(pool: &SqlitePool, novel_id: &str) -> Result<(), AppError> {
    let now = now_string()?;
    let result = sqlx::query(
        "UPDATE novels SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(novel_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn soft_delete_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    chapter_id: &str,
) -> Result<(), AppError> {
    let now = now_string()?;
    let result = sqlx::query("UPDATE novel_chapters SET deleted_at = ?, updated_at = ? WHERE id = ? AND novel_id = ? AND deleted_at IS NULL")
        .bind(&now)
        .bind(&now)
        .bind(chapter_id)
        .bind(novel_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("UPDATE novels SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(novel_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn render_markdown(markdown: &str) -> String {
    let mut output = String::new();
    let mut in_script_block = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if in_script_block {
            if lower.contains("</script>") {
                in_script_block = false;
            }
            continue;
        }
        if lower.starts_with("<script") {
            if !lower.contains("</script>") {
                in_script_block = true;
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("# ") {
            output.push_str("<h1>");
            output.push_str(&render_inline_markdown(title));
            output.push_str("</h1>\n");
        } else if let Some(title) = trimmed.strip_prefix("## ") {
            output.push_str("<h2>");
            output.push_str(&render_inline_markdown(title));
            output.push_str("</h2>\n");
        } else {
            output.push_str("<p>");
            output.push_str(&render_inline_markdown(trimmed));
            output.push_str("</p>\n");
        }
    }
    output
}

fn validate_title(
    title: &str,
    max_length: usize,
    label: &str,
) -> Result<(String, String), AppError> {
    let title = title.trim().nfkc().collect::<String>();
    let length = title.graphemes(true).count();
    if length == 0 || length > max_length {
        return Err(AppError::BadRequest(format!(
            "{label}须为 1～{max_length} 个字符"
        )));
    }
    if title.chars().any(char::is_control) {
        return Err(AppError::BadRequest(format!("{label}不能包含控制字符")));
    }
    let key = title.to_lowercase().nfkc().collect();
    Ok((title, key))
}

fn render_inline_markdown(input: &str) -> String {
    let mut output = String::new();
    let mut strong_open = false;
    for part in input.split("**") {
        if strong_open {
            output.push_str("<strong>");
            output.push_str(&escape_html(part));
            output.push_str("</strong>");
        } else {
            output.push_str(&escape_html(part));
        }
        strong_open = !strong_open;
    }
    output
}

fn escape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            _ => output.push(character),
        }
    }
    output
}

fn now_string() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("格式化小说时间失败：{error}")))
}
