use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use deskanchor_core::desktop::{capture_current, restore_snapshot};
use deskanchor_core::snapshot::{Snapshot, diff_desktop};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => capture(None),
        [command] if command == "capture" => capture(None),
        [command, path] if command == "capture" => capture(Some(Path::new(path))),
        [command, path] if command == "diff" => diff(Path::new(path)),
        [command, path] if command == "restore" => restore(Path::new(path)),
        [command, path] if command == "verify-roundtrip" => verify_roundtrip(Path::new(path)),
        [command, path] if command == "verify-roundtrip-pmv2-host" => {
            set_per_monitor_v2_host()?;
            verify_roundtrip(Path::new(path))
        }
        _ => bail!(
            "usage: desktop_probe [capture [FILE] | diff FILE | restore FILE | verify-roundtrip RECOVERY_FILE | verify-roundtrip-pmv2-host RECOVERY_FILE]"
        ),
    }
}

fn set_per_monitor_v2_host() -> Result<()> {
    unsafe {
        // SAFETY: this is the first platform operation in this short-lived probe
        // process and no windows or DPI-dependent resources have been created.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    }
    .context("failed to configure the probe as a per-monitor-v2 host")
}

fn capture(path: Option<&Path>) -> Result<()> {
    let state = capture_current()?;
    let virtual_icons = state
        .icons
        .iter()
        .filter(|icon| icon.identity.value.starts_with("::"))
        .count();
    let snapshot = Snapshot::capture(state)?;
    if let Some(path) = path {
        write_new_snapshot(path, &snapshot)?;
    }
    println!(
        "captured {} icons ({} virtual) across {} monitor(s); saved={}",
        snapshot.icons.len(),
        virtual_icons,
        snapshot.display.monitors.len(),
        path.is_some(),
    );
    Ok(())
}

fn diff(path: &Path) -> Result<()> {
    let snapshot = read_snapshot(path)?;
    let current = capture_current()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&diff_desktop(&snapshot, &current).summary())?
    );
    Ok(())
}

fn restore(path: &Path) -> Result<()> {
    let snapshot = read_snapshot(path)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&restore_snapshot(&snapshot)?)?
    );
    Ok(())
}

fn verify_roundtrip(recovery_path: &Path) -> Result<()> {
    let original = Snapshot::capture(capture_current()?)?;
    let first_index = original
        .icons
        .iter()
        .enumerate()
        .find(|(_, icon)| !icon.identity.value.starts_with("::"))
        .map(|(index, _)| index)
        .context("need two filesystem desktop icons for round-trip verification")?;
    let second_index = original
        .icons
        .iter()
        .enumerate()
        .skip(first_index + 1)
        .find(|(_, icon)| {
            !icon.identity.value.starts_with("::")
                && (icon.x != original.icons[first_index].x
                    || icon.y != original.icons[first_index].y)
        })
        .map(|(index, _)| index)
        .context("need two filesystem desktop icons at distinct positions for verification")?;
    write_new_snapshot(recovery_path, &original)?;

    let mut swapped = original.clone();
    let first_position = (swapped.icons[first_index].x, swapped.icons[first_index].y);
    let second_position = (swapped.icons[second_index].x, swapped.icons[second_index].y);
    (swapped.icons[first_index].x, swapped.icons[first_index].y) = second_position;
    (swapped.icons[second_index].x, swapped.icons[second_index].y) = first_position;

    let movement_attempt = (|| -> Result<()> {
        let moved = restore_snapshot(&swapped)?;
        ensure!(
            !moved.blocked_display_mismatch,
            "test move was blocked by display mismatch"
        );
        ensure!(
            moved.failed.is_empty(),
            "test move reported {} failure(s)",
            moved.failed.len()
        );
        ensure!(
            moved.restored == 2,
            "test move restored {} icons instead of 2",
            moved.restored
        );
        let moved_state = capture_current()?;
        let moved_diff = diff_desktop(&swapped, &moved_state);
        ensure!(
            moved_diff.moved.is_empty(),
            "test positions did not persist after recapture"
        );
        Ok(())
    })();

    let recovery_attempt = (|| -> Result<()> {
        let recovered = restore_snapshot(&original)?;
        ensure!(
            !recovered.blocked_display_mismatch,
            "recovery was blocked by display mismatch"
        );
        ensure!(
            recovered.failed.is_empty(),
            "recovery reported {} failure(s)",
            recovered.failed.len()
        );
        let final_state = capture_current()?;
        let final_diff = diff_desktop(&original, &final_state);
        ensure!(
            final_diff.moved.is_empty(),
            "original positions were not fully restored"
        );
        Ok(())
    })();

    recovery_attempt
        .context("round-trip recovery failed; keep and restore the recovery snapshot")?;
    movement_attempt
        .context("round-trip movement verification failed after successful recovery")?;
    println!(
        "round-trip verified for 2 icons; original layout restored; recovery snapshot kept at {}",
        recovery_path.display()
    );
    Ok(())
}

fn read_snapshot(path: &Path) -> Result<Snapshot> {
    let json =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Snapshot::from_json(&json).context("invalid snapshot")
}

fn write_new_snapshot(path: &Path, snapshot: &Snapshot) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .with_context(|| {
            format!(
                "refusing to overwrite or unable to create {}",
                path.display()
            )
        })?;
    file.write_all(snapshot.to_pretty_json()?.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
