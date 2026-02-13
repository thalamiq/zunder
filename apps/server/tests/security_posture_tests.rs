#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use support::*;

#[tokio::test]
async fn security_headers_are_present() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let (status, headers, _body) = app.request(Method::GET, "/health", None).await?;
            assert_status(status, StatusCode::OK, "health");

            for (name, expected) in [
                ("x-content-type-options", "nosniff"),
                ("x-frame-options", "DENY"),
                ("referrer-policy", "no-referrer"),
                ("content-security-policy", "default-src 'none'"),
            ] {
                let got = headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(got, expected, "missing/incorrect header '{}'", name);
            }

            // HSTS should not be set for plain HTTP requests.
            assert!(headers.get("strict-transport-security").is_none());

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn cors_is_not_permissive_by_default() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let (status, headers, _body) = app
                .request_with_extra_headers(
                    Method::GET,
                    "/health",
                    None,
                    &[("origin", "https://evil.example")],
                )
                .await?;
            assert_status(status, StatusCode::OK, "health");
            assert!(
                headers.get("access-control-allow-origin").is_none(),
                "expected no permissive CORS by default"
            );
            Ok(())
        })
    })
    .await
}
