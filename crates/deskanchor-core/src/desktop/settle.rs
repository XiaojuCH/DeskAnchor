use crate::snapshot::SnapshotDiffSummary;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestoreSettlePolicy {
    pub poll_interval_ms: u64,
    pub deadline_ms: u64,
    pub required_stable_observations: u32,
}

impl RestoreSettlePolicy {
    pub fn validate(self) -> Result<(), SettlePolicyError> {
        if self.poll_interval_ms == 0 {
            return Err(SettlePolicyError::ZeroPollInterval);
        }
        if self.deadline_ms == 0 {
            return Err(SettlePolicyError::ZeroDeadline);
        }
        if self.required_stable_observations == 0 {
            return Err(SettlePolicyError::ZeroStableObservations);
        }
        Ok(())
    }
}

impl Default for RestoreSettlePolicy {
    fn default() -> Self {
        Self {
            poll_interval_ms: 150,
            deadline_ms: 2_000,
            required_stable_observations: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SettlePolicyError {
    #[error("restore settle poll interval must be greater than zero")]
    ZeroPollInterval,
    #[error("restore settle deadline must be greater than zero")]
    ZeroDeadline,
    #[error("restore settle stable-observation count must be greater than zero")]
    ZeroStableObservations,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettleDecision {
    RetryAfter { wait_ms: u64 },
    Settled,
    DeadlineReached,
}

pub(crate) struct SettleTracker {
    policy: RestoreSettlePolicy,
    attempts: u32,
    consecutive_stable: u32,
}

impl SettleTracker {
    pub(crate) fn new(policy: RestoreSettlePolicy) -> Self {
        Self {
            policy,
            attempts: 0,
            consecutive_stable: 0,
        }
    }

    pub(crate) fn observe(
        &mut self,
        elapsed_ms: u64,
        summary: SnapshotDiffSummary,
    ) -> SettleDecision {
        self.attempts = self.attempts.saturating_add(1);
        if summary.is_exact_match() {
            self.consecutive_stable = self.consecutive_stable.saturating_add(1);
        } else {
            self.consecutive_stable = 0;
        }

        if self.consecutive_stable >= self.policy.required_stable_observations {
            return SettleDecision::Settled;
        }
        if elapsed_ms >= self.policy.deadline_ms {
            return SettleDecision::DeadlineReached;
        }

        SettleDecision::RetryAfter {
            wait_ms: self
                .policy
                .poll_interval_ms
                .min(self.policy.deadline_ms - elapsed_ms),
        }
    }

    pub(crate) fn attempts(&self) -> u32 {
        self.attempts
    }

    pub(crate) fn consecutive_stable(&self) -> u32 {
        self.consecutive_stable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(moved: usize, missing: usize, new: usize) -> SnapshotDiffSummary {
        SnapshotDiffSummary {
            display_matches: true,
            unchanged: 5,
            moved,
            missing,
            new,
            ambiguous: 0,
        }
    }

    fn policy(required_stable_observations: u32) -> RestoreSettlePolicy {
        RestoreSettlePolicy {
            poll_interval_ms: 100,
            deadline_ms: 500,
            required_stable_observations,
        }
    }

    #[test]
    fn immediate_full_capture_can_settle_when_one_observation_is_required() {
        let mut tracker = SettleTracker::new(policy(1));
        assert_eq!(
            tracker.observe(0, summary(0, 0, 0)),
            SettleDecision::Settled
        );
        assert_eq!(tracker.attempts(), 1);
    }

    #[test]
    fn settles_after_a_retry_and_required_stable_observations() {
        let mut tracker = SettleTracker::new(policy(2));
        assert_eq!(
            tracker.observe(0, summary(1, 0, 0)),
            SettleDecision::RetryAfter { wait_ms: 100 }
        );
        assert_eq!(
            tracker.observe(100, summary(0, 0, 0)),
            SettleDecision::RetryAfter { wait_ms: 100 }
        );
        assert_eq!(
            tracker.observe(200, summary(0, 0, 0)),
            SettleDecision::Settled
        );
    }

    #[test]
    fn never_settles_before_the_deadline() {
        let mut tracker = SettleTracker::new(policy(2));
        assert!(matches!(
            tracker.observe(0, summary(1, 0, 0)),
            SettleDecision::RetryAfter { .. }
        ));
        assert_eq!(
            tracker.observe(500, summary(1, 0, 0)),
            SettleDecision::DeadlineReached
        );
    }

    #[test]
    fn later_drift_resets_stability() {
        let mut tracker = SettleTracker::new(policy(3));
        let _ = tracker.observe(0, summary(0, 0, 0));
        let _ = tracker.observe(100, summary(0, 0, 0));
        assert_eq!(tracker.consecutive_stable(), 2);

        let _ = tracker.observe(200, summary(1, 0, 0));
        assert_eq!(tracker.consecutive_stable(), 0);
        assert_eq!(
            tracker.observe(500, summary(1, 0, 0)),
            SettleDecision::DeadlineReached
        );
    }

    #[test]
    fn missing_or_new_items_never_count_as_an_exact_layout() {
        let mut tracker = SettleTracker::new(policy(1));
        assert!(matches!(
            tracker.observe(0, summary(0, 1, 0)),
            SettleDecision::RetryAfter { .. }
        ));
        assert_eq!(
            tracker.observe(500, summary(0, 0, 1)),
            SettleDecision::DeadlineReached
        );
    }

    #[test]
    fn display_mismatch_never_settles() {
        let mut tracker = SettleTracker::new(policy(1));
        let mut mismatched = summary(0, 0, 0);
        mismatched.display_matches = false;
        assert!(matches!(
            tracker.observe(0, mismatched),
            SettleDecision::RetryAfter { .. }
        ));
        assert_eq!(
            tracker.observe(500, mismatched),
            SettleDecision::DeadlineReached
        );
    }

    #[test]
    fn rejects_invalid_policies() {
        assert_eq!(
            RestoreSettlePolicy {
                poll_interval_ms: 0,
                ..RestoreSettlePolicy::default()
            }
            .validate(),
            Err(SettlePolicyError::ZeroPollInterval)
        );
    }
}
