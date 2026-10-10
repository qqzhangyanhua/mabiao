use std::io::{IsTerminal, Read};

use clap::{Parser, Subcommand};
use mabiao_server::{accounts, db, router, router_with_web, AppState};
use push_protocol::RemoteRole;

#[derive(Parser)]
#[command(name = "mabiao-server", about = "码表远程服务")]
struct Cli {
    /// PostgreSQL 连接串。clap 不许 global 参数同时 required，所以在 `run` 里再检查。
    #[arg(long, env = "DATABASE_URL", global = true, hide_env_values = true)]
    database_url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 跑迁移并启动 HTTP 服务。
    Serve {
        #[arg(long, env = "MABIAO_BIND", default_value = "0.0.0.0:8080")]
        bind: String,
        /// 管理网页构建产物（`server/web/dist`）所在目录。不设则只提供 API。
        #[arg(long, env = "MABIAO_WEB_DIR")]
        web_dir: Option<std::path::PathBuf>,
    },
    /// 创建管理员账号。唯一的开户入口之一，没有注册接口。
    /// 密码读环境变量 MABIAO_ADMIN_PASSWORD；没有就在终端提示输入，或从 stdin 读一行。
    CreateAdmin {
        #[arg(long)]
        account: String,
    },
}

fn read_password() -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(password) = std::env::var("MABIAO_ADMIN_PASSWORD") {
        return Ok(password);
    }
    if std::io::stdin().is_terminal() {
        let first = rpassword::prompt_password("管理员密码: ")?;
        let second = rpassword::prompt_password("再输一遍: ")?;
        if first != second {
            return Err("两次输入不一致".into());
        }
        return Ok(first);
    }
    let mut line = String::new();
    std::io::stdin().read_to_string(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// 过期 token 不影响鉴权（查询里带了 `expires_at > now()`），只是占行，定期清掉即可。
async fn purge_expired_tokens_hourly(pool: sqlx::PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        interval.tick().await;
        match mabiao_server::tokens::purge_expired(&pool).await {
            Ok(removed) if removed > 0 => tracing::info!(removed, "已清理过期 token"),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "清理过期 token 失败"),
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let database_url = cli
        .database_url
        .ok_or("缺少数据库连接串：设置环境变量 DATABASE_URL 或传 --database-url")?;
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;
    match cli.command {
        Command::Serve { bind, web_dir } => {
            match mabiao_server::team_pricing::backfill_missing(&pool).await {
                Ok(filled) if filled > 0 => tracing::info!(filled, "已补算统一费用"),
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "补算统一费用失败，可调用重算接口重试"),
            }
            tokio::spawn(purge_expired_tokens_hourly(pool.clone()));
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            tracing::info!(%bind, "mabiao-server 已启动");
            let state = AppState { pool };
            let app = match web_dir {
                Some(dir) => {
                    if !dir.join("index.html").is_file() {
                        return Err(format!("网页目录里没有 index.html：{}", dir.display()).into());
                    }
                    tracing::info!(dir = %dir.display(), "托管管理网页");
                    router_with_web(state, dir)
                }
                None => router(state),
            };
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
        Command::CreateAdmin { account } => {
            let password = read_password()?;
            let row = accounts::create(&pool, &account, &password, RemoteRole::Admin).await?;
            println!("已创建管理员 {}（id {}）", row.account, row.id);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("错误：{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn subcommands_parse_with_database_url_after_them() {
        let cli = Cli::try_parse_from([
            "mabiao-server",
            "create-admin",
            "--account",
            "root",
            "--database-url",
            "postgres://x",
        ])
        .unwrap();
        assert_eq!(cli.database_url.as_deref(), Some("postgres://x"));
        assert!(matches!(cli.command, Command::CreateAdmin { .. }));
    }
}
