//! Business logic layer
//!
//! Services orchestrate operations by coordinating repositories,
//! applying business rules, and managing transactions.

pub mod admin;
pub mod audit;
pub mod batch;
pub mod conditional;
pub mod conditional_references;
pub mod crud;
pub mod history;
pub mod indexing;
pub mod metadata;
pub mod metrics;
pub mod operation_executor;
pub mod operation_registry;
pub mod package;
pub(crate) mod referential_integrity;
pub mod runtime_config;
pub mod search;
pub mod summary;
pub mod system;
pub mod terminology;
pub mod transaction;

pub use admin::AdminService;
pub use audit::AuditService;
pub use batch::BatchService;
pub use conditional_references::ConditionalReferenceResolver;
pub use crud::CrudService;
pub use history::HistoryService;
pub use indexing::IndexingService;
pub use metadata::MetadataService;
pub use metrics::MetricsService;
pub use operation_executor::OperationExecutor;
pub use operation_registry::OperationRegistry;
pub use package::PackageService;
pub use runtime_config::RuntimeConfigService;
pub use search::SearchService;
pub use summary::SummaryFilter;
pub use system::SystemService;
pub use terminology::TerminologyService;
pub use transaction::TransactionService;
