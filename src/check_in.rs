use sqlx::{Sqlite, SqlitePool, Transaction};
use time::{Duration, OffsetDateTime, UtcOffset};
use ulid::Ulid;

use crate::{auth, currency, error::AppError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckInResult {
    Awarded,
    AlreadyCheckedIn,
}

/// 将 UTC 时间换算到业务时区，并返回该周周一的本地日期。
pub fn week_start_for(now_utc: OffsetDateTime, offset: UtcOffset) -> String {
    let local = now_utc.to_offset(offset);
    let monday = local - Duration::days(local.weekday().number_days_from_monday() as i64);
    let date = monday.date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

pub fn current_week_start(utc_offset_hours: i8) -> Result<String, AppError> {
    let offset = UtcOffset::from_hms(utc_offset_hours, 0, 0)
        .map_err(|error| AppError::Internal(format!("签到时区配置无效：{error}")))?;
    Ok(week_start_for(OffsetDateTime::now_utc(), offset))
}

pub async fn has_checked_in(
    pool: &SqlitePool,
    user_id: &str,
    week_start: &str,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM weekly_check_ins WHERE user_id = ? AND week_start = ?",
    )
    .bind(user_id)
    .bind(week_start)
    .fetch_one(pool)
    .await?
        > 0)
}

pub async fn perform(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    week_start: &str,
    reward_amount: i64,
) -> Result<CheckInResult, AppError> {
    if reward_amount <= 0 {
        return Err(AppError::BadRequest("签到奖励金额必须大于 0".to_owned()));
    }
    let already_checked_in = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM weekly_check_ins WHERE user_id = ? AND week_start = ?",
    )
    .bind(user_id)
    .bind(week_start)
    .fetch_one(&mut **transaction)
    .await?
        > 0;
    if already_checked_in {
        return Ok(CheckInResult::AlreadyCheckedIn);
    }

    let check_in_id = Ulid::new().to_string();
    let created_at = auth::now_string()?;
    let insert_result = sqlx::query(
        "INSERT INTO weekly_check_ins
         (id, user_id, week_start, reward_amount, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&check_in_id)
    .bind(user_id)
    .bind(week_start)
    .bind(reward_amount)
    .bind(created_at)
    .execute(&mut **transaction)
    .await;
    if let Err(error) = insert_result {
        if is_unique_violation(&error) {
            return Ok(CheckInResult::AlreadyCheckedIn);
        }
        return Err(error.into());
    }

    // 签到记录和货币流水共用事务，任何一步失败都会回滚本次签到。
    let idempotency_key = format!("weekly-check-in:{user_id}:{week_start}");
    currency::reward_weekly_check_in(
        transaction,
        user_id,
        &check_in_id,
        reward_amount,
        &idempotency_key,
    )
    .await?;
    Ok(CheckInResult::Awarded)
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database_error) if database_error.is_unique_violation())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

    fn timestamp(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }

    #[test]
    fn week_start_for_sunday_returns_previous_monday() {
        let week_start = week_start_for(timestamp("2026-08-16T02:00:00Z"), UtcOffset::UTC);
        assert_eq!(week_start, "2026-08-10");
    }

    #[test]
    fn week_start_for_monday_midnight_returns_current_monday() {
        let week_start = week_start_for(
            timestamp("2026-08-16T16:00:00Z"),
            UtcOffset::from_hms(8, 0, 0).unwrap(),
        );
        assert_eq!(week_start, "2026-08-17");
    }

    #[test]
    fn week_start_for_handles_year_boundary() {
        let week_start = week_start_for(
            timestamp("2027-01-03T16:00:00Z"),
            UtcOffset::from_hms(8, 0, 0).unwrap(),
        );
        assert_eq!(week_start, "2027-01-04");
    }
}
