import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { desktopSummary, snapshotTime } from "./format";

interface CurrentDesktopSummary {
  monitorCount: number;
  iconCount: number;
}

interface StoredSnapshot {
  id: string;
  createdAt: string;
  monitorCount: number;
  iconCount: number;
}

interface SnapshotDiffSummary {
  displayMatches: boolean;
  unchanged: number;
  moved: number;
  missing: number;
  new: number;
  ambiguous: number;
}

interface RestoreResult {
  restored: number;
  unchanged: number;
  skippedMissing: number;
  skippedAmbiguous: number;
  newItems: number;
  failed: Array<{ displayName: string; reason: string }>;
  blockedDisplayMismatch: boolean;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function App() {
  const [current, setCurrent] = useState<CurrentDesktopSummary | null>(null);
  const [snapshots, setSnapshots] = useState<StoredSnapshot[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("Ready");

  const refresh = useCallback(async () => {
    const [desktop, saved] = await Promise.all([
      invoke<CurrentDesktopSummary>("current_desktop"),
      invoke<StoredSnapshot[]>("list_snapshots"),
    ]);
    setCurrent(desktop);
    setSnapshots(saved);
  }, []);

  useEffect(() => {
    refresh().catch((error: unknown) => setMessage(errorMessage(error)));
  }, [refresh]);

  async function run(action: () => Promise<string>) {
    setBusy(true);
    try {
      setMessage(await action());
      await refresh();
    } catch (error: unknown) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  function saveSnapshot() {
    void run(async () => {
      const saved = await invoke<StoredSnapshot>("save_snapshot");
      return `Saved ${saved.iconCount} icons locally.`;
    });
  }

  function compareSnapshot(id: string) {
    void run(async () => {
      const diff = await invoke<SnapshotDiffSummary>("compare_snapshot", { id });
      const displayNote = diff.displayMatches ? "display matches" : "display differs";
      return `${displayNote} · unchanged ${diff.unchanged} · moved ${diff.moved} · missing ${diff.missing} · new ${diff.new} · ambiguous ${diff.ambiguous}`;
    });
  }

  function restoreSnapshot(id: string) {
    if (!window.confirm("Restore uniquely matched icons from this snapshot?")) {
      return;
    }
    void run(async () => {
      const result = await invoke<RestoreResult>("restore_snapshot", { id });
      if (result.blockedDisplayMismatch) {
        return "Restore blocked: the current display configuration differs from the snapshot.";
      }
      return `Restored ${result.restored} · unchanged ${result.unchanged} · missing ${result.skippedMissing} · ambiguous ${result.skippedAmbiguous} · failed ${result.failed.length}`;
    });
  }

  return (
    <main>
      <header>
        <div>
          <h1>DeskAnchor</h1>
          <p className="subtitle">Desktop layout snapshots for Windows</p>
        </div>
        <span className="phase">v0.1 · experimental</span>
      </header>

      <section aria-labelledby="current-heading" className="panel current-panel">
        <div>
          <h2 id="current-heading">Current desktop</h2>
          <p className="summary">
            {current === null
              ? "Reading Explorer desktop…"
              : desktopSummary(current.monitorCount, current.iconCount)}
          </p>
        </div>
        <button disabled={busy || current === null} onClick={saveSnapshot} type="button">
          Save snapshot
        </button>
      </section>

      <section aria-labelledby="snapshots-heading">
        <div className="section-heading">
          <h2 id="snapshots-heading">Snapshots</h2>
          <span>{snapshots.length} saved locally</span>
        </div>
        {snapshots.length === 0 ? (
          <div className="panel empty">No snapshots yet.</div>
        ) : (
          <div className="snapshot-list">
            {snapshots.map((snapshot) => (
              <article className="panel snapshot" key={snapshot.id}>
                <div>
                  <h3>{snapshotTime(snapshot.createdAt)}</h3>
                  <p>{desktopSummary(snapshot.monitorCount, snapshot.iconCount)}</p>
                </div>
                <div className="actions">
                  <button
                    className="secondary"
                    disabled={busy}
                    onClick={() => compareSnapshot(snapshot.id)}
                    type="button"
                  >
                    Compare
                  </button>
                  <button
                    disabled={busy}
                    onClick={() => restoreSnapshot(snapshot.id)}
                    type="button"
                  >
                    Restore
                  </button>
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <footer aria-live="polite">
        <span className={busy ? "status busy" : "status"}>{busy ? "Working…" : message}</span>
        <span>Snapshots never leave this PC.</span>
      </footer>
    </main>
  );
}
