use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::domain::reload::{GateDecision, ReloadGate, ReloadReason, WatchConfig, WatchMode};
use crate::domain::workbook::WorkbookModel;
use crate::io::loader::WorkbookLoader;
use crate::io::watch::{WatchEvent, WatchNotice, WatchSession, WatchStartup};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadSummary {
    pub sheet_fallback: bool,
    pub filter_reset: bool,
    pub focus_moved: bool,
    pub search_cleared: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadTrigger {
    Watch,
    Manual,
}

impl ReloadTrigger {
    pub fn label(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Manual => "manual",
        }
    }
}

pub enum ReloadMessage {
    Started {
        trigger: ReloadTrigger,
    },
    Loaded {
        workbook: WorkbookModel,
        trigger: ReloadTrigger,
    },
    Failed {
        trigger: ReloadTrigger,
        attempt: u32,
        message: String,
        notify: bool,
        retry_in: Duration,
    },
    WatchNotice(WatchNotice),
    WatchState {
        configured: WatchMode,
        effective: WatchMode,
    },
}

#[derive(Debug, Error)]
pub enum ReloadError {
    #[error("could not start reload worker: {0}")]
    Spawn(String),
}

pub struct ReloadContext {
    pub loader: WorkbookLoader,
    pub watch: WatchConfig,
}

pub struct ReloadHandle {
    pub events: Receiver<ReloadMessage>,
    commands: Sender<Command>,
    loading: Arc<AtomicBool>,
}

impl ReloadHandle {
    pub fn spawn(context: ReloadContext) -> Result<Self, ReloadError> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let loading = Arc::new(AtomicBool::new(false));
        let worker_loading = loading.clone();
        let worker_commands = command_tx.clone();
        std::thread::Builder::new()
            .name("sheetgaze-reload".to_string())
            .spawn(move || {
                run_worker(
                    context,
                    command_rx,
                    worker_commands,
                    event_tx,
                    worker_loading,
                )
            })
            .map_err(|error| ReloadError::Spawn(error.to_string()))?;
        Ok(Self {
            events: event_rx,
            commands: command_tx,
            loading,
        })
    }

    pub fn reload_now(&self) {
        if self.loading.load(Ordering::Relaxed) {
            return;
        }
        let _ = self.commands.send(Command::Now);
    }

    /// Updates the watcher baseline after sheetgaze itself wrote the file so
    /// the save does not trigger an automatic reload.
    pub fn set_baseline(&self, fingerprint: crate::domain::workbook::FileFingerprint) {
        let _ = self.commands.send(Command::Baseline(fingerprint));
    }
}

impl Drop for ReloadHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

enum Command {
    Now,
    Shutdown,
    Baseline(crate::domain::workbook::FileFingerprint),
    Watch(WatchEvent),
}

fn run_worker(
    context: ReloadContext,
    commands: Receiver<Command>,
    command_tx: Sender<Command>,
    events: Sender<ReloadMessage>,
    loading: Arc<AtomicBool>,
) {
    let start = Instant::now();
    let path = context.loader.path().to_path_buf();
    let configured = context.watch.mode;
    let initial_stamp = context.loader.fingerprint();
    let mut gate = ReloadGate::new(context.watch, initial_stamp);
    let mut watch_session = None;
    let mut effective = WatchMode::Off;
    let mut safety_deadline = None;

    match start_watch(&path, &context.watch, &command_tx) {
        Ok(startup) => {
            let WatchStartup {
                session,
                effective: mode,
                notice,
            } = startup;
            watch_session = Some(session);
            effective = mode;
            if let Some(notice) = notice {
                let _ = events.send(ReloadMessage::WatchNotice(notice));
            }
            if mode == WatchMode::Native {
                safety_deadline =
                    Some(now_ms(start).saturating_add(duration_ms(context.watch.native_safety)));
            }
        }
        Err(error) => {
            let _ = events.send(ReloadMessage::WatchNotice(WatchNotice::Unavailable(
                error.to_string(),
            )));
        }
    }
    let _ = events.send(ReloadMessage::WatchState {
        configured,
        effective,
    });

    loop {
        let now = now_ms(start);

        if effective == WatchMode::Native
            && let Some(deadline) = safety_deadline
            && now >= deadline
        {
            let stamp = context.loader.fingerprint();
            let _ = gate.on_signal(stamp, now);
            safety_deadline = Some(now.saturating_add(duration_ms(context.watch.native_safety)));
        }

        if let Some(deadline) = gate.next_deadline_ms()
            && now >= deadline
        {
            let stamp = context.loader.fingerprint();
            match gate.on_tick(stamp, now) {
                GateDecision::Load { reason } => {
                    let trigger = trigger_for(reason);
                    let announce = reason != ReloadReason::Retry;
                    if load_and_apply(
                        &context, &mut gate, trigger, announce, &start, &events, &loading,
                    ) {
                        continue;
                    }
                    break;
                }
                GateDecision::Idle | GateDecision::Wait { .. } => {}
            }
        }

        let wait = next_wait(start, &gate, safety_deadline);
        let received = match wait {
            Some(wait) => commands.recv_timeout(wait),
            None => commands.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(Command::Shutdown) => break,
            Ok(Command::Baseline(stamp)) => {
                gate.on_loaded(stamp);
            }
            Ok(Command::Now) => {
                if let GateDecision::Load { reason } = gate.on_manual() {
                    let trigger = trigger_for(reason);
                    if load_and_apply(
                        &context, &mut gate, trigger, true, &start, &events, &loading,
                    ) {
                        continue;
                    }
                    break;
                }
            }
            Ok(Command::Watch(WatchEvent::Changed)) => {
                let now = now_ms(start);
                let stamp = context.loader.fingerprint();
                let _ = gate.on_signal(stamp, now);
            }
            Ok(Command::Watch(WatchEvent::BackendError(message))) => {
                let _ = events.send(ReloadMessage::WatchNotice(WatchNotice::Fallback {
                    from: effective,
                    reason: message,
                }));
                watch_session = None;
                let fallback = WatchConfig {
                    mode: WatchMode::Poll,
                    ..context.watch
                };
                match start_watch(&path, &fallback, &command_tx) {
                    Ok(startup) => {
                        watch_session = Some(startup.session);
                        effective = WatchMode::Poll;
                        safety_deadline = None;
                    }
                    Err(error) => {
                        effective = WatchMode::Off;
                        safety_deadline = None;
                        let _ = events.send(ReloadMessage::WatchNotice(WatchNotice::Unavailable(
                            error.to_string(),
                        )));
                    }
                }
                let _ = events.send(ReloadMessage::WatchState {
                    configured,
                    effective,
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(watch_session);
}

fn load_and_apply(
    context: &ReloadContext,
    gate: &mut ReloadGate,
    trigger: ReloadTrigger,
    announce: bool,
    start: &Instant,
    events: &Sender<ReloadMessage>,
    loading: &AtomicBool,
) -> bool {
    if announce {
        let _ = events.send(ReloadMessage::Started { trigger });
    }
    loading.store(true, Ordering::Relaxed);
    let before = context.loader.fingerprint();
    let result = context.loader.load();
    loading.store(false, Ordering::Relaxed);
    let now = now_ms(*start);
    match result {
        Ok(workbook) => {
            gate.on_loaded(before);
            let _ = events.send(ReloadMessage::Loaded { workbook, trigger });
            let after = context.loader.fingerprint();
            if after != before {
                let _ = gate.on_signal(after, now);
            }
            true
        }
        Err(error) => {
            let decision = gate.on_failed(now);
            let crate::domain::reload::RetryDecision::Retry {
                attempt, notify, ..
            } = decision;
            let _ = events.send(ReloadMessage::Failed {
                trigger,
                attempt,
                message: error.to_string(),
                notify,
                retry_in: context.watch.retry,
            });
            true
        }
    }
}

fn start_watch(
    path: &Path,
    config: &WatchConfig,
    commands: &Sender<Command>,
) -> Result<WatchStartup, crate::io::watch::WatchError> {
    let commands = commands.clone();
    WatchSession::start(path, config, move |event| {
        let _ = commands.send(Command::Watch(event));
    })
}

fn trigger_for(reason: ReloadReason) -> ReloadTrigger {
    match reason {
        ReloadReason::Manual => ReloadTrigger::Manual,
        ReloadReason::WatchChange | ReloadReason::Retry => ReloadTrigger::Watch,
    }
}

fn next_wait(start: Instant, gate: &ReloadGate, safety_deadline: Option<u64>) -> Option<Duration> {
    let now = now_ms(start);
    let gate_deadline = gate.next_deadline_ms();
    let deadline = match (gate_deadline, safety_deadline) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }?;
    Some(Duration::from_millis(deadline.saturating_sub(now).max(1)))
}

fn now_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
