use std::io::Cursor;

use image::{
    AnimationDecoder, GenericImageView, ImageDecoder, ImageFormat,
    codecs::gif::{GifDecoder, GifEncoder, Repeat},
};
use sqlx::{FromRow, SqlitePool, sqlite::SqliteQueryResult};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    config::MemeConfig,
    error::AppError,
    model::{Role, User},
};

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_DELETED: &str = "deleted";

#[derive(Clone, Debug, FromRow)]
pub struct MemeRow {
    pub id: String,
    pub author_user_id: String,
    pub storage_name: String,
    pub media_type: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub created_at_epoch: i64,
    pub reviewed_at: Option<String>,
    pub reviewed_by: Option<String>,
    pub username: String,
    pub nickname: String,
}

#[derive(Clone, Debug)]
pub struct MemeWithTags {
    pub row: MemeRow,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MemePage {
    pub items: Vec<MemeWithTags>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProcessedMeme {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
}

#[derive(Clone, Debug)]
pub struct NewMeme {
    pub storage_name: String,
    pub media_type: String,
    pub title: String,
    pub tags: Vec<String>,
}

pub fn process_image(bytes: Vec<u8>, config: &MemeConfig) -> Result<ProcessedMeme, AppError> {
    if bytes.is_empty() || bytes.len() > config.max_upload_bytes {
        return Err(AppError::BadRequest(format!(
            "Meme 文件必须小于 {} KiB",
            config.max_upload_bytes / 1024
        )));
    }

    let format = image::guess_format(&bytes)
        .map_err(|_| AppError::BadRequest("无法识别 Meme 文件".to_owned()))?;
    let processed = match format {
        ImageFormat::Png => process_static(bytes, ImageFormat::Png, "png", "image/png", config),
        ImageFormat::Jpeg => process_static(bytes, ImageFormat::Jpeg, "jpg", "image/jpeg", config),
        ImageFormat::Gif => process_gif(bytes, config),
        _ => Err(AppError::BadRequest(
            "Meme 仅支持 PNG、JPEG 和 GIF".to_owned(),
        )),
    }?;
    if processed.bytes.len() > config.max_upload_bytes {
        return Err(AppError::BadRequest(
            "Meme 处理后体积过大，请降低图片复杂度或尺寸".to_owned(),
        ));
    }
    Ok(processed)
}

pub fn validate_title(title: &str, config: &MemeConfig) -> Result<String, AppError> {
    let title = title.trim();
    let length = title.graphemes(true).count();
    if length == 0 || length > config.max_title_length {
        return Err(AppError::BadRequest(format!(
            "标题须为 1～{} 个字符",
            config.max_title_length
        )));
    }
    if title.chars().any(char::is_control) {
        return Err(AppError::BadRequest("标题不能包含控制字符".to_owned()));
    }
    Ok(title.to_owned())
}

pub fn normalize_tags(raw_tags: &str, config: &MemeConfig) -> Result<Vec<String>, AppError> {
    let mut tags = Vec::new();
    let mut keys = Vec::new();
    for raw_tag in raw_tags.split([',', '，']) {
        let tag = raw_tag.trim();
        if tag.is_empty() {
            continue;
        }
        let length = tag.graphemes(true).count();
        if length > config.max_tag_length {
            return Err(AppError::BadRequest(format!(
                "单个标签不能超过 {} 个字符",
                config.max_tag_length
            )));
        }
        if tag.chars().any(char::is_control) {
            return Err(AppError::BadRequest("标签不能包含控制字符".to_owned()));
        }
        let key = tag_key(tag);
        if keys.iter().any(|existing| existing == &key) {
            continue;
        }
        tags.push(tag.to_owned());
        keys.push(key);
        // 标签数在清洗去重后再限制，避免重复标签占用名额。
        if tags.len() > config.max_tags_per_meme {
            return Err(AppError::BadRequest(format!(
                "每个 Meme 最多 {} 个标签",
                config.max_tags_per_meme
            )));
        }
    }
    Ok(tags)
}

pub async fn create(
    pool: &SqlitePool,
    author: &User,
    new_meme: NewMeme,
) -> Result<String, AppError> {
    let now = OffsetDateTime::now_utc();
    let created_at = now
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("格式化 Meme 创建时间失败：{error}")))?;
    let meme_id = Ulid::new().to_string();
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO memes (id, author_user_id, storage_name, media_type, title, status, created_at, created_at_epoch) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&meme_id)
    .bind(&author.id)
    .bind(&new_meme.storage_name)
    .bind(&new_meme.media_type)
    .bind(&new_meme.title)
    .bind(STATUS_PENDING)
    .bind(created_at)
    .bind(now.unix_timestamp())
    .execute(&mut *transaction)
    .await?;

    for tag in new_meme.tags {
        let tag_id = find_or_create_tag(&mut transaction, &tag).await?;
        sqlx::query("INSERT INTO meme_tag_links (meme_id, tag_id) VALUES (?, ?)")
            .bind(&meme_id)
            .bind(tag_id)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;
    Ok(meme_id)
}

pub async fn list_approved(
    pool: &SqlitePool,
    tag: Option<&str>,
    cursor: Option<&str>,
    config: &MemeConfig,
) -> Result<MemePage, AppError> {
    let query_limit = config.page_size + 1;
    let cursor = cursor.map(parse_cursor).transpose()?;
    let tag_key = tag
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(tag_key);

    let rows = match (tag_key.as_deref(), cursor) {
        (Some(tag_key), Some((cursor_epoch, cursor_id))) => sqlx::query_as::<_, MemeRow>(
            "SELECT memes.id, memes.author_user_id, memes.storage_name, memes.media_type, memes.title, memes.status, memes.created_at, memes.created_at_epoch, memes.reviewed_at, memes.reviewed_by, users.username, users.nickname FROM memes JOIN users ON users.id = memes.author_user_id JOIN meme_tag_links ON meme_tag_links.meme_id = memes.id JOIN meme_tags ON meme_tags.id = meme_tag_links.tag_id WHERE memes.status = ? AND meme_tags.name_key = ? AND (memes.created_at_epoch < ? OR (memes.created_at_epoch = ? AND memes.id < ?)) ORDER BY memes.created_at_epoch DESC, memes.id DESC LIMIT ?",
        )
        .bind(STATUS_APPROVED)
        .bind(tag_key)
        .bind(cursor_epoch)
        .bind(cursor_epoch)
        .bind(cursor_id)
        .bind(query_limit)
        .fetch_all(pool)
        .await?,
        (Some(tag_key), None) => sqlx::query_as::<_, MemeRow>(
            "SELECT memes.id, memes.author_user_id, memes.storage_name, memes.media_type, memes.title, memes.status, memes.created_at, memes.created_at_epoch, memes.reviewed_at, memes.reviewed_by, users.username, users.nickname FROM memes JOIN users ON users.id = memes.author_user_id JOIN meme_tag_links ON meme_tag_links.meme_id = memes.id JOIN meme_tags ON meme_tags.id = meme_tag_links.tag_id WHERE memes.status = ? AND meme_tags.name_key = ? ORDER BY memes.created_at_epoch DESC, memes.id DESC LIMIT ?",
        )
        .bind(STATUS_APPROVED)
        .bind(tag_key)
        .bind(query_limit)
        .fetch_all(pool)
        .await?,
        (None, Some((cursor_epoch, cursor_id))) => sqlx::query_as::<_, MemeRow>(
            "SELECT memes.id, memes.author_user_id, memes.storage_name, memes.media_type, memes.title, memes.status, memes.created_at, memes.created_at_epoch, memes.reviewed_at, memes.reviewed_by, users.username, users.nickname FROM memes JOIN users ON users.id = memes.author_user_id WHERE memes.status = ? AND (memes.created_at_epoch < ? OR (memes.created_at_epoch = ? AND memes.id < ?)) ORDER BY memes.created_at_epoch DESC, memes.id DESC LIMIT ?",
        )
        .bind(STATUS_APPROVED)
        .bind(cursor_epoch)
        .bind(cursor_epoch)
        .bind(cursor_id)
        .bind(query_limit)
        .fetch_all(pool)
        .await?,
        (None, None) => sqlx::query_as::<_, MemeRow>(
            "SELECT memes.id, memes.author_user_id, memes.storage_name, memes.media_type, memes.title, memes.status, memes.created_at, memes.created_at_epoch, memes.reviewed_at, memes.reviewed_by, users.username, users.nickname FROM memes JOIN users ON users.id = memes.author_user_id WHERE memes.status = ? ORDER BY memes.created_at_epoch DESC, memes.id DESC LIMIT ?",
        )
        .bind(STATUS_APPROVED)
        .bind(query_limit)
        .fetch_all(pool)
        .await?,
    };
    page_from_rows(pool, rows, config.page_size as usize).await
}

pub async fn list_for_admin(pool: &SqlitePool) -> Result<Vec<MemeWithTags>, AppError> {
    let rows = sqlx::query_as::<_, MemeRow>(
        "SELECT memes.id, memes.author_user_id, memes.storage_name, memes.media_type, memes.title, memes.status, memes.created_at, memes.created_at_epoch, memes.reviewed_at, memes.reviewed_by, users.username, users.nickname FROM memes JOIN users ON users.id = memes.author_user_id WHERE memes.status = ? ORDER BY memes.created_at_epoch DESC, memes.id DESC",
    )
    .bind(STATUS_PENDING)
    .fetch_all(pool)
    .await?;
    attach_tags(pool, rows).await
}

pub async fn approve(
    pool: &SqlitePool,
    meme_id: &str,
    reviewer: &User,
) -> Result<SqliteQueryResult, AppError> {
    ensure_admin(reviewer)?;
    let reviewed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("格式化 Meme 审核时间失败：{error}")))?;
    let result =
        sqlx::query("UPDATE memes SET status = ?, reviewed_at = ?, reviewed_by = ? WHERE id = ?")
            .bind(STATUS_APPROVED)
            .bind(reviewed_at)
            .bind(&reviewer.id)
            .bind(meme_id)
            .execute(pool)
            .await?;
    Ok(result)
}

pub async fn mark_deleted(
    pool: &SqlitePool,
    meme_id: &str,
    reviewer: &User,
) -> Result<SqliteQueryResult, AppError> {
    ensure_admin(reviewer)?;
    let reviewed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("格式化 Meme 删除时间失败：{error}")))?;
    let result =
        sqlx::query("UPDATE memes SET status = ?, reviewed_at = ?, reviewed_by = ? WHERE id = ?")
            .bind(STATUS_DELETED)
            .bind(reviewed_at)
            .bind(&reviewer.id)
            .bind(meme_id)
            .execute(pool)
            .await?;
    Ok(result)
}

pub async fn image_info(
    pool: &SqlitePool,
    meme_id: &str,
    can_view_pending: bool,
) -> Result<(String, String), AppError> {
    let (storage_name, media_type, status) = sqlx::query_as::<_, (String, String, String)>(
        "SELECT storage_name, media_type, status FROM memes WHERE id = ?",
    )
    .bind(meme_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if status == STATUS_APPROVED || (can_view_pending && status != STATUS_DELETED) {
        Ok((storage_name, media_type))
    } else {
        Err(AppError::NotFound)
    }
}

fn process_static(
    bytes: Vec<u8>,
    format: ImageFormat,
    extension: &'static str,
    media_type: &'static str,
    config: &MemeConfig,
) -> Result<ProcessedMeme, AppError> {
    let image = image::load_from_memory_with_format(&bytes, format)
        .map_err(|_| AppError::BadRequest("Meme 图片已损坏或格式不正确".to_owned()))?;
    let (width, height) = image.dimensions();
    validate_dimensions(width, height, 1, config)?;

    // 静态图重新编码后再公开，避免用户原图中的额外元数据直接暴露。
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, format)
        .map_err(|error| AppError::Internal(format!("Meme 重新编码失败：{error}")))?;
    Ok(ProcessedMeme {
        bytes: output.into_inner(),
        extension,
        media_type,
        width,
        height,
        frame_count: 1,
    })
}

fn process_gif(bytes: Vec<u8>, config: &MemeConfig) -> Result<ProcessedMeme, AppError> {
    let decoder = GifDecoder::new(Cursor::new(bytes))
        .map_err(|_| AppError::BadRequest("GIF 已损坏或格式不正确".to_owned()))?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height, 1, config)?;

    let mut frames = Vec::new();
    for decoded in decoder.into_frames() {
        if frames.len() >= config.max_gif_frames {
            return Err(AppError::BadRequest(format!(
                "GIF 不能超过 {} 帧",
                config.max_gif_frames
            )));
        }
        let frame = decoded.map_err(|_| AppError::BadRequest("GIF 包含无法解码的帧".to_owned()))?;
        frames.push(frame);
        validate_dimensions(width, height, frames.len(), config)?;
    }
    if frames.is_empty() {
        return Err(AppError::BadRequest("GIF 不包含有效画面".to_owned()));
    }

    // GIF 第一版保留动画，只做重新编码和限制校验，不做裁剪压缩参数调节。
    let frame_count = frames.len();
    let mut output = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut output);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|error| AppError::Internal(format!("GIF 循环设置失败：{error}")))?;
        encoder
            .encode_frames(frames)
            .map_err(|error| AppError::Internal(format!("GIF 重新编码失败：{error}")))?;
    }

    Ok(ProcessedMeme {
        bytes: output,
        extension: "gif",
        media_type: "image/gif",
        width,
        height,
        frame_count,
    })
}

fn validate_dimensions(
    width: u32,
    height: u32,
    frames: usize,
    config: &MemeConfig,
) -> Result<(), AppError> {
    if width == 0 || height == 0 || width > config.max_dimension || height > config.max_dimension {
        return Err(AppError::BadRequest(format!(
            "Meme 尺寸不能超过 {}×{}",
            config.max_dimension, config.max_dimension
        )));
    }
    let max_decoded_pixels = u64::from(config.max_dimension)
        .saturating_mul(u64::from(config.max_dimension))
        .saturating_mul(config.max_gif_frames as u64);
    let decoded_pixels = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(frames as u64);
    if decoded_pixels > max_decoded_pixels {
        return Err(AppError::BadRequest(
            "GIF 解码后的总像素过大，请减少尺寸或帧数".to_owned(),
        ));
    }
    Ok(())
}

async fn find_or_create_tag(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    tag: &str,
) -> Result<String, AppError> {
    let key = tag_key(tag);
    if let Some(id) = sqlx::query_scalar::<_, String>("SELECT id FROM meme_tags WHERE name_key = ?")
        .bind(&key)
        .fetch_optional(&mut **transaction)
        .await?
    {
        return Ok(id);
    }

    let id = Ulid::new().to_string();
    let insert_result = sqlx::query("INSERT INTO meme_tags (id, name, name_key) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(tag)
        .bind(&key)
        .execute(&mut **transaction)
        .await;
    match insert_result {
        Ok(_) => Ok(id),
        Err(error) if is_unique_violation(&error) => {
            sqlx::query_scalar::<_, String>("SELECT id FROM meme_tags WHERE name_key = ?")
                .bind(key)
                .fetch_one(&mut **transaction)
                .await
                .map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
}

async fn page_from_rows(
    pool: &SqlitePool,
    mut rows: Vec<MemeRow>,
    page_size: usize,
) -> Result<MemePage, AppError> {
    let next_cursor = if rows.len() > page_size {
        let extra = rows.pop().expect("多取一条时应存在 cursor 来源");
        Some(format!("{}:{}", extra.created_at_epoch, extra.id))
    } else {
        None
    };
    let items = attach_tags(pool, rows).await?;
    Ok(MemePage { items, next_cursor })
}

async fn attach_tags(pool: &SqlitePool, rows: Vec<MemeRow>) -> Result<Vec<MemeWithTags>, AppError> {
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let tags = sqlx::query_scalar::<_, String>(
            "SELECT meme_tags.name FROM meme_tags JOIN meme_tag_links ON meme_tag_links.tag_id = meme_tags.id WHERE meme_tag_links.meme_id = ? ORDER BY meme_tags.name ASC",
        )
        .bind(&row.id)
        .fetch_all(pool)
        .await?;
        items.push(MemeWithTags { row, tags });
    }
    Ok(items)
}

fn parse_cursor(cursor: &str) -> Result<(i64, String), AppError> {
    let (epoch, id) = cursor
        .split_once(':')
        .ok_or_else(|| AppError::BadRequest("分页参数无效".to_owned()))?;
    let epoch = epoch
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("分页参数无效".to_owned()))?;
    if id.is_empty() {
        return Err(AppError::BadRequest("分页参数无效".to_owned()));
    }
    Ok((epoch, id.to_owned()))
}

fn tag_key(tag: &str) -> String {
    tag.nfkc().collect::<String>().to_lowercase()
}

fn ensure_admin(user: &User) -> Result<(), AppError> {
    if matches!(user.parsed_role(), Role::Admin | Role::SuperAdmin) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}
