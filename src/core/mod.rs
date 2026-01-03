//! Core infrastructure modules for Tach
//!
//! This module contains foundational components used across the codebase:
//! - `allocator`: Jemalloc control for deterministic snapshots
//! - `config`: Configuration parsing and CLI options
//! - `diagnostics`: Pre-flight self-test and system diagnostics
//! - `environment`: Virtual environment detection
//! - `errors`: Unified error types for all Tach operations
//! - `lifecycle`: Process lifecycle management
//! - `protocol`: IPC protocol definitions
//! - `signals`: Signal handling for graceful shutdown

pub mod allocator;
pub mod config;
pub mod diagnostics;
pub mod environment;
pub mod errors;
pub mod lifecycle;
pub mod protocol;
pub mod signals;
