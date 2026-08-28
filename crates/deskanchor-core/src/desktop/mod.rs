//! Windows Explorer desktop capture and restoration.

mod model;
mod settle;

pub use model::{
    CoordinateSpace, DesktopIcon, DesktopState, DisplayConfiguration, Dpi, IconIdentity,
    IconIdentityKind, Monitor, MonitorIdentity, Rect, normalized_monitor_signature,
};
pub use settle::{RestoreSettlePolicy, SettlePolicyError};

#[cfg(windows)]
mod discovery;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod monitors;
#[cfg(windows)]
mod restore;

#[cfg(windows)]
pub use icons::capture_current;
#[cfg(windows)]
pub(crate) use restore::restore_snapshot_subset;
#[cfg(windows)]
pub use restore::{
    RestoreFailure, RestoreOutcome, RestoreResult, RestoreVerification, VerificationStatus,
    restore_snapshot, restore_snapshot_with_policy,
};
