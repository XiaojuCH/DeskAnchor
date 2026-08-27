//! Developer-only destructive verification tooling.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::desktop::{
    DesktopIcon, RestoreOutcome, RestoreResult, capture_current, restore_snapshot,
    restore_snapshot_subset,
};
use crate::snapshot::{Snapshot, SnapshotDiffSummary, diff_desktop};

pub const DESTRUCTIVE_OPT_IN_ENV: &str = "DESKANCHOR_DESTRUCTIVE_TESTS";
pub const VERIFICATION_FAILPOINT_ENV: &str = "DESKANCHOR_VERIFICATION_FAILPOINT";
pub const AFTER_MUTATION_FAILPOINT: &str = "after-mutation";
pub const FIXTURE_A: &str = "DeskAnchor-Test-A.txt";
pub const FIXTURE_B: &str = "DeskAnchor-Test-B.txt";
const RECOVERY_FORMAT_VERSION: u32 = 1;
const ACTIVE_RECOVERY_FILE: &str = "active-recovery.json";

#[derive(Clone, Debug)]
pub struct VerificationRecoveryStore {
    root: PathBuf,
}

impl VerificationRecoveryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn local_default() -> Result<Self> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .context("LOCALAPPDATA is unavailable; cannot choose verification recovery storage")?;
        Ok(Self::new(
            PathBuf::from(local_app_data)
                .join("DeskAnchor")
                .join("verification"),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn active_path(&self) -> PathBuf {
        self.root.join(ACTIVE_RECOVERY_FILE)
    }

    fn begin(&self, snapshot: &Snapshot) -> Result<RecoveryRecord> {
        let active_path = self.active_path();
        ensure!(
            !active_path.exists(),
            "previous incomplete recovery snapshot exists at {}; run recover-last-verification before starting a new destructive test",
            active_path.display()
        );
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create verification recovery directory {}",
                self.root.display()
            )
        })?;
        let record = RecoveryRecord {
            format_version: RECOVERY_FORMAT_VERSION,
            verification_id: verification_id(snapshot),
            status: RecoveryStatus::Active,
            completed_at: None,
            snapshot: snapshot.clone(),
        };
        write_new_json_atomically(&active_path, &record).context(
            "failed to persist the recovery snapshot; no desktop mutation was attempted",
        )?;
        Ok(record)
    }

    fn load_active(&self) -> Result<RecoveryRecord> {
        let path = self.active_path();
        let json = fs::read_to_string(&path).with_context(|| {
            format!(
                "no readable incomplete recovery snapshot exists at {}",
                path.display()
            )
        })?;
        let record: RecoveryRecord = serde_json::from_str(&json)
            .with_context(|| format!("invalid recovery record at {}", path.display()))?;
        ensure!(
            record.format_version == RECOVERY_FORMAT_VERSION,
            "unsupported recovery record format {}",
            record.format_version
        );
        ensure!(
            record.status == RecoveryStatus::Active,
            "active recovery record has an invalid completed status"
        );
        record.snapshot.validate()?;
        Ok(record)
    }

    fn archive_completed(
        &self,
        record: &RecoveryRecord,
        status: RecoveryStatus,
    ) -> Result<PathBuf> {
        ensure!(
            status != RecoveryStatus::Active,
            "completion status is required"
        );
        let active = self.load_active()?;
        ensure!(
            active.verification_id == record.verification_id,
            "active recovery record changed while verification was running"
        );

        let mut completed = record.clone();
        completed.status = status;
        completed.completed_at = Some(now_rfc3339()?);
        let archive_root = self.root.join("records");
        fs::create_dir_all(&archive_root).with_context(|| {
            format!(
                "failed to create recovery archive {}",
                archive_root.display()
            )
        })?;
        let status_name = match status {
            RecoveryStatus::Verified => "verified",
            RecoveryStatus::RecoveredAfterFailure => "recovered-after-failure",
            RecoveryStatus::RecoveredByCommand => "recovered-by-command",
            RecoveryStatus::Active => bail!("completion status is required"),
        };
        let archive_path = allocate_archive_path(
            &archive_root,
            &format!("{}-{status_name}", completed.verification_id),
        )?;
        write_new_json_atomically(&archive_path, &completed)
            .context("failed to preserve completed recovery evidence")?;
        fs::remove_file(self.active_path()).context(
            "desktop recovery was verified and archived, but the active recovery marker could not be removed",
        )?;
        Ok(archive_path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryRecord {
    format_version: u32,
    verification_id: String,
    status: RecoveryStatus,
    completed_at: Option<String>,
    snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RecoveryStatus {
    Active,
    Verified,
    RecoveredAfterFailure,
    RecoveredByCommand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationFailpoint {
    Disabled,
    AfterMutation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AfterMutationAction {
    ContinueNormalRecovery,
    LeaveActiveForManualRecovery,
}

impl VerificationFailpoint {
    fn after_mutation_action(self) -> AfterMutationAction {
        match self {
            Self::Disabled => AfterMutationAction::ContinueNormalRecovery,
            Self::AfterMutation => AfterMutationAction::LeaveActiveForManualRecovery,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DestructiveVerificationSummary {
    pub monitor_count: usize,
    pub scale_percentages: Vec<Option<u32>>,
    pub icons_before: usize,
    pub mutation: RestoreResult,
    pub mutation_diff: SnapshotDiffSummary,
    pub recovery: RecoverySummary,
}

#[derive(Clone, Debug)]
pub struct RecoverySummary {
    pub restore: RestoreResult,
    pub final_diff: SnapshotDiffSummary,
    pub evidence_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct RecoveryRequiredSummary {
    pub monitor_count: usize,
    pub scale_percentages: Vec<Option<u32>>,
    pub icons_before: usize,
    pub mutation: RestoreResult,
    pub mutation_diff: SnapshotDiffSummary,
    pub active_recovery_path: PathBuf,
}

#[derive(Clone, Debug)]
#[must_use]
pub enum DestructiveVerificationRun {
    Verified(Box<DestructiveVerificationSummary>),
    RecoveryRequired(Box<RecoveryRequiredSummary>),
}

impl DestructiveVerificationSummary {
    pub fn print_human_readable(&self) {
        println!("DeskAnchor destructive round-trip verification\n");
        println!(
            "OS: {} ({})",
            std::env::consts::OS,
            std::env::var("PROCESSOR_ARCHITECTURE")
                .unwrap_or_else(|_| "unknown architecture".into())
        );
        println!(
            "Display configuration: {} monitor(s); scale percentages: {:?}",
            self.monitor_count, self.scale_percentages
        );
        println!("Icons before: {}\n", self.icons_before);
        println!("Fixture A: {FIXTURE_A}");
        println!("Fixture B: {FIXTURE_B}\n");
        println!("Mutation: PASS");
        println!("Restore immediate: PASS");
        println!("Settle verification: PASS");
        println!("attempts: {}", self.recovery.restore.verification.attempts);
        println!(
            "elapsed: {} ms\n",
            self.recovery.restore.verification.elapsed_ms
        );
        print_diff("Final diff", self.recovery.final_diff);
        println!("\nRecovery: CONFIRMED");
        println!("Evidence: {}", self.recovery.evidence_path.display());
        println!("\nRESULT: PASS");
    }
}

impl RecoverySummary {
    pub fn print_human_readable(&self) {
        println!("DeskAnchor verification recovery\n");
        println!("Recovery restore: PASS ({:?})", self.restore.outcome);
        let settle_result = match self.restore.verification.settle {
            crate::desktop::VerificationStatus::Passed => "PASS",
            crate::desktop::VerificationStatus::NotRequired => "NOT REQUIRED",
            crate::desktop::VerificationStatus::NotRun
            | crate::desktop::VerificationStatus::Failed => "UNEXPECTED",
        };
        println!("Settle verification: {settle_result}");
        println!("attempts: {}", self.restore.verification.attempts);
        println!("elapsed: {} ms\n", self.restore.verification.elapsed_ms);
        print_diff("Final diff", self.final_diff);
        println!("Final diff exact: true");
        println!("\nEvidence archived: {}", self.evidence_path.display());
        println!("Active recovery cleared: true");
        println!("\nRESULT: RECOVERED");
    }
}

impl RecoveryRequiredSummary {
    pub fn print_human_readable(&self) {
        println!("DeskAnchor destructive round-trip verification\n");
        println!(
            "OS: {} ({})",
            std::env::consts::OS,
            std::env::var("PROCESSOR_ARCHITECTURE")
                .unwrap_or_else(|_| "unknown architecture".into())
        );
        println!(
            "Display configuration: {} monitor(s); scale percentages: {:?}",
            self.monitor_count, self.scale_percentages
        );
        println!("Icons before: {}\n", self.icons_before);
        println!("Fixture A: {FIXTURE_A}");
        println!("Fixture B: {FIXTURE_B}\n");
        println!("Mutation: PASS");
        println!("Mutation immediate readback: PASS");
        println!("Settle verification: PASS");
        println!("attempts: {}", self.mutation.verification.attempts);
        println!("elapsed: {} ms", self.mutation.verification.elapsed_ms);
        println!("Mutation full diff exact: true");
        println!("\nRECOVERY FAILPOINT TRIGGERED");
        println!("Failpoint: {AFTER_MUTATION_FAILPOINT}");
        println!("Normal recovery intentionally skipped.");
        println!("Desktop is intentionally left in the mutated fixture state.\n");
        println!("Active recovery:");
        println!("{}\n", self.active_recovery_path.display());
        println!("Next command:");
        println!("deskanchor-verify.exe recover-last-verification");
        println!("\nRESULT: RECOVERY_REQUIRED");
    }
}

pub fn require_destructive_opt_in() -> Result<()> {
    require_destructive_opt_in_value(std::env::var(DESTRUCTIVE_OPT_IN_ENV).ok().as_deref())
}

pub fn run_destructive_roundtrip(
    store: VerificationRecoveryStore,
) -> Result<DestructiveVerificationRun> {
    let failpoint = verification_failpoint_from_values(
        std::env::var(DESTRUCTIVE_OPT_IN_ENV).ok().as_deref(),
        std::env::var(VERIFICATION_FAILPOINT_ENV).ok().as_deref(),
    )?;
    ensure!(
        !store.active_path().exists(),
        "previous incomplete recovery snapshot exists at {}; run recover-last-verification before starting a new destructive test",
        store.active_path().display()
    );

    let original = Snapshot::capture(capture_current()?)?;
    let first_index = unique_fixture_index(&original.icons, FIXTURE_A)?;
    let second_index = unique_fixture_index(&original.icons, FIXTURE_B)?;
    ensure!(
        first_index != second_index,
        "fixture identities unexpectedly resolved to the same desktop item"
    );
    ensure!(
        (original.icons[first_index].x, original.icons[first_index].y)
            != (
                original.icons[second_index].x,
                original.icons[second_index].y
            ),
        "fixtures must start at distinct positions; no desktop mutation was attempted"
    );

    let record = store.begin(&original)?;
    let mut guard = RecoveryGuard::armed(store, record);
    let pre_mutation = capture_current()?;
    let pre_mutation_diff = diff_desktop(&original, &pre_mutation).summary();
    ensure!(
        pre_mutation_diff.is_exact_match(),
        "desktop changed after the recovery snapshot was persisted; no fixture mutation was attempted: {pre_mutation_diff:?}"
    );
    let mut swapped = original.clone();
    let first_position = (swapped.icons[first_index].x, swapped.icons[first_index].y);
    let second_position = (swapped.icons[second_index].x, swapped.icons[second_index].y);
    (swapped.icons[first_index].x, swapped.icons[first_index].y) = second_position;
    (swapped.icons[second_index].x, swapped.icons[second_index].y) = first_position;

    let allowed_identities = HashSet::from([
        swapped.icons[first_index].identity.clone(),
        swapped.icons[second_index].identity.clone(),
    ]);
    let mutation = restore_snapshot_subset(&swapped, allowed_identities)?;
    ensure!(
        mutation.outcome == RestoreOutcome::Settled,
        "controlled fixture mutation did not settle: {:?}",
        mutation.outcome
    );
    ensure!(
        mutation.restored == 2,
        "controlled fixture mutation positioned {} items instead of exactly 2",
        mutation.restored
    );
    let mutated_state = capture_current()?;
    let mutation_diff = diff_desktop(&swapped, &mutated_state).summary();
    ensure!(
        mutation_diff.is_exact_match(),
        "controlled fixture mutation failed full recapture verification: {mutation_diff:?}"
    );

    let monitor_count = original.display.monitors.len();
    let scale_percentages = original
        .display
        .monitors
        .iter()
        .map(|monitor| monitor.scale_percent)
        .collect();
    let icons_before = original.icons.len();
    if failpoint.after_mutation_action() == AfterMutationAction::LeaveActiveForManualRecovery {
        let active_recovery_path = guard.disarm_for_manual_recovery()?;
        return Ok(DestructiveVerificationRun::RecoveryRequired(Box::new(
            RecoveryRequiredSummary {
                monitor_count,
                scale_percentages,
                icons_before,
                mutation,
                mutation_diff,
                active_recovery_path,
            },
        )));
    }
    let recovery = guard.recover_and_complete(RecoveryStatus::Verified)?;
    Ok(DestructiveVerificationRun::Verified(Box::new(
        DestructiveVerificationSummary {
            monitor_count,
            scale_percentages,
            icons_before,
            mutation,
            mutation_diff,
            recovery,
        },
    )))
}

pub fn recover_last_verification(store: VerificationRecoveryStore) -> Result<RecoverySummary> {
    require_destructive_opt_in()?;
    let record = store.load_active()?;
    recover_record(&store, &record, RecoveryStatus::RecoveredByCommand)
}

struct RecoveryGuard {
    store: VerificationRecoveryStore,
    record: Option<RecoveryRecord>,
    state: RecoveryGuardState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryGuardState {
    Armed,
    ManualRecoveryRequired,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryDropAction {
    RecoverAutomatically,
    LeaveActive,
}

impl RecoveryGuardState {
    fn drop_action(self) -> RecoveryDropAction {
        match self {
            Self::Armed => RecoveryDropAction::RecoverAutomatically,
            Self::ManualRecoveryRequired | Self::Completed => RecoveryDropAction::LeaveActive,
        }
    }
}

impl RecoveryGuard {
    fn armed(store: VerificationRecoveryStore, record: RecoveryRecord) -> Self {
        Self {
            store,
            record: Some(record),
            state: RecoveryGuardState::Armed,
        }
    }

    fn disarm_for_manual_recovery(&mut self) -> Result<PathBuf> {
        ensure!(
            self.state == RecoveryGuardState::Armed && self.record.is_some(),
            "recovery guard must be armed before entering manual recovery state"
        );
        self.state = RecoveryGuardState::ManualRecoveryRequired;
        Ok(self.store.active_path())
    }

    fn recover_and_complete(&mut self, status: RecoveryStatus) -> Result<RecoverySummary> {
        ensure!(
            self.state == RecoveryGuardState::Armed,
            "only an armed recovery guard can complete automatic recovery"
        );
        let record = self
            .record
            .as_ref()
            .context("recovery guard is no longer armed")?;
        let summary = recover_record(&self.store, record, status)?;
        self.record = None;
        self.state = RecoveryGuardState::Completed;
        Ok(summary)
    }
}

impl Drop for RecoveryGuard {
    fn drop(&mut self) {
        if self.state.drop_action() == RecoveryDropAction::LeaveActive {
            return;
        }
        let Some(record) = self.record.as_ref() else {
            return;
        };
        match recover_record(&self.store, record, RecoveryStatus::RecoveredAfterFailure) {
            Ok(summary) => eprintln!(
                "DeskAnchor recovery guard restored the original layout; evidence: {}",
                summary.evidence_path.display()
            ),
            Err(error) => eprintln!(
                "DeskAnchor recovery guard could not confirm restoration: {error:#}. The active recovery snapshot remains at {}",
                self.store.active_path().display()
            ),
        }
    }
}

fn require_destructive_opt_in_value(value: Option<&str>) -> Result<()> {
    ensure!(
        value == Some("1"),
        "destructive desktop verification is disabled; explicitly set {DESTRUCTIVE_OPT_IN_ENV}=1 after reviewing the safety instructions"
    );
    Ok(())
}

fn verification_failpoint_from_values(
    destructive_opt_in: Option<&str>,
    failpoint: Option<&str>,
) -> Result<VerificationFailpoint> {
    require_destructive_opt_in_value(destructive_opt_in)?;
    match failpoint {
        None => Ok(VerificationFailpoint::Disabled),
        Some(AFTER_MUTATION_FAILPOINT) => Ok(VerificationFailpoint::AfterMutation),
        Some(value) => bail!(
            "unknown {VERIFICATION_FAILPOINT_ENV} value {value:?}; only {AFTER_MUTATION_FAILPOINT:?} is supported"
        ),
    }
}

fn recover_record(
    store: &VerificationRecoveryStore,
    record: &RecoveryRecord,
    status: RecoveryStatus,
) -> Result<RecoverySummary> {
    let restore = restore_snapshot(&record.snapshot)?;
    if status == RecoveryStatus::Verified {
        ensure!(
            restore.outcome == RestoreOutcome::Settled,
            "normal verification recovery did not perform and settle a real restore: {:?}",
            restore.outcome
        );
    }
    ensure!(
        matches!(
            restore.outcome,
            RestoreOutcome::Settled | RestoreOutcome::NothingToRestore
        ),
        "original layout restore was not fully settled: {:?}",
        restore.outcome
    );
    let final_state = capture_current()?;
    let final_diff = diff_desktop(&record.snapshot, &final_state).summary();
    ensure!(
        final_diff.is_exact_match(),
        "original layout failed final full recapture verification: {final_diff:?}"
    );
    let evidence_path = store.archive_completed(record, status)?;
    Ok(RecoverySummary {
        restore,
        final_diff,
        evidence_path,
    })
}

fn unique_fixture_index(icons: &[DesktopIcon], fixture_name: &str) -> Result<usize> {
    let matches: Vec<_> = icons
        .iter()
        .enumerate()
        .filter(|(_, icon)| {
            Path::new(&icon.identity.value)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(fixture_name))
        })
        .map(|(index, _)| index)
        .collect();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => bail!(
            "required fixture {fixture_name} was not found; create it manually and place it at a distinct desktop position"
        ),
        _ => bail!(
            "fixture {fixture_name} is ambiguous ({} matches); no desktop mutation was attempted",
            matches.len()
        ),
    }
}

fn verification_id(snapshot: &Snapshot) -> String {
    let timestamp: String = snapshot
        .created_at
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!("verification-{timestamp}-{}", std::process::id())
}

fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format recovery completion timestamp")
}

fn allocate_archive_path(root: &Path, stem: &str) -> Result<PathBuf> {
    for suffix in 0..100_u8 {
        let name = if suffix == 0 {
            format!("{stem}.json")
        } else {
            format!("{stem}-{suffix}.json")
        };
        let path = root.join(name);
        if !path.exists() {
            return Ok(path);
        }
    }
    bail!("could not allocate a unique recovery evidence filename")
}

fn write_new_json_atomically(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .context("recovery record path has no parent directory")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("recovery record filename is not valid UTF-8")?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let json = serde_json::to_vec_pretty(value)?;
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        file.write_all(&json)
            .context("failed to write recovery JSON")?;
        file.sync_all().context("failed to flush recovery JSON")?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to atomically publish {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn print_diff(label: &str, summary: SnapshotDiffSummary) {
    println!("{label}:");
    println!("unchanged: {}", summary.unchanged);
    println!("moved: {}", summary.moved);
    println!("missing: {}", summary.missing);
    println!("new: {}", summary.new);
    println!("ambiguous: {}", summary.ambiguous);
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::desktop::{
        CoordinateSpace, DisplayConfiguration, IconIdentity, Monitor, MonitorIdentity, Rect,
    };

    fn sample_snapshot() -> Snapshot {
        let monitors = vec![Monitor {
            identity: MonitorIdentity {
                device_path: Some("monitor-a".into()),
                edid_manufacturer_id: None,
                edid_product_code_id: None,
                connector_instance: None,
            },
            device_name: r"\\.\DISPLAY1".into(),
            friendly_name: None,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            work_area: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            primary: true,
            scale_percent: Some(100),
            dpi: None,
        }];
        Snapshot {
            schema_version: crate::snapshot::CURRENT_SCHEMA_VERSION,
            created_at: "2026-08-27T01:02:03Z".into(),
            display: DisplayConfiguration {
                signature: crate::desktop::normalized_monitor_signature(&monitors),
                coordinate_space: CoordinateSpace::ExplorerDesktopView,
                monitors,
            },
            icons: vec![DesktopIcon {
                identity: IconIdentity::shell_parsing_name(
                    r"C:\Users\tester\Desktop\DeskAnchor-Test-A.txt".into(),
                ),
                display_name: FIXTURE_A.into(),
                x: 10,
                y: 20,
            }],
        }
    }

    #[test]
    fn active_recovery_record_blocks_a_second_verification() {
        let temporary = tempdir().expect("create temp directory");
        let store = VerificationRecoveryStore::new(temporary.path());
        let snapshot = sample_snapshot();
        let first = store.begin(&snapshot).expect("persist recovery record");
        assert!(store.begin(&snapshot).is_err());
        assert_eq!(store.load_active().expect("load recovery record"), first);
    }

    #[test]
    fn completed_recovery_is_archived_before_active_marker_is_removed() {
        let temporary = tempdir().expect("create temp directory");
        let store = VerificationRecoveryStore::new(temporary.path());
        let record = store.begin(&sample_snapshot()).expect("begin recovery");
        let archive = store
            .archive_completed(&record, RecoveryStatus::Verified)
            .expect("archive recovery");
        assert!(archive.exists());
        assert!(!store.active_path().exists());
        let archived: RecoveryRecord =
            serde_json::from_str(&fs::read_to_string(archive).expect("read archived recovery"))
                .expect("parse archived recovery");
        assert_eq!(archived.status, RecoveryStatus::Verified);
        assert!(archived.completed_at.is_some());
    }

    #[test]
    fn fixture_lookup_refuses_missing_and_duplicate_names() {
        let icon = sample_snapshot().icons.remove(0);
        assert!(unique_fixture_index(&[], FIXTURE_A).is_err());
        assert!(unique_fixture_index(&[icon.clone(), icon], FIXTURE_A).is_err());
    }

    #[test]
    fn failpoint_is_disabled_by_default() {
        assert_eq!(
            verification_failpoint_from_values(Some("1"), None)
                .expect("parse default verification settings"),
            VerificationFailpoint::Disabled
        );
        assert_eq!(
            VerificationFailpoint::Disabled.after_mutation_action(),
            AfterMutationAction::ContinueNormalRecovery
        );
    }

    #[test]
    fn after_mutation_failpoint_requires_destructive_opt_in() {
        let error = verification_failpoint_from_values(None, Some(AFTER_MUTATION_FAILPOINT))
            .expect_err("failpoint must require destructive opt-in");
        assert!(error.to_string().contains(DESTRUCTIVE_OPT_IN_ENV));

        assert_eq!(
            verification_failpoint_from_values(Some("1"), Some(AFTER_MUTATION_FAILPOINT))
                .expect("parse explicitly enabled failpoint"),
            VerificationFailpoint::AfterMutation
        );
    }

    #[test]
    fn unknown_failpoint_is_rejected() {
        let error = verification_failpoint_from_values(Some("1"), Some("unexpected"))
            .expect_err("unknown failpoint must fail closed");
        let message = error.to_string();
        assert!(message.contains(VERIFICATION_FAILPOINT_ENV));
        assert!(message.contains(AFTER_MUTATION_FAILPOINT));
    }

    #[test]
    fn only_after_mutation_failpoint_requests_manual_recovery() {
        assert_eq!(
            VerificationFailpoint::Disabled.after_mutation_action(),
            AfterMutationAction::ContinueNormalRecovery
        );
        assert_eq!(
            VerificationFailpoint::AfterMutation.after_mutation_action(),
            AfterMutationAction::LeaveActiveForManualRecovery
        );
        assert_eq!(
            RecoveryGuardState::Armed.drop_action(),
            RecoveryDropAction::RecoverAutomatically
        );
    }

    #[test]
    fn explicit_manual_recovery_state_disarms_raii_and_keeps_active_record() {
        let temporary = tempdir().expect("create temp directory");
        let store = VerificationRecoveryStore::new(temporary.path());
        let record = store.begin(&sample_snapshot()).expect("begin recovery");
        let active_path = store.active_path();
        {
            let mut guard = RecoveryGuard::armed(store.clone(), record);
            assert_eq!(guard.state, RecoveryGuardState::Armed);
            assert_eq!(
                guard
                    .disarm_for_manual_recovery()
                    .expect("enter manual recovery state"),
                active_path
            );
            assert_eq!(guard.state, RecoveryGuardState::ManualRecoveryRequired);
            assert!(guard.disarm_for_manual_recovery().is_err());
        }
        assert!(active_path.exists());
        assert!(!store.root().join("records").exists());
        assert_eq!(
            store.load_active().expect("active recovery remains").status,
            RecoveryStatus::Active
        );
    }
}
