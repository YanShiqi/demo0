use time::{Duration, OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

pub fn friendly_rfc3339(value: &str, utc_offset_hours: i8) -> String {
    let now_utc = OffsetDateTime::now_utc();
    friendly_rfc3339_at(value, utc_offset_hours, now_utc)
}

fn friendly_rfc3339_at(value: &str, utc_offset_hours: i8, now_utc: OffsetDateTime) -> String {
    let Ok(offset) = UtcOffset::from_hms(utc_offset_hours, 0, 0) else {
        return value.to_owned();
    };
    let Ok(datetime) = OffsetDateTime::parse(value, &Rfc3339) else {
        return value.to_owned();
    };
    // 数据库存 UTC 标准时间，展示时才转换成本地日期，避免存储层混入地区格式。
    let local = datetime.to_offset(offset);
    let local_now = now_utc.to_offset(offset);
    let local_date = local.date();
    let today = local_now.date();
    let yesterday = (local_now - Duration::days(1)).date();

    if local_date == today {
        format!("今天 {}", hour_minute(local))
    } else if local_date == yesterday {
        format!("昨天 {}", hour_minute(local))
    } else {
        format!(
            "{:04}-{:02}-{:02} {}",
            local.year(),
            u8::from(local.month()),
            local.day(),
            hour_minute(local)
        )
    }
}

fn hour_minute(datetime: OffsetDateTime) -> String {
    format!("{:02}:{:02}", datetime.hour(), datetime.minute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_today_and_yesterday_in_configured_offset() {
        let now = OffsetDateTime::parse("2026-07-22T12:00:00Z", &Rfc3339).unwrap();
        assert_eq!(
            friendly_rfc3339_at("2026-07-22T08:49:27.518906151Z", 8, now),
            "今天 16:49"
        );
        assert_eq!(
            friendly_rfc3339_at("2026-07-21T15:30:00Z", 8, now),
            "昨天 23:30"
        );
        assert_eq!(
            friendly_rfc3339_at("2026-07-20T15:30:00Z", 8, now),
            "2026-07-20 23:30"
        );
    }

    #[test]
    fn returns_original_value_when_input_is_not_rfc3339() {
        assert_eq!(
            friendly_rfc3339_at("not-a-time", 8, OffsetDateTime::now_utc()),
            "not-a-time"
        );
    }
}
