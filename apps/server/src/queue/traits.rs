//! Job queue trait definition

use super::models::*;
use crate::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use uuid::Uuid;

/// Abstract interface for job queue implementations
#[async_trait]
pub trait JobQueue: Send + Sync {
    /// Enqueue a new job
    async fn enqueue(
        &self,
        job_type: String,
        parameters: serde_json::Value,
        priority: JobPriority,
        retry_policy: Option<RetryPolicy>,
    ) -> Result<Uuid>;

    /// Dequeue the next available job
    async fn dequeue(&self, job_types: &[String], worker_id: &str) -> Result<Option<Job>>;

    /// Listen for new jobs (streaming interface)
    /// Returns a stream of jobs matching the specified types
    async fn listen<'a>(&'a self, job_types: &'a [String]) -> Result<BoxStream<'a, Result<Job>>>;

    /// Get job by ID
    async fn get_job(&self, job_id: Uuid) -> Result<Option<Job>>;

    /// Update job progress
    async fn update_progress(
        &self,
        job_id: Uuid,
        processed_items: i32,
        total_items: Option<i32>,
        progress_data: Option<serde_json::Value>,
    ) -> Result<()>;

    /// Mark job as completed
    async fn complete_job(
        &self,
        job_id: Uuid,
        final_results: Option<serde_json::Value>,
    ) -> Result<()>;

    /// Mark job as failed and optionally schedule retry
    async fn fail_job(&self, job_id: Uuid, error_message: String, retry: bool) -> Result<()>;

    /// Request job cancellation
    async fn cancel_job(&self, job_id: Uuid) -> Result<bool>;

    /// Check if job was cancelled
    async fn is_cancelled(&self, job_id: Uuid) -> Result<bool>;

    /// Delete a single job (must be in a terminal state)
    async fn delete_job(&self, job_id: Uuid) -> Result<bool>;

    /// Health check
    async fn health_check(&self) -> Result<serde_json::Value>;

    /// Cleanup old completed/failed jobs
    async fn cleanup_old_jobs(&self, days: i32) -> Result<i64>;
}
