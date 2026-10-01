//! Read-only patch collection, reconciliation, and health endpoints.

pub mod api;
pub mod app;
pub mod cache;
pub mod collector;
pub mod config;
pub mod domain;
pub mod observability;
pub mod scheduler;
#[cfg(windows)]
pub mod service;
