use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::Snapshot;

#[derive(Clone, Debug)]
pub struct SnapshotStore {
    root: PathBuf,
}

impl SnapshotStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn local_default() -> Result<Self> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .context("LOCALAPPDATA is unavailable; cannot choose local snapshot storage")?;
        Ok(Self::new(
            PathBuf::from(local_app_data)
                .join("DeskAnchor")
                .join("snapshots"),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save(&self, snapshot: &Snapshot) -> Result<StoredSnapshot> {
        let json = snapshot.to_pretty_json()?;
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create snapshot directory {}",
                self.root.display()
            )
        })?;

        let stem: String = snapshot
            .created_at
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect();
        for suffix in 0..100_u8 {
            let id = if suffix == 0 {
                format!("snapshot-{stem}.json")
            } else {
                format!("snapshot-{stem}-{suffix}.json")
            };
            let destination = self.root.join(&id);
            if destination.exists() {
                continue;
            }
            let temporary = self.root.join(format!(".{id}.tmp"));
            let write_result = (|| -> Result<()> {
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temporary)
                    .with_context(|| format!("failed to create {}", temporary.display()))?;
                file.write_all(json.as_bytes())
                    .context("failed to write snapshot JSON")?;
                file.sync_all().context("failed to flush snapshot JSON")?;
                fs::rename(&temporary, &destination)
                    .context("failed to atomically publish snapshot")?;
                Ok(())
            })();
            if write_result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            write_result?;
            return Ok(StoredSnapshot::from_snapshot(id, snapshot));
        }
        bail!("could not allocate a unique snapshot filename")
    }

    pub fn load(&self, id: &str) -> Result<Snapshot> {
        validate_snapshot_id(id)?;
        let path = self.root.join(id);
        let json = fs::read_to_string(&path)
            .with_context(|| format!("failed to read snapshot {}", path.display()))?;
        Snapshot::from_json(&json).context("snapshot validation failed")
    }

    pub fn list(&self) -> Result<Vec<StoredSnapshot>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to list {}", self.root.display()))?
        {
            let entry = entry.context("failed to inspect snapshot directory entry")?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new("json")) {
                continue;
            }
            let Some(id) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            let snapshot = self.load(id)?;
            entries.push(StoredSnapshot::from_snapshot(id.into(), &snapshot));
        }
        entries.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(entries)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSnapshot {
    pub id: String,
    pub created_at: String,
    pub monitor_count: usize,
    pub icon_count: usize,
}

impl StoredSnapshot {
    fn from_snapshot(id: String, snapshot: &Snapshot) -> Self {
        Self {
            id,
            created_at: snapshot.created_at.clone(),
            monitor_count: snapshot.display.monitors.len(),
            icon_count: snapshot.icons.len(),
        }
    }
}

fn validate_snapshot_id(id: &str) -> Result<()> {
    let path = Path::new(id);
    let mut components = path.components();
    let valid_component =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    let valid_characters = !id.is_empty()
        && id.ends_with(".json")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !valid_component || !valid_characters {
        bail!("invalid snapshot id")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::model::tests::sample_snapshot;

    #[test]
    fn store_round_trip_and_listing() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let snapshot = sample_snapshot();
        let saved = store.save(&snapshot).expect("save snapshot");
        assert_eq!(store.load(&saved.id).expect("load snapshot"), snapshot);
        assert_eq!(store.list().expect("list snapshots"), vec![saved]);
    }

    #[test]
    fn path_traversal_snapshot_id_is_rejected() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        assert!(store.load("../secret.json").is_err());
    }
}
