//! Inline (in-process) job queue implementation.
//!
//! This queue executes supported jobs immediately in the caller's task instead of
//! persisting them and relying on background workers.
//!
//! Primary use-case: deterministic integration tests that need search indexing to
//! be completed before a response is observed.

use super::{Job, JobPriority, JobQueue, JobStatus, RetryPolicy};
use crate::{db::PostgresResourceStore, services::IndexingService, Result};
use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use sqlx::PgPool;
use std::{collections::HashMap, sync::Mutex};
use uuid::Uuid;

#[derive(Debug, serde::Deserialize)]
struct IndexSearchParams {
    resource_type: String,
    resource_ids: Vec<String>,
}

/// Inline job queue that runs supported jobs synchronously.
pub struct InlineJobQueue {
    pool: PgPool,
    indexing_service: std::sync::Arc<IndexingService>,
    jobs: Mutex<HashMap<Uuid, Job>>,
}

impl InlineJobQueue {
    pub fn new(pool: PgPool, indexing_service: std::sync::Arc<IndexingService>) -> Self {
        Self {
            pool,
            indexing_service,
            jobs: Mutex::new(HashMap::new()),
        }
    }

    async fn run_index_search(&self, job_id: Uuid, parameters: serde_json::Value) -> Result<()> {
        let params: IndexSearchParams = serde_json::from_value(parameters).map_err(|e| {
            crate::Error::Internal(format!("Failed to parse job parameters: {}", e))
        })?;

        if params.resource_ids.is_empty() {
            return Ok(());
        }

        let store = PostgresResourceStore::new(self.pool.clone());
        let resources = store
            .load_resources_batch(&params.resource_type, &params.resource_ids)
            .await?;

        self.indexing_service
            .index_resources_auto(&resources)
            .await?;

        self.update_progress(
            job_id,
            resources.len() as i32,
            Some(resources.len() as i32),
            None,
        )
        .await?;
        self.complete_job(job_id, None).await?;

        Ok(())
    }

    fn insert_job(&self, job: Job) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.insert(job.id, job);
    }

    fn update_job<F>(&self, job_id: Uuid, f: F) -> Result<()>
    where
        F: FnOnce(&mut Job),
    {
        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.get_mut(&job_id);
        if let Some(job) = job {
            f(job);
        }
        Ok(())
    }
}

#[async_trait]
impl JobQueue for InlineJobQueue {
    async fn enqueue(
        &self,
        job_type: String,
        parameters: serde_json::Value,
        priority: JobPriority,
        retry_policy: Option<RetryPolicy>,
    ) -> Result<Uuid> {
        let job_id = Uuid::new_v4();
        let now = Utc::now();
        let retry_policy_json =
            serde_json::to_value(retry_policy.unwrap_or_default()).map_err(|e| {
                crate::Error::Internal(format!("Failed to serialize retry policy: {}", e))
            })?;

        let job = Job {
            id: job_id,
            job_type: job_type.clone(),
            status: JobStatus::Running,
            priority: priority as i32,
            parameters: parameters.clone(),
            progress: None,
            retry_policy: retry_policy_json,
            retry_count: 0,
            processed_items: 0,
            total_items: None,
            error_message: None,
            last_error_at: None,
            scheduled_at: None,
            cancel_requested: false,
            created_at: now,
            started_at: Some(now),
            completed_at: None,
            worker_id: Some("inline".to_string()),
        };
        self.insert_job(job);

        let result = match job_type.as_str() {
            "index_search" => self.run_index_search(job_id, parameters).await,
            // Unsupported jobs are treated as no-ops in inline mode.
            _ => {
                self.complete_job(job_id, None).await?;
                Ok(())
            }
        };

        if let Err(e) = result {
            let _ = self
                .fail_job(job_id, format!("Inline job failed: {}", e), false)
                .await;
            return Err(e);
        }

        Ok(job_id)
    }

    async fn dequeue(&self, _job_types: &[String], _worker_id: &str) -> Result<Option<Job>> {
        Ok(None)
    }

    async fn listen<'a>(&'a self, _job_types: &'a [String]) -> Result<BoxStream<'a, Result<Job>>> {
        Ok(Box::pin(futures::stream::empty()))
    }

    async fn get_job(&self, job_id: Uuid) -> Result<Option<Job>> {
        let jobs = self.jobs.lock().unwrap();
        Ok(jobs.get(&job_id).cloned())
    }

    async fn update_progress(
        &self,
        job_id: Uuid,
        processed_items: i32,
        total_items: Option<i32>,
        progress_data: Option<serde_json::Value>,
    ) -> Result<()> {
        self.update_job(job_id, |job| {
            job.processed_items = processed_items;
            if let Some(total) = total_items {
                job.total_items = Some(total);
            }
            if progress_data.is_some() {
                job.progress = progress_data;
            }
        })
    }

    async fn complete_job(
        &self,
        job_id: Uuid,
        final_results: Option<serde_json::Value>,
    ) -> Result<()> {
        let now = Utc::now();
        self.update_job(job_id, |job| {
            // Respect cancel_requested: mark as cancelled instead of completed
            job.status = if job.cancel_requested {
                JobStatus::Cancelled
            } else {
                JobStatus::Completed
            };
            job.completed_at = Some(now);
            if final_results.is_some() {
                job.progress = final_results;
            }
        })
    }

    async fn fail_job(&self, job_id: Uuid, error_message: String, _retry: bool) -> Result<()> {
        let now = Utc::now();
        self.update_job(job_id, |job| {
            job.status = JobStatus::Failed;
            job.error_message = Some(error_message);
            job.last_error_at = Some(now);
            job.completed_at = Some(now);
        })
    }

    async fn cancel_job(&self, job_id: Uuid) -> Result<bool> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&job_id) {
            job.cancel_requested = true;
            // Immediately cancel pending jobs
            if job.status == JobStatus::Pending {
                job.status = JobStatus::Cancelled;
                job.completed_at = Some(chrono::Utc::now());
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn is_cancelled(&self, job_id: Uuid) -> Result<bool> {
        let jobs = self.jobs.lock().unwrap();
        Ok(jobs
            .get(&job_id)
            .map(|j| j.cancel_requested)
            .unwrap_or(false))
    }

    async fn delete_job(&self, job_id: Uuid) -> Result<bool> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get(&job_id) {
            let deletable = matches!(
                job.status,
                JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
            ) || (job.cancel_requested
                && matches!(job.status, JobStatus::Running | JobStatus::Pending));
            if deletable {
                jobs.remove(&job_id);
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn health_check(&self) -> Result<serde_json::Value> {
        let jobs = self.jobs.lock().unwrap();
        Ok(serde_json::json!({
            "type": "inline",
            "jobs": jobs.len(),
        }))
    }

    async fn cleanup_old_jobs(&self, _days: i32) -> Result<i64> {
        // Inline queue is ephemeral; keep jobs for debugging unless explicitly cleared.
        Ok(0)
    }
}
