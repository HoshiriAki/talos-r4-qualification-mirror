//! Provider-neutral Integration Fabric.
//!
//! This module deliberately lives outside the synchronous `SystemModule` ABI.
//! Provider manifests are descriptive, while tenant-scoped instance/binding
//! resolution plus a controlled KeyStore determine whether an integration can
//! execute. No third-party connector is allowed to receive a business
//! repository handle from this boundary.

pub mod catalog;
pub(crate) mod catalog_authority;
pub(crate) mod catalog_repository;
#[cfg(feature = "postgres")]
pub(crate) mod catalog_repository_postgres;
pub(crate) mod catalog_repository_sqlite;
pub mod deposit;
pub(crate) mod deposit_refund_persistence_contract;
#[cfg(feature = "postgres")]
pub(crate) mod deposit_refund_persistence_postgres;
pub(crate) mod deposit_refund_persistence_sqlite;
pub mod egress;
pub(crate) mod egress_dispatch;
pub mod governed_worker;
pub mod keystore;
pub mod module;
pub mod operation;
pub(crate) mod operation_runtime_contract;
#[cfg(feature = "postgres")]
pub(crate) mod operation_runtime_postgres;
pub mod runtime;
pub(crate) mod scheduler_persistence_contract;
#[cfg(feature = "postgres")]
pub(crate) mod scheduler_persistence_postgres;
pub(crate) mod scheduler_persistence_sqlite;
pub(crate) mod settlement_admission;
pub mod store;
pub mod transport;
pub mod types;
pub mod webhook;
pub mod webhook_event;
pub(crate) mod webhook_persistence_contract;
#[cfg(feature = "postgres")]
pub(crate) mod webhook_persistence_postgres;
pub(crate) mod webhook_persistence_sqlite;
pub mod worker;

#[cfg(all(test, feature = "postgres"))]
mod catalog_repository_pg_tests;
#[cfg(all(test, feature = "postgres"))]
mod deposit_refund_persistence_postgres_tests;
#[cfg(all(test, feature = "sqlite"))]
mod hardening_tests;
#[cfg(all(test, feature = "postgres"))]
mod integration_module_postgres_tests;
#[cfg(all(test, feature = "postgres"))]
mod operation_runtime_postgres_tests;
#[cfg(all(test, feature = "postgres"))]
mod scheduler_persistence_postgres_tests;
#[cfg(all(test, feature = "postgres"))]
mod webhook_event_pg_tests;
#[cfg(all(test, feature = "postgres"))]
mod webhook_persistence_postgres_tests;

pub use governed_worker::GovernedIntegrationWorker;
pub use module::IntegrationModule;
pub use worker::FixtureIntegrationWorker;
