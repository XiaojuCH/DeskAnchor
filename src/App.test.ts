import { describe, expect, it } from "vitest";
import { restoreResultMessage, type RestoreResult } from "./App";

function settleFailure(): RestoreResult {
  return {
    outcome: "settleVerificationFailed",
    restored: 2,
    unchanged: 4,
    skippedMissing: 0,
    skippedAmbiguous: 0,
    newItems: 0,
    failed: [],
    blockedDisplayMismatch: false,
    verification: {
      immediate: "passed",
      settle: "failed",
      attempts: 2,
      elapsedMs: 180,
      stableObservations: 0,
      requiredStableObservations: 3,
      finalDiff: {
        displayMatches: true,
        unchanged: 4,
        moved: 2,
        missing: 0,
        new: 0,
        ambiguous: 0,
      },
      error: "failed to recapture the complete desktop during settle verification: Shell capture failed",
    },
  };
}

describe("restoreResultMessage", () => {
  it("prioritizes the real capture error over a stale diff or deadline text", () => {
    const message = restoreResultMessage(settleFailure());

    expect(message).toContain("verification capture failed");
    expect(message).toContain("Shell capture failed");
    expect(message).not.toContain("did not settle");
    expect(message).not.toContain("remaining moved");
  });
});
