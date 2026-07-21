use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub avatar_dir: PathBuf,
    pub cookie_secure: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        // 本地开发优先读取仓库根目录 .env，系统环境变量仍可覆盖其中的值。
        let _ = dotenvy::dotenv();
        let host = env::var("APP_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
        let port = env::var("APP_PORT")
            .unwrap_or_else(|_| "6324".to_owned())
            .parse()
            .context("APP_PORT 必须是有效端口")?;
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/app.db?mode=rwc".to_owned());
        let avatar_dir = env::var("AVATAR_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/avatars"));
        let cookie_secure = parse_bool("COOKIE_SECURE", false)?;

        Ok(Self {
            host,
            port,
            database_url,
            avatar_dir,
            cookie_secure,
        })
    }

    pub fn socket_address(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .context("APP_HOST 或 APP_PORT 无效")
    }
}

fn parse_bool(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("{name} 必须是 true 或 false")),
        Err(_) => Ok(default),
    }
}
