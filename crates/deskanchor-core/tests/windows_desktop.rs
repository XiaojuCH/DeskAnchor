#![cfg(windows)]

use std::collections::HashSet;

use deskanchor_core::desktop::capture_current;

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
