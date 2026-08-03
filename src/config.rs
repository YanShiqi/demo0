use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/default.toml";
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 6324;
const DEFAULT_DATABASE_URL: &str = "sqlite://data/app.db?mode=rwc";
const DEFAULT_AVATAR_DIR: &str = "data/avatars";
const DEFAULT_COOKIE_SECURE: bool = false;
const DEFAULT_DISPLAY_UTC_OFFSET_HOURS: i8 = 8;
const DEFAULT_MESSAGE_RETENTION_DAYS: i64 = 5;
const DEFAULT_MESSAGE_LIMIT_PER_USER: i64 = 5;
const DEFAULT_MESSAGE_MAX_LENGTH: usize = 300;
const DEFAULT_MESSAGE_PAGE_SIZE: i64 = 30;
const DEFAULT_MESSAGE_HOME_PREVIEW_LIMIT: i64 = 5;
const DEFAULT_MESSAGE_CLEANUP_INTERVAL_HOURS: u64 = 6;
const DEFAULT_MEME_DIR: &str = "data/memes";
const DEFAULT_MEME_MAX_UPLOAD_BYTES: usize = 3 * 1024 * 1024;
const DEFAULT_MEME_MAX_DIMENSION: u32 = 3000;
const DEFAULT_MEME_MAX_GIF_FRAMES: usize = 120;
const DEFAULT_MEME_MAX_DECODED_PIXELS: u64 = 50_000_000;
const DEFAULT_MEME_PAGE_SIZE: i64 = 20;
const DEFAULT_MEME_HOME_PREVIEW_LIMIT: i64 = 6;
const DEFAULT_MEME_POPULAR_TAG_LIMIT: i64 = 10;
const DEFAULT_MEME_MAX_TAGS_PER_MEME: usize = 5;
const DEFAULT_MEME_MAX_TAG_LENGTH: usize = 20;
const DEFAULT_MEME_MAX_TITLE_LENGTH: usize = 60;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub avatar_dir: PathBuf,
    pub cookie_secure: bool,
    pub display: DisplayConfig,
    pub messages: MessageConfig,
    pub memes: MemeConfig,
}

#[derive(Clone, Debug)]
pub struct DisplayConfig {
    pub utc_offset_hours: i8,
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

#[derive(Clone, Debug)]
pub struct MemeConfig {
    pub dir: PathBuf,
    pub max_upload_bytes: usize,
    pub max_dimension: u32,
    pub max_gif_frames: usize,
    pub max_decoded_pixels: u64,
    pub page_size: i64,
    pub home_preview_limit: i64,
    pub popular_tag_limit: i64,
    pub max_tags_per_meme: usize,
    pub max_tag_length: usize,
    pub max_title_length: usize,
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
        let display = DisplayConfig::from_sources(file_config.display)?;
        let messages = MessageConfig::from_sources(file_config.messages)?;
        let memes = MemeConfig::from_sources(file_config.memes)?;

        Ok(Self {
            host,
            port,
            database_url,
            avatar_dir,
            cookie_secure,
            display,
            messages,
            memes,
        })
    }

    pub fn socket_address(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .context("APP_HOST 或 APP_PORT 无效")
    }
}

impl MemeConfig {
    fn from_sources(file_config: Option<MemeFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let dir = env::var("MEME_DIR")
            .ok()
            .or(file_config.dir)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_MEME_DIR));
        let max_upload_bytes = parse_optional_env("MEME_MAX_UPLOAD_BYTES")?
            .or(file_config.max_upload_bytes)
            .unwrap_or(DEFAULT_MEME_MAX_UPLOAD_BYTES);
        let max_dimension = parse_optional_env("MEME_MAX_DIMENSION")?
            .or(file_config.max_dimension)
            .unwrap_or(DEFAULT_MEME_MAX_DIMENSION);
        let max_gif_frames = parse_optional_env("MEME_MAX_GIF_FRAMES")?
            .or(file_config.max_gif_frames)
            .unwrap_or(DEFAULT_MEME_MAX_GIF_FRAMES);
        let max_decoded_pixels = parse_optional_env("MEME_MAX_DECODED_PIXELS")?
            .or(file_config.max_decoded_pixels)
            .unwrap_or(DEFAULT_MEME_MAX_DECODED_PIXELS);
        let page_size = parse_optional_env("MEME_PAGE_SIZE")?
            .or(file_config.page_size)
            .unwrap_or(DEFAULT_MEME_PAGE_SIZE);
        let home_preview_limit = parse_optional_env("MEME_HOME_PREVIEW_LIMIT")?
            .or(file_config.home_preview_limit)
            .unwrap_or(DEFAULT_MEME_HOME_PREVIEW_LIMIT);
        let popular_tag_limit = parse_optional_env("MEME_POPULAR_TAG_LIMIT")?
            .or(file_config.popular_tag_limit)
            .unwrap_or(DEFAULT_MEME_POPULAR_TAG_LIMIT);
        let max_tags_per_meme = parse_optional_env("MEME_MAX_TAGS_PER_MEME")?
            .or(file_config.max_tags_per_meme)
            .unwrap_or(DEFAULT_MEME_MAX_TAGS_PER_MEME);
        let max_tag_length = parse_optional_env("MEME_MAX_TAG_LENGTH")?
            .or(file_config.max_tag_length)
            .unwrap_or(DEFAULT_MEME_MAX_TAG_LENGTH);
        let max_title_length = parse_optional_env("MEME_MAX_TITLE_LENGTH")?
            .or(file_config.max_title_length)
            .unwrap_or(DEFAULT_MEME_MAX_TITLE_LENGTH);

        anyhow::ensure!(max_upload_bytes > 0, "MEME_MAX_UPLOAD_BYTES 必须大于 0");
        anyhow::ensure!(max_dimension > 0, "MEME_MAX_DIMENSION 必须大于 0");
        anyhow::ensure!(max_gif_frames > 0, "MEME_MAX_GIF_FRAMES 必须大于 0");
        anyhow::ensure!(max_decoded_pixels > 0, "MEME_MAX_DECODED_PIXELS 必须大于 0");
        anyhow::ensure!(page_size > 0, "MEME_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(home_preview_limit > 0, "MEME_HOME_PREVIEW_LIMIT 必须大于 0");
        anyhow::ensure!(popular_tag_limit > 0, "MEME_POPULAR_TAG_LIMIT 必须大于 0");
        anyhow::ensure!(max_tags_per_meme > 0, "MEME_MAX_TAGS_PER_MEME 必须大于 0");
        anyhow::ensure!(max_tag_length > 0, "MEME_MAX_TAG_LENGTH 必须大于 0");
        anyhow::ensure!(max_title_length > 0, "MEME_MAX_TITLE_LENGTH 必须大于 0");

        Ok(Self {
            dir,
            max_upload_bytes,
            max_dimension,
            max_gif_frames,
            max_decoded_pixels,
            page_size,
            home_preview_limit,
            popular_tag_limit,
            max_tags_per_meme,
            max_tag_length,
            max_title_length,
        })
    }
}

impl DisplayConfig {
    fn from_sources(file_config: Option<DisplayFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let utc_offset_hours = parse_optional_env("DISPLAY_UTC_OFFSET_HOURS")?
            .or(file_config.utc_offset_hours)
            .unwrap_or(DEFAULT_DISPLAY_UTC_OFFSET_HOURS);
        time::UtcOffset::from_hms(utc_offset_hours, 0, 0)
            .context("DISPLAY_UTC_OFFSET_HOURS 必须是有效 UTC 小时偏移")?;
        Ok(Self { utc_offset_hours })
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
    display: Option<DisplayFileConfig>,
    messages: Option<MessageFileConfig>,
    memes: Option<MemeFileConfig>,
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
struct DisplayFileConfig {
    utc_offset_hours: Option<i8>,
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

#[derive(Debug, Default, Deserialize)]
struct MemeFileConfig {
    dir: Option<String>,
    max_upload_bytes: Option<usize>,
    max_dimension: Option<u32>,
    max_gif_frames: Option<usize>,
    max_decoded_pixels: Option<u64>,
    page_size: Option<i64>,
    home_preview_limit: Option<i64>,
    popular_tag_limit: Option<i64>,
    max_tags_per_meme: Option<usize>,
    max_tag_length: Option<usize>,
    max_title_length: Option<usize>,
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
