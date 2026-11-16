//! Control plane library: HTTP/WS API, auth, persistence, orchestration.
//!
//! Milestone 1: SQLite schema, bootstrap admin, login + JWT middleware.

pub mod auth;
pub mod bootstrap;
pub mod config;
pub mod daemon;
pub mod error;
pub mod middleware;
pub mod properties;
pub mod routes;
pub mod scheduler;
pub mod state;
pub mod telemetry;
