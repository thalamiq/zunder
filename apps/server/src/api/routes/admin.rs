use crate::api::handlers::{admin, jobs, packages, runtime_config};
use crate::state::AppState;
use axum::{
    routing::{get, post, put},
    Router,
};

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        // UI configuration and authentication
        .route("/ui/config", get(admin::get_ui_config))
        .route("/ui/auth", post(admin::authenticate))
        .route("/ui/session", get(admin::get_ui_session))
        .route("/ui/logout", post(admin::logout))
        // Package management
        .route("/packages", get(packages::list_packages))
        .route("/packages/install", post(packages::install_package))
        .route("/packages/:id", get(packages::get_package))
        .route(
            "/packages/:id/resources",
            get(packages::list_package_resources),
        )
        // Job management
        .route("/jobs", get(jobs::list_jobs))
        .route("/jobs/health", get(jobs::get_queue_health))
        .route("/jobs/cleanup", post(jobs::cleanup_old_jobs))
        .route("/jobs/:id", get(jobs::get_job).delete(jobs::delete_job))
        .route("/jobs/:id/cancel", post(jobs::cancel_job))
        // Resource stats
        .route("/resources/stats", get(admin::get_resource_type_stats))
        // Resource references (for graph visualization)
        .route(
            "/resources/:resource_type/:id/references",
            get(admin::get_resource_references),
        )
        .route(
            "/resources/references/batch",
            post(admin::get_batch_references),
        )
        // Search parameter indexing status
        .route(
            "/search-parameters/indexing-status",
            get(admin::get_search_parameter_indexing_status),
        )
        .route(
            "/search-parameters/indexing-status/:resource_type",
            get(admin::get_search_parameter_indexing_status_by_type),
        )
        // SearchParameters admin listing
        .route("/search-parameters", get(admin::list_search_parameters))
        .route(
            "/search-parameters/:id/toggle-active",
            post(admin::toggle_search_parameter_active),
        )
        // Search indexing introspection
        .route(
            "/search/index-tables",
            get(admin::get_search_index_table_status),
        )
        .route(
            "/search/hash-collisions",
            get(admin::get_search_hash_collisions),
        )
        // Compartment memberships
        .route(
            "/compartments/memberships",
            get(admin::get_compartment_memberships),
        )
        // Audit log (internal, read-only - audit logs are immutable)
        .route("/audit/events", get(admin::list_audit_events))
        .route("/audit/events/:id", get(admin::get_audit_event))
        // Runtime configuration
        .route("/config", get(runtime_config::list_config))
        .route("/config/audit", get(runtime_config::get_audit_log))
        .route("/config/:key", get(runtime_config::get_config))
        .route("/config/:key", put(runtime_config::update_config))
        .route("/config/:key/reset", post(runtime_config::reset_config))
}
