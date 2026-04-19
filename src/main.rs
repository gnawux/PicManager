use clap::{Parser, Subcommand};
use picmanager::{config::Config, storage, importer, dedup};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "picmanager", version, about = "家庭照片管理工具")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 从指定目录导入照片
    Import {
        /// 源目录路径
        dir: PathBuf,
    },
    /// 扫描重复照片并交互式确认
    Dedup,
    /// 启动 Web 服务
    Serve,
    /// 显示当前生效配置
    Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load();

    std::fs::create_dir_all(&config.library_path)?;
    let pool = storage::connect(&config.db_url()).await?;

    match cli.command {
        Command::Import { dir } => {
            println!("从 {} 导入照片...", dir.display());
            let summary = importer::import_dir(&pool, &dir).await?;
            println!(
                "完成：共 {} 张，导入 {}，跳过 {}，失败 {}",
                summary.total, summary.imported, summary.skipped, summary.errors
            );
        }
        Command::Dedup => {
            let n = dedup::scan(&pool).await?;
            println!("扫描完成，发现 {n} 个新重复组");

            let groups = dedup::list_groups(&pool).await?;
            if groups.is_empty() {
                println!("没有待确认的重复组");
            }
            for group in &groups {
                println!("\n--- 重复组 {} ---", group.group_id);
                for m in &group.members {
                    println!("  [{}] {}", m.photo_id, m.path);
                    if let Some(t) = &m.taken_at { println!("       拍摄时间: {t}"); }
                    if let Some(c) = &m.camera   { println!("       相机: {c}"); }
                }
                print!("保留哪张（输入 photo_id，多个用逗号分隔，s=跳过）: ");
                use std::io::{self, Write};
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let input = input.trim();
                if input == "s" || input.is_empty() { continue; }
                let keep_ids: Vec<i64> = input.split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                dedup::resolve(&pool, group.group_id, &keep_ids).await?;
                println!("已确认");
            }
        }
        Command::Config => {
            println!("library_path : {}", config.library_path.display());
            println!("db_path      : {}", config.db_path.display());
            println!("host         : {}", config.host);
            println!("port         : {}", config.port);
            println!("thumb_size   : {}", config.thumb_size);
            let cfg_file = dirs::config_dir()
                .map(|p| p.join("picmanager/config.toml").display().to_string())
                .unwrap_or_else(|| "(unknown)".to_string());
            println!("config file  : {cfg_file}");
            return Ok(());
        }
        Command::Serve => {
            picmanager::web::serve(pool, config).await?;
        }
    }
    Ok(())
}
