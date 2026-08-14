use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::updates::{self, UpdateEntry};

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
const DEFAULT_MEME_PROFILE_PAGE_SIZE: i64 = 12;
const DEFAULT_MEME_HOME_PREVIEW_LIMIT: i64 = 6;
const DEFAULT_MEME_POPULAR_TAG_LIMIT: i64 = 10;
const DEFAULT_MEME_MAX_TAGS_PER_MEME: usize = 5;
const DEFAULT_MEME_MAX_TAG_LENGTH: usize = 20;
const DEFAULT_MEME_MAX_TITLE_LENGTH: usize = 60;
const DEFAULT_MEME_APPROVAL_REWARD_ENABLED: bool = true;
const DEFAULT_MEME_APPROVAL_REWARD_AMOUNT: i64 = 2;
const DEFAULT_NOVEL_HOME_PREVIEW_LIMIT: i64 = 5;
const DEFAULT_NOVEL_CHAPTER_MAX_UPLOAD_BYTES: usize = 256 * 1024;
const DEFAULT_NOVEL_MAX_TITLE_LENGTH: usize = 60;
const DEFAULT_NOVEL_MAX_CHAPTER_TITLE_LENGTH: usize = 80;
const DEFAULT_NOVEL_CHAPTER_COMMENT_MAX_LENGTH: usize = 300;
const DEFAULT_NOVEL_CHAPTER_COMMENT_PAGE_SIZE: i64 = 50;
const DEFAULT_UPDATES_FILE: &str = "content/updates.toml";
const DEFAULT_UPDATES_HOME_PREVIEW_LIMIT: i64 = 3;
const DEFAULT_CURRENCY_NAME: &str = "洲币";
const DEFAULT_CURRENCY_SYMBOL: &str = "🪙";
const DEFAULT_CURRENCY_LOG_PAGE_SIZE: i64 = 30;
const DEFAULT_CURRENCY_ADMIN_RECENT_LOG_LIMIT: i64 = 10;
const DEFAULT_CURRENCY_MAX_ADMIN_ADJUST_AMOUNT: i64 = 99_999;
const DEFAULT_CURRENCY_ADMIN_USER_SEARCH_LIMIT: i64 = 20;
const DEFAULT_CURRENCY_MAX_NOTE_LENGTH: usize = 200;
const DEFAULT_CHECK_IN_ENABLED: bool = true;
const DEFAULT_CHECK_IN_REWARD_AMOUNT: i64 = 1;
const DEFAULT_SHOP_ENABLED: bool = true;
const DEFAULT_SHOP_ICON_DIR: &str = "data/shop/product-icons";
const DEFAULT_SHOP_PAGE_SIZE: i64 = 12;
const DEFAULT_SHOP_VOUCHER_PAGE_SIZE: i64 = 20;
const DEFAULT_SHOP_ADMIN_NOTE_MAX_LENGTH: usize = 200;
const DEFAULT_SHOP_TOKEN_LOOKUP_MAX_ATTEMPTS: usize = 20;
const DEFAULT_SHOP_TOKEN_LOOKUP_WINDOW_SECONDS: u64 = 60;
const DEFAULT_SHOP_ICON_UPLOAD_MAX_BYTES: usize = 5 * 1024 * 1024;
const DEFAULT_SHOP_ICON_INPUT_MAX_DIMENSION: u32 = 4096;
const DEFAULT_SHOP_ICON_MAX_GIF_FRAMES: usize = 120;
const DEFAULT_SHOP_ICON_MAX_DECODED_PIXELS: u64 = 80_000_000;
const DEFAULT_SHOP_ICON_MAX_STORED_BYTES: usize = 1024 * 1024;
const DEFAULT_SHOP_ICON_RESIZE_DIMENSIONS: &[u32] = &[512, 384, 256];

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
    pub novels: NovelConfig,
    pub updates: UpdateConfig,
    pub currency: CurrencyConfig,
    pub check_in: CheckInConfig,
    pub shop: ShopConfig,
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
    pub profile_page_size: i64,
    pub home_preview_limit: i64,
    pub popular_tag_limit: i64,
    pub max_tags_per_meme: usize,
    pub max_tag_length: usize,
    pub max_title_length: usize,
    pub approval_reward_enabled: bool,
    pub approval_reward_amount: i64,
}

#[derive(Clone, Debug)]
pub struct NovelConfig {
    pub home_preview_limit: i64,
    pub chapter_max_upload_bytes: usize,
    pub max_title_length: usize,
    pub max_chapter_title_length: usize,
    pub chapter_comment_max_length: usize,
    pub chapter_comment_page_size: i64,
}

#[derive(Clone, Debug)]
pub struct UpdateConfig {
    pub file: PathBuf,
    pub home_preview_limit: i64,
    pub entries: Vec<UpdateEntry>,
}

#[derive(Clone, Debug)]
pub struct CurrencyConfig {
    pub name: String,
    pub symbol: String,
    pub log_page_size: i64,
    pub admin_recent_log_limit: i64,
    pub max_admin_adjust_amount: i64,
    pub admin_user_search_limit: i64,
    pub max_note_length: usize,
}

#[derive(Clone, Debug)]
pub struct CheckInConfig {
    pub enabled: bool,
    pub reward_amount: i64,
}

#[derive(Clone, Debug)]
pub struct ShopConfig {
    pub enabled: bool,
    pub icon_dir: PathBuf,
    pub page_size: i64,
    pub voucher_page_size: i64,
    pub admin_note_max_length: usize,
    pub token_lookup_max_attempts: usize,
    pub token_lookup_window_seconds: u64,
    pub icon_upload_max_bytes: usize,
    pub icon_input_max_dimension: u32,
    pub icon_max_gif_frames: usize,
    pub icon_max_decoded_pixels: u64,
    pub icon_max_stored_bytes: usize,
    pub icon_resize_dimensions: Vec<u32>,
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
        let novels = NovelConfig::from_sources(file_config.novels)?;
        let updates = UpdateConfig::from_sources(file_config.updates)?;
        let currency = CurrencyConfig::from_sources(file_config.currency)?;
        let check_in = CheckInConfig::from_sources(file_config.check_in)?;
        let shop = ShopConfig::from_sources(file_config.shop)?;

        Ok(Self {
            host,
            port,
            database_url,
            avatar_dir,
            cookie_secure,
            display,
            messages,
            memes,
            novels,
            updates,
            currency,
            check_in,
            shop,
        })
    }

    pub fn socket_address(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .context("APP_HOST 或 APP_PORT 无效")
    }
}

impl ShopConfig {
    fn from_sources(file_config: Option<ShopFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let enabled = parse_optional_env("SHOP_ENABLED")?
            .or(file_config.enabled)
            .unwrap_or(DEFAULT_SHOP_ENABLED);
        let icon_dir = env::var("SHOP_ICON_DIR")
            .ok()
            .or(file_config.icon_dir)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SHOP_ICON_DIR));
        let page_size = parse_optional_env("SHOP_PAGE_SIZE")?
            .or(file_config.page_size)
            .unwrap_or(DEFAULT_SHOP_PAGE_SIZE);
        let voucher_page_size = parse_optional_env("SHOP_VOUCHER_PAGE_SIZE")?
            .or(file_config.voucher_page_size)
            .unwrap_or(DEFAULT_SHOP_VOUCHER_PAGE_SIZE);
        let admin_note_max_length = parse_optional_env("SHOP_ADMIN_NOTE_MAX_LENGTH")?
            .or(file_config.admin_note_max_length)
            .unwrap_or(DEFAULT_SHOP_ADMIN_NOTE_MAX_LENGTH);
        let token_lookup_max_attempts = parse_optional_env("SHOP_TOKEN_LOOKUP_MAX_ATTEMPTS")?
            .or(file_config.token_lookup_max_attempts)
            .unwrap_or(DEFAULT_SHOP_TOKEN_LOOKUP_MAX_ATTEMPTS);
        let token_lookup_window_seconds = parse_optional_env("SHOP_TOKEN_LOOKUP_WINDOW_SECONDS")?
            .or(file_config.token_lookup_window_seconds)
            .unwrap_or(DEFAULT_SHOP_TOKEN_LOOKUP_WINDOW_SECONDS);
        let icon_upload_max_bytes = parse_optional_env("SHOP_ICON_UPLOAD_MAX_BYTES")?
            .or(file_config.icon_upload_max_bytes)
            .unwrap_or(DEFAULT_SHOP_ICON_UPLOAD_MAX_BYTES);
        let icon_input_max_dimension = parse_optional_env("SHOP_ICON_INPUT_MAX_DIMENSION")?
            .or(file_config.icon_input_max_dimension)
            .unwrap_or(DEFAULT_SHOP_ICON_INPUT_MAX_DIMENSION);
        let icon_max_gif_frames = parse_optional_env("SHOP_ICON_MAX_GIF_FRAMES")?
            .or(file_config.icon_max_gif_frames)
            .unwrap_or(DEFAULT_SHOP_ICON_MAX_GIF_FRAMES);
        let icon_max_decoded_pixels = parse_optional_env("SHOP_ICON_MAX_DECODED_PIXELS")?
            .or(file_config.icon_max_decoded_pixels)
            .unwrap_or(DEFAULT_SHOP_ICON_MAX_DECODED_PIXELS);
        let icon_max_stored_bytes = parse_optional_env("SHOP_ICON_MAX_STORED_BYTES")?
            .or(file_config.icon_max_stored_bytes)
            .unwrap_or(DEFAULT_SHOP_ICON_MAX_STORED_BYTES);
        let icon_resize_dimensions = parse_optional_env_list("SHOP_ICON_RESIZE_DIMENSIONS")?
            .or(file_config.icon_resize_dimensions)
            .unwrap_or_else(|| DEFAULT_SHOP_ICON_RESIZE_DIMENSIONS.to_vec());

        // 启动前拒绝无效限制，避免后续请求在不安全的边界条件下运行。
        anyhow::ensure!(page_size > 0, "SHOP_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(voucher_page_size > 0, "SHOP_VOUCHER_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(
            admin_note_max_length > 0,
            "SHOP_ADMIN_NOTE_MAX_LENGTH 必须大于 0"
        );
        anyhow::ensure!(
            token_lookup_max_attempts > 0,
            "SHOP_TOKEN_LOOKUP_MAX_ATTEMPTS 必须大于 0"
        );
        anyhow::ensure!(
            token_lookup_window_seconds > 0,
            "SHOP_TOKEN_LOOKUP_WINDOW_SECONDS 必须大于 0"
        );
        anyhow::ensure!(
            icon_upload_max_bytes > 0,
            "SHOP_ICON_UPLOAD_MAX_BYTES 必须大于 0"
        );
        anyhow::ensure!(
            icon_input_max_dimension > 0,
            "SHOP_ICON_INPUT_MAX_DIMENSION 必须大于 0"
        );
        anyhow::ensure!(
            icon_max_gif_frames > 0,
            "SHOP_ICON_MAX_GIF_FRAMES 必须大于 0"
        );
        anyhow::ensure!(
            icon_max_decoded_pixels > 0,
            "SHOP_ICON_MAX_DECODED_PIXELS 必须大于 0"
        );
        anyhow::ensure!(
            icon_max_stored_bytes > 0,
            "SHOP_ICON_MAX_STORED_BYTES 必须大于 0"
        );
        validate_resize_dimensions(&icon_resize_dimensions)?;
        Ok(Self {
            enabled,
            icon_dir,
            page_size,
            voucher_page_size,
            admin_note_max_length,
            token_lookup_max_attempts,
            token_lookup_window_seconds,
            icon_upload_max_bytes,
            icon_input_max_dimension,
            icon_max_gif_frames,
            icon_max_decoded_pixels,
            icon_max_stored_bytes,
            icon_resize_dimensions,
        })
    }
}

impl UpdateConfig {
    fn from_sources(file_config: Option<UpdatesFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let file = env::var("UPDATES_FILE")
            .ok()
            .or(file_config.file)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_UPDATES_FILE));
        let home_preview_limit = parse_optional_env("UPDATES_HOME_PREVIEW_LIMIT")?
            .or(file_config.home_preview_limit)
            .unwrap_or(DEFAULT_UPDATES_HOME_PREVIEW_LIMIT);
        anyhow::ensure!(
            home_preview_limit > 0,
            "UPDATES_HOME_PREVIEW_LIMIT 必须大于 0"
        );
        let entries = updates::load_file(&file)?;
        Ok(Self {
            file,
            home_preview_limit,
            entries,
        })
    }
}

impl CurrencyConfig {
    fn from_sources(file_config: Option<CurrencyFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let name = env::var("CURRENCY_NAME")
            .ok()
            .or(file_config.name)
            .unwrap_or_else(|| DEFAULT_CURRENCY_NAME.to_owned());
        let symbol = env::var("CURRENCY_SYMBOL")
            .ok()
            .or(file_config.symbol)
            .unwrap_or_else(|| DEFAULT_CURRENCY_SYMBOL.to_owned());
        let log_page_size = parse_optional_env("CURRENCY_LOG_PAGE_SIZE")?
            .or(file_config.log_page_size)
            .unwrap_or(DEFAULT_CURRENCY_LOG_PAGE_SIZE);
        let admin_recent_log_limit = parse_optional_env("CURRENCY_ADMIN_RECENT_LOG_LIMIT")?
            .or(file_config.admin_recent_log_limit)
            .unwrap_or(DEFAULT_CURRENCY_ADMIN_RECENT_LOG_LIMIT);
        let max_admin_adjust_amount = parse_optional_env("CURRENCY_MAX_ADMIN_ADJUST_AMOUNT")?
            .or(file_config.max_admin_adjust_amount)
            .unwrap_or(DEFAULT_CURRENCY_MAX_ADMIN_ADJUST_AMOUNT);
        let admin_user_search_limit = parse_optional_env("CURRENCY_ADMIN_USER_SEARCH_LIMIT")?
            .or(file_config.admin_user_search_limit)
            .unwrap_or(DEFAULT_CURRENCY_ADMIN_USER_SEARCH_LIMIT);
        let max_note_length = parse_optional_env("CURRENCY_MAX_NOTE_LENGTH")?
            .or(file_config.max_note_length)
            .unwrap_or(DEFAULT_CURRENCY_MAX_NOTE_LENGTH);

        anyhow::ensure!(!name.trim().is_empty(), "CURRENCY_NAME 不能为空");
        anyhow::ensure!(!symbol.trim().is_empty(), "CURRENCY_SYMBOL 不能为空");
        anyhow::ensure!(log_page_size > 0, "CURRENCY_LOG_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(
            admin_recent_log_limit > 0,
            "CURRENCY_ADMIN_RECENT_LOG_LIMIT 必须大于 0"
        );
        anyhow::ensure!(
            max_admin_adjust_amount > 0,
            "CURRENCY_MAX_ADMIN_ADJUST_AMOUNT 必须大于 0"
        );
        anyhow::ensure!(
            admin_user_search_limit > 0,
            "CURRENCY_ADMIN_USER_SEARCH_LIMIT 必须大于 0"
        );
        anyhow::ensure!(max_note_length > 0, "CURRENCY_MAX_NOTE_LENGTH 必须大于 0");

        Ok(Self {
            name,
            symbol,
            log_page_size,
            admin_recent_log_limit,
            max_admin_adjust_amount,
            admin_user_search_limit,
            max_note_length,
        })
    }
}

impl CheckInConfig {
    fn from_sources(file_config: Option<CheckInFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let enabled = parse_optional_env("CHECK_IN_ENABLED")?
            .or(file_config.enabled)
            .unwrap_or(DEFAULT_CHECK_IN_ENABLED);
        let reward_amount = parse_optional_env("CHECK_IN_REWARD_AMOUNT")?
            .or(file_config.reward_amount)
            .unwrap_or(DEFAULT_CHECK_IN_REWARD_AMOUNT);
        anyhow::ensure!(reward_amount > 0, "CHECK_IN_REWARD_AMOUNT 必须大于 0");
        Ok(Self {
            enabled,
            reward_amount,
        })
    }
}

impl NovelConfig {
    fn from_sources(file_config: Option<NovelFileConfig>) -> Result<Self> {
        let file_config = file_config.unwrap_or_default();
        let home_preview_limit = parse_optional_env("NOVEL_HOME_PREVIEW_LIMIT")?
            .or(file_config.home_preview_limit)
            .unwrap_or(DEFAULT_NOVEL_HOME_PREVIEW_LIMIT);
        let chapter_max_upload_bytes = parse_optional_env("NOVEL_CHAPTER_MAX_UPLOAD_BYTES")?
            .or(file_config.chapter_max_upload_bytes)
            .unwrap_or(DEFAULT_NOVEL_CHAPTER_MAX_UPLOAD_BYTES);
        let max_title_length = parse_optional_env("NOVEL_MAX_TITLE_LENGTH")?
            .or(file_config.max_title_length)
            .unwrap_or(DEFAULT_NOVEL_MAX_TITLE_LENGTH);
        let max_chapter_title_length = parse_optional_env("NOVEL_MAX_CHAPTER_TITLE_LENGTH")?
            .or(file_config.max_chapter_title_length)
            .unwrap_or(DEFAULT_NOVEL_MAX_CHAPTER_TITLE_LENGTH);
        let chapter_comment_max_length = parse_optional_env("NOVEL_CHAPTER_COMMENT_MAX_LENGTH")?
            .or(file_config.chapter_comment_max_length)
            .unwrap_or(DEFAULT_NOVEL_CHAPTER_COMMENT_MAX_LENGTH);
        let chapter_comment_page_size = parse_optional_env("NOVEL_CHAPTER_COMMENT_PAGE_SIZE")?
            .or(file_config.chapter_comment_page_size)
            .unwrap_or(DEFAULT_NOVEL_CHAPTER_COMMENT_PAGE_SIZE);

        anyhow::ensure!(
            home_preview_limit > 0,
            "NOVEL_HOME_PREVIEW_LIMIT 必须大于 0"
        );
        anyhow::ensure!(
            chapter_max_upload_bytes > 0,
            "NOVEL_CHAPTER_MAX_UPLOAD_BYTES 必须大于 0"
        );
        anyhow::ensure!(max_title_length > 0, "NOVEL_MAX_TITLE_LENGTH 必须大于 0");
        anyhow::ensure!(
            max_chapter_title_length > 0,
            "NOVEL_MAX_CHAPTER_TITLE_LENGTH 必须大于 0"
        );
        anyhow::ensure!(
            chapter_comment_max_length > 0,
            "NOVEL_CHAPTER_COMMENT_MAX_LENGTH 必须大于 0"
        );
        anyhow::ensure!(
            chapter_comment_page_size > 0,
            "NOVEL_CHAPTER_COMMENT_PAGE_SIZE 必须大于 0"
        );

        Ok(Self {
            home_preview_limit,
            chapter_max_upload_bytes,
            max_title_length,
            max_chapter_title_length,
            chapter_comment_max_length,
            chapter_comment_page_size,
        })
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
        let profile_page_size = parse_optional_env("MEME_PROFILE_PAGE_SIZE")?
            .or(file_config.profile_page_size)
            .unwrap_or(DEFAULT_MEME_PROFILE_PAGE_SIZE);
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
        let approval_reward_enabled = parse_optional_env("MEME_APPROVAL_REWARD_ENABLED")?
            .or(file_config.approval_reward_enabled)
            .unwrap_or(DEFAULT_MEME_APPROVAL_REWARD_ENABLED);
        let approval_reward_amount = parse_optional_env("MEME_APPROVAL_REWARD_AMOUNT")?
            .or(file_config.approval_reward_amount)
            .unwrap_or(DEFAULT_MEME_APPROVAL_REWARD_AMOUNT);

        anyhow::ensure!(max_upload_bytes > 0, "MEME_MAX_UPLOAD_BYTES 必须大于 0");
        anyhow::ensure!(max_dimension > 0, "MEME_MAX_DIMENSION 必须大于 0");
        anyhow::ensure!(max_gif_frames > 0, "MEME_MAX_GIF_FRAMES 必须大于 0");
        anyhow::ensure!(max_decoded_pixels > 0, "MEME_MAX_DECODED_PIXELS 必须大于 0");
        anyhow::ensure!(page_size > 0, "MEME_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(profile_page_size > 0, "MEME_PROFILE_PAGE_SIZE 必须大于 0");
        anyhow::ensure!(home_preview_limit > 0, "MEME_HOME_PREVIEW_LIMIT 必须大于 0");
        anyhow::ensure!(popular_tag_limit > 0, "MEME_POPULAR_TAG_LIMIT 必须大于 0");
        anyhow::ensure!(max_tags_per_meme > 0, "MEME_MAX_TAGS_PER_MEME 必须大于 0");
        anyhow::ensure!(max_tag_length > 0, "MEME_MAX_TAG_LENGTH 必须大于 0");
        anyhow::ensure!(max_title_length > 0, "MEME_MAX_TITLE_LENGTH 必须大于 0");
        anyhow::ensure!(
            approval_reward_amount > 0,
            "MEME_APPROVAL_REWARD_AMOUNT 必须大于 0"
        );

        Ok(Self {
            dir,
            max_upload_bytes,
            max_dimension,
            max_gif_frames,
            max_decoded_pixels,
            page_size,
            profile_page_size,
            home_preview_limit,
            popular_tag_limit,
            max_tags_per_meme,
            max_tag_length,
            max_title_length,
            approval_reward_enabled,
            approval_reward_amount,
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
    novels: Option<NovelFileConfig>,
    updates: Option<UpdatesFileConfig>,
    currency: Option<CurrencyFileConfig>,
    check_in: Option<CheckInFileConfig>,
    shop: Option<ShopFileConfig>,
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
    profile_page_size: Option<i64>,
    home_preview_limit: Option<i64>,
    popular_tag_limit: Option<i64>,
    max_tags_per_meme: Option<usize>,
    max_tag_length: Option<usize>,
    max_title_length: Option<usize>,
    approval_reward_enabled: Option<bool>,
    approval_reward_amount: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct NovelFileConfig {
    home_preview_limit: Option<i64>,
    chapter_max_upload_bytes: Option<usize>,
    max_title_length: Option<usize>,
    max_chapter_title_length: Option<usize>,
    chapter_comment_max_length: Option<usize>,
    chapter_comment_page_size: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdatesFileConfig {
    file: Option<String>,
    home_preview_limit: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct CurrencyFileConfig {
    name: Option<String>,
    symbol: Option<String>,
    log_page_size: Option<i64>,
    admin_recent_log_limit: Option<i64>,
    max_admin_adjust_amount: Option<i64>,
    admin_user_search_limit: Option<i64>,
    max_note_length: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct CheckInFileConfig {
    enabled: Option<bool>,
    reward_amount: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct ShopFileConfig {
    enabled: Option<bool>,
    icon_dir: Option<String>,
    page_size: Option<i64>,
    voucher_page_size: Option<i64>,
    admin_note_max_length: Option<usize>,
    token_lookup_max_attempts: Option<usize>,
    token_lookup_window_seconds: Option<u64>,
    icon_upload_max_bytes: Option<usize>,
    icon_input_max_dimension: Option<u32>,
    icon_max_gif_frames: Option<usize>,
    icon_max_decoded_pixels: Option<u64>,
    icon_max_stored_bytes: Option<usize>,
    icon_resize_dimensions: Option<Vec<u32>>,
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

fn parse_optional_env_list(name: &str) -> Result<Option<Vec<u32>>> {
    match env::var(name) {
        Ok(value) => parse_dimension_list(&value, name).map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("读取 {name} 失败")),
    }
}

fn parse_dimension_list(value: &str, name: &str) -> Result<Vec<u32>> {
    value
        .split(',')
        .map(str::trim)
        .map(|part| {
            part.parse::<u32>()
                .map_err(|error| anyhow::anyhow!("{name} 配置值无效：{error}"))
        })
        .collect()
}

fn validate_resize_dimensions(dimensions: &[u32]) -> Result<()> {
    anyhow::ensure!(
        !dimensions.is_empty(),
        "SHOP_ICON_RESIZE_DIMENSIONS 不能为空"
    );
    anyhow::ensure!(
        dimensions.iter().all(|dimension| *dimension > 0),
        "SHOP_ICON_RESIZE_DIMENSIONS 必须全部大于 0"
    );
    anyhow::ensure!(
        dimensions.windows(2).all(|pair| pair[0] > pair[1]),
        "SHOP_ICON_RESIZE_DIMENSIONS 必须严格降序排列"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shop_config_loads_runtime_icon_limits() {
        let config = ShopConfig::from_sources(Some(ShopFileConfig {
            enabled: Some(false),
            icon_dir: Some("data/test-shop-icons".to_owned()),
            page_size: Some(8),
            voucher_page_size: Some(16),
            admin_note_max_length: Some(120),
            token_lookup_max_attempts: Some(7),
            token_lookup_window_seconds: Some(45),
            icon_upload_max_bytes: Some(5_000_000),
            icon_input_max_dimension: Some(2048),
            icon_max_gif_frames: Some(60),
            icon_max_decoded_pixels: Some(12_000_000),
            icon_max_stored_bytes: Some(700_000),
            icon_resize_dimensions: Some(vec![480, 320, 160]),
        }))
        .unwrap();

        assert!(!config.enabled);
        assert_eq!(config.icon_dir, PathBuf::from("data/test-shop-icons"));
        assert_eq!(config.icon_upload_max_bytes, 5_000_000);
        assert_eq!(config.icon_input_max_dimension, 2048);
        assert_eq!(config.icon_max_gif_frames, 60);
        assert_eq!(config.icon_max_decoded_pixels, 12_000_000);
        assert_eq!(config.icon_max_stored_bytes, 700_000);
        assert_eq!(config.icon_resize_dimensions, vec![480, 320, 160]);
    }

    #[test]
    fn shop_config_defaults_to_runtime_icon_storage_directory() {
        let config = ShopConfig::from_sources(Some(ShopFileConfig::default())).unwrap();

        assert_eq!(config.icon_dir, PathBuf::from("data/shop/product-icons"));
    }

    #[test]
    fn shop_config_rejects_empty_or_non_descending_resize_dimensions() {
        let empty = ShopConfig::from_sources(Some(ShopFileConfig {
            icon_resize_dimensions: Some(Vec::new()),
            ..ShopFileConfig::default()
        }));
        assert!(empty.is_err());

        let non_descending = ShopConfig::from_sources(Some(ShopFileConfig {
            icon_resize_dimensions: Some(vec![256, 512]),
            ..ShopFileConfig::default()
        }));
        assert!(non_descending.is_err());
    }

    #[test]
    fn shop_config_parses_comma_separated_resize_dimensions() {
        assert_eq!(
            parse_dimension_list("512, 384,256", "SHOP_ICON_RESIZE_DIMENSIONS").unwrap(),
            vec![512, 384, 256]
        );
        assert!(parse_dimension_list("", "SHOP_ICON_RESIZE_DIMENSIONS").is_err());
        assert!(parse_dimension_list("512,invalid", "SHOP_ICON_RESIZE_DIMENSIONS").is_err());
    }
}
