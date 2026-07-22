use sqlx::{FromRow, SqlitePool};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{task::JoinHandle, time::MissedTickBehavior};
use ulid::Ulid;
use unicode_segmentation::UnicodeSegmentation;

use crate::{config::MessageConfig, error::AppError, model::Role};

const SECONDS_PER_HOUR: u64 = 60 * 60;

#[derive(Clone, Debug, FromRow)]
pub struct PublicMessageRow {
    pub id: String,
    pub author_user_id: String,
    pub body: String,
    pub created_at: String,
    pub username: String,
    pub nickname: String,
    pub role: String,
}

pub fn validate_body(body: &str, max_length: usize) -> Result<String, AppError> {
    let body = body.trim().replace("\r\n", "\n").replace('\r', "\n");
    let length = body.graphemes(true).count();
    // 留言按用户看到的字符簇计数，所以 emoji 和组合字符不会被错误拆开。
    if length == 0 || length > max_length {
        return Err(AppError::BadRequest(format!(
            "留言内容须为 1～{max_length} 个字符"
        )));
    }
    if body
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(AppError::BadRequest(
            "留言内容不能包含特殊控制字符".to_owned(),
        ));
    }
    Ok(body)
}

pub async fn list_recent(
    pool: &SqlitePool,
    config: &MessageConfig,
) -> Result<Vec<PublicMessageRow>, AppError> {
    list_recent_limited(pool, config, config.page_size).await
}

pub async fn list_recent_limited(
    pool: &SqlitePool,
    config: &MessageConfig,
    limit: i64,
) -> Result<Vec<PublicMessageRow>, AppError> {
    let cutoff_epoch = cutoff_epoch(config)?;
    Ok(sqlx::query_as::<_, PublicMessageRow>(
        "SELECT public_messages.id, public_messages.author_user_id, public_messages.body, public_messages.created_at, users.username, users.nickname, users.role FROM public_messages JOIN users ON users.id = public_messages.author_user_id WHERE public_messages.deleted_at IS NULL AND public_messages.created_at_epoch >= ? ORDER BY public_messages.created_at_epoch DESC, public_messages.id DESC LIMIT ?",
    )
    .bind(cutoff_epoch)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn list_by_author(
    pool: &SqlitePool,
    author_user_id: &str,
    config: &MessageConfig,
) -> Result<Vec<PublicMessageRow>, AppError> {
    let cutoff_epoch = cutoff_epoch(config)?;
    Ok(sqlx::query_as::<_, PublicMessageRow>(
        "SELECT public_messages.id, public_messages.author_user_id, public_messages.body, public_messages.created_at, users.username, users.nickname, users.role FROM public_messages JOIN users ON users.id = public_messages.author_user_id WHERE public_messages.author_user_id = ? AND public_messages.deleted_at IS NULL AND public_messages.created_at_epoch >= ? ORDER BY public_messages.created_at_epoch DESC, public_messages.id DESC LIMIT ?",
    )
    .bind(author_user_id)
    .bind(cutoff_epoch)
    .bind(config.page_size)
    .fetch_all(pool)
    .await?)
}

pub async fn create(
    pool: &SqlitePool,
    author_user_id: &str,
    body: &str,
    config: &MessageConfig,
) -> Result<(), AppError> {
    let body = validate_body(body, config.max_length)?;
    let cutoff_epoch = cutoff_epoch(config)?;
    let current_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public_messages WHERE author_user_id = ? AND deleted_at IS NULL AND created_at_epoch >= ?",
    )
    .bind(author_user_id)
    .bind(cutoff_epoch)
    .fetch_one(pool)
    .await?;
    if current_count >= config.limit_per_user {
        return Err(AppError::BadRequest(format!(
            "{} 天内最多发布 {} 条留言",
            config.retention_days, config.limit_per_user
        )));
    }

    let now = OffsetDateTime::now_utc();
    let created_at = now
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("格式化留言时间失败：{error}")))?;
    sqlx::query(
        "INSERT INTO public_messages (id, author_user_id, body, created_at, created_at_epoch) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(Ulid::new().to_string())
    .bind(author_user_id)
    .bind(body)
    .bind(created_at)
    .bind(now.unix_timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_deleted(
    pool: &SqlitePool,
    message_id: &str,
    actor_user_id: &str,
    actor_role: Role,
) -> Result<(), AppError> {
    let author_user_id =
        sqlx::query_scalar::<_, String>("SELECT author_user_id FROM public_messages WHERE id = ?")
            .bind(message_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;
    if author_user_id != actor_user_id && !matches!(actor_role, Role::Admin | Role::SuperAdmin) {
        return Err(AppError::Forbidden);
    }
    let deleted_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("格式化删除时间失败：{error}")))?;
    sqlx::query("UPDATE public_messages SET deleted_at = ? WHERE id = ?")
        .bind(deleted_at)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn cleanup_expired(pool: &SqlitePool, config: &MessageConfig) -> Result<u64, AppError> {
    let cutoff_epoch = cutoff_epoch(config)?;
    let result = sqlx::query("DELETE FROM public_messages WHERE created_at_epoch < ?")
        .bind(cutoff_epoch)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub fn spawn_cleanup_task(pool: SqlitePool, config: MessageConfig) -> JoinHandle<()> {
    tokio::spawn(async move {
        let interval_seconds = config
            .cleanup_interval_hours
            .saturating_mul(SECONDS_PER_HOUR);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            match cleanup_expired(&pool, &config).await {
                Ok(removed) => {
                    if removed > 0 {
                        tracing::info!(removed, "已清理过期公共留言");
                    }
                }
                Err(error) => tracing::warn!(%error, "清理过期公共留言失败"),
            }
        }
    })
}

fn cutoff_epoch(config: &MessageConfig) -> Result<i64, AppError> {
    Ok((OffsetDateTime::now_utc() - Duration::days(config.retention_days)).unix_timestamp())
}
