use tokio_cron_scheduler::{Job, JobScheduler};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{info, error};
use crate::scheduler::jobs::*;
use crate::scheduler::monitoring::JobMonitor;

pub struct SchedulerManager {
    scheduler: JobScheduler,
    monitor: Arc<JobMonitor>,
}

impl SchedulerManager {
    pub async fn new(db_pool: PgPool) -> Result<Self, Box<dyn std::error::Error>> {
        let scheduler = JobScheduler::new().await?;
        let monitor = Arc::new(JobMonitor::new(db_pool.clone()));
        
        Ok(Self {
            scheduler,
            monitor,
        })
    }

    pub async fn start(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting scheduler with jobs");

        // Daily tip summary - runs at 1 AM daily
        self.add_daily_summary_job(db_pool.clone()).await?;
        
        // Weekly creator reports - runs Sunday at 2 AM
        self.add_weekly_report_job(db_pool.clone()).await?;
        
        // Database cleanup - runs daily at 3 AM
        self.add_cleanup_job(db_pool.clone()).await?;
        
        // Cache warming - runs every 6 hours
        self.add_cache_warming_job(db_pool.clone()).await?;
        
        // Analytics aggregation - runs hourly
        self.add_analytics_job(db_pool.clone()).await?;

        // Scheduled database backup - runs daily at 2 AM
        self.add_backup_job(db_pool.clone()).await?;

        // Key rotation - runs weekly on Sunday at 3 AM
        self.add_key_rotation_job(db_pool.clone()).await?;

        self.scheduler.start().await?;
        info!("Scheduler started successfully");
        
        Ok(())
    }

    async fn add_daily_summary_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();
        
        let job = Job::new_async("0 0 1 * * *", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running daily summary job");
                mon.record_start("daily_summary").await;
                
                match generate_daily_summary(&pool).await {
                    Ok(_) => {
                        info!("Daily summary completed successfully");
                        mon.record_success("daily_summary").await;
                    }
                    Err(e) => {
                        error!("Daily summary failed: {}", e);
                        mon.record_failure("daily_summary", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }

    async fn add_weekly_report_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();
        
        let job = Job::new_async("0 0 2 * * 0", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running weekly report job");
                mon.record_start("weekly_report").await;
                
                match generate_weekly_report(&pool).await {
                    Ok(_) => {
                        info!("Weekly report completed successfully");
                        mon.record_success("weekly_report").await;
                    }
                    Err(e) => {
                        error!("Weekly report failed: {}", e);
                        mon.record_failure("weekly_report", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }

    async fn add_cleanup_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();
        
        let job = Job::new_async("0 0 3 * * *", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running cleanup job");
                mon.record_start("cleanup").await;
                
                match cleanup_old_data(&pool).await {
                    Ok(_) => {
                        info!("Cleanup completed successfully");
                        mon.record_success("cleanup").await;
                    }
                    Err(e) => {
                        error!("Cleanup failed: {}", e);
                        mon.record_failure("cleanup", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }

    async fn add_cache_warming_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();
        
        let job = Job::new_async("0 0 */6 * * *", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running cache warming job");
                mon.record_start("cache_warming").await;
                
                match warm_cache(&pool).await {
                    Ok(_) => {
                        info!("Cache warming completed successfully");
                        mon.record_success("cache_warming").await;
                    }
                    Err(e) => {
                        error!("Cache warming failed: {}", e);
                        mon.record_failure("cache_warming", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }

    async fn add_analytics_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();
        
        let job = Job::new_async("0 0 * * * *", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running analytics aggregation job");
                mon.record_start("analytics").await;
                
                match aggregate_analytics(&pool).await {
                    Ok(_) => {
                        info!("Analytics aggregation completed successfully");
                        mon.record_success("analytics").await;
                    }
                    Err(e) => {
                        error!("Analytics aggregation failed: {}", e);
                        mon.record_failure("analytics", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Shutting down scheduler");
        self.scheduler.shutdown().await?;
        Ok(())
    }

    async fn add_backup_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();

        let job = Job::new_async("0 0 2 * * *", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running scheduled backup job");
                mon.record_start("scheduled_backup").await;

                // Record backup initiation
                let _ = crate::controllers::export_controller::record_backup(
                    &pool,
                    "scheduled",
                    "initiated",
                    None,
                    None,
                    None,
                )
                .await;

                let output = std::process::Command::new("./scripts/backup.sh").output();
                match output {
                    Ok(out) if out.status.success() => {
                        // Parse the JSON output from the backup script
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let backup_info: serde_json::Value = match serde_json::from_str(&stdout.trim()) {
                            Ok(json) => json,
                            Err(_) => serde_json::json!({}),
                        };

                        let size = backup_info.get("size").and_then(|v| v.as_i64());
                        let location = backup_info.get("file").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let checksum = backup_info.get("checksum").and_then(|v| v.as_str()).map(|s| s.to_string());

                        info!("Scheduled backup completed successfully");
                        let _ = crate::controllers::export_controller::record_backup(
                            &pool,
                            "scheduled",
                            "completed",
                            size,
                            location.as_deref(),
                            checksum.as_deref(),
                        )
                        .await;
                        mon.record_success("scheduled_backup").await;
                    }
                    Ok(out) => {
                        let err = String::from_utf8_lossy(&out.stderr).to_string();
                        error!("Scheduled backup failed: {}", err);
                        let _ = crate::controllers::export_controller::record_backup(
                            &pool,
                            "scheduled",
                            "failed",
                            None,
                            None,
                            None,
                        )
                        .await;
                        mon.record_failure("scheduled_backup", &err).await;
                    }
                    Err(e) => {
                        error!("Failed to run backup script: {}", e);
                        mon.record_failure("scheduled_backup", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }

    async fn add_key_rotation_job(&self, db_pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitor.clone();

        let job = Job::new_async("0 0 3 * * 0", move |_uuid, _l| {
            let pool = db_pool.clone();
            let mon = monitor.clone();
            Box::pin(async move {
                info!("Running key rotation job");
                mon.record_start("key_rotation").await;

                match crate::crypto::encryption::global_encryption_manager() {
                    Ok(manager) => {
                        match manager.rotate_keys().await {
                            Ok(_) => {
                                info!("Key rotation completed successfully");
                                mon.record_success("key_rotation").await;
                            }
                            Err(e) => {
                                error!("Key rotation failed: {}", e);
                                mon.record_failure("key_rotation", &e.to_string()).await;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get encryption manager: {}", e);
                        mon.record_failure("key_rotation", &e.to_string()).await;
                    }
                }
            })
        })?;

        self.scheduler.add(job).await?;
        Ok(())
    }
}
