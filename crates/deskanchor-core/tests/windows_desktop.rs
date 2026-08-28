#![cfg(windows)]

use std::collections::HashSet;

use deskanchor_core::desktop::capture_current;
use deskanchor_core::verification::{
    DestructiveVerificationRun, VerificationRecoveryStore, require_destructive_opt_in,
    run_destructive_roundtrip,
};

#[test]
#[ignore = "requires an interactive Windows Explorer desktop"]
fn captures_real_explorer_desktop_with_unique_nonempty_identities() {
    let state = capture_current().expect("capture interactive Explorer desktop");
    assert!(!state.display.monitors.is_empty());
    assert!(!state.icons.is_empty());
    assert!(
        state
            .icons
            .iter()
            .all(|icon| !icon.identity.value.is_empty())
    );
    let unique: HashSet<_> = state.icons.iter().map(|icon| &icon.identity).collect();
    assert_eq!(unique.len(), state.icons.len());
}

#[test]
#[ignore = "DESTRUCTIVE: requires an isolated interactive Windows VM and explicit opt-in"]
fn destructive_fixture_round_trip_restores_the_complete_layout() {
    require_destructive_opt_in().expect(
        "destructive test remains disabled; set DESKANCHOR_DESTRUCTIVE_TESTS=1 only in the isolated validation VM",
    );
    let store = VerificationRecoveryStore::local_default()
        .expect("choose local verification recovery directory");
    match run_destructive_roundtrip(store).expect("run guarded destructive verification") {
        DestructiveVerificationRun::Verified(summary) => {
            summary.print_human_readable();
            assert!(summary.mutation_diff.is_exact_match());
            assert!(summary.recovery.final_diff.is_exact_match());
        }
        DestructiveVerificationRun::RecoveryRequired(summary) => {
            summary.print_human_readable();
            assert!(summary.mutation_diff.is_exact_match());
        }
    }
}
