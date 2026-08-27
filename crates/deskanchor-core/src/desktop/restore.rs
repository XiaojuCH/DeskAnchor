use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Shell::SVSI_POSITIONITEM;

use super::discovery::{DesktopFolderView, on_shell_sta};
use super::icons::{LiveIcon, capture_current_sta, enumerate_icons};
use super::model::{DesktopState, DisplayConfiguration, IconIdentity};
use super::monitors::capture_monitors;
use super::settle::{RestoreSettlePolicy, SettleDecision, SettleTracker};
use crate::snapshot::{Snapshot, SnapshotDiffSummary, diff_desktop};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RestoreOutcome {
    Settled,
    NothingToRestore,
    UnresolvedItems,
    BlockedDisplayMismatch,
    ShellPositioningFailed,
    ImmediateVerificationFailed,
    SettleVerificationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerificationStatus {
    NotRun,
    NotRequired,
    Passed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreVerification {
    pub immediate: VerificationStatus,
    pub settle: VerificationStatus,
    pub attempts: u32,
    pub elapsed_ms: u64,
    pub stable_observations: u32,
    pub required_stable_observations: u32,
    pub final_diff: Option<SnapshotDiffSummary>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub outcome: RestoreOutcome,
    pub restored: usize,
    pub unchanged: usize,
    pub skipped_missing: usize,
    pub skipped_ambiguous: usize,
    pub new_items: usize,
    pub failed: Vec<RestoreFailure>,
    /// Retained for existing callers; `outcome` is the authoritative status.
    pub blocked_display_mismatch: bool,
    pub verification: RestoreVerification,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreFailure {
    pub display_name: String,
    pub reason: String,
}

pub fn restore_snapshot(snapshot: &Snapshot) -> Result<RestoreResult> {
    restore_snapshot_with_policy(snapshot, RestoreSettlePolicy::default())
}

pub fn restore_snapshot_with_policy(
    snapshot: &Snapshot,
    policy: RestoreSettlePolicy,
) -> Result<RestoreResult> {
    snapshot.validate()?;
    policy.validate()?;
    let snapshot = snapshot.clone();
    on_shell_sta(move || restore_snapshot_sta(&snapshot, policy, None))
}

pub(crate) fn restore_snapshot_subset(
    snapshot: &Snapshot,
    allowed_identities: HashSet<IconIdentity>,
) -> Result<RestoreResult> {
    snapshot.validate()?;
    let policy = RestoreSettlePolicy::default();
    policy.validate()?;
    let snapshot = snapshot.clone();
    on_shell_sta(move || restore_snapshot_sta(&snapshot, policy, Some(&allowed_identities)))
}

fn restore_snapshot_sta(
    snapshot: &Snapshot,
    policy: RestoreSettlePolicy,
    allowed_identities: Option<&HashSet<IconIdentity>>,
) -> Result<RestoreResult> {
    let desktop = DesktopFolderView::open()?;
    let live_icons = enumerate_icons(&desktop)?;
    let current = DesktopState {
        display: DisplayConfiguration::new(capture_monitors()?),
        icons: live_icons.iter().map(|icon| icon.model.clone()).collect(),
    };
    let diff = diff_desktop(snapshot, &current);
    let initial_summary = diff.summary();
    let mut result = RestoreResult {
        outcome: RestoreOutcome::UnresolvedItems,
        restored: 0,
        unchanged: diff.unchanged.len(),
        skipped_missing: diff.missing.len(),
        skipped_ambiguous: diff.ambiguous.len(),
        new_items: diff.new.len(),
        failed: Vec::new(),
        blocked_display_mismatch: !diff.display_matches,
        verification: RestoreVerification {
            immediate: VerificationStatus::NotRun,
            settle: VerificationStatus::NotRun,
            attempts: 0,
            elapsed_ms: 0,
            stable_observations: 0,
            required_stable_observations: policy.required_stable_observations,
            final_diff: Some(initial_summary),
            error: None,
        },
    };
    if result.blocked_display_mismatch {
        result.outcome = RestoreOutcome::BlockedDisplayMismatch;
        result.verification.immediate = VerificationStatus::NotRequired;
        result.verification.settle = VerificationStatus::NotRequired;
        return Ok(result);
    }
    if diff.moved.is_empty() {
        result.outcome = if initial_summary.is_exact_match() {
            RestoreOutcome::NothingToRestore
        } else {
            RestoreOutcome::UnresolvedItems
        };
        result.verification.immediate = VerificationStatus::NotRequired;
        result.verification.settle = VerificationStatus::NotRequired;
        return Ok(result);
    }

    let live_groups = group_live_icons(&live_icons);
    let mut move_indices = Vec::with_capacity(diff.moved.len());
    let mut pidls = Vec::with_capacity(diff.moved.len());
    let mut positions = Vec::with_capacity(diff.moved.len());
    for movement in &diff.moved {
        if allowed_identities.is_some_and(|allowed| !allowed.contains(&movement.identity)) {
            continue;
        }
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

    if move_indices.is_empty() {
        result.outcome = RestoreOutcome::UnresolvedItems;
        result.verification.immediate = VerificationStatus::NotRequired;
        result.verification.settle = VerificationStatus::NotRequired;
        return Ok(result);
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
        result.outcome = RestoreOutcome::ShellPositioningFailed;
        return Ok(result);
    }

    for (index, expected) in &move_indices {
        let actual = unsafe {
            // SAFETY: the current item PIDL remains owned and valid for this view.
            desktop
                .view
                .GetItemPosition(live_icons[*index].pidl.as_ptr())
        };
        match actual {
            Ok(actual) if actual.x == expected.x && actual.y == expected.y => {
                result.restored += 1;
            }
            Ok(actual) => result.failed.push(RestoreFailure {
                display_name: live_icons[*index].model.display_name.clone(),
                reason: format!(
                    "Explorer reported position ({}, {}) instead of ({}, {})",
                    actual.x, actual.y, expected.x, expected.y
                ),
            }),
            Err(error) => result.failed.push(RestoreFailure {
                display_name: live_icons[*index].model.display_name.clone(),
                reason: format!("failed to verify restored position: {error}"),
            }),
        }
    }
    if !result.failed.is_empty() {
        result.outcome = RestoreOutcome::ImmediateVerificationFailed;
        result.verification.immediate = VerificationStatus::Failed;
        return Ok(result);
    }
    result.verification.immediate = VerificationStatus::Passed;

    // Drop item PIDLs and the original view before reacquiring complete desktop
    // states. Each settle observation therefore sees fresh Explorer objects.
    drop(live_icons);
    drop(desktop);
    settle_restore(snapshot, policy, &mut result);
    Ok(result)
}

fn settle_restore(snapshot: &Snapshot, policy: RestoreSettlePolicy, result: &mut RestoreResult) {
    let started = Instant::now();
    let mut tracker = SettleTracker::new(policy);
    loop {
        let current = match capture_current_sta() {
            Ok(current) => current,
            Err(error) => {
                result.outcome = RestoreOutcome::SettleVerificationFailed;
                result.verification.settle = VerificationStatus::Failed;
                result.verification.attempts = tracker.attempts();
                result.verification.elapsed_ms = elapsed_millis(started);
                result.verification.error = Some(format!(
                    "failed to recapture the complete desktop during settle verification: {error:#}"
                ));
                return;
            }
        };
        let summary = diff_desktop(snapshot, &current).summary();
        let elapsed_ms = elapsed_millis(started);
        let decision = tracker.observe(elapsed_ms, summary);
        result.verification.attempts = tracker.attempts();
        result.verification.elapsed_ms = elapsed_ms;
        result.verification.stable_observations = tracker.consecutive_stable();
        result.verification.final_diff = Some(summary);

        match decision {
            SettleDecision::Settled => {
                result.outcome = RestoreOutcome::Settled;
                result.verification.settle = VerificationStatus::Passed;
                return;
            }
            SettleDecision::DeadlineReached => {
                result.outcome = RestoreOutcome::SettleVerificationFailed;
                result.verification.settle = VerificationStatus::Failed;
                return;
            }
            SettleDecision::RetryAfter { wait_ms } => {
                thread::sleep(Duration::from_millis(wait_ms));
            }
        }
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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
