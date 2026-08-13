use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use ulid::Ulid;

use crate::{
    config::CurrencyConfig,
    error::AppError,
    model::{Role, User},
};

pub const REASON_ADMIN_GRANT: &str = "admin_grant";
pub const REASON_ADMIN_DEDUCT: &str = "admin_deduct";
pub const REASON_SPEND: &str = "spend";
pub const REASON_MEME_APPROVAL_REWARD: &str = "meme_approval_reward";
pub const REASON_WEEKLY_CHECK_IN: &str = "weekly_check_in";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrencyReason {
    AdminGrant,
    AdminDeduct,
    Spend,
    MemeApprovalReward,
    WeeklyCheckIn,
}

impl CurrencyReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdminGrant => REASON_ADMIN_GRANT,
            Self::AdminDeduct => REASON_ADMIN_DEDUCT,
            Self::Spend => REASON_SPEND,
            Self::MemeApprovalReward => REASON_MEME_APPROVAL_REWARD,
            Self::WeeklyCheckIn => REASON_WEEKLY_CHECK_IN,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrencyChange {
    pub log_id: String,
    pub operation_id: String,
    pub user_id: String,
    pub amount_delta: i64,
    pub balance_after: i64,
}

#[derive(Clone, Debug, FromRow)]
pub struct CurrencyLog {
    pub id: String,
    pub operation_id: String,
    pub user_id: String,
    pub amount_delta: i64,
    pub balance_after: i64,
    pub reason: String,
    pub operator_user_id: Option<String>,
    pub related_id: Option<String>,
    pub idempotency_key: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct RecentCurrencyLog {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub nickname: String,
    pub amount_delta: i64,
    pub balance_after: i64,
    pub reason: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct UserBalance {
    pub id: String,
    pub username: String,
    pub nickname: String,
    pub role: String,
    pub currency_balance: i64,
}

struct BalanceChangeRequest<'a> {
    target_user_id: &'a str,
    amount_delta: i64,
    reason: CurrencyReason,
    operator_user_id: Option<&'a str>,
    related_id: Option<&'a str>,
    note: &'a str,
    idempotency_key: &'a str,
}

pub async fn grant_currency(
    transaction: &mut Transaction<'_, Sqlite>,
    target_user_id: &str,
    amount: i64,
    actor: &User,
    note: &str,
    config: &CurrencyConfig,
) -> Result<CurrencyChange, AppError> {
    ensure_super_admin(actor)?;
    validate_adjustment(amount, note, config)?;
    change_balance(
        transaction,
        BalanceChangeRequest {
            target_user_id,
            amount_delta: amount,
            reason: CurrencyReason::AdminGrant,
            operator_user_id: Some(actor.id.as_str()),
            related_id: None,
            note,
            idempotency_key: &Ulid::new().to_string(),
        },
    )
    .await
}

pub async fn deduct_currency(
    transaction: &mut Transaction<'_, Sqlite>,
    target_user_id: &str,
    amount: i64,
    actor: &User,
    note: &str,
    config: &CurrencyConfig,
) -> Result<CurrencyChange, AppError> {
    ensure_super_admin(actor)?;
    validate_adjustment(amount, note, config)?;
    change_balance(
        transaction,
        BalanceChangeRequest {
            target_user_id,
            amount_delta: -amount,
            reason: CurrencyReason::AdminDeduct,
            operator_user_id: Some(actor.id.as_str()),
            related_id: None,
            note,
            idempotency_key: &Ulid::new().to_string(),
        },
    )
    .await
}

/// 为未来业务消费预留的统一扣款入口；业务层应以稳定的幂等键重试。
pub async fn spend_currency(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    amount: i64,
    reason: CurrencyReason,
    related_id: Option<&str>,
    idempotency_key: &str,
    note: &str,
) -> Result<CurrencyChange, AppError> {
    if amount <= 0 {
        return Err(AppError::BadRequest("消费金额必须大于 0".to_owned()));
    }
    if idempotency_key.trim().is_empty() {
        return Err(AppError::BadRequest("消费幂等键不能为空".to_owned()));
    }
    if reason != CurrencyReason::Spend {
        return Err(AppError::BadRequest("消费原因无效".to_owned()));
    }

    if let Some(existing) = sqlx::query_as::<_, CurrencyLog>(
        "SELECT id, operation_id, user_id, amount_delta, balance_after, reason,
                operator_user_id, related_id, idempotency_key, note, created_at
         FROM currency_logs WHERE idempotency_key = ?",
    )
    .bind(idempotency_key)
    .fetch_optional(&mut **transaction)
    .await?
    {
        if existing.user_id == user_id && existing.amount_delta == -amount {
            return Ok(CurrencyChange {
                log_id: existing.id,
                operation_id: existing.operation_id,
                user_id: existing.user_id,
                amount_delta: existing.amount_delta,
                balance_after: existing.balance_after,
            });
        }
        return Err(AppError::BadRequest(
            "消费幂等键已被其他操作使用".to_owned(),
        ));
    }

    change_balance(
        transaction,
        BalanceChangeRequest {
            target_user_id: user_id,
            amount_delta: -amount,
            reason,
            operator_user_id: Some(user_id),
            related_id,
            note,
            idempotency_key,
        },
    )
    .await
}

pub async fn reward_meme_approval(
    transaction: &mut Transaction<'_, Sqlite>,
    provider_user_id: &str,
    reviewer_user_id: &str,
    meme_id: &str,
    amount: i64,
) -> Result<CurrencyChange, AppError> {
    if amount <= 0 {
        return Err(AppError::BadRequest("审核奖励金额必须大于 0".to_owned()));
    }
    let idempotency_key = format!("meme-approval:{meme_id}");
    change_balance(
        transaction,
        BalanceChangeRequest {
            target_user_id: provider_user_id,
            amount_delta: amount,
            reason: CurrencyReason::MemeApprovalReward,
            operator_user_id: Some(reviewer_user_id),
            related_id: Some(meme_id),
            note: "Meme 审核通过奖励",
            idempotency_key: &idempotency_key,
        },
    )
    .await
}

pub async fn reward_weekly_check_in(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    check_in_id: &str,
    amount: i64,
    idempotency_key: &str,
) -> Result<CurrencyChange, AppError> {
    if amount <= 0 {
        return Err(AppError::BadRequest("签到奖励金额必须大于 0".to_owned()));
    }
    change_balance(
        transaction,
        BalanceChangeRequest {
            target_user_id: user_id,
            amount_delta: amount,
            reason: CurrencyReason::WeeklyCheckIn,
            operator_user_id: Some(user_id),
            related_id: Some(check_in_id),
            note: "每周签到奖励",
            idempotency_key,
        },
    )
    .await
}

async fn change_balance(
    transaction: &mut Transaction<'_, Sqlite>,
    request: BalanceChangeRequest<'_>,
) -> Result<CurrencyChange, AppError> {
    let current = sqlx::query_scalar::<_, i64>("SELECT currency_balance FROM users WHERE id = ?")
        .bind(request.target_user_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    let balance_after = current
        .checked_add(request.amount_delta)
        .ok_or_else(|| AppError::BadRequest("货币余额超出可表示范围".to_owned()))?;
    if balance_after < 0 {
        return Err(AppError::BadRequest("货币余额不足".to_owned()));
    }

    // 在事务内同时更新余额和流水，避免余额与流水出现分叉。
    sqlx::query(
        "UPDATE users SET currency_balance = ?, updated_at = ?
         WHERE id = ? AND currency_balance = ?",
    )
    .bind(balance_after)
    .bind(crate::auth::now_string()?)
    .bind(request.target_user_id)
    .bind(current)
    .execute(&mut **transaction)
    .await?
    .rows_affected()
    .eq(&1)
    .then_some(())
    .ok_or_else(|| AppError::BadRequest("货币余额已发生变化，请重试".to_owned()))?;

    let log_id = Ulid::new().to_string();
    let operation_id = Ulid::new().to_string();
    let created_at = crate::auth::now_string()?;
    sqlx::query(
        "INSERT INTO currency_logs
         (id, operation_id, user_id, amount_delta, balance_after, reason,
          operator_user_id, related_id, idempotency_key, note, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&log_id)
    .bind(&operation_id)
    .bind(request.target_user_id)
    .bind(request.amount_delta)
    .bind(balance_after)
    .bind(request.reason.as_str())
    .bind(request.operator_user_id)
    .bind(request.related_id)
    .bind(request.idempotency_key)
    .bind(request.note)
    .bind(created_at)
    .execute(&mut **transaction)
    .await?;

    Ok(CurrencyChange {
        log_id,
        operation_id,
        user_id: request.target_user_id.to_owned(),
        amount_delta: request.amount_delta,
        balance_after,
    })
}

fn ensure_super_admin(actor: &User) -> Result<(), AppError> {
    if actor.parsed_role() == Role::SuperAdmin {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn validate_adjustment(amount: i64, note: &str, config: &CurrencyConfig) -> Result<(), AppError> {
    if amount <= 0 || amount > config.max_admin_adjust_amount {
        return Err(AppError::BadRequest(format!(
            "调整金额必须在 1 到 {} 之间",
            config.max_admin_adjust_amount
        )));
    }
    let trimmed = note.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("调整备注不能为空".to_owned()));
    }
    if trimmed.chars().count() > config.max_note_length {
        return Err(AppError::BadRequest("调整备注过长".to_owned()));
    }
    Ok(())
}

pub async fn count_logs(pool: &SqlitePool, user_id: &str) -> Result<i64, AppError> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM currency_logs WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn list_logs(
    pool: &SqlitePool,
    user_id: &str,
    page: i64,
    page_size: i64,
) -> Result<Vec<CurrencyLog>, AppError> {
    let offset = (page.saturating_sub(1)).saturating_mul(page_size);
    Ok(sqlx::query_as::<_, CurrencyLog>(
        "SELECT id, operation_id, user_id, amount_delta, balance_after, reason,
                operator_user_id, related_id, idempotency_key, note, created_at
         FROM currency_logs WHERE user_id = ?
         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
    )
    .bind(user_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(pool)
    .await?)
}

pub async fn list_recent_logs(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<RecentCurrencyLog>, AppError> {
    Ok(sqlx::query_as::<_, RecentCurrencyLog>(
        "SELECT currency_logs.id, currency_logs.user_id, users.username, users.nickname,
                currency_logs.amount_delta, currency_logs.balance_after, currency_logs.reason,
                currency_logs.note, currency_logs.created_at
         FROM currency_logs
         INNER JOIN users ON users.id = currency_logs.user_id
         ORDER BY currency_logs.created_at DESC, currency_logs.id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn search_users(
    pool: &SqlitePool,
    query: Option<&str>,
    limit: i64,
) -> Result<Vec<UserBalance>, AppError> {
    let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };
    let pattern = format!("%{query}%");
    Ok(sqlx::query_as::<_, UserBalance>(
        "SELECT id, username, nickname, role, currency_balance FROM users
         WHERE username LIKE ? OR nickname LIKE ?
         ORDER BY username_key LIMIT ?",
    )
    .bind(&pattern)
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn find_user_balance(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Option<UserBalance>, AppError> {
    Ok(sqlx::query_as::<_, UserBalance>(
        "SELECT id, username, nickname, role, currency_balance FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}
