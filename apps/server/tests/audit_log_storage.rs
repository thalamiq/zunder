#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use support::{assert_status, with_test_app_with_config};

#[tokio::test]
async fn audit_events_are_written_to_audit_log_table() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.logging.audit.enabled = true;
            config.logging.audit.interactions.capabilities = true;
        },
        |app| {
            Box::pin(async move {
                let (status, _headers, _body) =
                    app.request(Method::GET, "/fhir/metadata", None).await?;
                assert_status(status, StatusCode::OK, "metadata");

                // Audit writes are async; wait briefly for the insert.
                for _ in 0..50 {
                    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
                        .fetch_one(&app.state.db_pool)
                        .await?;
                    if count > 0 {
                        return Ok(());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }

                anyhow::bail!("expected at least one row in audit_log after a FHIR request");
            })
        },
    )
    .await
}
