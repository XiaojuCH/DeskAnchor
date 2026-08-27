//! Windows Explorer desktop capture and restoration.

mod model;

pub use model::{
    CoordinateSpace, DesktopIcon, DesktopState, DisplayConfiguration, Dpi, IconIdentity,
    IconIdentityKind, Monitor, MonitorIdentity, Rect, normalized_monitor_signature,
};

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
pub use restore::{RestoreFailure, RestoreResult, restore_snapshot};
