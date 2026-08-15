use clap::{Parser, Subcommand};
use picmanager::{activities, album, application::{Application, CallerKind, ImportCommand}, apple, config::Config, face, metadata, migration, storage, importer, jobs};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

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
        /// 每批最多导入 N 张照片（结合 --log 可随时中断后恢复）
        #[arg(long, value_name = "N")]
        batch_size: Option<usize>,
        /// NDJSON 导入日志路径（记录每张文件的导入结果，重跑时自动跳过已处理文件）
        #[arg(long, value_name = "FILE")]
        log: Option<PathBuf>,
        /// 预览模式：只扫描计数，不实际导入
        #[arg(long)]
        dry_run: bool,
    },
    /// 扫描重复照片（在 Web 界面确认去重操作）
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
    /// 管理运动记录（FIT/GPX 文件）
    Activities {
        #[command(subcommand)]
        action: ActivitiesAction,
    },
    /// 照片元数据管理
    Photos {
        #[command(subcommand)]
        action: PhotosAction,
    },
    /// 检查和迁移现有照片目录
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// 创建、校验和恢复 SQLite 目录备份
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Apple Photos inventory and synchronization
    Apple {
        #[command(subcommand)]
        action: AppleAction,
    },
}

#[derive(Subcommand)]
enum AppleAction {
    /// Ingest a complete metadata-only inventory produced by photobridge
    Inventory {
        file: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Ingest a complete incremental change batch produced by photobridge
    Changes {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify and commit a completed PhotoBridge rendition package
    CommitPackage {
        #[arg(long)]
        source_id: i64,
        package: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum MigrateAction {
    /// 只读检查数据库、照片计数和文件完整性
    Inspect {
        /// 输出 JSON，便于保存迁移前后的基线
        #[arg(long)]
        json: bool,
        /// 跳过逐个照片文件存在性检查
        #[arg(long)]
        skip_files: bool,
    },
    /// 为现有 photos 记录建立兼容的本地来源和文件版本目录
    BackfillLocal {
        /// 只报告将要创建的目录记录，不写数据库
        #[arg(long)]
        dry_run: bool,
        /// 输出 JSON 报告
        #[arg(long)]
        json: bool,
    },
    /// 验证现有照片与新资产目录是否完整一致
    Verify {
        /// 输出 JSON 报告
        #[arg(long)]
        json: bool,
        /// 跳过逐个照片文件存在性检查
        #[arg(long)]
        skip_files: bool,
    },
    /// 显示最近的迁移执行记录
    Report {
        /// 最多显示的记录数
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
    },
    /// 检查目录、媒体文件、暂存写入和派生缓存的一致性
    Reconcile {
        /// 修复可安全重建的目录状态；不会删除原始媒体
        #[arg(long)]
        repair: bool,
        /// 输出 JSON 报告
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum BackupAction {
    /// 在线创建一致的数据库快照
    Create,
    /// 列出当前保留的备份
    List,
    /// 校验备份完整性
    Verify { path: PathBuf },
    /// 恢复到一个尚不存在的新数据库文件
    Restore { backup: PathBuf, target: PathBuf },
}

#[derive(Subcommand)]
enum PhotosAction {
    /// 回填时区偏移：从 EXIF 读取 OffsetTimeOriginal（或 GPS 推断），更新 timezone_offset 为 NULL 的照片
    BackfillTimezones {
        /// 预览模式：只统计数量，不实际修改数据库
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ActivitiesAction {
    /// 导入 FIT/GPX 文件或目录
    Import {
        /// 文件或目录路径
        path: PathBuf,
        /// 预览模式：只扫描计数，不实际导入
        #[arg(long)]
        dry_run: bool,
    },
    /// 从 USB 连接的 Garmin 设备导入新文件
    SyncUsb {
        /// 设备卷名（含此字符串），默认自动检测 /Volumes/ 下含 GARMIN 的卷
        #[arg(long, default_value = "GARMIN")]
        device: String,
    },
    /// 为 title 为空的运动记录生成自动标题（{类型}-{日期}-{距离}@{城市}）
    UpdateTitles {
        /// 预览模式：只统计数量，不实际修改数据库
        #[arg(long)]
        dry_run: bool,
    },
    /// 从已保存的 FIT 文件重新读取设备名称和传感器信息（不改动标题/时间/轨迹点）
    FixMetadata {
        /// 预览模式：只统计数量，不实际修改数据库
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum FacesAction {
    /// 分析照片中的人脸（省略 --photo-ids 则全库重分析）
    Analyze {
        /// 指定照片 ID（逗号分隔），省略则分析全库
        #[arg(long, value_delimiter = ',')]
        photo_ids: Vec<i64>,
        /// 只重分析已旋转/翻转且有人脸记录的照片（修复方向变更后 embedding 失效的情况）
        #[arg(long)]
        rotated_only: bool,
    },
}

#[derive(Subcommand)]
enum ModelsAction {
    /// 下载模型文件到配置目录
    Fetch,
    /// 将配置目录中的模型文件复制到项目 models/ 目录，以便编译进二进制
    Bundle {
        /// 项目根目录（含 models/ 子目录），默认为当前目录
        #[arg(long, default_value = ".")]
        project_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load();

    std::fs::create_dir_all(&config.library_path)?;
    let pool = storage::connect_with_settings(
        &config.db_url(),
        config.database_max_connections,
        std::time::Duration::from_millis(config.database_busy_timeout_ms),
    ).await?;
    let application = Application::new(pool.clone(), config.clone());

    match cli.command {
        Command::Import { dir, copy, batch_size, log, dry_run } => {
            import_with_progress(&application, &dir, copy, batch_size, log.as_deref(), dry_run).await?;
        }
        Command::Dedup { full } => {
            let context = application.request_context(CallerKind::Cli);
            let queued = jobs::handlers::enqueue_dedup_scan(&application, &context, full).await?;
            let worker = jobs::WorkerRuntime::new(
                pool.clone(), jobs::handlers::registry(application.clone()),
                jobs::WorkerConfig::default(), "cli-dedup-worker",
            ).start();
            let completed = loop {
                let job = jobs::get(&pool, queued.job.id).await?;
                if matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled") {
                    break job;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            };
            worker.shutdown().await;
            if completed.status != "succeeded" {
                anyhow::bail!(completed.error_message.unwrap_or_else(|| "去重扫描失败".into()));
            }
            let n = completed.result()?.and_then(|value| value["groups_created"].as_u64()).unwrap_or(0);
            println!("扫描完成，发现 {n} 个新重复组");

            let groups = application.dedup().list(&context).await?;
            if groups.is_empty() {
                println!("没有待确认的重复组，无需操作");
            } else {
                println!("共 {} 个待确认的重复组，请启动 Web 界面（picmanager serve）确认", groups.len());
            }
        }
        Command::Config => {
            println!("library_path : {}", config.library_path.display());
            println!("db_path      : {}", config.db_path.display());
            println!("host         : {}", config.host);
            println!("port         : {}", config.port);
            println!("thumb_size   : {}", config.thumb_size);
            println!("db_pool_size : {}", config.database_max_connections);
            println!("db_busy_ms   : {}", config.database_busy_timeout_ms);
            println!("backup_keep  : {}", config.backup_retention);
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
            FacesAction::Analyze { photo_ids, rotated_only } => {
                let scope = if rotated_only {
                    let ids = face::job::scope_for_rotated_with_faces(&pool).await?;
                    println!("找到 {} 张旋转后未重分析的照片", ids.len());
                    if ids.is_empty() {
                        println!("无需重分析，退出。");
                        return Ok(());
                    }
                    Some(ids)
                } else if photo_ids.is_empty() {
                    None
                } else {
                    Some(photo_ids)
                };
                let context = application.request_context(CallerKind::Cli);
                let queued = jobs::handlers::enqueue_face_analysis(&application, &context, scope).await?;
                let job_id = queued.job.id;
                let worker = jobs::WorkerRuntime::new(
                    pool.clone(), jobs::handlers::registry(application.clone()),
                    jobs::WorkerConfig::default(), "cli-analysis-worker",
                ).start();
                println!("人脸分析任务已启动（job_id={job_id}），等待完成…");
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let job = jobs::get(&pool, job_id).await?;
                    if matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled") {
                        println!("任务 {job_id} 完成：{}", job.status);
                        break;
                    }
                }
                worker.shutdown().await;
            }
        },
        Command::Models { action } => match action {
            ModelsAction::Fetch => {
                fetch_models(&config).await?;
            }
            ModelsAction::Bundle { project_dir } => {
                bundle_models(&project_dir).await?;
            }
        },
        Command::FillMissing { faces, geo } => {
            fill_missing(&application, faces, geo).await?;
        }
        Command::Activities { action } => match action {
            ActivitiesAction::Import { path, dry_run } => {
                import_activities(&pool, &path, &config.activities_dir(), dry_run).await?;
            }
            ActivitiesAction::SyncUsb { device } => {
                sync_usb_activities(&pool, &device, &config.activities_dir()).await?;
            }
            ActivitiesAction::UpdateTitles { dry_run } => {
                let (updated, skipped) = activities::update_titles(&pool, dry_run).await;
                let label = if dry_run { "[dry-run] " } else { "" };
                println!("{label}完成：{}标题 {updated}，无法生成（缺日期）{skipped}",
                    if dry_run { "可生成" } else { "已生成" });
            }
            ActivitiesAction::FixMetadata { dry_run } => {
                let (fixed, skipped, failed) = activities::fix_metadata(&pool, dry_run).await;
                let label = if dry_run { "[dry-run] " } else { "" };
                println!("{label}完成：已修复 {fixed}，跳过（文件不存在/无法匹配）{skipped}，解析失败 {failed}");
            }
        },
        Command::Photos { action } => match action {
            PhotosAction::BackfillTimezones { dry_run } => {
                backfill_timezones(&pool, dry_run).await?;
            }
        },
        Command::Migrate { action } => match action {
            MigrateAction::Inspect { json, skip_files } => {
                let report = migration::inspect(&pool, !skip_files).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("schema version       : {}", report.schema_version);
                    println!("photos               : {}", report.total_photos);
                    for (status, count) in &report.status_counts {
                        println!("  {status:<18} : {count}");
                    }
                    println!(
                        "active counter        : {} (actual {}, {})",
                        report.recorded_active_photos,
                        report.active_photos,
                        if report.active_count_matches { "ok" } else { "MISMATCH" },
                    );
                    println!("duplicate SHA groups : {}", report.duplicate_sha_groups);
                    println!("foreign key errors   : {}", report.foreign_key_violations);
                    println!("SQLite integrity     : {}", report.sqlite_integrity);
                    if report.checked_files {
                        println!("missing files        : {}", report.missing_files);
                        for path in &report.missing_file_samples {
                            println!("  missing: {path}");
                        }
                    } else {
                        println!("missing files        : not checked");
                    }
                    println!("result               : {}", if report.is_healthy() { "healthy" } else { "issues found" });
                }
                if !report.is_healthy() {
                    std::process::exit(2);
                }
            }
            MigrateAction::BackfillLocal { dry_run, json } => {
                let report = migration::backfill_legacy_local(&pool, dry_run).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("legacy photos       : {}", report.total_photos);
                    println!("existing assets     : {}", report.existing_assets);
                    println!("existing sources    : {}", report.existing_sources);
                    println!("existing variants   : {}", report.existing_variants);
                    if dry_run {
                        println!("mode                : dry-run (no changes written)");
                        println!("assets to create    : {}", report.total_photos - report.existing_assets);
                    } else {
                        println!("created assets      : {}", report.created_assets);
                        println!("created sources     : {}", report.created_sources);
                        println!("created variants    : {}", report.created_variants);
                        println!("created links       : {}", report.created_links);
                        println!("migration run       : {}", report.migration_run_id.unwrap_or_default());
                    }
                }
            }
            MigrateAction::Verify { json, skip_files } => {
                let report = migration::verify_catalog(&pool, !skip_files).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("catalog assets          : {}", report.total_assets);
                    println!("linked assets           : {}", report.linked_assets);
                    println!("photos without assets   : {}", report.photos_without_assets);
                    println!("assets without photos   : {}", report.assets_without_photos);
                    println!("legacy sources          : {}", report.legacy_sources);
                    println!("photos without sources  : {}", report.photos_without_legacy_sources);
                    println!("orphan legacy sources   : {}", report.legacy_sources_without_assets);
                    println!("variants                : {}", report.total_variants);
                    println!("assets without primary  : {}", report.assets_without_primary_variants);
                    println!("conflicting links       : {}", report.conflicting_links);
                    println!("catalog result          : {}", if report.is_healthy() { "healthy" } else { "issues found" });
                }
                if !report.is_healthy() {
                    std::process::exit(2);
                }
            }
            MigrateAction::Report { limit, json } => {
                let runs = migration::list_migration_runs(&pool, limit).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&runs)?);
                } else if runs.is_empty() {
                    println!("no migration runs recorded");
                } else {
                    for run in runs {
                        println!(
                            "#{:<5} {:<28} {:<10} {}",
                            run.id,
                            run.kind,
                            run.status,
                            run.finished_at.as_deref().or(run.started_at.as_deref()).unwrap_or(&run.created_at),
                        );
                        if let Some(error) = run.error {
                            println!("        error: {error}");
                        }
                    }
                }
            }
            MigrateAction::Reconcile { repair, json } => {
                let report = storage::reconcile(&pool, &config, repair).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("missing photo files       : {}", report.missing_photo_files);
                    println!("missing variant files     : {}", report.missing_variant_files);
                    println!("invalid master pointers   : {}", report.invalid_master_pointers);
                    println!("invalid display pointers  : {}", report.invalid_display_pointers);
                    println!("incomplete file intents   : {}", report.incomplete_filesystem_intents);
                    println!("missing ready thumbnails  : {}", report.missing_ready_thumbnails);
                    println!("stale cache files         : {}", report.stale_cache_files);
                    println!("repaired records          : {}", report.repaired_records);
                    println!("removed cache files       : {}", report.removed_cache_files);
                }
            }
        },
        Command::Backup { action } => match action {
            BackupAction::Create => {
                let report = storage::create_backup(
                    &pool,
                    &config.backup_dir(),
                    config.backup_retention as usize,
                ).await?;
                println!("备份完成：{}（{} bytes，完整性 {}）", report.path.display(), report.bytes, report.integrity);
            }
            BackupAction::List => {
                for path in storage::list_backups(&config.backup_dir())? {
                    println!("{}", path.display());
                }
            }
            BackupAction::Verify { path } => {
                let report = storage::verify_backup(&path).await?;
                println!("备份有效：{}（{} bytes）", report.path.display(), report.bytes);
            }
            BackupAction::Restore { backup, target } => {
                let report = storage::restore_backup(&backup, &target).await?;
                println!("恢复完成：{}（完整性 {}）", report.path.display(), report.integrity);
            }
        },
        Command::Apple { action } => match action {
            AppleAction::Inventory { file, dry_run, json } => {
                let report = apple::ingest_inventory(&pool, &file, dry_run).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("Photos assets      : {}", report.total_assets);
                    println!("exact legacy links : {}", report.exact_links);
                    println!("ambiguous links    : {}", report.ambiguous_links);
                    println!("review candidates  : {}", report.review_candidates);
                    println!("queued for export  : {}", report.queued_assets);
                    println!("policy excluded    : {}", report.excluded_assets);
                    println!("now missing        : {}", report.missing_assets);
                    println!("mode               : {}", if dry_run { "dry-run" } else { "committed" });
                }
            }
            AppleAction::Changes { file, json } => {
                let report = apple::ingest_changes(&pool, &file).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("changed assets     : {}", report.changed_assets);
                    println!("removed assets     : {}", report.removed_assets);
                    println!("exact legacy links : {}", report.exact_links);
                    println!("review candidates  : {}", report.review_candidates);
                    println!("queued work        : {}", report.queued_assets);
                    println!("sync job           : {}", report.job_id);
                }
            }
            AppleAction::CommitPackage { source_id, package, json } => {
                let result = apple::commit_rendition_package(&pool, source_id, &package).await?;
                if result.derived_invalidated {
                    let context = application.request_context(CallerKind::Cli);
                    let correlation = context.request_id.to_string();
                    jobs::handlers::enqueue_derived_maintenance(
                        &application, &context, Some(vec![result.photo_id]),
                    ).await?;
                    let worker = jobs::WorkerRuntime::new(
                        pool.clone(), jobs::handlers::registry(application.clone()),
                        jobs::WorkerConfig::default(), "cli-derived-worker",
                    ).start();
                    loop {
                        let active = jobs::list(&pool, None, None, None, 500).await?
                            .into_iter()
                            .any(|job| job.correlation_id.as_deref() == Some(&correlation)
                                && matches!(job.status.as_str(), "queued" | "running" | "retry_wait"));
                        if !active { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    worker.shutdown().await;
                }
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("source             : {}", result.source_id);
                    println!("photo              : {}", result.photo_id);
                    println!("asset              : {}", result.asset_id);
                    println!("original variant   : {}", result.original_variant_id);
                    println!("current variant    : {}", result.current_variant_id.map(|id| id.to_string()).unwrap_or_else(|| "none".into()));
                    println!("master variant     : {}", result.master_variant_id);
                    println!("display variant    : {}", result.display_variant_id);
                    println!("display revision   : {}", result.display_revision);
                }
            }
        },
    }
    Ok(())
}

async fn import_with_progress(
    application: &Application,
    dir: &std::path::Path,
    copy: bool,
    batch_size: Option<usize>,
    log_path: Option<&std::path::Path>,
    dry_run: bool,
) -> anyhow::Result<()> {
    println!("从 {} 导入照片，扫描中…", dir.display());

    let progress = importer::SharedImportProgress::default();
    let context = application.request_context(CallerKind::Cli);
    let progress2 = progress.clone();
    let command = ImportCommand {
        source_dir: dir.to_path_buf(), copy_only: copy, batch_size,
        log_path: log_path.map(|path| path.to_path_buf()), dry_run,
    };
    let queued = application.imports().enqueue(&context, command).await?;
    let pool = application.pool().clone();
    let worker = jobs::WorkerRuntime::new(
        pool.clone(),
        jobs::handlers::registry(application.clone()),
        jobs::WorkerConfig::default(),
        "cli-worker",
    ).start();
    let job_id = queued.job.id;
    let handle = tokio::spawn(async move {
        let result = loop {
            let job = jobs::get(&pool, job_id).await?;
            progress2.total.store(job.progress_total.unwrap_or(0).max(0) as usize, Relaxed);
            progress2.processed.store(job.progress_completed.max(0) as usize, Relaxed);
            match job.status.as_str() {
                "succeeded" => {
                    let value = job.result()?.ok_or_else(|| anyhow::anyhow!("导入任务缺少结果摘要"))?;
                    let summary: jobs::handlers::ImportJobResult = serde_json::from_value(value)?;
                    break Ok(importer::BatchResult {
                        summary: importer::ImportSummary {
                            total: summary.total,
                            imported: summary.imported,
                            skipped: summary.skipped,
                            errors: summary.errors,
                        },
                        total_files: summary.total_files,
                        remaining: summary.remaining,
                    });
                }
                "failed" => break Err(anyhow::anyhow!(job.error_message.unwrap_or_else(|| "导入任务失败".into()))),
                "cancelled" => break Err(anyhow::anyhow!("导入任务已取消")),
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        };
        if !worker.shutdown().await {
            tracing::warn!("CLI import worker did not stop within the shutdown timeout");
        }
        result
    });

    let start = std::time::Instant::now();
    let print_interval = std::time::Duration::from_secs(60);
    let mut last_print = std::time::Instant::now()
        .checked_sub(print_interval)
        .unwrap_or(std::time::Instant::now());

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let done = handle.is_finished();

        let now = std::time::Instant::now();
        if now.duration_since(last_print) >= print_interval || done {
            last_print = now;
            let total = progress.total.load(Relaxed);
            let processed = progress.processed.load(Relaxed);
            let faces = progress.faces_found.load(Relaxed);
            let gps_found = progress.gps_found.load(Relaxed);
            let geo_total = progress.geo_total.load(Relaxed);
            let geo_done = progress.geo_done.load(Relaxed);
            let geo_cache_hits = progress.geo_cache_hits.load(Relaxed);
            let geo_failed = progress.geo_failed.load(Relaxed);

            if total > 0 {
                let elapsed = start.elapsed();
                let mins = elapsed.as_secs() / 60;
                let secs = elapsed.as_secs() % 60;
                let import_pct = processed * 100 / total;

                let geo_str = if processed < total {
                    if gps_found > 0 {
                        format!("等待中（{}张含GPS）", gps_found)
                    } else {
                        "等待中".to_string()
                    }
                } else if geo_total == 0 && !done {
                    // Import done but geo phase hasn't written geo_total yet.
                    if gps_found > 0 {
                        format!("地理编码中（{}张）…", gps_found)
                    } else {
                        "地理编码中…".to_string()
                    }
                } else if geo_total == 0 {
                    "无 GPS".to_string()
                } else {
                    let remaining = geo_total.saturating_sub(geo_done);
                    format!(
                        "共{}张GPS，已查询{}张，缓存命中{}张，失败{}张，编码中{}张",
                        geo_total, geo_done, geo_cache_hits, geo_failed, remaining,
                    )
                };

                println!(
                    "[{:02}:{:02}:{:02}] 导入：{}/{} ({}%) ｜ 人脸：{} 张 ｜ 地理：{}",
                    mins / 60, mins % 60, secs,
                    processed, total, import_pct,
                    faces,
                    geo_str,
                );
            }
        }

        if done {
            break;
        }
    }

    let batch_result = handle.await??;
    if dry_run {
        println!(
            "[dry-run] 目录 {} 个文件，将处理 {} 个（本批），剩余 {} 个待处理",
            batch_result.total_files,
            batch_result.summary.total,
            batch_result.remaining,
        );
        return Ok(());
    }
    let elapsed = start.elapsed();
    let remaining_note = if batch_result.remaining > 0 {
        format!("（剩余 {} 张未处理）", batch_result.remaining)
    } else {
        String::new()
    };
    println!(
        "\n完成（耗时 {} 分 {} 秒）：共 {} 张，导入 {}，跳过 {}，失败 {}{}",
        elapsed.as_secs() / 60,
        elapsed.as_secs() % 60,
        batch_result.summary.total,
        batch_result.summary.imported,
        batch_result.summary.skipped,
        batch_result.summary.errors,
        remaining_note,
    );
    Ok(())
}

async fn fill_missing(
    application: &Application,
    only_faces: bool,
    only_geo: bool,
) -> anyhow::Result<()> {
    let pool = application.pool();
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
    let context = application.request_context(CallerKind::Cli);
    let face_job_id: Option<i64> = if fill_faces && !face_ids.is_empty() {
        Some(jobs::handlers::enqueue_face_analysis(application, &context, Some(face_ids)).await?.job.id)
    } else {
        if fill_faces { println!("  人脸：所有照片已分析，跳过。"); }
        None
    };

    let geo_job_id: Option<i64> = if fill_geo && geo_total > 0 {
        Some(jobs::handlers::enqueue_geocode(application, &context).await?.job.id)
    } else {
        if fill_geo { println!("  地理：所有 GPS 照片已编码，跳过。"); }
        None
    };

    if face_job_id.is_none() && geo_job_id.is_none() {
        println!("无需补全，退出。");
        return Ok(());
    }
    let worker = jobs::WorkerRuntime::new(
        pool.clone(), jobs::handlers::registry(application.clone()),
        jobs::WorkerConfig::default(), "cli-fill-worker",
    ).start();

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
                let status = jobs::get(pool, id).await.map(|job| job.status).unwrap_or_else(|_| "running".into());
                matches!(status.as_str(), "succeeded" | "failed" | "cancelled")
            }
        };

        let geo_done = match geo_job_id {
            None => true,
            Some(id) => jobs::get(pool, id).await
                .map(|job| matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled"))
                .unwrap_or(false),
        };

        let now = std::time::Instant::now();
        if now.duration_since(last_print) >= print_interval || (face_done && geo_done) {
            last_print = now;
            let elapsed = start.elapsed();
            let mins = elapsed.as_secs() / 60;
            let secs = elapsed.as_secs() % 60;
            let mut parts: Vec<String> = Vec::new();

            if let Some(id) = face_job_id {
                let job = jobs::get(pool, id).await.ok();
                let processed = job.as_ref().map_or(0, |job| job.progress_completed);
                let t = job.and_then(|job| job.progress_total).unwrap_or(0);
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
        let processed = jobs::get(pool, id).await.map_or(0, |job| job.progress_completed);
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

    worker.shutdown().await;

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
            "https://github.com/yakhyo/face-reidentification/releases/download/v0.0.1/w600k_mbf.onnx",
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

async fn import_activities(
    pool: &sqlx::SqlitePool,
    path: &std::path::Path,
    activities_dir: &std::path::Path,
    dry_run: bool,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(activities_dir)?;

    if path.is_file() {
        if dry_run {
            println!("[dry-run] 将导入 1 个文件");
            return Ok(());
        }
        match activities::import_one(pool, path, activities_dir).await? {
            activities::ImportOutcome::Imported(id) => println!("已导入（id={id}）"),
            activities::ImportOutcome::Skipped => println!("跳过（已存在相同文件）"),
        }
    } else {
        let summary = activities::import_dir_activities(pool, path, activities_dir, dry_run).await;
        if dry_run {
            println!("[dry-run] 目录含 {} 个运动文件", summary.total);
        } else {
            println!(
                "共 {} 个文件，导入 {}，跳过（重复）{}，失败 {}",
                summary.total, summary.imported, summary.skipped, summary.failed,
            );
        }
    }
    Ok(())
}

async fn sync_usb_activities(
    pool: &sqlx::SqlitePool,
    device_name: &str,
    activities_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let volumes = std::path::Path::new("/Volumes");
    if !volumes.exists() {
        anyhow::bail!("未找到 /Volumes 目录（需在 macOS 上运行）");
    }

    let garmin_vol = std::fs::read_dir(volumes)?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .to_uppercase()
                .contains(&device_name.to_uppercase())
        })
        .map(|e| e.path());

    let vol = garmin_vol.ok_or_else(|| {
        anyhow::anyhow!("未检测到含 '{device_name}' 的 USB 设备，请确认设备已连接")
    })?;

    let activity_src = vol.join("GARMIN").join("Activity");
    if !activity_src.exists() {
        anyhow::bail!("未找到 Garmin Activity 目录：{}", activity_src.display());
    }

    println!("从 {} 导入 Garmin 运动文件…", activity_src.display());
    import_activities(pool, &activity_src, activities_dir, false).await
}

async fn bundle_models(project_dir: &std::path::Path) -> anyhow::Result<()> {
    let src = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("picmanager/models");
    let dst = project_dir.join("models");
    std::fs::create_dir_all(&dst)?;

    let model_files = ["face_detector.onnx", "arcface_mobilenetv1.onnx", "yolov8n.onnx"];
    let mut copied = 0usize;
    for name in &model_files {
        let src_file = src.join(name);
        if src_file.exists() {
            let dst_file = dst.join(name);
            std::fs::copy(&src_file, &dst_file)?;
            println!("复制 {name} → {}", dst_file.display());
            copied += 1;
        } else {
            println!("跳过 {name}（未找到，请先运行 models fetch）");
        }
    }
    if copied > 0 {
        println!("\n已复制 {copied} 个模型文件到 {}。", dst.display());
        println!("重新编译（cargo build --release）后，模型将内置于二进制文件中。");
    } else {
        println!("\n未复制任何文件。请先运行 `picmanager models fetch` 下载模型。");
    }
    Ok(())
}

async fn backfill_timezones(pool: &sqlx::SqlitePool, dry_run: bool) -> anyhow::Result<()> {
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM photos WHERE timezone_offset IS NULL AND import_status = 'imported'",
    )
    .fetch_one(pool)
    .await?;
    println!(
        "{}共 {} 张照片时区未知，{}读取 EXIF…",
        if dry_run { "[dry-run] " } else { "" },
        total.0,
        if dry_run { "预览" } else { "开始" },
    );

    let (updated, no_tz, missing) = metadata::backfill_timezones(pool, dry_run).await?;

    let label = if dry_run { "[dry-run] " } else { "" };
    println!(
        "{label}完成：{}时区 {updated}，无 EXIF 时区信息 {no_tz}，文件不存在 {missing}",
        if dry_run { "可更新" } else { "已更新" },
    );
    Ok(())
}
