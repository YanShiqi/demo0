use std::io::{self, Write};

use anyhow::{Context, bail};
use demo0::{auth, config::Config, db, model::Role};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    tokio::fs::create_dir_all(&config.avatar_dir)
        .await
        .context("创建数据目录失败")?;
    let pool = db::connect(&config.database_url).await?;
    let existing = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE role = ?")
        .bind(Role::SuperAdmin.as_str())
        .fetch_one(&pool)
        .await?;
    if existing > 0 {
        bail!("超级管理员已经存在；为避免意外提权，本命令不会创建第二个");
    }

    let username = prompt("用户名（3～32 位小写字母、数字或下划线）: ")?;
    let nickname = prompt("昵称（1～32 个字符，支持 emoji）: ")?;
    let password =
        rpassword::prompt_password("密码（12～128 个字符）: ").context("读取密码失败")?;
    let confirmation = rpassword::prompt_password("再次输入密码: ").context("读取确认密码失败")?;
    if password != confirmation {
        bail!("两次输入的密码不一致");
    }

    let user = auth::create_user(&pool, &username, &nickname, &password, Role::SuperAdmin).await?;
    println!("已创建超级管理员：{} (@{})", user.nickname, user.username);
    Ok(())
}

fn prompt(message: &str) -> anyhow::Result<String> {
    print!("{message}");
    io::stdout().flush().context("刷新终端输出失败")?;
    let mut value = String::new();
    io::stdin().read_line(&mut value).context("读取输入失败")?;
    Ok(value.trim().to_owned())
}
