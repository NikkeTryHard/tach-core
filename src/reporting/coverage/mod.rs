//! Coverage submodules
//!
//! This module implements zero-overhead coverage collection:
//! - `shm`: Shared memory ring buffers
//! - `aggregator`: Coverage data aggregation
//! - `callbacks`: PyO3 FFI exports for Python callbacks

pub mod aggregator;
pub mod callbacks;
pub mod shm;

// Re-export main types
pub use super::coverage::*;
