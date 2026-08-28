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

export interface RestoreResult {
  outcome:
    | "settled"
    | "nothingToRestore"
    | "unresolvedItems"
    | "blockedDisplayMismatch"
    | "shellPositioningFailed"
    | "immediateVerificationFailed"
    | "settleVerificationFailed";
  restored: number;
  unchanged: number;
  skippedMissing: number;
  skippedAmbiguous: number;
  newItems: number;
  failed: Array<{ displayName: string; reason: string }>;
  blockedDisplayMismatch: boolean;
  verification: {
    immediate: "notRun" | "notRequired" | "passed" | "failed";
    settle: "notRun" | "notRequired" | "passed" | "failed";
    attempts: number;
    elapsedMs: number;
    stableObservations: number;
    requiredStableObservations: number;
    finalDiff: SnapshotDiffSummary | null;
    error: string | null;
  };
}

export function restoreResultMessage(result: RestoreResult): string {
  switch (result.outcome) {
    case "settled":
      return `Restored ${result.restored} and settled after ${result.verification.attempts} full verification capture(s).`;
    case "nothingToRestore":
      return "Nothing to restore: the desktop already exactly matches this snapshot.";
    case "blockedDisplayMismatch":
      return "Restore blocked: the current display configuration differs from the snapshot.";
    case "unresolvedItems":
      return `Restore incomplete: missing ${result.skippedMissing} · new ${result.newItems} · ambiguous ${result.skippedAmbiguous}.`;
    case "shellPositioningFailed":
      return `Restore failed during Shell positioning (${result.failed.length} item failure(s)).`;
    case "immediateVerificationFailed":
      return `Restore failed immediate position readback (${result.failed.length} item failure(s)).`;
    case "settleVerificationFailed": {
      if (result.verification.error !== null) {
        return `Restore verification capture failed: ${result.verification.error}`;
      }
      const remaining = result.verification.finalDiff;
      const detail = remaining === null
        ? "complete desktop recapture failed"
        : `remaining moved ${remaining.moved} · missing ${remaining.missing} · new ${remaining.new} · ambiguous ${remaining.ambiguous}`;
      return `Restore did not settle within the observation window: ${detail}.`;
    }
  }
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
      return restoreResultMessage(result);
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
