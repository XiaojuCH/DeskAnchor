//! Reusable domain and Windows integration code for DeskAnchor.

pub mod desktop;
pub mod snapshot;

#[cfg(windows)]
#[doc(hidden)]
pub mod verification;
