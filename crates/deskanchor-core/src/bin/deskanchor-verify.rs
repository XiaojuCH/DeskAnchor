use std::path::PathBuf;

use anyhow::{Result, bail};
use deskanchor_core::verification::{
    DestructiveVerificationRun, VerificationRecoveryStore, recover_last_verification,
    run_destructive_roundtrip,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next();
    let recovery_root = match (arguments.next(), arguments.next()) {
        (None, None) => None,
        (Some(flag), Some(path)) if flag == "--recovery-dir" => Some(PathBuf::from(path)),
        _ => bail!(usage()),
    };
    let store = match recovery_root {
        Some(root) => VerificationRecoveryStore::new(root),
        None => VerificationRecoveryStore::local_default()?,
    };

    match command.as_deref() {
        Some("verify-destructive") => {
            match run_destructive_roundtrip(store)? {
                DestructiveVerificationRun::Verified(summary) => summary.print_human_readable(),
                DestructiveVerificationRun::RecoveryRequired(summary) => {
                    summary.print_human_readable()
                }
            }
            Ok(())
        }
        Some("recover-last-verification") => {
            let summary = recover_last_verification(store)?;
            summary.print_human_readable();
            Ok(())
        }
        _ => bail!(usage()),
    }
}

fn usage() -> &'static str {
    "usage: deskanchor-verify <verify-destructive|recover-last-verification> [--recovery-dir PATH]\nBoth commands require DESKANCHOR_DESTRUCTIVE_TESTS=1."
}
