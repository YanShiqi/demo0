use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/default.toml";
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 6324;
const DEFAULT_DATABASE_URL: &str = "sqlite://data/app.db?mode=rwc";
const DEFAULT_AVATAR_DIR: &str = "data/avatars";
const DEFAULT_COOKIE_SECURE: bool = false;
const DEFAULT_MESSAGE_RETENTION_DAYS: i64 = 5;
const DEFAULT_MESSAGE_LIMIT_PER_USER: i64 = 5;
const DEFAULT_MESSAGE_MAX_LENGTH: usize = 300;
const DEFAULT_MESSAGE_PAGE_SIZE: i64 = 30;
const DEFAULT_MESSAGE_HOME_PREVIEW_LIMIT: i64 = 5;
const DEFAULT_MESSAGE_CLEANUP_INTERVAL_HOURS: u64 = 6;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub avatar_dir: PathBuf,
    pub cookie_secure: bool,
    pub messages: MessageConfig,
}

#[derive(Clone, Debug)]
pub struct MessageConfig {
    pub retention_days: i64,
    pub limit_per_user: i64,
    pub max_length: usize,
    pub page_size: i64,
    pub home_preview_limit: i64,
    pub cleanup_interval_hours: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let file_config = FileConfig::load(DEFAULT_CONFIG_PATH)?;
        // 本地开发优先读取仓库根目录 .env，系统环境变量可覆盖 TOML 中的部署差异。
        let _ = dotenvy::dotenv();
        let host = env::var("APP_HOST")
            .ok()
            .or(file_config
                .server
                .as_ref()
                .and_then(|server| server.host.clone()))
            .unwrap_or_else(|| DEFAULT_HOST.to_owned());
        let port = parse_optional_env("APP_PORT")?
            .or(file_config.server.as_ref().and_then(|server| server.port))
            .unwrap_or(DEFAULT_PORT);
        let database_url = env::var("DATABASE_URL")
            .ok()
            .or(file_config
                .database
                .as_ref()
                .and_then(|database| database.url.clone()))
            .unwrap_or_else(|| DEFAULT_DATABASE_URL.to_owned());
        let avatar_dir = env::var("AVATAR_DIR")
            .ok()
            .or(file_config
                .avatar
                .as_ref()
                .and_then(|avatar| avatar.dir.clone()))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_AVATAR_DIR));
        let cookie_secure = parse_optional_env("COOKIE_SECURE")?
            .or(file_config
                .session
                .as_ref()
                .and_then(|session| session.cookie_secure))
            .unwrap_or(DEFAULT_COOKIE_SECURE);
        let messages = MessageConfig::from_sources(file_config.messages)?;

        Ok(Self {
            host,
            port,
            database_url,
            avatar_dir,
            cookie_secure,
            messages,
        })
    }

    pub fn socket_address(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .context("APP_HOST 或 APP_PORT 无效")
    }
}

impl MessageConfig {
    fn from_sources(file_config: Option<MessageFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let retention_days = parse_optional_env("MESSAGE_RETENTION_DAYS")?
            .or(file_config.retention_days)
            .unwrap_or(DEFAULT_MESSAGE_RETENTION_DAYS);
        let limit_per_user = parse_optional_env("MESSAGE_LIMIT_PER_USER")?
            .or(file_config.limit_per_user)
            .unwrap_or(DEFAULT_MESSAGE_LIMIT_PER_USER);
        let max_length = parse_optional_env("MESSAGE_MAX_LENGTH")?
            .or(file_config.max_length)
            .unwrap_or(DEFAULT_MESSAGE_MAX_LENGTH);
        let page_size = parse_optional_env("MESSAGE_PAGE_SIZE")?
            .or(file_config.page_size)
            .unwrap_or(DEFAULT_MESSAGE_PAGE_SIZE);
        let home_preview_limit = parse_optional_env("MESSAGE_HOME_PREVIEW_LIMIT")?
            .or(file_config.home_preview_limit)
            .unwrap_or(DEFAULT_MESSAGE_HOME_PREVIEW_LIMIT);
        let cleanup_interval_hours = parse_optional_env("MESSAGE_CLEANUP_INTERVAL_HOURS")?
            .or(file_config.cleanup_interval_hours)
            .unwrap_or(DEFAULT_MESSAGE_CLEANUP_INTERVAL_HOURS);

        anyhow::ensure!(retention_days > 0, "MESSAGE_RETENTION_DAYS 必须大于 0");
        anyhow::ensure!(limit_per_user > 0, "MESSAGE_LIMIT_PER_USER 必须大于 0");
        anyhow::ensure!(max_length > 0, "MESSAGE_MAX_LENGTH 必须大于 0");
        anyhow::ensure!(page_size > 0, "MESSAGE_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(
            home_preview_limit > 0,
            "MESSAGE_HOME_PREVIEW_LIMIT 必须大于 0"
        );
        anyhow::ensure!(
            cleanup_interval_hours > 0,
            "MESSAGE_CLEANUP_INTERVAL_HOURS 必须大于 0"
        );

        Ok(Self {
            retention_days,
            limit_per_user,
            max_length,
            page_size,
            home_preview_limit,
            cleanup_interval_hours,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    server: Option<ServerFileConfig>,
    database: Option<DatabaseFileConfig>,
    avatar: Option<AvatarFileConfig>,
    session: Option<SessionFileConfig>,
    messages: Option<MessageFileConfig>,
}

impl FileConfig {
    fn load(path: &str) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                toml::from_str(&content).with_context(|| format!("{path} 不是有效的 TOML 配置"))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("读取 {path} 失败")),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ServerFileConfig {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct DatabaseFileConfig {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AvatarFileConfig {
    dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionFileConfig {
    cookie_secure: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct MessageFileConfig {
    retention_days: Option<i64>,
    limit_per_user: Option<i64>,
    max_length: Option<usize>,
    page_size: Option<i64>,
    home_preview_limit: Option<i64>,
    cleanup_interval_hours: Option<u64>,
}

fn parse_optional_env<T>(name: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map(Some)
            .map_err(|error| anyhow::anyhow!("{name} 配置值无效：{error}")),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("读取 {name} 失败")),
    }
}
