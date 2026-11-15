use std::time::Duration;

use crate::domain::workbook::FileFingerprint;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WatchMode {
    #[default]
    Poll,
    Native,
    Auto,
    Off,
}

impl WatchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Poll => "poll",
            Self::Native => "native",
            Self::Auto => "auto",
            Self::Off => "off",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WatchConfig {
    pub mode: WatchMode,
    pub poll_interval: Duration,
    pub debounce: Duration,
    pub retry: Duration,
    pub native_safety: Duration,
    pub failure_notice_interval: u32,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            mode: WatchMode::Poll,
            poll_interval: Duration::from_millis(1_000),
            debounce: Duration::from_millis(750),
            retry: Duration::from_millis(1_000),
            native_safety: Duration::from_millis(5_000),
            failure_notice_interval: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadReason {
    WatchChange,
    Manual,
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateDecision {
    Idle,
    Wait { until_ms: u64 },
    Load { reason: ReloadReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryDecision {
    Retry {
        attempt: u32,
        next_ms: u64,
        notify: bool,
    },
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    stamp: FileFingerprint,
    deadline_ms: u64,
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Idle,
    Debouncing(Pending),
    Loading,
    Retry { next_ms: u64 },
}

pub struct ReloadGate {
    config: WatchConfig,
    last_loaded: FileFingerprint,
    phase: Phase,
    attempts: u32,
}

impl ReloadGate {
    pub fn new(config: WatchConfig, last_loaded: FileFingerprint) -> Self {
        Self {
            config,
            last_loaded,
            phase: Phase::Idle,
            attempts: 0,
        }
    }

    pub fn on_signal(&mut self, stamp: FileFingerprint, now_ms: u64) -> GateDecision {
        match self.phase {
            Phase::Loading => GateDecision::Idle,
            Phase::Retry { next_ms, .. } => GateDecision::Wait { until_ms: next_ms },
            Phase::Idle | Phase::Debouncing(_) => {
                if stamp == self.last_loaded {
                    self.phase = Phase::Idle;
                    return GateDecision::Idle;
                }
                let deadline_ms = now_ms.saturating_add(duration_ms(self.config.debounce));
                self.phase = Phase::Debouncing(Pending { stamp, deadline_ms });
                GateDecision::Wait {
                    until_ms: deadline_ms,
                }
            }
        }
    }

    pub fn on_tick(&mut self, stamp: FileFingerprint, now_ms: u64) -> GateDecision {
        match self.phase {
            Phase::Idle | Phase::Loading => GateDecision::Idle,
            Phase::Debouncing(pending) => {
                if now_ms < pending.deadline_ms {
                    return GateDecision::Wait {
                        until_ms: pending.deadline_ms,
                    };
                }
                if stamp != pending.stamp {
                    let deadline_ms = now_ms.saturating_add(duration_ms(self.config.debounce));
                    self.phase = Phase::Debouncing(Pending { stamp, deadline_ms });
                    return GateDecision::Wait {
                        until_ms: deadline_ms,
                    };
                }
                if stamp == self.last_loaded {
                    self.phase = Phase::Idle;
                    return GateDecision::Idle;
                }
                self.phase = Phase::Loading;
                GateDecision::Load {
                    reason: ReloadReason::WatchChange,
                }
            }
            Phase::Retry { next_ms, .. } => {
                if now_ms < next_ms {
                    return GateDecision::Wait { until_ms: next_ms };
                }
                if stamp == self.last_loaded {
                    self.phase = Phase::Idle;
                    self.attempts = 0;
                    return GateDecision::Idle;
                }
                self.phase = Phase::Loading;
                GateDecision::Load {
                    reason: ReloadReason::Retry,
                }
            }
        }
    }

    pub fn on_manual(&mut self) -> GateDecision {
        if matches!(self.phase, Phase::Loading) {
            return GateDecision::Idle;
        }
        self.phase = Phase::Loading;
        GateDecision::Load {
            reason: ReloadReason::Manual,
        }
    }

    pub fn on_loaded(&mut self, stamp: FileFingerprint) {
        self.last_loaded = stamp;
        self.attempts = 0;
        self.phase = Phase::Idle;
    }

    pub fn on_failed(&mut self, now_ms: u64) -> RetryDecision {
        self.attempts = self.attempts.saturating_add(1);
        let next_ms = now_ms.saturating_add(duration_ms(self.config.retry));
        let interval = self.config.failure_notice_interval.max(1);
        let notify = self.attempts == 1 || self.attempts.is_multiple_of(interval);
        self.phase = Phase::Retry { next_ms };
        RetryDecision::Retry {
            attempt: self.attempts,
            next_ms,
            notify,
        }
    }

    pub fn next_deadline_ms(&self) -> Option<u64> {
        match self.phase {
            Phase::Debouncing(pending) => Some(pending.deadline_ms),
            Phase::Retry { next_ms, .. } => Some(next_ms),
            Phase::Idle | Phase::Loading => None,
        }
    }

    pub fn is_retrying(&self) -> bool {
        matches!(self.phase, Phase::Retry { .. })
    }

    pub fn last_loaded(&self) -> FileFingerprint {
        self.last_loaded
    }

    pub fn attempt(&self) -> u32 {
        self.attempts
    }

    pub fn reset_retry(&mut self) {
        self.attempts = 0;
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(len: u64, modified_ms: u128) -> FileFingerprint {
        FileFingerprint {
            len,
            modified_ms,
            present: true,
        }
    }

    fn gate() -> ReloadGate {
        ReloadGate::new(WatchConfig::default(), stamp(10, 1))
    }

    #[test]
    fn ignores_unchanged_stamp() {
        let mut gate = gate();
        assert_eq!(gate.on_signal(stamp(10, 1), 0), GateDecision::Idle);
    }

    #[test]
    fn debounces_until_stable() {
        let mut gate = gate();
        assert_eq!(
            gate.on_signal(stamp(11, 2), 0),
            GateDecision::Wait { until_ms: 750 }
        );
        assert_eq!(
            gate.on_tick(stamp(11, 2), 700),
            GateDecision::Wait { until_ms: 750 }
        );
        assert_eq!(
            gate.on_tick(stamp(11, 2), 750),
            GateDecision::Load {
                reason: ReloadReason::WatchChange
            }
        );
        assert_eq!(gate.on_tick(stamp(11, 2), 760), GateDecision::Idle);
    }

    #[test]
    fn extends_debounce_when_stamp_changes() {
        let mut gate = gate();
        gate.on_signal(stamp(11, 2), 0);
        assert_eq!(
            gate.on_tick(stamp(12, 3), 800),
            GateDecision::Wait { until_ms: 1550 }
        );
        assert_eq!(
            gate.on_tick(stamp(12, 3), 1550),
            GateDecision::Load {
                reason: ReloadReason::WatchChange
            }
        );
    }

    #[test]
    fn reverting_to_loaded_stamp_cancels_reload() {
        let mut gate = gate();
        gate.on_signal(stamp(11, 2), 0);
        assert_eq!(gate.on_signal(stamp(10, 1), 100), GateDecision::Idle);
        assert_eq!(gate.on_tick(stamp(10, 1), 1000), GateDecision::Idle);
    }

    #[test]
    fn manual_bypasses_debounce_and_is_not_repeated_while_loading() {
        let mut gate = gate();
        assert_eq!(
            gate.on_manual(),
            GateDecision::Load {
                reason: ReloadReason::Manual
            }
        );
        assert_eq!(gate.on_manual(), GateDecision::Idle);
    }

    #[test]
    fn retries_after_failure_and_notifies_on_first_and_third_attempt() {
        let mut gate = gate();
        gate.on_manual();
        let RetryDecision::Retry {
            attempt,
            next_ms,
            notify,
        } = gate.on_failed(0);
        assert_eq!((attempt, next_ms, notify), (1, 1000, true));
        assert_eq!(
            gate.on_tick(stamp(11, 2), 1000),
            GateDecision::Load {
                reason: ReloadReason::Retry
            }
        );
        let RetryDecision::Retry { notify, .. } = gate.on_failed(1000);
        assert!(!notify);
        gate.on_tick(stamp(11, 2), 2000);
        let RetryDecision::Retry {
            attempt, notify, ..
        } = gate.on_failed(2000);
        assert_eq!(attempt, 3);
        assert!(notify);
    }

    #[test]
    fn successful_load_resets_retry_state() {
        let mut gate = gate();
        gate.on_manual();
        gate.on_failed(0);
        gate.on_loaded(stamp(11, 2));
        assert!(!gate.is_retrying());
        assert_eq!(gate.attempt(), 0);
        assert_eq!(gate.last_loaded(), stamp(11, 2));
    }

    #[test]
    fn missing_file_is_distinct_from_empty_file() {
        let mut gate = gate();
        let missing = FileFingerprint::default();
        assert_ne!(missing, stamp(0, 0));
        assert_eq!(
            gate.on_signal(missing, 0),
            GateDecision::Wait { until_ms: 750 }
        );
    }

    #[test]
    fn retry_stops_when_file_returns_to_loaded_stamp() {
        let mut gate = gate();
        gate.on_manual();
        gate.on_failed(0);
        assert_eq!(gate.on_tick(stamp(10, 1), 1000), GateDecision::Idle);
    }
}
