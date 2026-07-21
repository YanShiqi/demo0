use std::sync::Arc;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::http::{HeaderMap, HeaderValue, header};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cookie::{Cookie, SameSite};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use subtle::ConstantTimeEq;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    config::Config,
    error::AppError,
    model::{Role, SessionContext, SessionRow, User},
};

const SESSION_COOKIE: &str = "demo0_session";
const SESSION_DAYS: i64 = 7;
const NICKNAME_LENGTH: std::ops::RangeInclusive<usize> = 1..=32;

#[derive(Debug)]
pub struct ValidatedRegistration {
    pub username: String,
    pub username_key: String,
    pub nickname: String,
    pub nickname_key: String,
}

pub fn validate_registration(
    username: &str,
    nickname: &str,
    password: &str,
) -> Result<ValidatedRegistration, AppError> {
    let username = username.trim().to_lowercase();
    if !(3..=32).contains(&username.chars().count())
        || !username.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
    {
        return Err(AppError::BadRequest(
            "用户名须为 3～32 位小写字母、数字或下划线".to_owned(),
        ));
    }

    let nickname = nickname.trim().nfkc().collect::<String>();
    // 昵称按用户看到的“字符簇”计数，让单个 emoji（如 🐷）也能作为有效昵称。
    let nickname_length = nickname.graphemes(true).count();
    if !NICKNAME_LENGTH.contains(&nickname_length) || nickname.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "昵称须为 1～32 个字符且不能包含控制字符".to_owned(),
        ));
    }

    validate_password(password)?;
    let nickname_key = nickname.to_lowercase().nfkc().collect();
    Ok(ValidatedRegistration {
        username_key: username.clone(),
        username,
        nickname,
        nickname_key,
    })
}

pub fn validate_nickname(nickname: &str) -> Result<(String, String), AppError> {
    let nickname = nickname.trim().nfkc().collect::<String>();
    let length = nickname.graphemes(true).count();
    if !NICKNAME_LENGTH.contains(&length) || nickname.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "昵称须为 1～32 个字符且不能包含控制字符".to_owned(),
        ));
    }
    let key = nickname.to_lowercase().nfkc().collect();
    Ok((nickname, key))
}

pub fn validate_password_confirmation(
    password: &str,
    password_confirmation: &str,
) -> Result<(), AppError> {
    if password != password_confirmation {
        return Err(AppError::BadRequest("两次输入的密码不一致".to_owned()));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    let length = password.chars().count();
    if !(12..=128).contains(&length) {
        return Err(AppError::BadRequest(
            "密码长度须为 12～128 个字符".to_owned(),
        ));
    }
    Ok(())
}

pub async fn create_user(
    pool: &SqlitePool,
    username: &str,
    nickname: &str,
    password: &str,
    role: Role,
) -> Result<User, AppError> {
    let input = validate_registration(username, nickname, password)?;

    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE username_key = ?")
        .bind(&input.username_key)
        .fetch_one(pool)
        .await?
        > 0
    {
        return Err(AppError::BadRequest("用户名已被使用".to_owned()));
    }
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE nickname_key = ?")
        .bind(&input.nickname_key)
        .fetch_one(pool)
        .await?
        > 0
    {
        return Err(AppError::BadRequest("昵称已被使用".to_owned()));
    }

    let password = password.to_owned();
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| AppError::Internal(format!("密码任务异常：{error}")))??;
    let now = now_string()?;
    let id = Ulid::new().to_string();
    let result = sqlx::query(
        "INSERT INTO users (id, username, username_key, nickname, nickname_key, password_hash, role, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.username)
    .bind(&input.username_key)
    .bind(&input.nickname)
    .bind(&input.nickname_key)
    .bind(password_hash)
    .bind(role.as_str())
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await;

    if let Err(error) = result {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            return Err(AppError::BadRequest("用户名或昵称已被使用".to_owned()));
        }
        return Err(error.into());
    }

    find_user_by_id(pool, &id)
        .await?
        .ok_or_else(|| AppError::Internal("创建用户后无法读取用户".to_owned()))
}

pub async fn authenticate(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<Option<User>, AppError> {
    let username_key = username.trim().to_lowercase();
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username_key = ?")
        .bind(username_key)
        .fetch_optional(pool)
        .await?;

    let Some(user) = user else {
        // 即使用户不存在也执行一次散列，降低通过响应时间枚举用户名的风险。
        let password = password.to_owned();
        tokio::task::spawn_blocking(move || fake_password_check(&password))
            .await
            .map_err(|error| AppError::Internal(format!("密码任务异常：{error}")))?;
        return Ok(None);
    };

    let hash = user.password_hash.clone();
    let password = password.to_owned();
    let valid = tokio::task::spawn_blocking(move || verify_password(&hash, &password))
        .await
        .map_err(|error| AppError::Internal(format!("密码任务异常：{error}")))?;
    Ok(valid.then_some(user))
}

pub async fn find_user_by_id(pool: &SqlitePool, id: &str) -> Result<Option<User>, AppError> {
    Ok(
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn load_or_create_session(
    pool: &SqlitePool,
    config: &Arc<Config>,
    headers: &HeaderMap,
) -> Result<SessionContext, AppError> {
    if let Some(session) = load_session(pool, headers).await? {
        return Ok(SessionContext {
            row: session,
            new_cookie: None,
        });
    }

    let (raw_token, row) = insert_session(pool, None).await?;
    Ok(SessionContext {
        row,
        new_cookie: Some(build_session_cookie(config, &raw_token)),
    })
}

pub async fn require_session(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<SessionRow, AppError> {
    load_session(pool, headers)
        .await?
        .ok_or(AppError::Unauthorized)
}

pub async fn current_user(
    pool: &SqlitePool,
    session: &SessionRow,
) -> Result<Option<User>, AppError> {
    match &session.user_id {
        Some(user_id) => find_user_by_id(pool, user_id).await,
        None => Ok(None),
    }
}

pub async fn sign_in(
    pool: &SqlitePool,
    config: &Arc<Config>,
    old_session: &SessionRow,
    user_id: &str,
) -> Result<String, AppError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM web_sessions WHERE token_hash = ?")
        .bind(&old_session.token_hash)
        .execute(&mut *transaction)
        .await?;
    let (raw_token, row) = new_session(Some(user_id.to_owned()))?;
    sqlx::query(
        "INSERT INTO web_sessions (token_hash, user_id, csrf_token, expires_at, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&row.token_hash)
    .bind(&row.user_id)
    .bind(&row.csrf_token)
    .bind(row.expires_at)
    .bind(&row.created_at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(build_session_cookie(config, &raw_token))
}

pub async fn sign_out(pool: &SqlitePool, session: &SessionRow) -> Result<(), AppError> {
    sqlx::query("DELETE FROM web_sessions WHERE token_hash = ?")
        .bind(&session.token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn expired_session_cookie(config: &Arc<Config>) -> String {
    let mut cookie = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::ZERO)
        .build();
    cookie.set_secure(config.cookie_secure);
    cookie.to_string()
}

pub fn verify_csrf(session: &SessionRow, submitted: &str) -> Result<(), AppError> {
    if bool::from(session.csrf_token.as_bytes().ct_eq(submitted.as_bytes())) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

pub fn set_cookie_header(value: &str) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(value)
        .map_err(|error| AppError::Internal(format!("Cookie 构造失败：{error}")))
}

async fn load_session(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<Option<SessionRow>, AppError> {
    let Some(raw_token) = read_cookie(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let token_hash = hash_token(&raw_token);
    Ok(sqlx::query_as::<_, SessionRow>(
        "SELECT token_hash, user_id, csrf_token, expires_at, created_at FROM web_sessions WHERE token_hash = ? AND expires_at > ?",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(pool)
    .await?)
}

async fn insert_session(
    pool: &SqlitePool,
    user_id: Option<String>,
) -> Result<(String, SessionRow), AppError> {
    let (raw_token, row) = new_session(user_id)?;
    sqlx::query("DELETE FROM web_sessions WHERE expires_at <= ?")
        .bind(OffsetDateTime::now_utc().unix_timestamp())
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO web_sessions (token_hash, user_id, csrf_token, expires_at, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&row.token_hash)
    .bind(&row.user_id)
    .bind(&row.csrf_token)
    .bind(row.expires_at)
    .bind(&row.created_at)
    .execute(pool)
    .await?;
    Ok((raw_token, row))
}

fn new_session(user_id: Option<String>) -> Result<(String, SessionRow), AppError> {
    let raw_token = random_token(32);
    let now = OffsetDateTime::now_utc();
    let row = SessionRow {
        token_hash: hash_token(&raw_token),
        user_id,
        csrf_token: random_token(32),
        expires_at: (now + Duration::days(SESSION_DAYS)).unix_timestamp(),
        created_at: now
            .format(&Rfc3339)
            .map_err(|error| AppError::Internal(format!("时间格式化失败：{error}")))?,
    };
    Ok((raw_token, row))
}

fn build_session_cookie(config: &Arc<Config>, raw_token: &str) -> String {
    let mut cookie = Cookie::build((SESSION_COOKIE, raw_token.to_owned()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::days(SESSION_DAYS))
        .build();
    cookie.set_secure(config.cookie_secure);
    cookie.to_string()
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| Cookie::parse(part.trim().to_owned()).ok())
        .find(|cookie| cookie.name() == name)
        .map(|cookie| cookie.value().to_owned())
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn now_string() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::Internal(format!("时间格式化失败：{error}")))
}

fn password_hasher() -> Result<Argon2<'static>, AppError> {
    let params = Params::new(19 * 1024, 2, 1, None)
        .map_err(|error| AppError::Internal(format!("Argon2 参数无效：{error}")))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    password_hasher()?
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::Internal(format!("密码散列失败：{error}")))
}

fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    password_hasher()
        .and_then(|hasher| {
            hasher
                .verify_password(password.as_bytes(), &parsed_hash)
                .map_err(|error| AppError::Internal(error.to_string()))
        })
        .is_ok()
}

fn fake_password_check(password: &str) {
    // 不存在的用户也执行一次昂贵散列，使其成本接近正常密码校验。
    let _ = hash_password(password);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_registration_fields() {
        let input =
            validate_registration(" Alice_1 ", "  Ａlice  ", "correct horse battery").unwrap();
        assert_eq!(input.username, "alice_1");
        assert_eq!(input.nickname, "Alice");
        assert_eq!(input.nickname_key, "alice");
    }

    #[test]
    fn accepts_single_emoji_nickname() {
        let input = validate_registration("pig_user", "🐷", "correct horse battery").unwrap();
        assert_eq!(input.nickname, "🐷");
    }

    #[test]
    fn rejects_short_passwords() {
        let error = validate_registration("alice", "小艾", "too-short").unwrap_err();
        assert!(error.to_string().contains("12～128"));
    }

    #[test]
    fn rejects_mismatched_password_confirmation() {
        let error =
            validate_password_confirmation("first password", "second password").unwrap_err();
        assert!(error.to_string().contains("不一致"));
    }
}
