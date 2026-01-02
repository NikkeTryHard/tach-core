//! Core infrastructure modules for Tach
//!
//! This module contains foundational components used across the codebase:
//! - `allocator`: Jemalloc control for deterministic snapshots
//! - `config`: Configuration parsing and CLI options
//! - `environment`: Virtual environment detection
//! - `lifecycle`: Process lifecycle management
//! - `protocol`: IPC protocol definitions
//! - `signals`: Signal handling for graceful shutdown

pub mod allocator;
pub mod config;
pub mod environment;
pub mod lifecycle;
pub mod protocol;
pub mod signals;
