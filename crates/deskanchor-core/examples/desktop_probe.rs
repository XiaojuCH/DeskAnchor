use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use deskanchor_core::desktop::{capture_current, restore_snapshot};
use deskanchor_core::snapshot::{Snapshot, diff_desktop};

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => capture(None),
        [command] if command == "capture" => capture(None),
        [command, path] if command == "capture" => capture(Some(Path::new(path))),
        [command, path] if command == "diff" => diff(Path::new(path)),
        [command, path] if command == "restore" => restore(Path::new(path)),
        _ => bail!(
            "usage: desktop_probe [capture [FILE] | diff FILE | restore FILE]\nUse the guarded deskanchor-verify binary for destructive verification."
        ),
    }
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
