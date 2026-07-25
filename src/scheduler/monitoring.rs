use sqlx::PgPool;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tracing::{info, error};

#[derive(Clone)]
pub struct JobStatus {
    pub name: String,
    pub last_run: Option<chrono::DateTime<Utc>>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub run_count: i64,
    pub failure_count: i64,
    pub avg_duration_ms: Option<i64>,
}

pub struct JobMonitor {
    db_pool: PgPool,
    active_jobs: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
}

impl JobMonitor {
    pub fn new(db_pool: PgPool) -> Self {
        Self {
            db_pool,
            active_jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn record_start(&self, job_name: &str) {
        let mut jobs = self.active_jobs.write().await;
        jobs.insert(job_name.to_string(), Utc::now());
        info!("Job '{}' started", job_name);
    }

    pub async fn record_success(&self, job_name: &str) {
        let duration = self.calculate_duration(job_name).await;
        
        if let Err(e) = self.save_job_run(job_name, "success", None, duration).await {
            error!("Failed to record job success: {}", e);
        }
        
        info!("Job '{}' completed successfully in {}ms", job_name, duration.unwrap_or(0));
    }

    pub async fn record_failure(&self, job_name: &str, error_msg: &str) {
        let duration = self.calculate_duration(job_name).await;
        
        if let Err(e) = self.save_job_run(job_name, "failure", Some(error_msg), duration).await {
            error!("Failed to record job failure: {}", e);
        }
        
        error!("Job '{}' failed after {}ms: {}", job_name, duration.unwrap_or(0), error_msg);
        
        // TODO: Send alert notification for failures
    }

    async fn calculate_duration(&self, job_name: &str) -> Option<i64> {
        let mut jobs = self.active_jobs.write().await;
        if let Some(start_time) = jobs.remove(job_name) {
            let duration = Utc::now().signed_duration_since(start_time);
            Some(duration.num_milliseconds())
        } else {
            None
        }
    }

    async fn save_job_run(
        &self,
        job_name: &str,
        status: &str,
        error_msg: Option<&str>,
        duration_ms: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO job_runs (job_name, status, error_message, duration_ms, run_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            job_name,
            status,
            error_msg,
            duration_ms,
            Utc::now().naive_utc()
        )
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    pub async fn get_job_status(&self, job_name: &str) -> Result<Option<JobStatus>, sqlx::Error> {
        let status = sqlx::query_as!(
            JobStatus,
            r#"
            SELECT
                $1 as "name!",
                MAX(run_at)::timestamptz as last_run,
                (SELECT status FROM job_runs WHERE job_name = $1 ORDER BY run_at DESC LIMIT 1) as last_status,
                (SELECT error_message FROM job_runs WHERE job_name = $1 ORDER BY run_at DESC LIMIT 1) as last_error,
                COUNT(*) as "run_count!",
                COUNT(*) FILTER (WHERE status = 'failure') as "failure_count!",
                AVG(duration_ms)::bigint as avg_duration_ms
            FROM job_runs
            WHERE job_name = $1
            GROUP BY job_name
            "#,
            job_name
        )
        .fetch_optional(&self.db_pool)
        .await?;

        Ok(status)
    }

    pub async fn get_all_job_statuses(&self) -> Result<Vec<JobStatus>, sqlx::Error> {
        let statuses = sqlx::query_as!(
            JobStatus,
            r#"
            SELECT
                job_name as "name!",
                MAX(run_at)::timestamptz as last_run,
                (SELECT status FROM job_runs jr2 WHERE jr2.job_name = jr.job_name ORDER BY run_at DESC LIMIT 1) as last_status,
                (SELECT error_message FROM job_runs jr2 WHERE jr2.job_name = jr.job_name ORDER BY run_at DESC LIMIT 1) as last_error,
                COUNT(*) as "run_count!",
                COUNT(*) FILTER (WHERE status = 'failure') as "failure_count!",
                AVG(duration_ms)::bigint as avg_duration_ms
            FROM job_runs jr
            GROUP BY job_name
            ORDER BY MAX(run_at) DESC
            "#
        )
        .fetch_all(&self.db_pool)
        .await?;

        Ok(statuses)
    }
}
