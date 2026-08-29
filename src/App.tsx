import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { desktopSummary, snapshotTime } from "./format";

interface CurrentDesktopSummary {
  monitorCount: number;
  iconCount: number;
}

export interface SavedLayoutSummary {
  createdAt: string;
  monitorCount: number;
  iconCount: number;
}

export interface SnapshotDiffSummary {
  displayMatches: boolean;
  unchanged: number;
  moved: number;
  missing: number;
  new: number;
  ambiguous: number;
}

interface SavedLayoutComparison {
  savedLayout: SavedLayoutSummary;
  currentDesktop: CurrentDesktopSummary;
  diff: SnapshotDiffSummary;
}

type CurrentDesktopState =
  | { kind: "loading" }
  | { kind: "ready"; summary: CurrentDesktopSummary }
  | { kind: "failed"; message: string };

type ComparisonState =
  | { kind: "loading" }
  | { kind: "exact"; diff: SnapshotDiffSummary }
  | { kind: "changed"; diff: SnapshotDiffSummary }
  | { kind: "displayMismatch"; diff: SnapshotDiffSummary }
  | { kind: "failed"; message: string };

type SavedLayoutState =
  | { kind: "loading" }
  | { kind: "none" }
  | { kind: "ready"; summary: SavedLayoutSummary; comparison: ComparisonState }
  | { kind: "unavailable"; message: string };

type Operation = "idle" | "saving" | "comparing";

interface Notice {
  tone: "success" | "error";
  message: string;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") {
      return message;
    }
  }
  return "An unknown error occurred.";
}

export function comparisonState(diff: SnapshotDiffSummary): ComparisonState {
  if (!diff.displayMatches) {
    return { kind: "displayMismatch", diff };
  }
  if (
    diff.moved === 0 &&
    diff.missing === 0 &&
    diff.new === 0 &&
    diff.ambiguous === 0
  ) {
    return { kind: "exact", diff };
  }
  return { kind: "changed", diff };
}

function DiffCounts({ diff }: { diff: SnapshotDiffSummary }) {
  return (
    <div className="diff-details">
      <dl className="diff-counts" aria-label="Saved layout differences">
        <div>
          <dt>Unchanged</dt>
          <dd>{diff.unchanged}</dd>
        </div>
        <div>
          <dt>Moved</dt>
          <dd>{diff.moved}</dd>
        </div>
        <div>
          <dt>Missing</dt>
          <dd>{diff.missing}</dd>
        </div>
        <div>
          <dt>New</dt>
          <dd>{diff.new}</dd>
        </div>
        <div>
          <dt>Ambiguous</dt>
          <dd>{diff.ambiguous}</dd>
        </div>
      </dl>
      <p className="diff-legend">
        Moved: saved icons changed position. Missing: saved items are no longer present. New:
        items were added after saving. Ambiguous: items cannot be uniquely identified.
      </p>
    </div>
  );
}

function Comparison({ state }: { state: ComparisonState }) {
  switch (state.kind) {
    case "loading":
      return <p className="comparison-note">Comparing with the current desktop…</p>;
    case "failed":
      return (
        <div className="callout error" role="alert">
          <strong>Current desktop comparison failed.</strong>
          <span>{state.message}</span>
        </div>
      );
    case "exact":
      return (
        <div className="comparison-result">
          <div className="callout success">
            <strong>Current desktop matches the saved layout.</strong>
            <span>No saved icon positions have changed.</span>
          </div>
          <DiffCounts diff={state.diff} />
        </div>
      );
    case "changed":
      return (
        <div className="comparison-result">
          <div className="callout warning">
            <strong>Changes detected.</strong>
            <span>
              Moved icons can be identified; missing, new, and ambiguous items remain distinct.
            </span>
          </div>
          <DiffCounts diff={state.diff} />
        </div>
      );
    case "displayMismatch":
      return (
        <div className="comparison-result">
          <div className="callout error" role="alert">
            <strong>The display configuration differs from the saved layout.</strong>
            <span>DeskAnchor does not remap or scale coordinates in Phase 1.</span>
          </div>
          <DiffCounts diff={state.diff} />
        </div>
      );
  }
}

export default function App() {
  const [current, setCurrent] = useState<CurrentDesktopState>({ kind: "loading" });
  const [saved, setSaved] = useState<SavedLayoutState>({ kind: "loading" });
  const [operation, setOperation] = useState<Operation>("idle");
  const [notice, setNotice] = useState<Notice | null>(null);
  const requestGeneration = useRef(0);

  const loadWorkflow = useCallback(async (generation: number, showLoading: boolean) => {
    if (showLoading) {
      setCurrent({ kind: "loading" });
      setSaved({ kind: "loading" });
    }

    const [currentResult, savedResult] = await Promise.allSettled([
      invoke<CurrentDesktopSummary>("current_desktop"),
      invoke<SavedLayoutSummary | null>("get_saved_layout"),
    ]);
    if (generation !== requestGeneration.current) {
      return;
    }

    if (currentResult.status === "fulfilled") {
      setCurrent({ kind: "ready", summary: currentResult.value });
    } else {
      setCurrent({ kind: "failed", message: errorMessage(currentResult.reason) });
    }

    if (savedResult.status === "rejected") {
      setSaved({ kind: "unavailable", message: errorMessage(savedResult.reason) });
      return;
    }
    if (savedResult.value === null) {
      setSaved({ kind: "none" });
      return;
    }
    if (currentResult.status === "rejected") {
      setSaved({
        kind: "ready",
        summary: savedResult.value,
        comparison: { kind: "failed", message: errorMessage(currentResult.reason) },
      });
      return;
    }

    const summary = savedResult.value;
    setSaved({ kind: "ready", summary, comparison: { kind: "loading" } });
    try {
      const comparison = await invoke<SavedLayoutComparison>("compare_saved_layout");
      if (generation === requestGeneration.current) {
        setCurrent({ kind: "ready", summary: comparison.currentDesktop });
        setSaved({
          kind: "ready",
          summary: comparison.savedLayout,
          comparison: comparisonState(comparison.diff),
        });
      }
    } catch (error: unknown) {
      if (generation === requestGeneration.current) {
        setSaved({
          kind: "ready",
          summary,
          comparison: { kind: "failed", message: errorMessage(error) },
        });
      }
    }
  }, []);

  const refresh = useCallback(async () => {
    const generation = ++requestGeneration.current;
    setOperation("comparing");
    setNotice(null);
    try {
      await loadWorkflow(generation, true);
    } finally {
      if (generation === requestGeneration.current) {
        setOperation("idle");
      }
    }
  }, [loadWorkflow]);

  useEffect(() => {
    const generation = ++requestGeneration.current;
    void loadWorkflow(generation, true);
    return () => {
      if (requestGeneration.current === generation) {
        requestGeneration.current += 1;
      }
    };
  }, [loadWorkflow]);

  async function saveLayout() {
    const replacing = saved.kind === "ready" || saved.kind === "unavailable";
    if (
      replacing &&
      !window.confirm(
        "Replace the saved layout with the current desktop? Phase 1 does not keep layout history.",
      )
    ) {
      return;
    }

    const generation = ++requestGeneration.current;
    setOperation("saving");
    setNotice(null);
    try {
      await invoke<SavedLayoutSummary>("save_saved_layout");
      if (generation !== requestGeneration.current) {
        return;
      }
      setNotice({
        tone: "success",
        message: replacing ? "Saved layout replaced." : "Layout saved.",
      });
      await loadWorkflow(generation, false);
    } catch (error: unknown) {
      if (generation === requestGeneration.current) {
        setNotice({
          tone: "error",
          message: `${replacing ? "Replace" : "Save"} failed: ${errorMessage(error)}`,
        });
      }
    } finally {
      if (generation === requestGeneration.current) {
        setOperation("idle");
      }
    }
  }

  const busy = operation !== "idle";
  const operationLabel = operation === "saving" ? "Saving…" : "Comparing…";

  return (
    <main>
      <header>
        <div>
          <h1>DeskAnchor</h1>
          <p className="subtitle">One saved desktop layout, stored only on this PC</p>
        </div>
        <span className="phase">Phase 1A</span>
      </header>

      <section aria-labelledby="current-heading" className="panel current-panel">
        <div>
          <h2 id="current-heading">Current desktop</h2>
          <p className="summary">
            {current.kind === "loading" && "Reading Explorer desktop…"}
            {current.kind === "ready" &&
              desktopSummary(current.summary.monitorCount, current.summary.iconCount)}
            {current.kind === "failed" && `Desktop capture failed: ${current.message}`}
          </p>
        </div>
        <button className="secondary" disabled={busy} onClick={() => void refresh()} type="button">
          Refresh
        </button>
      </section>

      <section aria-labelledby="saved-layout-heading">
        <div className="section-heading">
          <h2 id="saved-layout-heading">Saved Layout</h2>
          <span>One canonical layout</span>
        </div>

        {saved.kind === "loading" && (
          <div className="panel empty">Reading the saved layout…</div>
        )}

        {saved.kind === "none" && (
          <div className="panel empty empty-action">
            <div>
              <h3>No saved layout yet</h3>
              <p>Save the current desktop so you can compare it after icons move or change.</p>
            </div>
            <button disabled={busy} onClick={() => void saveLayout()} type="button">
              Save Layout
            </button>
          </div>
        )}

        {saved.kind === "unavailable" && (
          <div className="panel saved-layout-card">
            <div className="callout error" role="alert">
              <strong>Saved layout unavailable</strong>
              <span>{saved.message}</span>
            </div>
            <p className="replacement-note">
              Compare is blocked. You can retry, or explicitly replace the unreadable saved layout.
            </p>
            <div className="actions">
              <button className="secondary" disabled={busy} onClick={() => void refresh()} type="button">
                Retry
              </button>
              <button disabled={busy} onClick={() => void saveLayout()} type="button">
                Replace Saved Layout
              </button>
            </div>
          </div>
        )}

        {saved.kind === "ready" && (
          <article className="panel saved-layout-card">
            <div className="saved-layout-heading">
              <div>
                <h3>Saved {snapshotTime(saved.summary.createdAt)}</h3>
                <p>{desktopSummary(saved.summary.monitorCount, saved.summary.iconCount)}</p>
              </div>
              <div className="actions">
                <button className="secondary" disabled={busy} onClick={() => void refresh()} type="button">
                  Compare
                </button>
                <button disabled={busy} onClick={() => void saveLayout()} type="button">
                  Replace Saved Layout
                </button>
              </div>
            </div>
            <p className="replacement-note">
              Replacing this layout permanently discards the previous product-visible layout.
              Phase 1 does not keep history.
            </p>
            <Comparison state={saved.comparison} />
          </article>
        )}
      </section>

      <footer aria-live="polite">
        <span
          className={`status ${busy ? "busy" : ""} ${notice?.tone === "error" ? "error-text" : ""}`}
        >
          {busy ? operationLabel : notice?.message ?? "Ready"}
        </span>
        <span>Saved layouts never leave this PC.</span>
      </footer>
    </main>
  );
}
