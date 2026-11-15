//! Diagnostic callbacks observe native frames without requesting additional frames.
#[cfg(test)]
mod tests;
mod workload;

pub use workload::workspace;

use crate::{Workspace, live::Live, panels::Screen};
use caelum_core::diagnostics::{self, Mark};
use std::cell::{Cell, RefCell};

enum Completion {
    Frame,
    History { drawn: bool },
}

struct Pending {
    mark: Mark,
    target: Option<usize>,
    navigation: Option<u64>,
    completion: Completion,
}

#[derive(Clone, Copy, Default)]
enum Replay {
    #[default]
    Disabled,
    Pending(workload::Scenario),
    Running,
}

#[derive(Default)]
pub struct State {
    pending: RefCell<Vec<Pending>>,
    navigation: Cell<u64>,
    replay: Cell<Replay>,
    first_frame: Cell<bool>,
}

impl State {
    pub fn begin(&self, label: &'static str) -> Option<Mark> {
        let mark = diagnostics::next("interaction", label, 0)?;
        mark.record("begin", 0);
        Some(mark)
    }

    pub fn track(&self, mark: Option<Mark>, target: Option<usize>, history: bool) {
        let Some(mark) = mark else { return };
        let navigation = history.then(|| {
            let next = self.navigation.get().wrapping_add(1);
            self.navigation.set(next);
            next
        });
        let mut pending = self.pending.borrow_mut();
        if pending.len() == 256 {
            mark.record("overflow", 0);
            return;
        }
        pending.push(Pending {
            mark,
            target,
            navigation,
            completion: if history {
                Completion::History { drawn: false }
            } else {
                Completion::Frame
            },
        });
    }

    fn finish_frame(&self, live: &Live, commits: u64) {
        self.pending.borrow_mut().retain_mut(|pending| {
            let visible = match pending.target {
                Some(index) => {
                    live.app.selected == Some(index)
                        && live.panels.screen == Screen::Conversation
                        && live.panels.overlay.is_none()
                }
                None => live.panels.screen == Screen::Home,
            };
            if !visible
                || pending
                    .navigation
                    .is_some_and(|n| n != self.navigation.get())
            {
                pending.mark.record("abandoned", 0);
                return false;
            }
            match &mut pending.completion {
                Completion::Frame => {
                    pending.mark.record("drawn", commits);
                    false
                }
                Completion::History { drawn } => {
                    if !*drawn {
                        pending.mark.record("drawn", commits);
                        *drawn = true;
                    }
                    if pending.target.is_some_and(|i| {
                        live.app.sessions[i].loaded && !live.app.sessions[i].loading
                    }) {
                        pending.mark.record("ready", commits);
                        false
                    } else {
                        true
                    }
                }
            }
        });
    }
}

pub fn edit(live: &Live) {
    let target = (live.panels.screen == Screen::Conversation)
        .then_some(live.app.selected)
        .flatten();
    live.latency
        .track(live.latency.begin("edit"), target, false);
}

pub fn encoded(frame: &rune::FrameEncoded) {
    let outcome = match frame.outcome() {
        rune::FrameOutcome::Drawn => "drawn",
        rune::FrameOutcome::Unchanged => "unchanged",
        _ => "skipped",
    };
    if let Some(mark) = diagnostics::mark("frame", outcome, frame.seq(), 0) {
        mark.record("update", (frame.update_ms().unwrap_or(0.0) * 1000.0) as u64);
        mark.record("encode", (frame.encode_ms().unwrap_or(0.0) * 1000.0) as u64);
    }
}

pub fn drawn(rt: &rune::Runtime<Workspace>) {
    let Some(live) = &rt.state.live else { return };
    if diagnostics::enabled() {
        if !live.latency.first_frame.replace(true)
            && let Some(mark) = diagnostics::mark("startup", "first_frame", 0, 0)
        {
            mark.record("drawn", 0);
        }
        live.latency.finish_frame(live, rt.stats.commits);
    }
    if let Replay::Pending(scenario) = live.latency.replay.get()
        && let Some(handle) = &rt.state.handle
    {
        live.latency.replay.set(Replay::Running);
        workload::run(handle.clone(), scenario);
    }
}
