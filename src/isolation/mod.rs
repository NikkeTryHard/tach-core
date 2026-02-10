//! Process isolation modules
//!
//! This module handles test isolation:
//! - `namespace`: Linux namespace setup and OverlayFS
//! - `sandbox`: Landlock and Seccomp sandboxing (Iron Dome)
//! - `snapshot`: userfaultfd memory snapshots
//! - `calibration`: TLS self-calibration for mimalloc offset discovery

pub mod calibration;
pub mod namespace;
pub mod sandbox;
pub mod snapshot;

// Re-export main functions from namespace for backward compatibility
pub use namespace::{is_overlayfs, setup_filesystem};

// Re-export calibration for Zygote warm-up
pub use calibration::TlsCalibration;

// Re-export sandbox types and functions
pub use sandbox::{
    NetworkIsolationStatus, SandboxStatus, apply_iron_dome, apply_iron_dome_with_network,
    apply_landlock, apply_landlock_network, apply_seccomp, detect_landlock_abi,
    supports_landlock_network,
};
