use deskanchor_core::desktop::{RestoreResult, capture_current, restore_snapshot as restore};
use deskanchor_core::snapshot::{
    SavedLayoutSummary, Snapshot, SnapshotDiffSummary, SnapshotStore, StoredSnapshot, diff_desktop,
};
use serde::Serialize;
use tauri::State;

struct AppState {
    snapshots: SnapshotStore,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentDesktopSummary {
    monitor_count: usize,
    icon_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedLayoutComparison {
    saved_layout: SavedLayoutSummary,
    current_desktop: CurrentDesktopSummary,
    diff: SnapshotDiffSummary,
}

type CommandResult<T> = Result<T, String>;

#[tauri::command]
async fn current_desktop() -> CommandResult<CurrentDesktopSummary> {
    run_blocking(|| {
        let desktop = capture_current().map_err(command_error)?;
        Ok(CurrentDesktopSummary {
            monitor_count: desktop.display.monitors.len(),
            icon_count: desktop.icons.len(),
        })
    })
    .await
}

#[tauri::command]
async fn get_saved_layout(state: State<'_, AppState>) -> CommandResult<Option<SavedLayoutSummary>> {
    let store = state.snapshots.clone();
    run_blocking(move || {
        store
            .load_saved_layout()
            .map(|snapshot| snapshot.as_ref().map(SavedLayoutSummary::from_snapshot))
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
async fn save_saved_layout(state: State<'_, AppState>) -> CommandResult<SavedLayoutSummary> {
    let store = state.snapshots.clone();
    run_blocking(move || {
        let desktop = capture_current().map_err(command_error)?;
        let snapshot = Snapshot::capture(desktop).map_err(command_error)?;
        store.replace_saved_layout(&snapshot).map_err(command_error)
    })
    .await
}

#[tauri::command]
async fn compare_saved_layout(state: State<'_, AppState>) -> CommandResult<SavedLayoutComparison> {
    let store = state.snapshots.clone();
    run_blocking(move || {
        let snapshot = store
            .load_saved_layout()
            .map_err(command_error)?
            .ok_or_else(|| "no saved layout exists".to_string())?;
        let current = capture_current().map_err(command_error)?;
        let diff = diff_desktop(&snapshot, &current).summary();
        Ok(SavedLayoutComparison {
            saved_layout: SavedLayoutSummary::from_snapshot(&snapshot),
            current_desktop: CurrentDesktopSummary {
                monitor_count: current.display.monitors.len(),
                icon_count: current.icons.len(),
            },
            diff,
        })
    })
    .await
}

#[tauri::command]
async fn save_snapshot(state: State<'_, AppState>) -> CommandResult<StoredSnapshot> {
    let store = state.snapshots.clone();
    run_blocking(move || {
        let desktop = capture_current().map_err(command_error)?;
        let snapshot = Snapshot::capture(desktop).map_err(command_error)?;
        store.save(&snapshot).map_err(command_error)
    })
    .await
}

#[tauri::command]
async fn list_snapshots(state: State<'_, AppState>) -> CommandResult<Vec<StoredSnapshot>> {
    let store = state.snapshots.clone();
    run_blocking(move || store.list().map_err(command_error)).await
}

#[tauri::command]
async fn compare_snapshot(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<SnapshotDiffSummary> {
    let store = state.snapshots.clone();
    run_blocking(move || {
        let snapshot = store.load(&id).map_err(command_error)?;
        let current = capture_current().map_err(command_error)?;
        Ok(diff_desktop(&snapshot, &current).summary())
    })
    .await
}

#[tauri::command]
async fn restore_snapshot(state: State<'_, AppState>, id: String) -> CommandResult<RestoreResult> {
    let store = state.snapshots.clone();
    run_blocking(move || {
        let snapshot = store.load(&id).map_err(command_error)?;
        restore(&snapshot).map_err(command_error)
    })
    .await
}

async fn run_blocking<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("background operation failed: {error}"))?
}

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let snapshots = SnapshotStore::local_default()?;
    tauri::Builder::default()
        .manage(AppState { snapshots })
        .invoke_handler(tauri::generate_handler![
            current_desktop,
            get_saved_layout,
            save_saved_layout,
            compare_saved_layout,
            save_snapshot,
            list_snapshots,
            compare_snapshot,
            restore_snapshot,
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}
