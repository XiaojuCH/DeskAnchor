use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Shell::SVSI_POSITIONITEM;

use super::discovery::{DesktopFolderView, on_shell_sta};
use super::icons::{LiveIcon, enumerate_icons};
use super::model::{DesktopState, DisplayConfiguration, IconIdentity};
use super::monitors::capture_monitors;
use crate::snapshot::{Snapshot, diff_desktop};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored: usize,
    pub unchanged: usize,
    pub skipped_missing: usize,
    pub skipped_ambiguous: usize,
    pub new_items: usize,
    pub failed: Vec<RestoreFailure>,
    pub blocked_display_mismatch: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreFailure {
    pub display_name: String,
    pub reason: String,
}

pub fn restore_snapshot(snapshot: &Snapshot) -> Result<RestoreResult> {
    snapshot.validate()?;
    let snapshot = snapshot.clone();
    on_shell_sta(move || restore_snapshot_sta(&snapshot))
}

fn restore_snapshot_sta(snapshot: &Snapshot) -> Result<RestoreResult> {
    let desktop = DesktopFolderView::open()?;
    let live_icons = enumerate_icons(&desktop)?;
    let current = DesktopState {
        display: DisplayConfiguration::new(capture_monitors()?),
        icons: live_icons.iter().map(|icon| icon.model.clone()).collect(),
    };
    let diff = diff_desktop(snapshot, &current);
    let mut result = RestoreResult {
        restored: 0,
        unchanged: diff.unchanged.len(),
        skipped_missing: diff.missing.len(),
        skipped_ambiguous: diff.ambiguous.len(),
        new_items: diff.new.len(),
        failed: Vec::new(),
        blocked_display_mismatch: !diff.display_matches,
    };
    if result.blocked_display_mismatch || diff.moved.is_empty() {
        return Ok(result);
    }

    let live_groups = group_live_icons(&live_icons);
    let mut move_indices = Vec::with_capacity(diff.moved.len());
    let mut pidls = Vec::with_capacity(diff.moved.len());
    let mut positions = Vec::with_capacity(diff.moved.len());
    for movement in &diff.moved {
        let Some(indices) = live_groups.get(&movement.identity) else {
            continue;
        };
        if indices.len() != 1 {
            continue;
        }
        let index = indices[0];
        move_indices.push((index, movement.snapshot));
        pidls.push(live_icons[index].pidl.as_ptr());
        positions.push(POINT {
            x: movement.snapshot.x,
            y: movement.snapshot.y,
        });
    }

    let positioning = unsafe {
        // SAFETY: every PIDL belongs to this live folder view, both arrays have
        // exactly `cidl` entries, and all allocations outlive the synchronous call.
        desktop.view.SelectAndPositionItems(
            pidls.len() as u32,
            pidls.as_ptr(),
            Some(positions.as_ptr()),
            SVSI_POSITIONITEM.0 as u32,
        )
    };
    if let Err(error) = positioning {
        let reason = format!("IFolderView::SelectAndPositionItems failed: {error}");
        result
            .failed
            .extend(move_indices.iter().map(|(index, _)| RestoreFailure {
                display_name: live_icons[*index].model.display_name.clone(),
                reason: reason.clone(),
            }));
        return Ok(result);
    }

    for (index, expected) in move_indices {
        let actual = unsafe {
            // SAFETY: the current item PIDL remains owned and valid for this view.
            desktop
                .view
                .GetItemPosition(live_icons[index].pidl.as_ptr())
        };
        match actual {
            Ok(actual) if actual.x == expected.x && actual.y == expected.y => {
                result.restored += 1;
            }
            Ok(actual) => result.failed.push(RestoreFailure {
                display_name: live_icons[index].model.display_name.clone(),
                reason: format!(
                    "Explorer reported position ({}, {}) instead of ({}, {})",
                    actual.x, actual.y, expected.x, expected.y
                ),
            }),
            Err(error) => result.failed.push(RestoreFailure {
                display_name: live_icons[index].model.display_name.clone(),
                reason: format!("failed to verify restored position: {error}"),
            }),
        }
    }
    Ok(result)
}

fn group_live_icons(icons: &[LiveIcon]) -> HashMap<IconIdentity, Vec<usize>> {
    let mut groups: HashMap<IconIdentity, Vec<usize>> = HashMap::new();
    for (index, icon) in icons.iter().enumerate() {
        groups
            .entry(icon.model.identity.clone())
            .or_default()
            .push(index);
    }
    groups
}
