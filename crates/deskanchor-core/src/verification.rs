//! Developer-only destructive verification tooling.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::io::{self, ErrorKind};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::desktop::{
    DesktopIcon, DesktopState, DisplayConfiguration, IconIdentity, RestoreOutcome, RestoreResult,
    capture_current, restore_snapshot_subset,
};
use crate::snapshot::{IconPosition, Snapshot, SnapshotDiffSummary, diff_desktop};

pub const DESTRUCTIVE_OPT_IN_ENV: &str = "DESKANCHOR_DESTRUCTIVE_TESTS";
pub const VERIFICATION_FAILPOINT_ENV: &str = "DESKANCHOR_VERIFICATION_FAILPOINT";
pub const AFTER_MUTATION_FAILPOINT: &str = "after-mutation";
pub const FIXTURE_A: &str = "DeskAnchor-Test-A.txt";
pub const FIXTURE_B: &str = "DeskAnchor-Test-B.txt";
const RECOVERY_FORMAT_VERSION: u32 = 2;
const ACTIVE_RECOVERY_FILE: &str = "active-recovery.json";
const INTEGRITY_ALGORITHM: &str = "sha256";
const MAX_VERIFICATION_ID_LENGTH: usize = 128;
const WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Debug, Error)]
pub enum VerificationRecoveryError {
    #[error(
        "a verification recovery claim is already active at {path}; recover it before starting another destructive verification"
    )]
    AlreadyActive { path: PathBuf },
    #[error("the active verification recovery claim is not owned by this run")]
    OwnershipMismatch,
}

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

    fn begin(
        &self,
        snapshot: &Snapshot,
        fixture_desktop_path: &Path,
        fixture_allowlist: Vec<IconIdentity>,
        mutation: MutationMetadata,
    ) -> Result<RecoveryRecord> {
        let active_path = self.active_path();
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create verification recovery directory {}",
                self.root.display()
            )
        })?;
        let record = RecoveryRecord::new(
            snapshot.clone(),
            fixture_desktop_path.to_path_buf(),
            fixture_allowlist,
            mutation,
        )?;
        validate_fixture_files(&record)?;
        let json = serde_json::to_vec_pretty(&record)?;
        let mut file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&active_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return Err(VerificationRecoveryError::AlreadyActive { path: active_path }.into());
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to atomically claim verification recovery ownership at {}",
                        active_path.display()
                    )
                });
            }
        };
        // Once create_new succeeds, every error deliberately leaves the claim in
        // place. A partial record is unreadable, but it still blocks later runs.
        file.write_all(&json).context(
            "failed to write the claimed recovery record; the fail-closed claim remains",
        )?;
        file.sync_all().context(
            "failed to durably flush the claimed recovery record; the fail-closed claim remains",
        )?;
        drop(file);

        let persisted = self.load_active().context(
            "the recovery record could not be verified after durable persistence; no desktop mutation was attempted",
        )?;
        ensure!(
            persisted == record,
            "the recovery record changed during persistence; no desktop mutation was attempted"
        );
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
        record.validate_active()?;
        Ok(record)
    }

    fn assert_ownership(&self, record: &RecoveryRecord) -> Result<()> {
        let active = self.load_active()?;
        if active.claim() != record.claim() || active != *record {
            return Err(VerificationRecoveryError::OwnershipMismatch.into());
        }
        Ok(())
    }

    fn archive_completed(
        &self,
        record: &RecoveryRecord,
        status: RecoveryStatus,
    ) -> Result<PathBuf> {
        self.archive_completed_with_remove(record, status, |path| fs::remove_file(path))
    }

    fn archive_completed_with_remove<F>(
        &self,
        record: &RecoveryRecord,
        status: RecoveryStatus,
        remove_active: F,
    ) -> Result<PathBuf>
    where
        F: FnOnce(&Path) -> io::Result<()>,
    {
        ensure!(
            status != RecoveryStatus::Active,
            "completion status is required"
        );
        self.assert_ownership(record)?;

        let mut completed = record.clone();
        completed.status = status;
        completed.completed_at = Some(now_rfc3339()?);
        completed.reseal()?;
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
        validate_verification_id(&completed.verification_id)?;
        let stem = format!("{}-{status_name}", completed.verification_id);
        let archive_path = write_archive_exclusively(&archive_root, &stem, &completed)
            .context("failed to preserve completed recovery evidence")?;

        // Evidence must exist before the active claim is cleared. Rechecking the
        // complete record prevents this run from deleting another run's claim.
        self.assert_ownership(record)?;
        let active_path = self.active_path();
        remove_active(&active_path).context(
            "desktop recovery was verified and archived, but the active recovery claim could not be removed",
        )?;
        Ok(archive_path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryRecord {
    format_version: u32,
    verification_id: String,
    ownership_token: String,
    status: RecoveryStatus,
    completed_at: Option<String>,
    snapshot: Snapshot,
    fixture_desktop_path: PathBuf,
    fixture_allowlist: Vec<IconIdentity>,
    expected_display: DisplayConfiguration,
    mutation: MutationMetadata,
    integrity: RecoveryIntegrity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecoveryClaim {
    verification_id: String,
    ownership_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryIntegrity {
    algorithm: String,
    digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MutationMetadata {
    kind: MutationKind,
    targets: Vec<MutationTarget>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MutationKind {
    SwapFixturePositions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MutationTarget {
    identity: IconIdentity,
    position: IconPosition,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryIntegrityPayload<'a> {
    format_version: u32,
    verification_id: &'a str,
    ownership_token: &'a str,
    status: RecoveryStatus,
    completed_at: &'a Option<String>,
    snapshot: &'a Snapshot,
    fixture_desktop_path: &'a Path,
    fixture_allowlist: &'a [IconIdentity],
    expected_display: &'a DisplayConfiguration,
    mutation: &'a MutationMetadata,
}

impl RecoveryRecord {
    fn new(
        snapshot: Snapshot,
        fixture_desktop_path: PathBuf,
        fixture_allowlist: Vec<IconIdentity>,
        mutation: MutationMetadata,
    ) -> Result<Self> {
        let mut record = Self {
            format_version: RECOVERY_FORMAT_VERSION,
            verification_id: verification_id(&snapshot),
            ownership_token: Uuid::new_v4().simple().to_string(),
            status: RecoveryStatus::Active,
            completed_at: None,
            expected_display: snapshot.display.clone(),
            snapshot,
            fixture_desktop_path,
            fixture_allowlist,
            mutation,
            integrity: RecoveryIntegrity {
                algorithm: INTEGRITY_ALGORITHM.into(),
                digest: String::new(),
            },
        };
        record.reseal()?;
        record.validate_active()?;
        Ok(record)
    }

    fn claim(&self) -> RecoveryClaim {
        RecoveryClaim {
            verification_id: self.verification_id.clone(),
            ownership_token: self.ownership_token.clone(),
        }
    }

    fn validate_active(&self) -> Result<()> {
        ensure!(
            self.format_version == RECOVERY_FORMAT_VERSION,
            "unsupported recovery record format {}",
            self.format_version
        );
        validate_verification_id(&self.verification_id)?;
        validate_ownership_token(&self.ownership_token)?;
        self.validate_integrity()?;
        ensure!(
            self.status == RecoveryStatus::Active,
            "active recovery record has an invalid completed status"
        );
        ensure!(
            self.completed_at.is_none(),
            "active recovery record unexpectedly has a completion timestamp"
        );
        self.snapshot.validate()?;
        ensure!(
            self.expected_display == self.snapshot.display,
            "recovery record expected display does not match its original snapshot"
        );
        validate_fixture_plan(self)?;
        Ok(())
    }

    fn integrity_payload(&self) -> RecoveryIntegrityPayload<'_> {
        RecoveryIntegrityPayload {
            format_version: self.format_version,
            verification_id: &self.verification_id,
            ownership_token: &self.ownership_token,
            status: self.status,
            completed_at: &self.completed_at,
            snapshot: &self.snapshot,
            fixture_desktop_path: &self.fixture_desktop_path,
            fixture_allowlist: &self.fixture_allowlist,
            expected_display: &self.expected_display,
            mutation: &self.mutation,
        }
    }

    fn reseal(&mut self) -> Result<()> {
        self.integrity.algorithm = INTEGRITY_ALGORITHM.into();
        self.integrity.digest = digest_payload(&self.integrity_payload())?;
        Ok(())
    }

    fn validate_integrity(&self) -> Result<()> {
        ensure!(
            self.integrity.algorithm == INTEGRITY_ALGORITHM,
            "unsupported recovery integrity algorithm"
        );
        ensure!(
            self.integrity.digest.len() == 64
                && self
                    .integrity
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "invalid recovery integrity data"
        );
        let expected = digest_payload(&self.integrity_payload())?;
        ensure!(
            self.integrity.digest == expected,
            "recovery record integrity verification failed"
        );
        Ok(())
    }
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

    let original = Snapshot::capture(capture_current()?)?;
    let fixture_desktop_path = dirs::desktop_dir()
        .context("the current user's Desktop known-folder path is unavailable")?;
    let first_index = unique_fixture_index(&original.icons, FIXTURE_A, &fixture_desktop_path)?;
    let second_index = unique_fixture_index(&original.icons, FIXTURE_B, &fixture_desktop_path)?;
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

    let mut swapped = original.clone();
    let first_position = IconPosition {
        x: swapped.icons[first_index].x,
        y: swapped.icons[first_index].y,
    };
    let second_position = IconPosition {
        x: swapped.icons[second_index].x,
        y: swapped.icons[second_index].y,
    };
    swapped.icons[first_index].x = second_position.x;
    swapped.icons[first_index].y = second_position.y;
    swapped.icons[second_index].x = first_position.x;
    swapped.icons[second_index].y = first_position.y;
    let fixture_allowlist = vec![
        swapped.icons[first_index].identity.clone(),
        swapped.icons[second_index].identity.clone(),
    ];
    let mutation_metadata = MutationMetadata {
        kind: MutationKind::SwapFixturePositions,
        targets: vec![
            MutationTarget {
                identity: swapped.icons[first_index].identity.clone(),
                position: second_position,
            },
            MutationTarget {
                identity: swapped.icons[second_index].identity.clone(),
                position: first_position,
            },
        ],
    };
    let record = store.begin(
        &original,
        &fixture_desktop_path,
        fixture_allowlist.clone(),
        mutation_metadata,
    )?;
    let mut guard = RecoveryGuard::armed(store, record);
    guard.assert_ownership()?;
    let pre_mutation = capture_current()?;
    let pre_mutation_diff = diff_desktop(&original, &pre_mutation).summary();
    ensure!(
        pre_mutation_diff.is_exact_match(),
        "desktop changed after the recovery snapshot was persisted; no fixture mutation was attempted: {pre_mutation_diff:?}"
    );

    // This is the only normal verification mutation. Revalidate the persisted
    // claim immediately before granting the stored two-identity capability.
    guard.assert_ownership()?;
    let allowed_identities = fixture_allowlist.into_iter().collect();
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
        self.assert_ownership()?;
        self.state = RecoveryGuardState::ManualRecoveryRequired;
        Ok(self.store.active_path())
    }

    fn assert_ownership(&self) -> Result<()> {
        let record = self
            .record
            .as_ref()
            .context("recovery guard is no longer armed")?;
        self.store.assert_ownership(record)
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
    store.assert_ownership(record)?;
    validate_current_fixture_desktop(record)?;
    let current = capture_current()?;
    validate_recovery_preflight(record, &current)?;

    // Recovery is a desktop mutation too. The claim and complete sealed record
    // must still belong to this run immediately before the fixture-only write.
    store.assert_ownership(record)?;
    let allowed_identities = record.fixture_allowlist.iter().cloned().collect();
    let restore = restore_snapshot_subset(&record.snapshot, allowed_identities)?;
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

fn validate_current_fixture_desktop(record: &RecoveryRecord) -> Result<()> {
    let current_desktop = dirs::desktop_dir()
        .context("the current user's Desktop known-folder path is unavailable")?;
    let stored = fs::canonicalize(&record.fixture_desktop_path)
        .context("the stored fixture Desktop path is no longer resolvable")?;
    let current = fs::canonicalize(current_desktop)
        .context("the current user's Desktop known-folder path is not resolvable")?;
    ensure!(
        windows_paths_equal(&stored, &current),
        "the current user's Desktop fixture path changed since verification began; no desktop mutation was attempted"
    );
    Ok(())
}

fn validate_recovery_preflight(record: &RecoveryRecord, current: &DesktopState) -> Result<()> {
    record.validate_active()?;
    validate_fixture_files(record)?;
    ensure!(
        current.display.coordinate_space == record.expected_display.coordinate_space
            && current.display.signature == record.expected_display.signature,
        "recovery blocked because the current display configuration is incompatible; no desktop mutation was attempted"
    );

    for identity in &record.fixture_allowlist {
        let occurrences = current
            .icons
            .iter()
            .filter(|icon| icon.identity == *identity)
            .count();
        ensure!(
            occurrences == 1,
            "recovery fixture identities are not uniquely resolvable; no desktop mutation was attempted"
        );
    }

    let allowlist: HashSet<_> = record.fixture_allowlist.iter().cloned().collect();
    let non_fixture_snapshot = Snapshot {
        icons: record
            .snapshot
            .icons
            .iter()
            .filter(|icon| !allowlist.contains(&icon.identity))
            .cloned()
            .collect(),
        ..record.snapshot.clone()
    };
    let non_fixture_current = DesktopState {
        display: current.display.clone(),
        icons: current
            .icons
            .iter()
            .filter(|icon| !allowlist.contains(&icon.identity))
            .cloned()
            .collect(),
    };
    let summary = diff_desktop(&non_fixture_snapshot, &non_fixture_current).summary();
    ensure!(
        summary.is_exact_match(),
        "external desktop drift detected before recovery; no desktop mutation was attempted (moved {}, missing {}, new {}, ambiguous {})",
        summary.moved,
        summary.missing,
        summary.new,
        summary.ambiguous
    );
    Ok(())
}

fn validate_fixture_plan(record: &RecoveryRecord) -> Result<()> {
    ensure!(
        record.fixture_desktop_path.is_absolute(),
        "recovery fixture Desktop path must be absolute"
    );
    ensure!(
        record.fixture_allowlist.len() == 2,
        "recovery fixture allowlist must contain exactly two identities"
    );
    let unique_allowlist: HashSet<_> = record.fixture_allowlist.iter().collect();
    ensure!(
        unique_allowlist.len() == 2,
        "recovery fixture identities must be unique"
    );

    let mut fixture_names = HashSet::new();
    let mut original_positions = HashMap::new();
    for identity in &record.fixture_allowlist {
        let fixture_name = expected_fixture_name(identity, &record.fixture_desktop_path)
            .context("recovery fixture identity is outside the stored Desktop fixture paths")?;
        fixture_names.insert(fixture_name);
        let snapshot_matches: Vec<_> = record
            .snapshot
            .icons
            .iter()
            .filter(|icon| icon.identity == *identity)
            .collect();
        let [icon] = snapshot_matches.as_slice() else {
            bail!("each recovery fixture identity must occur exactly once in the original snapshot")
        };
        original_positions.insert(
            identity,
            IconPosition {
                x: icon.x,
                y: icon.y,
            },
        );
    }
    ensure!(
        fixture_names == HashSet::from([FIXTURE_A, FIXTURE_B]),
        "recovery fixture allowlist must contain only the two named fixtures"
    );

    ensure!(
        record.mutation.targets.len() == 2,
        "fixture mutation metadata must contain exactly two targets"
    );
    let mutation_targets: HashMap<_, _> = record
        .mutation
        .targets
        .iter()
        .map(|target| (&target.identity, target.position))
        .collect();
    ensure!(
        mutation_targets.len() == 2
            && mutation_targets
                .keys()
                .all(|identity| unique_allowlist.contains(identity)),
        "fixture mutation targets must exactly match the recovery allowlist"
    );

    let first_identity = &record.fixture_allowlist[0];
    let second_identity = &record.fixture_allowlist[1];
    let first_original = original_positions[first_identity];
    let second_original = original_positions[second_identity];
    ensure!(
        first_original != second_original,
        "recovery fixtures must have distinct original coordinates"
    );
    ensure!(
        mutation_targets[first_identity] == second_original
            && mutation_targets[second_identity] == first_original,
        "fixture mutation metadata must describe an exact swap of the original fixture coordinates"
    );
    Ok(())
}

fn validate_fixture_files(record: &RecoveryRecord) -> Result<()> {
    for identity in &record.fixture_allowlist {
        let fixture_name = expected_fixture_name(identity, &record.fixture_desktop_path)
            .context("recovery fixture identity is outside the stored Desktop fixture paths")?;
        validate_fixture_file(identity, fixture_name, &record.fixture_desktop_path)?;
    }
    Ok(())
}

fn unique_fixture_index(
    icons: &[DesktopIcon],
    fixture_name: &str,
    fixture_desktop_path: &Path,
) -> Result<usize> {
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
        [index] => {
            validate_fixture_file(&icons[*index].identity, fixture_name, fixture_desktop_path)?;
            Ok(*index)
        }
        [] => bail!(
            "required fixture {fixture_name} was not found; create it manually and place it at a distinct desktop position"
        ),
        _ => bail!(
            "fixture {fixture_name} is ambiguous ({} matches); no desktop mutation was attempted",
            matches.len()
        ),
    }
}

fn validate_fixture_file(
    identity: &IconIdentity,
    fixture_name: &str,
    fixture_desktop_path: &Path,
) -> Result<()> {
    let candidate = Path::new(&identity.value);
    ensure!(
        candidate.is_absolute(),
        "fixture {fixture_name} is not backed by an absolute filesystem path"
    );
    let expected = fixture_desktop_path.join(fixture_name);
    let metadata = fs::symlink_metadata(candidate)
        .with_context(|| format!("fixture {fixture_name} is not a readable filesystem item"))?;
    ensure!(
        metadata.file_type().is_file(),
        "fixture {fixture_name} must be a regular file"
    );
    ensure!(
        metadata.file_attributes() & WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "fixture {fixture_name} must not be a symlink or reparse-point indirection"
    );
    let candidate = fs::canonicalize(candidate)
        .with_context(|| format!("failed to resolve fixture {fixture_name}"))?;
    let expected = fs::canonicalize(&expected).with_context(|| {
        format!("the expected user Desktop fixture {fixture_name} does not exist")
    })?;
    ensure!(
        windows_paths_equal(&candidate, &expected),
        "fixture {fixture_name} is not the regular file at the current user's Desktop fixture path"
    );
    Ok(())
}

fn expected_fixture_name(identity: &IconIdentity, desktop: &Path) -> Option<&'static str> {
    [FIXTURE_A, FIXTURE_B].into_iter().find(|fixture_name| {
        windows_paths_equal(Path::new(&identity.value), &desktop.join(fixture_name))
    })
}

fn windows_paths_equal(left: &Path, right: &Path) -> bool {
    normalize_windows_path(left) == normalize_windows_path(right)
}

fn normalize_windows_path(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_owned();
    }
    value.trim_end_matches('\\').to_ascii_lowercase()
}

fn verification_id(snapshot: &Snapshot) -> String {
    let timestamp: String = snapshot
        .created_at
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!(
        "verification-{timestamp}-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    )
}

fn validate_verification_id(value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "verification ID must not be empty");
    ensure!(
        value.len() <= MAX_VERIFICATION_ID_LENGTH,
        "verification ID is too long"
    );
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "verification ID contains unsafe characters"
    );
    ensure!(
        !value.contains(".."),
        "verification ID must not contain a traversal segment"
    );
    Ok(())
}

fn validate_ownership_token(value: &str) -> Result<()> {
    ensure!(
        value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "recovery ownership token is invalid"
    );
    Ok(())
}

fn digest_payload(value: &impl Serialize) -> Result<String> {
    let bytes =
        serde_json::to_vec(value).context("failed to serialize recovery integrity payload")?;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").context("failed to encode recovery integrity digest")?;
    }
    Ok(encoded)
}

fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format recovery completion timestamp")
}

fn write_archive_exclusively(root: &Path, stem: &str, value: &impl Serialize) -> Result<PathBuf> {
    let json = serde_json::to_vec_pretty(value)?;
    for suffix in 0..100_u8 {
        let name = if suffix == 0 {
            format!("{stem}.json")
        } else {
            format!("{stem}-{suffix}.json")
        };
        let path = root.join(name);
        let mut file = match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to create {}", path.display()));
            }
        };
        file.write_all(&json)
            .with_context(|| format!("failed to write {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush {}", path.display()))?;
        return Ok(path);
    }
    bail!("could not allocate a unique recovery evidence filename")
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
    use std::sync::{Arc, Barrier};
    use std::thread;

    use serde_json::Value;
    use tempfile::tempdir;

    use super::*;
    use crate::desktop::{
        CoordinateSpace, DisplayConfiguration, IconIdentity, Monitor, MonitorIdentity, Rect,
    };

    fn create_fixture_files(desktop: &Path) {
        fs::create_dir_all(desktop).expect("create fixture Desktop");
        fs::write(desktop.join(FIXTURE_A), []).expect("create fixture A");
        fs::write(desktop.join(FIXTURE_B), []).expect("create fixture B");
    }

    fn icon(path: PathBuf, display_name: &str, x: i32, y: i32) -> DesktopIcon {
        DesktopIcon {
            identity: IconIdentity::shell_parsing_name(path.to_string_lossy().into_owned()),
            display_name: display_name.into(),
            x,
            y,
        }
    }

    fn sample_snapshot(desktop: &Path) -> Snapshot {
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
            icons: vec![
                icon(desktop.join(FIXTURE_A), FIXTURE_A, 10, 20),
                icon(desktop.join(FIXTURE_B), FIXTURE_B, 110, 120),
                icon(desktop.join("Other.txt"), "Other.txt", 210, 220),
            ],
        }
    }

    fn fixture_plan(snapshot: &Snapshot) -> (Vec<IconIdentity>, MutationMetadata) {
        let fixture_allowlist = vec![
            snapshot.icons[0].identity.clone(),
            snapshot.icons[1].identity.clone(),
        ];
        let mutation = MutationMetadata {
            kind: MutationKind::SwapFixturePositions,
            targets: vec![
                MutationTarget {
                    identity: fixture_allowlist[0].clone(),
                    position: IconPosition {
                        x: snapshot.icons[1].x,
                        y: snapshot.icons[1].y,
                    },
                },
                MutationTarget {
                    identity: fixture_allowlist[1].clone(),
                    position: IconPosition {
                        x: snapshot.icons[0].x,
                        y: snapshot.icons[0].y,
                    },
                },
            ],
        };
        (fixture_allowlist, mutation)
    }

    fn begin_record(
        store: &VerificationRecoveryStore,
        snapshot: &Snapshot,
        desktop: &Path,
    ) -> RecoveryRecord {
        let (allowlist, mutation) = fixture_plan(snapshot);
        store
            .begin(snapshot, desktop, allowlist, mutation)
            .expect("begin recovery")
    }

    fn current_from(snapshot: &Snapshot) -> DesktopState {
        DesktopState {
            display: snapshot.display.clone(),
            icons: snapshot.icons.clone(),
        }
    }

    fn rewrite_active(store: &VerificationRecoveryStore, value: &Value) {
        fs::write(
            store.active_path(),
            serde_json::to_vec_pretty(value).expect("serialize tampered record"),
        )
        .expect("rewrite active record");
    }

    fn tamper_active(store: &VerificationRecoveryStore, mutate: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_str(
            &fs::read_to_string(store.active_path()).expect("read active record"),
        )
        .expect("parse active record as value");
        mutate(&mut value);
        rewrite_active(store, &value);
    }

    #[test]
    fn concurrent_begin_allows_exactly_one_atomic_claim() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let store = store.clone();
            let snapshot = snapshot.clone();
            let desktop = desktop.clone();
            workers.push(thread::spawn(move || {
                let (allowlist, mutation) = fixture_plan(&snapshot);
                barrier.wait();
                store.begin(&snapshot, &desktop, allowlist, mutation)
            }));
        }
        barrier.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("join begin worker"))
            .collect();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let failure = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one begin must fail");
        assert!(matches!(
            failure.downcast_ref::<VerificationRecoveryError>(),
            Some(VerificationRecoveryError::AlreadyActive { .. })
        ));
        assert!(store.load_active().is_ok());
    }

    #[test]
    fn partial_crash_claim_blocks_a_new_begin_fail_closed() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        fs::create_dir_all(store.root()).expect("create recovery root");
        fs::write(store.active_path(), b"{\"formatVersion\":").expect("write partial crash claim");
        let snapshot = sample_snapshot(&desktop);
        let (allowlist, mutation) = fixture_plan(&snapshot);

        let error = store
            .begin(&snapshot, &desktop, allowlist, mutation)
            .expect_err("partial claim must block a new run");
        assert!(matches!(
            error.downcast_ref::<VerificationRecoveryError>(),
            Some(VerificationRecoveryError::AlreadyActive { .. })
        ));
        assert!(store.load_active().is_err());
        assert!(store.active_path().exists());
    }

    #[test]
    fn ownership_mismatch_cannot_authorize_mutation() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let record = begin_record(&store, &snapshot, &desktop);
        let mut impostor = record.clone();
        impostor.ownership_token = Uuid::new_v4().simple().to_string();
        impostor.reseal().expect("reseal impostor record");

        let error = store
            .assert_ownership(&impostor)
            .expect_err("ownership mismatch must fail closed");
        assert!(matches!(
            error.downcast_ref::<VerificationRecoveryError>(),
            Some(VerificationRecoveryError::OwnershipMismatch)
        ));
        assert_eq!(store.load_active().expect("original claim remains"), record);
    }

    #[test]
    fn one_run_cannot_archive_or_clear_another_runs_claim() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let first = begin_record(&store, &snapshot, &desktop);
        let (allowlist, mutation) = fixture_plan(&snapshot);
        let second = RecoveryRecord::new(snapshot, desktop, allowlist, mutation)
            .expect("create second run record");
        fs::remove_file(store.active_path()).expect("replace active claim for test");
        rewrite_active(
            &store,
            &serde_json::to_value(&second).expect("serialize second claim"),
        );

        assert!(
            store
                .archive_completed(&first, RecoveryStatus::Verified)
                .is_err()
        );
        assert_eq!(store.load_active().expect("second claim remains"), second);
        assert!(!store.root().join("records").exists());
    }

    #[test]
    fn completed_recovery_is_archived_before_active_marker_is_removed() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
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
        archived
            .validate_integrity()
            .expect("archive remains sealed");
    }

    #[test]
    fn archive_failure_leaves_active_claim() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
        fs::write(store.root().join("records"), b"not a directory")
            .expect("block archive directory creation");

        assert!(
            store
                .archive_completed(&record, RecoveryStatus::Verified)
                .is_err()
        );
        assert_eq!(store.load_active().expect("active claim remains"), record);
    }

    #[test]
    fn active_claim_removal_failure_is_reported_and_leaves_claim() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let record = begin_record(&store, &sample_snapshot(&desktop), &desktop);

        let error = store
            .archive_completed_with_remove(&record, RecoveryStatus::Verified, |_| {
                Err(io::Error::new(
                    ErrorKind::PermissionDenied,
                    "injected removal failure",
                ))
            })
            .expect_err("removal failure must not report completion");
        assert!(error.to_string().contains("could not be removed"));
        assert_eq!(store.load_active().expect("active claim remains"), record);
        assert_eq!(
            fs::read_dir(store.root().join("records"))
                .expect("read records")
                .count(),
            1
        );
    }

    #[test]
    fn recovery_preflight_accepts_fixture_only_mutation_and_two_item_allowlist() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let record = begin_record(&store, &snapshot, &desktop);
        let mut current = current_from(&snapshot);
        let first = (current.icons[0].x, current.icons[0].y);
        let second = (current.icons[1].x, current.icons[1].y);
        (current.icons[0].x, current.icons[0].y) = second;
        (current.icons[1].x, current.icons[1].y) = first;

        validate_recovery_preflight(&record, &current)
            .expect("fixture-only mutation is recoverable");
        assert_eq!(record.fixture_allowlist.len(), 2);
        assert_eq!(
            record
                .fixture_allowlist
                .iter()
                .collect::<HashSet<_>>()
                .len(),
            2
        );
    }

    #[test]
    fn recovery_preflight_rejects_moved_non_fixture() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let record = begin_record(&store, &snapshot, &desktop);
        let mut current = current_from(&snapshot);
        current.icons[2].x += 50;

        let error = validate_recovery_preflight(&record, &current)
            .expect_err("external movement must fail closed");
        assert!(
            error
                .to_string()
                .contains("external desktop drift detected")
        );
        assert!(store.active_path().exists());
    }

    #[test]
    fn recovery_preflight_rejects_missing_and_new_non_fixture_items() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let record = begin_record(&store, &snapshot, &desktop);

        let mut missing = current_from(&snapshot);
        missing.icons.remove(2);
        assert!(validate_recovery_preflight(&record, &missing).is_err());

        let mut added = current_from(&snapshot);
        added
            .icons
            .push(icon(desktop.join("New.txt"), "New.txt", 300, 400));
        assert!(validate_recovery_preflight(&record, &added).is_err());
        assert!(store.active_path().exists());
    }

    #[test]
    fn recovery_preflight_rejects_an_unresolvable_fixture_identity() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let record = begin_record(&store, &snapshot, &desktop);
        let mut current = current_from(&snapshot);
        current.icons.remove(0);

        let error = validate_recovery_preflight(&record, &current)
            .expect_err("unresolvable fixture must fail closed");
        assert!(error.to_string().contains("not uniquely resolvable"));
        assert!(store.active_path().exists());
    }

    #[test]
    fn recovery_preflight_reports_ambiguous_non_fixture_summary() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let snapshot = sample_snapshot(&desktop);
        let record = begin_record(&store, &snapshot, &desktop);
        let mut current = current_from(&snapshot);
        current.icons.push(current.icons[2].clone());

        let error =
            validate_recovery_preflight(&record, &current).expect_err("ambiguity must fail closed");
        assert!(error.to_string().contains("ambiguous 1"));
    }

    #[test]
    fn regular_fixture_is_accepted() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let snapshot = sample_snapshot(&desktop);
        assert_eq!(
            unique_fixture_index(&snapshot.icons, FIXTURE_A, &desktop)
                .expect("regular Desktop fixture is valid"),
            0
        );
    }

    #[test]
    fn directory_fixture_is_rejected() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        fs::create_dir_all(desktop.join(FIXTURE_A)).expect("create fixture directory");
        fs::write(desktop.join(FIXTURE_B), []).expect("create fixture B");
        let snapshot = sample_snapshot(&desktop);
        let error = unique_fixture_index(&snapshot.icons, FIXTURE_A, &desktop)
            .expect_err("directory must not be accepted as fixture");
        assert!(error.to_string().contains("regular file"));
    }

    #[test]
    fn fixture_at_wrong_desktop_path_is_rejected() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        let elsewhere = temporary.path().join("Elsewhere");
        create_fixture_files(&desktop);
        fs::create_dir_all(&elsewhere).expect("create other directory");
        fs::write(elsewhere.join(FIXTURE_A), []).expect("create wrong fixture");
        let wrong = icon(elsewhere.join(FIXTURE_A), FIXTURE_A, 10, 20);

        let error = unique_fixture_index(&[wrong], FIXTURE_A, &desktop)
            .expect_err("wrong Desktop path must fail closed");
        assert!(error.to_string().contains("current user's Desktop"));
    }

    #[test]
    fn duplicate_fixture_basename_is_rejected() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        let public = temporary.path().join("PublicDesktop");
        create_fixture_files(&desktop);
        fs::create_dir_all(&public).expect("create public Desktop");
        fs::write(public.join(FIXTURE_A), []).expect("create duplicate fixture");
        let mut snapshot = sample_snapshot(&desktop);
        snapshot
            .icons
            .push(icon(public.join(FIXTURE_A), FIXTURE_A, 300, 400));

        let error = unique_fixture_index(&snapshot.icons, FIXTURE_A, &desktop)
            .expect_err("duplicate basename must fail closed");
        assert!(error.to_string().contains("ambiguous"));
    }

    #[test]
    fn invalid_verification_ids_are_rejected() {
        for value in ["../foo", r"..\foo", r"C:\foo", "foo/bar", r"foo\bar", ""] {
            assert!(
                validate_verification_id(value).is_err(),
                "unsafe ID {value:?} was accepted"
            );
        }
        assert!(validate_verification_id(&"a".repeat(MAX_VERIFICATION_ID_LENGTH + 1)).is_err());
    }

    #[test]
    fn generated_verification_id_is_strictly_valid() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
        validate_verification_id(&record.verification_id).expect("generated ID is safe");
    }

    #[test]
    fn edited_fixture_coordinate_is_rejected() {
        assert_tamper_rejected(|value| value["snapshot"]["icons"][0]["x"] = 999.into());
    }

    #[test]
    fn edited_non_fixture_coordinate_is_rejected() {
        assert_tamper_rejected(|value| value["snapshot"]["icons"][2]["y"] = 999.into());
    }

    #[test]
    fn edited_fixture_identity_is_rejected() {
        assert_tamper_rejected(|value| {
            value["fixtureAllowlist"][0]["value"] = "C:\\Desktop\\edited.txt".into();
        });
    }

    #[test]
    fn edited_verification_id_is_rejected() {
        assert_tamper_rejected(|value| {
            value["verificationId"] = "verification-valid-but-edited".into();
        });
    }

    #[test]
    fn edited_mutation_metadata_is_rejected() {
        assert_tamper_rejected(|value| {
            value["mutation"]["targets"][0]["position"]["x"] = 777.into();
        });
    }

    #[test]
    fn invalid_integrity_data_is_rejected() {
        assert_tamper_rejected(|value| value["integrity"]["digest"] = "00".repeat(32).into());
    }

    fn assert_tamper_rejected(mutate: impl FnOnce(&mut Value)) {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let _record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
        tamper_active(&store, mutate);
        assert!(store.load_active().is_err());
        assert!(store.active_path().exists());
    }

    #[test]
    fn truncated_recovery_record_is_rejected() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let _record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
        fs::write(store.active_path(), b"{\"formatVersion\":").expect("truncate active record");
        assert!(store.load_active().is_err());
        assert!(store.active_path().exists());
    }

    #[test]
    fn wrong_recovery_record_version_is_rejected() {
        let temporary = tempdir().expect("create temp directory");
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let _record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
        tamper_active(&store, |value| value["formatVersion"] = 999.into());
        let error = store
            .load_active()
            .expect_err("wrong version must fail closed");
        assert!(
            error
                .to_string()
                .contains("unsupported recovery record format")
        );
        assert!(store.active_path().exists());
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
        let desktop = temporary.path().join("Desktop");
        create_fixture_files(&desktop);
        let store = VerificationRecoveryStore::new(temporary.path().join("recovery"));
        let record = begin_record(&store, &sample_snapshot(&desktop), &desktop);
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
