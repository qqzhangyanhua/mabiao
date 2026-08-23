//! 一次性读官方额度，输出稳定 JSON，给 agent / 脚本用。
//!
//!   mabiao-quota            # 读缓存，不联网
//!   mabiao-quota --refresh  # 先取一次再输出（受退避约束）
//!
//! 默认只读缓存：额度接口大多限流很紧，脚本轮询理应打我们已经缓存的结果，
//! 而不是每次都去打上游。要新鲜数据才显式 `--refresh`。
//!
//! 读的是应用同一个 sqlite（WAL，只读连接不阻塞正在写的应用）。应用没跑过、
//! 库还不存在时输出空结果而不是报错——脚本不该因为「还没用过」而挂掉。

use mabiao_lib::domain::{OfficialQuotaConfig, OfficialQuotaDto};
use mabiao_lib::{official_quota, paths, store};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("用法: mabiao-quota [--refresh]");
        println!("  不带参数  读本机缓存的官方额度，不联网");
        println!("  --refresh 先取一次再输出（冷却中的账号会被跳过）");
        return;
    }
    let refresh = args.iter().any(|arg| arg == "--refresh");
    if let Some(unknown) = args
        .iter()
        .find(|arg| !matches!(arg.as_str(), "--refresh" | "-h" | "--help"))
    {
        eprintln!("未知参数：{unknown}（可用：--refresh）");
        std::process::exit(2);
    }

    match run(refresh) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run(refresh: bool) -> Result<String, String> {
    let db_path = paths::app_data_dir().join(mabiao_lib::backup::DB_NAME);
    if !db_path.exists() {
        // 还没跑过应用：给一份形状正确的空结果，脚本照样能解析。
        return render(&OfficialQuotaDto {
            rows: Vec::new(),
            alerts_enabled: false,
            stale_after_minutes: official_quota::STALE_AFTER_MINUTES,
            undetected: Vec::new(),
            hidden_providers: Vec::new(),
        });
    }
    let path = db_path.to_string_lossy().to_string();
    let custom_paths = official_quota::custom::store::CustomQuotaPaths::app_data();
    let custom_config = official_quota::custom::store::load_config(&custom_paths.config);

    if refresh {
        // 取数在写之前完成，写完立刻放锁，尽量少打扰正在运行的应用。
        let custom = official_quota::custom::store::load_providers(&custom_paths);
        let results = official_quota::fetch_all_targets(&custom);
        let conn = store::open_db(&path)?;
        official_quota::apply_fetch_results(&conn, results)?;
    }

    let conn = store::open_readonly(&path)?;
    let config =
        official_quota::load_config(&paths::app_data_dir().join(official_quota::CONFIG_NAME));
    render(&official_quota::load_dto(
        &conn,
        &OfficialQuotaConfig {
            alerts_enabled: config.alerts_enabled,
            hidden_providers: config.hidden_providers,
        },
        &custom_config.providers,
        chrono::Utc::now(),
    ))
}

fn render(dto: &OfficialQuotaDto) -> Result<String, String> {
    serde_json::to_string_pretty(dto).map_err(|e| format!("序列化额度失败：{e}"))
}
