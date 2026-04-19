use clap::{Parser, Subcommand};
use picmanager::{config::Config, face, storage, importer, dedup};
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
        /// 复制文件（保留源文件），不移动
        #[arg(long)]
        copy: bool,
    },
    /// 扫描重复照片并交互式确认
    Dedup,
    /// 启动 Web 服务
    Serve,
    /// 显示当前生效配置
    Config,
    /// 人脸检测与特征提取
    Faces {
        #[command(subcommand)]
        action: FacesAction,
    },
    /// 管理模型文件
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
}

#[derive(Subcommand)]
enum FacesAction {
    /// 分析照片中的人脸（省略 --photo-ids 则全库重分析）
    Analyze {
        /// 指定照片 ID（逗号分隔），省略则分析全库
        #[arg(long, value_delimiter = ',')]
        photo_ids: Vec<i64>,
    },
}

#[derive(Subcommand)]
enum ModelsAction {
    /// 下载模型文件到配置目录
    Fetch,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load();

    std::fs::create_dir_all(&config.library_path)?;
    let pool = storage::connect(&config.db_url()).await?;

    match cli.command {
        Command::Import { dir, copy } => {
            println!("从 {} 导入照片...", dir.display());
            let summary = importer::import_dir(&pool, &dir, &config.library_path, copy).await?;
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
        Command::Faces { action } => match action {
            FacesAction::Analyze { photo_ids } => {
                let scope = if photo_ids.is_empty() { None } else { Some(photo_ids) };
                let job_id = face::job::run_job(&pool, scope).await?;
                println!("人脸分析任务已启动（job_id={job_id}），等待完成…");
                // Poll until done
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let status: String =
                        sqlx::query_scalar("SELECT status FROM face_jobs WHERE id = ?")
                            .bind(job_id)
                            .fetch_one(&pool)
                            .await?;
                    if status != "running" {
                        println!("任务 {job_id} 完成：{status}");
                        break;
                    }
                }
            }
        },
        Command::Models { action } => match action {
            ModelsAction::Fetch => {
                fetch_models(&config).await?;
            }
        },
    }
    Ok(())
}

async fn fetch_models(config: &Config) -> anyhow::Result<()> {
    let models_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("picmanager/models");
    std::fs::create_dir_all(&models_dir)?;

    let downloads: &[(&str, &str)] = &[
        (
            "face_detector.onnx",
            "https://github.com/Linzaer/Ultra-Light-Fast-Generic-Face-Detector-1MB/raw/master/models/onnx/version-slim-320.onnx",
        ),
        (
            "arcface_mobilenetv1.onnx",
            "https://github.com/deepinsight/insightface/releases/download/v0.7/w600k_mbf.onnx",
        ),
    ];

    let client = reqwest::Client::new();
    for (filename, url) in downloads {
        let dest = models_dir.join(filename);
        if dest.exists() {
            println!("{filename} 已存在，跳过");
            continue;
        }
        println!("下载 {filename}…");
        let bytes = client.get(*url).send().await?.bytes().await?;
        std::fs::write(&dest, &bytes)?;
        println!("  → {} ({} KB)", dest.display(), bytes.len() / 1024);
    }
    let _ = config; // library path not used here
    Ok(())
}
