// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import App, {
  comparisonState,
  type SavedLayoutSummary,
  type SnapshotDiffSummary,
} from "./App";

const current = { monitorCount: 1, iconCount: 6 };
const saved: SavedLayoutSummary = {
  createdAt: "2026-08-28T01:02:03Z",
  monitorCount: 1,
  iconCount: 6,
};

function diff(overrides: Partial<SnapshotDiffSummary> = {}): SnapshotDiffSummary {
  return {
    displayMatches: true,
    unchanged: 6,
    moved: 0,
    missing: 0,
    new: 0,
    ambiguous: 0,
    ...overrides,
  };
}

function comparison(
  savedLayout: SavedLayoutSummary,
  snapshotDiff: SnapshotDiffSummary = diff(),
  currentDesktop = current,
) {
  return { savedLayout, currentDesktop, diff: snapshotDiff };
}

function mockWorkflow(
  savedLayout: SavedLayoutSummary | null,
  snapshotDiff: SnapshotDiffSummary = diff(),
) {
  invokeMock.mockImplementation((command: string) => {
    switch (command) {
      case "current_desktop":
        return Promise.resolve(current);
      case "get_saved_layout":
        return Promise.resolve(savedLayout);
      case "compare_saved_layout":
        return Promise.resolve(comparison(savedLayout!, snapshotDiff));
      default:
        return Promise.reject(new Error(`unexpected command: ${command}`));
    }
  });
}

function countFor(label: string): string | null | undefined {
  return screen.getByText(label).nextElementSibling?.textContent;
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

beforeEach(() => {
  invokeMock.mockReset();
  vi.spyOn(window, "confirm").mockReturnValue(true);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("single saved layout workflow", () => {
  it("shows the no-saved-layout state", async () => {
    mockWorkflow(null);

    render(<App />);

    expect(await screen.findByText("No saved layout yet")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Save Layout" })).toBeTruthy();
    expect(screen.queryByText("Snapshots")).toBeNull();
  });

  it("shows an exact saved-layout comparison", async () => {
    mockWorkflow(saved);

    render(<App />);

    expect(
      await screen.findByText("Current desktop matches the saved layout."),
    ).toBeTruthy();
    expect(countFor("Unchanged")).toBe("6");
    expect(countFor("Moved")).toBe("0");
  });

  it("shows all five change counts", async () => {
    mockWorkflow(
      { ...saved, iconCount: 10 },
      diff({ unchanged: 5, moved: 2, missing: 1, new: 1, ambiguous: 1 }),
    );

    render(<App />);

    expect(await screen.findByText("Changes detected.")).toBeTruthy();
    expect(countFor("Unchanged")).toBe("5");
    expect(countFor("Moved")).toBe("2");
    expect(countFor("Missing")).toBe("1");
    expect(countFor("New")).toBe("1");
    expect(countFor("Ambiguous")).toBe("1");
  });

  it("keeps display mismatch separate from normal changes", async () => {
    mockWorkflow(saved, diff({ displayMatches: false, unchanged: 5, moved: 1 }));

    render(<App />);

    expect(
      await screen.findByText("The display configuration differs from the saved layout."),
    ).toBeTruthy();
    expect(screen.getByText("DeskAnchor does not remap or scale coordinates in Phase 1.")).toBeTruthy();
    expect(comparisonState(diff({ displayMatches: false })).kind).toBe("displayMismatch");
  });

  it("shows a corrupt or unavailable saved layout as blocked", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "current_desktop") {
        return Promise.resolve(current);
      }
      if (command === "get_saved_layout") {
        return Promise.reject(new Error("saved layout validation failed"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(<App />);

    expect(await screen.findByText("Saved layout unavailable")).toBeTruthy();
    expect(screen.getByText("saved layout validation failed")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Replace Saved Layout" })).toBeTruthy();
  });

  it("reports current desktop capture failure", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "current_desktop") {
        return Promise.reject(new Error("Explorer desktop unavailable"));
      }
      if (command === "get_saved_layout") {
        return Promise.resolve(saved);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(<App />);

    expect(
      await screen.findByText("Desktop capture failed: Explorer desktop unavailable"),
    ).toBeTruthy();
    expect(screen.getByText("Current desktop comparison failed.")).toBeTruthy();
  });

  it("saves the first layout and refreshes to exact", async () => {
    let savedLayout: SavedLayoutSummary | null = null;
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "current_desktop":
          return Promise.resolve(current);
        case "get_saved_layout":
          return Promise.resolve(savedLayout);
        case "save_saved_layout":
          savedLayout = saved;
          return Promise.resolve(saved);
        case "compare_saved_layout":
          return Promise.resolve(comparison(savedLayout!, diff()));
        default:
          return Promise.reject(new Error(`unexpected command: ${command}`));
      }
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Save Layout" }));

    expect(
      await screen.findByText("Current desktop matches the saved layout."),
    ).toBeTruthy();
    expect(screen.getByText("Layout saved.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Replace Saved Layout" })).toBeTruthy();
  });

  it("replaces the saved layout only after confirmation", async () => {
    let savedLayout = saved;
    const replacement = { ...saved, createdAt: "2026-08-28T02:03:04Z", iconCount: 9 };
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "current_desktop":
          return Promise.resolve({ ...current, iconCount: 9 });
        case "get_saved_layout":
          return Promise.resolve(savedLayout);
        case "compare_saved_layout":
          return Promise.resolve(
            comparison(savedLayout, diff({ unchanged: 9 }), { ...current, iconCount: 9 }),
          );
        case "save_saved_layout":
          savedLayout = replacement;
          return Promise.resolve(replacement);
        default:
          return Promise.reject(new Error(`unexpected command: ${command}`));
      }
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Replace Saved Layout" }));

    expect(await screen.findByText("Saved layout replaced.")).toBeTruthy();
    expect(screen.getAllByText("1 display · 9 icons")).toHaveLength(2);
    expect(window.confirm).toHaveBeenCalledWith(
      expect.stringContaining("Phase 1 does not keep layout history"),
    );
  });

  it("does not claim success or discard the visible old layout after replace failure", async () => {
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "current_desktop":
          return Promise.resolve(current);
        case "get_saved_layout":
          return Promise.resolve(saved);
        case "compare_saved_layout":
          return Promise.resolve(comparison(saved, diff()));
        case "save_saved_layout":
          return Promise.reject(new Error("sharing violation"));
        default:
          return Promise.reject(new Error(`unexpected command: ${command}`));
      }
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Replace Saved Layout" }));

    expect(await screen.findByText("Replace failed: sharing violation")).toBeTruthy();
    expect(screen.getAllByText("1 display · 6 icons")).toHaveLength(2);
    expect(screen.queryByText("Saved layout replaced.")).toBeNull();
  });

  it("ignores an older startup response after a newer refresh completes", async () => {
    const oldCurrent = deferred<typeof current>();
    const oldSaved = deferred<SavedLayoutSummary | null>();
    let currentCalls = 0;
    let savedCalls = 0;
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "current_desktop":
          currentCalls += 1;
          return currentCalls === 1 ? oldCurrent.promise : Promise.resolve(current);
        case "get_saved_layout":
          savedCalls += 1;
          return savedCalls === 1 ? oldSaved.promise : Promise.resolve(saved);
        case "compare_saved_layout":
          return Promise.resolve(comparison(saved, diff()));
        default:
          return Promise.reject(new Error(`unexpected command: ${command}`));
      }
    });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByText("Current desktop matches the saved layout."),
    ).toBeTruthy();

    await act(async () => {
      oldCurrent.resolve({ monitorCount: 2, iconCount: 99 });
      oldSaved.resolve(null);
      await oldCurrent.promise;
      await oldSaved.promise;
    });

    await waitFor(() => {
      expect(screen.getByText("Current desktop matches the saved layout.")).toBeTruthy();
      expect(screen.queryByText("No saved layout yet")).toBeNull();
      expect(screen.queryByText("2 displays · 99 icons")).toBeNull();
    });
  });
});
