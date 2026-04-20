use clap::{Parser, Subcommand};
use picmanager::{album, config::Config, face, storage, importer, dedup};
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
    Dedup {
        /// 重置扫描状态并全库重新扫描（默认为增量扫描）
        #[arg(long)]
        full: bool,
    },
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
    /// 为缺少人脸或地理元数据的照片批量补全（默认两类都补）
    FillMissing {
        /// 仅补充未进行人脸分析的照片
        #[arg(long)]
        faces: bool,
        /// 仅补充有 GPS 但缺地理编码的照片
        #[arg(long)]
        geo: bool,
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
        Command::Dedup { full } => {
            let n = if full {
                dedup::scan_full(&pool).await?
            } else {
                dedup::scan(&pool).await?
            };
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
        Command::FillMissing { faces, geo } => {
            fill_missing(&pool, faces, geo).await?;
        }
    }
    Ok(())
}

async fn fill_missing(
    pool: &sqlx::SqlitePool,
    only_faces: bool,
    only_geo: bool,
) -> anyhow::Result<()> {
    // No flags = fill both
    let fill_faces = only_faces || (!only_faces && !only_geo);
    let fill_geo = only_geo || (!only_faces && !only_geo);

    // ── Phase 1: count pending work ───────────────────────────────────────────
    let face_ids: Vec<i64> = if fill_faces {
        face::job::scope_for_missing(pool).await?
    } else {
        vec![]
    };
    let geo_total: i64 = if fill_geo {
        album::location::count_missing_geo(pool).await?
    } else {
        0
    };

    println!("开始补全缺失元数据…");
    if fill_faces {
        println!("  待补充人脸分析：{} 张", face_ids.len());
    }
    if fill_geo {
        println!("  待补充地理编码：{} 张", geo_total);
    }

    if face_ids.is_empty() && geo_total == 0 {
        println!("无需补全，退出。");
        return Ok(());
    }

    // ── Phase 2: start tasks ──────────────────────────────────────────────────
    let face_job_id: Option<i64> = if fill_faces && !face_ids.is_empty() {
        Some(face::job::run_job(pool, Some(face_ids)).await?)
    } else {
        if fill_faces { println!("  人脸：所有照片已分析，跳过。"); }
        None
    };

    let pool2 = pool.clone();
    let geo_handle: Option<tokio::task::JoinHandle<_>> = if fill_geo && geo_total > 0 {
        Some(tokio::spawn(async move {
            album::group_by_location(&pool2).await
        }))
    } else {
        if fill_geo { println!("  地理：所有 GPS 照片已编码，跳过。"); }
        None
    };

    if face_job_id.is_none() && geo_handle.is_none() {
        println!("无需补全，退出。");
        return Ok(());
    }

    // ── Phase 3: progress loop ────────────────────────────────────────────────
    let start = std::time::Instant::now();
    let print_interval = std::time::Duration::from_secs(60);
    let mut last_print = std::time::Instant::now()
        .checked_sub(print_interval)
        .unwrap_or(std::time::Instant::now());

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let face_done = match face_job_id {
            None => true,
            Some(id) => {
                let status: String =
                    sqlx::query_scalar("SELECT status FROM face_jobs WHERE id = ?")
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .unwrap_or_else(|_| "running".to_string());
                status != "running"
            }
        };

        let geo_done = geo_handle.as_ref().map_or(true, |h| h.is_finished());

        let now = std::time::Instant::now();
        if now.duration_since(last_print) >= print_interval || (face_done && geo_done) {
            last_print = now;
            let elapsed = start.elapsed();
            let mins = elapsed.as_secs() / 60;
            let secs = elapsed.as_secs() % 60;
            let mut parts: Vec<String> = Vec::new();

            if let Some(id) = face_job_id {
                let (processed, total): (i64, Option<i64>) =
                    sqlx::query_as("SELECT processed, total FROM face_jobs WHERE id = ?")
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .unwrap_or((0, None));
                let t = total.unwrap_or(0);
                let pct = if t > 0 { processed * 100 / t } else { 100 };
                parts.push(format!("人脸：{processed}/{t} ({pct}%)"));
            }

            if fill_geo {
                let remaining = album::location::count_missing_geo(pool).await.unwrap_or(0);
                let done = (geo_total - remaining).max(0);
                let pct = if geo_total > 0 { done * 100 / geo_total } else { 100 };
                parts.push(format!("地理：{done}/{geo_total} ({pct}%)"));
            }

            println!("[{:02}:{:02}:{:02}] {}", mins / 60, mins % 60, secs, parts.join(" ｜ "));
        }

        if face_done && geo_done {
            break;
        }
    }

    // ── Phase 4: summary ──────────────────────────────────────────────────────
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    println!(
        "\n补全完成（耗时 {} 分 {} 秒）：",
        total_secs / 60,
        total_secs % 60
    );

    if let Some(id) = face_job_id {
        let processed: i64 =
            sqlx::query_scalar("SELECT processed FROM face_jobs WHERE id = ?")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
        let new_faces: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM faces f \
             JOIN photos p ON p.id = f.photo_id \
             WHERE p.import_status = 'imported'",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        println!("  人脸：分析了 {processed} 张照片，库中共 {new_faces} 个人脸记录");
    }

    if fill_geo && geo_total > 0 {
        let still_missing = album::location::count_missing_geo(pool).await.unwrap_or(0);
        let encoded = (geo_total - still_missing).max(0);
        let failed = still_missing;
        println!(
            "  地理：编码了 {encoded} 个新位置，{failed} 张无城市信息（已跳过），共 {geo_total} 张待处理"
        );
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
