//! PostgreSQL-backed job queue implementation using LISTEN/NOTIFY

use super::{helpers::try_dequeue_job, models::*, traits::JobQueue};
use crate::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use sqlx::{PgPool, Row};
use tokio::time::{Duration, MissedTickBehavior};
use uuid::Uuid;

pub struct PostgresJobQueue {
    pool: PgPool,
    listen_poll_interval: Duration,
}

impl PostgresJobQueue {
    pub fn new(pool: PgPool, listen_poll_interval_seconds: u64) -> Self {
        Self {
            pool,
            listen_poll_interval: Duration::from_secs(listen_poll_interval_seconds.max(1)),
        }
    }
}

#[async_trait]
impl JobQueue for PostgresJobQueue {
    async fn enqueue(
        &self,
        job_type: String,
        parameters: serde_json::Value,
        priority: JobPriority,
        retry_policy: Option<RetryPolicy>,
    ) -> Result<Uuid> {
        let job_id = Uuid::new_v4();
        let retry_policy_json =
            serde_json::to_value(retry_policy.unwrap_or_default()).map_err(|e| {
                crate::Error::Internal(format!("Failed to serialize retry policy: {}", e))
            })?;

        let priority_int = priority as i32;

        sqlx::query(
            r#"
            INSERT INTO jobs (id, job_type, status, parameters, priority, retry_policy)
            VALUES ($1, $2, 'pending', $3, $4, $5)
            "#,
        )
        .bind(job_id)
        .bind(&job_type)
        .bind(&parameters)
        .bind(priority_int)
        .bind(retry_policy_json)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        // Notify waiting workers
        sqlx::query("SELECT pg_notify('job_queue', $1)")
            .bind(&job_type)
            .execute(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        tracing::info!(
            "Enqueued job: {} (type: {}, priority: {:?})",
            job_id,
            job_type,
            priority
        );

        Ok(job_id)
    }

    async fn dequeue(&self, job_types: &[String], worker_id: &str) -> Result<Option<Job>> {
        let now = chrono::Utc::now();

        let result = sqlx::query_as::<_, Job>(
            r#"
            UPDATE jobs
            SET status = 'running',
                started_at = $1,
                worker_id = $2
            WHERE id = (
                SELECT id
                FROM jobs
                WHERE job_type = ANY($3)
                  AND status = 'pending'
                  AND cancel_requested = FALSE
                  AND (scheduled_at IS NULL OR scheduled_at <= $1)
                ORDER BY priority DESC, created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, job_type, status, priority, parameters, progress,
                      retry_policy, retry_count, processed_items, total_items,
                      error_message, last_error_at, scheduled_at, cancel_requested,
                      created_at, started_at, completed_at, worker_id
            "#,
        )
        .bind(now)
        .bind(worker_id)
        .bind(job_types)
        .fetch_optional(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        if let Some(ref job) = result {
            tracing::info!(
                "Dequeued job {} (type: {}) for worker {}",
                job.id,
                job.job_type,
                worker_id
            );
        }

        Ok(result)
    }

    async fn listen<'a>(&'a self, job_types: &'a [String]) -> Result<BoxStream<'a, Result<Job>>> {
        // Create a listener for the job_queue channel
        let mut listener = sqlx::postgres::PgListener::connect_with(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        listener
            .listen("job_queue")
            .await
            .map_err(crate::Error::Database)?;

        tracing::info!(
            "Job queue listener started on channel 'job_queue' for job types: {:?}",
            job_types
        );

        // Clone what we need for the stream
        let pool = self.pool.clone();
        let job_types = job_types.to_vec();
        let worker_id = format!("listener-{}", uuid::Uuid::new_v4());
        let listen_poll_interval = self.listen_poll_interval;

        // Create a stream that polls for jobs and listens for notifications
        let stream = async_stream::stream! {
            // First, poll for any existing pending jobs
            tracing::debug!("Polling for existing pending jobs...");
            loop {
                match try_dequeue_job(&pool, &job_types, &worker_id).await {
                    Ok(Some(job)) => {
                        tracing::info!("Polled job {} from queue", job.id);
                        yield Ok(job);
                    }
                    Ok(None) => {
                        // No more pending jobs, break to start listening
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Error polling for jobs: {}", e);
                        yield Err(e);
                        break;
                    }
                }
            }

            // Now listen for new job notifications
            tracing::debug!("Listening for job notifications...");
            let mut interval = tokio::time::interval(listen_poll_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                let should_try_dequeue = tokio::select! {
                    recv_res = listener.recv() => {
                        match recv_res {
                            Ok(notification) => {
                                let notified_job_type = notification.payload();
                                tracing::debug!("Received job notification for type: {}", notified_job_type);
                                job_types.is_empty() || job_types.contains(&notified_job_type.to_string())
                            }
                            Err(e) => {
                                tracing::error!("Error receiving notification: {}", e);
                                yield Err(crate::Error::Database(e));
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        tracing::trace!("Periodic queue poll tick");
                        true
                    }
                };

                if !should_try_dequeue {
                    continue;
                }

                match try_dequeue_job(&pool, &job_types, &worker_id).await {
                    Ok(Some(job)) => {
                        tracing::info!("Dequeued job {} after notification/poll", job.id);
                        yield Ok(job);
                    }
                    Ok(None) => {
                        // Job was already taken by another worker, or there is nothing pending.
                        tracing::trace!("No job available for this worker");
                    }
                    Err(e) => {
                        tracing::error!("Error dequeuing job: {}", e);
                        yield Err(e);
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn get_job(&self, job_id: Uuid) -> Result<Option<Job>> {
        let job = sqlx::query_as::<_, Job>(
            r#"
            SELECT id, job_type, status, priority, parameters, progress,
                   retry_policy, retry_count, processed_items, total_items,
                   error_message, last_error_at, scheduled_at, cancel_requested,
                   created_at, started_at, completed_at, worker_id
            FROM jobs
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        Ok(job)
    }

    async fn update_progress(
        &self,
        job_id: Uuid,
        processed_items: i32,
        total_items: Option<i32>,
        progress_data: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut query_builder = sqlx::QueryBuilder::new("UPDATE jobs SET processed_items = ");
        query_builder.push_bind(processed_items);

        if let Some(total) = total_items {
            query_builder.push(", total_items = ");
            query_builder.push_bind(total);
        }

        if let Some(progress) = progress_data {
            query_builder.push(", progress = ");
            query_builder.push_bind(progress);
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(job_id);

        query_builder
            .build()
            .execute(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        Ok(())
    }

    async fn complete_job(
        &self,
        job_id: Uuid,
        final_results: Option<serde_json::Value>,
    ) -> Result<()> {
        let now = chrono::Utc::now();

        // If cancel was requested while running, mark as cancelled instead of completed
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = CASE WHEN cancel_requested THEN 'cancelled' ELSE 'completed' END,
                completed_at = $1,
                progress = COALESCE($2, progress)
            WHERE id = $3
            RETURNING status
            "#,
        )
        .bind(now)
        .bind(final_results)
        .bind(job_id)
        .fetch_one(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let final_status: String = result.get("status");
        if final_status == "cancelled" {
            tracing::info!("Job {} marked as cancelled (cancel was requested during execution)", job_id);
        } else {
            tracing::info!("Job {} completed", job_id);
        }
        Ok(())
    }

    async fn fail_job(&self, job_id: Uuid, error_message: String, retry: bool) -> Result<()> {
        let now = chrono::Utc::now();

        if retry {
            // Check if job can be retried
            let job = self.get_job(job_id).await?;
            if let Some(job) = job {
                if job.can_retry() {
                    let retry_policy = job.get_retry_policy();
                    let next_retry_delay = retry_policy.calculate_delay(job.retry_count);
                    let scheduled_at = now + chrono::Duration::seconds(next_retry_delay as i64);

                    sqlx::query(
                        r#"
                        UPDATE jobs
                        SET status = 'pending',
                            retry_count = retry_count + 1,
                            last_error_at = $1,
                            error_message = $2,
                            scheduled_at = $3
                        WHERE id = $4
                        "#,
                    )
                    .bind(now)
                    .bind(&error_message)
                    .bind(scheduled_at)
                    .bind(job_id)
                    .execute(&self.pool)
                    .await
                    .map_err(crate::Error::Database)?;

                    tracing::warn!(
                        "Job {} failed, scheduled for retry at {}",
                        job_id,
                        scheduled_at
                    );
                    return Ok(());
                }
            }
        }

        // Mark as permanently failed
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'failed',
                completed_at = $1,
                error_message = $2,
                last_error_at = $1
            WHERE id = $3
            "#,
        )
        .bind(now)
        .bind(&error_message)
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        tracing::error!("Job {} failed: {}", job_id, error_message);
        Ok(())
    }

    async fn cancel_job(&self, job_id: Uuid) -> Result<bool> {
        let now = chrono::Utc::now();

        // Immediately cancel pending jobs (they haven't started)
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'cancelled',
                cancel_requested = TRUE,
                completed_at = $1
            WHERE id = $2 AND status = 'pending'
            "#,
        )
        .bind(now)
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        if result.rows_affected() > 0 {
            tracing::info!("Job {} cancelled (was pending)", job_id);
            return Ok(true);
        }

        // For running jobs, set the flag so the worker can check and stop
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET cancel_requested = TRUE
            WHERE id = $1 AND status = 'running'
            "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let cancelled = result.rows_affected() > 0;
        if cancelled {
            tracing::info!("Cancellation requested for running job {}", job_id);
        } else {
            tracing::warn!("Job {} not found or already in terminal state", job_id);
        }

        Ok(cancelled)
    }

    async fn is_cancelled(&self, job_id: Uuid) -> Result<bool> {
        let result: Option<bool> =
            sqlx::query_scalar("SELECT cancel_requested FROM jobs WHERE id = $1")
                .bind(job_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(crate::Error::Database)?;

        Ok(result.unwrap_or(false))
    }

    async fn delete_job(&self, job_id: Uuid) -> Result<bool> {
        // Allow deleting terminal jobs, or running/pending jobs where cancel was already requested
        let result = sqlx::query(
            r#"
            DELETE FROM jobs
            WHERE id = $1
              AND (
                status IN ('completed', 'failed', 'cancelled')
                OR (cancel_requested = TRUE AND status IN ('running', 'pending'))
              )
            "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            tracing::info!("Job {} deleted", job_id);
        }
        Ok(deleted)
    }

    async fn health_check(&self) -> Result<serde_json::Value> {
        // Get queue statistics
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total,
                COUNT(*) FILTER (WHERE status = 'pending') as pending,
                COUNT(*) FILTER (WHERE status = 'running') as running,
                COUNT(*) FILTER (WHERE status = 'completed') as completed,
                COUNT(*) FILTER (WHERE status = 'failed') as failed,
                COUNT(*) FILTER (WHERE status = 'cancelled') as cancelled
            FROM jobs
            WHERE created_at > NOW() - INTERVAL '24 hours'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let total: i64 = row.try_get("total").unwrap_or(0);
        let pending: i64 = row.try_get("pending").unwrap_or(0);
        let running: i64 = row.try_get("running").unwrap_or(0);
        let completed: i64 = row.try_get("completed").unwrap_or(0);
        let failed: i64 = row.try_get("failed").unwrap_or(0);
        let cancelled: i64 = row.try_get("cancelled").unwrap_or(0);

        Ok(serde_json::json!({
            "status": "ok",
            "backend": "postgres",
            "stats_24h": {
                "total": total,
                "pending": pending,
                "running": running,
                "completed": completed,
                "failed": failed,
                "cancelled": cancelled
            }
        }))
    }

    async fn cleanup_old_jobs(&self, days: i32) -> Result<i64> {
        let result = sqlx::query(
            r#"
            DELETE FROM jobs
            WHERE status IN ('completed', 'failed', 'cancelled')
              AND completed_at < NOW() - ($1 || ' days')::INTERVAL
            "#,
        )
        .bind(days)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let deleted = result.rows_affected() as i64;
        tracing::info!("Cleaned up {} old jobs (older than {} days)", deleted, days);
        Ok(deleted)
    }
}
