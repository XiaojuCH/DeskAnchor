use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::desktop::{DesktopIcon, DesktopState, IconIdentity};

use super::Snapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedIcon {
    pub identity: IconIdentity,
    pub display_name: String,
    pub current: IconPosition,
    pub snapshot: IconPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousIcon {
    pub identity: IconIdentity,
    pub snapshot_occurrences: usize,
    pub current_occurrences: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiff {
    pub display_matches: bool,
    pub unchanged: Vec<DesktopIcon>,
    pub moved: Vec<MovedIcon>,
    pub missing: Vec<DesktopIcon>,
    pub new: Vec<DesktopIcon>,
    pub ambiguous: Vec<AmbiguousIcon>,
}

impl SnapshotDiff {
    pub fn summary(&self) -> SnapshotDiffSummary {
        SnapshotDiffSummary {
            display_matches: self.display_matches,
            unchanged: self.unchanged.len(),
            moved: self.moved.len(),
            missing: self.missing.len(),
            new: self.new.len(),
            ambiguous: self.ambiguous.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiffSummary {
    pub display_matches: bool,
    pub unchanged: usize,
    pub moved: usize,
    pub missing: usize,
    pub new: usize,
    pub ambiguous: usize,
}

pub fn diff_desktop(snapshot: &Snapshot, current: &DesktopState) -> SnapshotDiff {
    let snapshot_groups = group_by_identity(&snapshot.icons);
    let current_groups = group_by_identity(&current.icons);
    let mut seen = HashSet::new();
    let mut unchanged = Vec::new();
    let mut moved = Vec::new();
    let mut missing = Vec::new();
    let mut ambiguous = Vec::new();

    for icon in &snapshot.icons {
        if !seen.insert(icon.identity.clone()) {
            continue;
        }
        let snapshot_matches = snapshot_groups
            .get(&icon.identity)
            .map_or(&[][..], Vec::as_slice);
        let current_matches = current_groups
            .get(&icon.identity)
            .map_or(&[][..], Vec::as_slice);
        if snapshot_matches.len() != 1 || current_matches.len() > 1 {
            ambiguous.push(AmbiguousIcon {
                identity: icon.identity.clone(),
                snapshot_occurrences: snapshot_matches.len(),
                current_occurrences: current_matches.len(),
            });
        } else if let Some(current_icon) = current_matches.first() {
            if icon.x == current_icon.x && icon.y == current_icon.y {
                unchanged.push((*current_icon).clone());
            } else {
                moved.push(MovedIcon {
                    identity: icon.identity.clone(),
                    display_name: current_icon.display_name.clone(),
                    current: IconPosition {
                        x: current_icon.x,
                        y: current_icon.y,
                    },
                    snapshot: IconPosition {
                        x: icon.x,
                        y: icon.y,
                    },
                });
            }
        } else {
            missing.push(icon.clone());
        }
    }

    let snapshot_identities: HashSet<_> = snapshot_groups.keys().copied().collect();
    let new = current
        .icons
        .iter()
        .filter(|icon| !snapshot_identities.contains(&icon.identity))
        .cloned()
        .collect();

    SnapshotDiff {
        display_matches: snapshot.display.signature == current.display.signature,
        unchanged,
        moved,
        missing,
        new,
        ambiguous,
    }
}

fn group_by_identity(icons: &[DesktopIcon]) -> HashMap<&IconIdentity, Vec<&DesktopIcon>> {
    let mut groups: HashMap<&IconIdentity, Vec<&DesktopIcon>> = HashMap::new();
    for icon in icons {
        groups.entry(&icon.identity).or_default().push(icon);
    }
    groups
}

#[cfg(test)]
mod tests {
    use crate::desktop::{DesktopIcon, DesktopState, IconIdentity};

    use super::*;
    use crate::snapshot::model::tests::sample_snapshot;

    fn icon(id: &str, x: i32, y: i32) -> DesktopIcon {
        DesktopIcon {
            identity: IconIdentity::shell_parsing_name(id.into()),
            display_name: id.into(),
            x,
            y,
        }
    }

    #[test]
    fn classifies_moved_unchanged_missing_and_new() {
        let mut snapshot = sample_snapshot();
        snapshot.icons = vec![
            icon("unchanged", 1, 2),
            icon("moved", 3, 4),
            icon("missing", 5, 6),
        ];
        let current = DesktopState {
            display: snapshot.display.clone(),
            icons: vec![
                icon("unchanged", 1, 2),
                icon("moved", 30, 40),
                icon("new", 7, 8),
            ],
        };
        assert_eq!(
            diff_desktop(&snapshot, &current).summary(),
            SnapshotDiffSummary {
                display_matches: true,
                unchanged: 1,
                moved: 1,
                missing: 1,
                new: 1,
                ambiguous: 0,
            }
        );
    }

    #[test]
    fn duplicate_identity_is_ambiguous_and_never_moved() {
        let mut snapshot = sample_snapshot();
        snapshot.icons = vec![icon("duplicate", 1, 2)];
        let current = DesktopState {
            display: snapshot.display.clone(),
            icons: vec![icon("duplicate", 3, 4), icon("duplicate", 5, 6)],
        };
        let diff = diff_desktop(&snapshot, &current);
        assert!(diff.moved.is_empty());
        assert_eq!(diff.ambiguous.len(), 1);
        assert_eq!(diff.ambiguous[0].current_occurrences, 2);
    }
}
