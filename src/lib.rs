//! Read-only patch collection, reconciliation, and health endpoints.

pub mod api;
pub mod app;
pub mod cache;
pub mod collector;
pub mod config;
pub mod domain;
mod export;
pub mod observability;
pub mod preflight;
pub mod scheduler;
#[cfg(windows)]
pub mod service;
