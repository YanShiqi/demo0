use anyhow::Context;
use demo0::{app, config::Config, db, public_messages};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("demo0=debug,tower_http=info")),
        )
        .init();

    let config = Config::from_env()?;
    tokio::fs::create_dir_all(&config.avatar_dir)
        .await
        .context("创建头像目录失败")?;
    let pool = db::connect(&config.database_url).await?;
    let removed_messages = public_messages::cleanup_expired(&pool, &config.messages).await?;
    if removed_messages > 0 {
        info!(removed_messages, "启动时已清理过期公共留言");
    }
    let _message_cleanup_task =
        public_messages::spawn_cleanup_task(pool.clone(), config.messages.clone());
    let app = app::build(pool, config.clone());
    let address = config.socket_address()?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("无法监听 {address}"))?;

    info!(%address, "网站已启动");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .context("服务器异常退出")
}
