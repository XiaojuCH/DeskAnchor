use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Snapshot;

const SAVED_LAYOUT_FILE: &str = "saved-layout.json";

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

    pub fn saved_layout_path(&self) -> PathBuf {
        self.root.join(SAVED_LAYOUT_FILE)
    }

    /// Loads the one product-visible saved layout.
    ///
    /// Legacy timestamp-based snapshots are deliberately ignored.
    pub fn load_saved_layout(&self) -> Result<Option<Snapshot>> {
        let path = self.saved_layout_path();
        let json = match fs::read_to_string(&path) {
            Ok(json) => json,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read saved layout {}", path.display()));
            }
        };
        Snapshot::from_json(&json)
            .context("saved layout validation failed")
            .map(Some)
    }

    /// Replaces the one product-visible saved layout without deleting the old
    /// file first. The fully written temporary file and destination are always
    /// in the same directory.
    pub fn replace_saved_layout(&self, snapshot: &Snapshot) -> Result<SavedLayoutSummary> {
        self.replace_saved_layout_with(snapshot, publish_saved_layout)
    }

    fn replace_saved_layout_with<F>(
        &self,
        snapshot: &Snapshot,
        publish: F,
    ) -> Result<SavedLayoutSummary>
    where
        F: FnOnce(&Path, &Path) -> Result<()>,
    {
        // Serialization validates the complete snapshot before storage is
        // created or the existing saved layout can be touched.
        let json = snapshot.to_pretty_json()?;
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create snapshot directory {}",
                self.root.display()
            )
        })?;

        let destination = self.saved_layout_path();
        let temporary = self.root.join(format!(
            ".saved-layout-{}.tmp",
            Uuid::new_v4().as_hyphenated()
        ));
        let write_result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .with_context(|| format!("failed to create {}", temporary.display()))?;
            file.write_all(json.as_bytes())
                .context("failed to write saved layout JSON")?;
            file.sync_all()
                .context("failed to flush saved layout JSON")?;
            drop(file);
            publish(&temporary, &destination)
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(SavedLayoutSummary::from_snapshot(snapshot))
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
            if is_saved_layout_id(id) {
                continue;
            }
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLayoutSummary {
    pub created_at: String,
    pub monitor_count: usize,
    pub icon_count: usize,
}

impl SavedLayoutSummary {
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        Self {
            created_at: snapshot.created_at.clone(),
            monitor_count: snapshot.display.monitors.len(),
            icon_count: snapshot.icons.len(),
        }
    }
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
    if is_saved_layout_id(id) {
        bail!("saved layout filename is reserved")
    }
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

fn is_saved_layout_id(id: &str) -> bool {
    id.eq_ignore_ascii_case(SAVED_LAYOUT_FILE)
}

#[cfg(windows)]
fn publish_saved_layout(temporary: &Path, destination: &Path) -> Result<()> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let temporary_wide: Vec<u16> = temporary.as_os_str().encode_wide().chain(once(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect();
    unsafe {
        // SAFETY: both paths are NUL-terminated UTF-16 buffers that remain live
        // for the synchronous call. Both paths are in SnapshotStore's same
        // directory, and Windows performs the replace without a delete-first gap.
        MoveFileExW(
            PCWSTR(temporary_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .with_context(|| {
        format!(
            "failed to atomically publish saved layout {}",
            destination.display()
        )
    })
}

#[cfg(not(windows))]
fn publish_saved_layout(temporary: &Path, destination: &Path) -> Result<()> {
    fs::rename(temporary, destination).with_context(|| {
        format!(
            "failed to atomically publish saved layout {}",
            destination.display()
        )
    })
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

    #[test]
    fn no_canonical_saved_layout_is_not_an_error() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());

        assert_eq!(store.load_saved_layout().expect("load saved layout"), None);
    }

    #[test]
    fn first_save_publishes_the_canonical_saved_layout() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let snapshot = sample_snapshot();

        let summary = store
            .replace_saved_layout(&snapshot)
            .expect("save canonical layout");
        drop(store);
        let reopened = SnapshotStore::new(temporary.path());

        assert_eq!(summary, SavedLayoutSummary::from_snapshot(&snapshot));
        assert_eq!(
            reopened
                .load_saved_layout()
                .expect("load saved layout after reopening the store"),
            Some(snapshot)
        );
        assert!(reopened.saved_layout_path().is_file());
    }

    #[test]
    fn successful_replace_publishes_the_new_snapshot() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let original = sample_snapshot();
        store
            .replace_saved_layout(&original)
            .expect("save original layout");
        let mut replacement = original;
        replacement.created_at = "2026-08-28T03:04:05Z".into();
        replacement.icons[0].x = 200;

        store
            .replace_saved_layout(&replacement)
            .expect("replace saved layout");

        assert_eq!(
            store.load_saved_layout().expect("load replacement"),
            Some(replacement)
        );
    }

    #[test]
    fn failed_publication_preserves_the_old_valid_snapshot() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let original = sample_snapshot();
        store
            .replace_saved_layout(&original)
            .expect("save original layout");
        let mut replacement = original.clone();
        replacement.created_at = "2026-08-28T03:04:05Z".into();

        let error = store
            .replace_saved_layout_with(&replacement, |_temporary, _destination| {
                bail!("controlled publication failure")
            })
            .expect_err("publication must fail");

        assert!(error.to_string().contains("controlled publication failure"));
        assert_eq!(
            store.load_saved_layout().expect("load preserved layout"),
            Some(original)
        );
    }

    #[test]
    fn invalid_replacement_is_rejected_before_the_old_snapshot_is_touched() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let original = sample_snapshot();
        store
            .replace_saved_layout(&original)
            .expect("save original layout");
        let mut invalid = original.clone();
        invalid.icons[0].identity.value.clear();

        let result = store.replace_saved_layout(&invalid);

        assert!(result.is_err());
        assert_eq!(
            store.load_saved_layout().expect("load preserved layout"),
            Some(original)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_sharing_failure_preserves_the_old_valid_snapshot() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let original = sample_snapshot();
        store
            .replace_saved_layout(&original)
            .expect("save original layout");
        let locked = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(store.saved_layout_path())
            .expect("open canonical layout without sharing");
        let mut replacement = original.clone();
        replacement.created_at = "2026-08-28T03:04:05Z".into();

        let result = store.replace_saved_layout(&replacement);
        drop(locked);

        assert!(result.is_err());
        assert_eq!(
            store.load_saved_layout().expect("load preserved layout"),
            Some(original)
        );
    }

    #[test]
    fn corrupt_canonical_saved_layout_is_rejected() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        fs::write(store.saved_layout_path(), "{not valid JSON")
            .expect("write corrupt saved layout");

        let error = store
            .load_saved_layout()
            .expect_err("corrupt layout must fail");

        assert!(error.to_string().contains("saved layout validation failed"));
    }

    #[test]
    fn unsupported_canonical_snapshot_is_rejected() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        fs::write(
            store.saved_layout_path(),
            r#"{"schemaVersion":999,"createdAt":"ignored"}"#,
        )
        .expect("write unsupported saved layout");

        let error = store
            .load_saved_layout()
            .expect_err("unsupported layout must fail");

        assert!(error.to_string().contains("saved layout validation failed"));
    }

    #[test]
    fn valid_canonical_saved_layout_is_excluded_from_legacy_listing() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let legacy = sample_snapshot();
        let stored_legacy = store.save(&legacy).expect("save legacy timestamp snapshot");

        assert_eq!(store.load_saved_layout().expect("load saved layout"), None);

        let mut canonical = legacy.clone();
        canonical.created_at = "2026-08-28T03:04:05Z".into();
        store
            .replace_saved_layout(&canonical)
            .expect("save canonical layout");

        assert_eq!(
            store.list().expect("list legacy snapshots"),
            vec![stored_legacy.clone()]
        );
        assert_eq!(
            store
                .load(&stored_legacy.id)
                .expect("load legacy timestamp snapshot"),
            legacy
        );
        assert_eq!(
            store.load_saved_layout().expect("load canonical layout"),
            Some(canonical)
        );
    }

    #[test]
    fn corrupt_canonical_saved_layout_does_not_break_legacy_listing() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let legacy = sample_snapshot();
        let stored_legacy = store.save(&legacy).expect("save legacy timestamp snapshot");
        fs::write(store.saved_layout_path(), "{not valid JSON")
            .expect("write corrupt saved layout");

        assert_eq!(
            store.list().expect("list legacy snapshots"),
            vec![stored_legacy]
        );
        let error = store
            .load_saved_layout()
            .expect_err("corrupt canonical layout must fail independently");
        assert!(error.to_string().contains("saved layout validation failed"));
    }

    #[test]
    fn legacy_direct_load_rejects_canonical_reserved_id() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let store = SnapshotStore::new(temporary.path());
        let canonical = sample_snapshot();
        store
            .replace_saved_layout(&canonical)
            .expect("save canonical layout");

        let error = store
            .load(SAVED_LAYOUT_FILE)
            .expect_err("legacy load must reject the canonical reserved id");

        assert!(
            error
                .to_string()
                .contains("saved layout filename is reserved")
        );
        assert_eq!(
            store.load_saved_layout().expect("load canonical layout"),
            Some(canonical)
        );
    }
}
