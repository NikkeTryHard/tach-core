//! Tach Core Library
//!
//! This library exposes the core modules for integration testing.
//! The binary entry point is in main.rs.

pub mod config;
pub mod debugger;
pub mod discovery;
pub mod environment;
pub mod isolation;
pub mod junit;
pub mod lifecycle;
pub mod loader;
pub mod logcapture;
pub mod protocol;
pub mod reporter;
pub mod resolver;
pub mod scheduler;
pub mod signals;
pub mod snapshot;
pub mod watch;
pub mod zygote;
