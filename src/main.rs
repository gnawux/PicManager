use clap::{Parser, Subcommand};
use picmanager::{config::Config, storage, importer};
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
    /// 启动 Web 服务
    Serve,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::default();

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
        Command::Serve => {
            picmanager::web::serve(pool, config).await?;
        }
    }
    Ok(())
}
