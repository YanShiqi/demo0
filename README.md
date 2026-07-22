# demo0

一个用于学习 Rust Web 开发的休闲网站。当前包含注册、登录、个人资料、公开主页、公共留言板、PNG/JPEG/GIF 头像，以及超级管理员分配管理员身份。

## 本地运行

```bash
cargo run
```

程序启动时会先读取 `config/default.toml`，再读取仓库根目录的 `.env`，系统环境变量可覆盖其中的值。当前 WSL 环境为配合 Windows `portproxy`，使用 `APP_HOST=0.0.0.0` 和端口 `6324`；Windows 浏览器仍访问 <http://127.0.0.1:6324>。

```bash
APP_HOST=127.0.0.1 \
APP_PORT=6324 \
DATABASE_URL='sqlite://data/app.db?mode=rwc' \
AVATAR_DIR=data/avatars \
cargo run
```

常用结构化配置位于 `config/default.toml`。例如公共留言板配置：

```toml
[display]
utc_offset_hours = 8

[messages]
retention_days = 5
limit_per_user = 5
max_length = 300
page_size = 30
home_preview_limit = 5
cleanup_interval_hours = 6
```

留言板入口为 <http://127.0.0.1:6324/messages>。服务启动时会清理一次过期留言，运行中也会按配置周期自动清理。

首次运行后，另开一个终端创建唯一的超级管理员：

```bash
cargo run --bin create_super_admin
```

程序会交互式读取用户名、昵称和密码，不会提供默认管理员密码。公开注册始终创建普通用户。

## WSL 内网访问

没有手动设置 Windows `portproxy` 时，开发阶段建议使用 `APP_HOST=127.0.0.1`，Windows 浏览器通过 `localhost:6324` 访问。当前机器已经把 Windows 端口转发到 WSL 网卡，因此 `.env` 使用 `APP_HOST=0.0.0.0`。这会监听全部 WSL IPv4 地址，请通过 Windows 防火墙限制访问范围。正式 HTTPS 环境应设置 `COOKIE_SECURE=true`。

## 验证

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
