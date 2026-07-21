use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    // SQLite 的外键开关和 WAL 属于数据库适配层细节，业务 SQL 不依赖这些方言。
    let options = SqliteConnectOptions::from_str(database_url)
        .context("DATABASE_URL 不是有效的 SQLite 地址")?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("连接 SQLite 失败")?;

    sqlx::migrate!()
        .run(&pool)
        .await
        .context("执行数据库迁移失败")?;
    Ok(pool)
}
