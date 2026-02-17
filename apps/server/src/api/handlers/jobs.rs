//! Background job management handlers (admin API)

use crate::{state::AppState, Result};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListJobsQuery {
    pub job_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// List all background jobs with optional filtering
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(q): Query<ListJobsQuery>,
) -> Result<Response> {
    // Get pagination parameters
    let limit = q.limit.unwrap_or(50).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);

    // Use the helper function
    let (jobs, total) = crate::queue::list_jobs(
        &state.db_pool,
        q.job_type.as_deref(),
        q.status.as_deref(),
        limit,
        offset,
    )
    .await?;

    // Convert to JSON-friendly format
    let jobs_json: Vec<serde_json::Value> = jobs
        .into_iter()
        .map(|job| {
            json!({
                "id": job.id,
                "jobType": job.job_type,
                "status": job.status,
                "priority": job.get_priority(),
                "parameters": job.parameters,
                "progress": job.progress,
                "processedItems": job.processed_items,
                "totalItems": job.total_items,
                "progressPercent": job.progress_percent(),
                "errorMessage": job.error_message,
                "lastErrorAt": job.last_error_at,
                "scheduledAt": job.scheduled_at,
                "cancelRequested": job.cancel_requested,
                "createdAt": job.created_at,
                "startedAt": job.started_at,
                "completedAt": job.completed_at,
                "workerId": job.worker_id,
                "retryCount": job.retry_count,
            })
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "jobs": jobs_json,
            "total": total,
            "limit": limit,
            "offset": offset
        })),
    )
        .into_response())
}

/// Get a single job by ID
pub async fn get_job(State(state): State<AppState>, Path(job_id): Path<Uuid>) -> Result<Response> {
    let job = state.job_queue.get_job(job_id).await?;

    match job {
        Some(job) => Ok((
            StatusCode::OK,
            Json(json!({
                "id": job.id,
                "jobType": job.job_type,
                "status": job.status,
                "priority": job.get_priority(),
                "parameters": job.parameters,
                "progress": job.progress,
                "processedItems": job.processed_items,
                "totalItems": job.total_items,
                "progressPercent": job.progress_percent(),
                "errorMessage": job.error_message,
                "lastErrorAt": job.last_error_at,
                "scheduledAt": job.scheduled_at,
                "cancelRequested": job.cancel_requested,
                "createdAt": job.created_at,
                "startedAt": job.started_at,
                "completedAt": job.completed_at,
                "workerId": job.worker_id,
                "retryCount": job.retry_count,
                "retryPolicy": job.retry_policy,
            })),
        )
            .into_response()),
        None => Err(crate::Error::ResourceNotFound {
            resource_type: "Job".to_string(),
            id: job_id.to_string(),
        }),
    }
}

/// Cancel a running or pending job
pub async fn cancel_job(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Response> {
    let cancelled = state.job_queue.cancel_job(job_id).await?;

    if cancelled {
        Ok((
            StatusCode::OK,
            Json(json!({
                "cancelled": true,
                "jobId": job_id
            })),
        )
            .into_response())
    } else {
        Err(crate::Error::Validation(
            "Job not found or already completed".to_string(),
        ))
    }
}

/// Delete a single job (must be in a terminal state)
pub async fn delete_job(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Response> {
    let deleted = state.job_queue.delete_job(job_id).await?;

    if deleted {
        Ok((
            StatusCode::OK,
            Json(json!({
                "deleted": true,
                "jobId": job_id
            })),
        )
            .into_response())
    } else {
        Err(crate::Error::Validation(
            "Job not found or still running/pending".to_string(),
        ))
    }
}

/// Get queue health and statistics
pub async fn get_queue_health(State(state): State<AppState>) -> Result<Response> {
    let health = state.job_queue.health_check().await?;
    Ok((StatusCode::OK, Json(health)).into_response())
}

/// Cleanup old completed jobs
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupJobsQuery {
    pub days: Option<i32>,
}

pub async fn cleanup_old_jobs(
    State(state): State<AppState>,
    Query(q): Query<CleanupJobsQuery>,
) -> Result<Response> {
    let days = q.days.unwrap_or(30);

    if days < 1 {
        return Err(crate::Error::Validation(
            "days must be at least 1".to_string(),
        ));
    }

    let deleted = state.job_queue.cleanup_old_jobs(days).await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "deleted": deleted,
            "days": days
        })),
    )
        .into_response())
}
