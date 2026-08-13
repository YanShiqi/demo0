use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use time::{Date, format_description};

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateEntry {
    pub date: String,
    pub version: String,
    pub title: String,
    pub summary: String,
    pub changes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdatesDocument {
    #[serde(default)]
    updates: Vec<UpdateEntry>,
}

/// 从仓库文件读取更新记录，并在启动阶段完成格式校验与排序。
pub fn load_file(path: &Path) -> Result<Vec<UpdateEntry>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("读取服务器更新记录文件 {} 失败", path.display()))?;
    let document: UpdatesDocument = toml::from_str(&content)
        .with_context(|| format!("服务器更新记录文件 {} 不是有效的 TOML", path.display()))?;
    let date_format = format_description::parse_borrowed::<2>("[year]-[month]-[day]")
        .context("初始化更新记录日期格式失败")?;

    for entry in &document.updates {
        Date::parse(&entry.date, &date_format)
            .with_context(|| format!("更新记录日期无效：{}", entry.date))?;
        ensure!(!entry.version.trim().is_empty(), "更新记录版本号不能为空");
        ensure!(!entry.title.trim().is_empty(), "更新记录标题不能为空");
        ensure!(!entry.summary.trim().is_empty(), "更新记录摘要不能为空");
        ensure!(!entry.changes.is_empty(), "更新记录至少需要一条变更内容");
        ensure!(
            entry.changes.iter().all(|change| !change.trim().is_empty()),
            "更新记录变更内容不能为空"
        );
    }

    let mut entries = document.updates.into_iter().enumerate().collect::<Vec<_>>();
    // 日期相同的记录按文件追加顺序倒序，保证新追加的同日更新显示在前面。
    entries.sort_by(|(left_index, left), (right_index, right)| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| right_index.cmp(left_index))
    });
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_and_sorts_updates_by_date_descending() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("updates.toml");
        fs::write(
            &path,
            r#"
                [[updates]]
                date = "2026-01-01"
                version = "0.1.0"
                title = "首个版本"
                summary = "初始版本"
                changes = ["注册登录"]

                [[updates]]
                date = "2026-02-01"
                version = "0.2.0"
                title = "第二个版本"
                summary = "新增功能"
                changes = ["留言板"]
            "#,
        )
        .unwrap();

        let entries = load_file(&path).unwrap();

        assert_eq!(entries[0].version, "0.2.0");
        assert_eq!(entries[1].version, "0.1.0");
    }

    #[test]
    fn sorts_same_day_updates_by_append_order_descending() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("same-day-updates.toml");
        fs::write(
            &path,
            r#"
                [[updates]]
                date = "2026-08-13"
                version = "0.1.0"
                title = "较早更新"
                summary = "先发布的内容"
                changes = ["初始功能"]

                [[updates]]
                date = "2026-08-13"
                version = "0.1.1"
                title = "较新更新"
                summary = "后发布的内容"
                changes = ["新增功能"]
            "#,
        )
        .unwrap();

        let entries = load_file(&path).unwrap();

        assert_eq!(entries[0].version, "0.1.1");
        assert_eq!(entries[1].version, "0.1.0");
    }

    #[test]
    fn rejects_invalid_date_and_missing_title() {
        let directory = tempdir().unwrap();
        let invalid_date = directory.path().join("invalid-date.toml");
        fs::write(
            &invalid_date,
            r#"
                [[updates]]
                date = "not-a-date"
                version = "0.1.0"
                title = "标题"
                summary = "摘要"
                changes = ["变更"]
            "#,
        )
        .unwrap();
        assert!(load_file(&invalid_date).is_err());

        let missing_title = directory.path().join("missing-title.toml");
        fs::write(
            &missing_title,
            r#"
                [[updates]]
                date = "2026-01-01"
                version = "0.1.0"
                summary = "摘要"
                changes = ["变更"]
            "#,
        )
        .unwrap();
        assert!(load_file(&missing_title).is_err());
    }

    #[test]
    fn reports_missing_update_file() {
        let directory = tempdir().unwrap();
        let missing = directory.path().join("missing.toml");
        let error = load_file(&missing).unwrap_err();
        assert!(error.to_string().contains("missing.toml"));
    }
}
