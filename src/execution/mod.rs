//! Test execution modules
//!
//! This module handles running tests:
//! - `scheduler`: Test scheduling and worker dispatch
//! - `watch`: File watching for test re-runs
//! - `zygote`: Fork server and worker pool management
//! - `plugin_bridge`: FD Teleporter for SCM_RIGHTS handover

pub mod plugin_bridge;
pub mod scheduler;
pub mod watch;
pub mod zygote;
