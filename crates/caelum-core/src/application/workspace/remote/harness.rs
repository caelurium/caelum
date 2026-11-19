use super::{Lane, Pending, Workspace};
use crate::{application::Command, remote::*};
use std::time::{Duration, Instant};

impl Workspace {
    pub fn send_remote(&mut self, id: &str, text: &str) -> bool {
        let Some(c) = self.remote.saved.checkouts.get_mut(id) else {
            return false;
        };
        if text.trim().is_empty()
            || text.len() > 64 * 1024
            || !matches!(c.phase, Phase::Ready { .. })
            || self.remote.jobs.contains_key(id)
            || c.thread.turn != Turn::Idle
            || self.remote.interrupts.contains_key(id)
        {
            return false;
        }
        let op = Operation::Prompt(
            c.target().unwrap(),
            c.thread.conversation.clone(),
            text.into(),
        );
        c.thread.append(Line {
            speaker: Speaker::You,
            text: text.into(),
            partial: false,
        });
        c.thread.turn = Turn::Submitting;
        c.thread.draft.clear();
        self.feed_request(id, op);
        true
    }

    pub fn recover_remote(&mut self, id: &str, prompt: &str) {
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        if let (Turn::Submitting, Some(target)) = (&c.thread.turn, c.target()) {
            self.feed_request(
                id,
                Operation::Recover(target, c.thread.conversation.clone(), prompt.trim().into()),
            );
        }
    }

    /// After inspecting Boat, the user can discard the uncertain submission. No network effect.
    pub fn dismiss_remote_submission(&mut self, id: &str) {
        if matches!(self.remote.feeds.get(id), Some(Job::Busy(_))) {
            return;
        }
        if let Some(c) = self.remote.saved.checkouts.get_mut(id) {
            if c.thread.turn == Turn::Submitting {
                c.thread.turn = Turn::Idle;
                c.thread.append(Line {
                    speaker: Speaker::Activity,
                    text: "Unconfirmed submission dismissed".into(),
                    partial: false,
                });
                self.remote.feeds.remove(id);
            }
        }
    }

    pub fn follow_remote(&mut self, id: &str) {
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        if let (Some(target), Turn::Following(run)) = (c.target(), &c.thread.turn) {
            self.feed_request(
                id,
                Operation::Follow(target, run.clone(), c.thread.cursor.clone()),
            );
        }
    }

    fn feed_request(&mut self, id: &str, op: Operation) {
        if matches!(self.remote.feeds.get(id), Some(Job::Busy(_))) {
            return;
        }
        let request = self.request(Command::Remote(op), Pending::Remote(Lane::Feed(id.into())));
        self.remote.feeds.insert(id.into(), Job::Busy(request));
    }

    pub fn interrupt_remote(&mut self, id: &str) {
        if self.remote.interrupts.contains_key(id) {
            return;
        }
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        if let (Some(target), Turn::Following(run)) = (c.target(), &c.thread.turn) {
            let request = self.request(
                Command::Remote(Operation::Interrupt(target, run.clone())),
                Pending::Remote(Lane::Interrupt(id.into(), run.clone())),
            );
            self.remote.interrupts.insert(id.into(), request);
        }
    }
    pub(super) fn complete_interrupt(
        &mut self,
        id: &str,
        req: u64,
        run: &Run,
        result: Result<Outcome, String>,
    ) {
        if self.remote.interrupts.get(id) != Some(&req) {
            return;
        }
        self.remote.interrupts.remove(id);
        if let Some(c) = self.remote.saved.checkouts.get_mut(id) {
            if matches!(&c.thread.turn,Turn::Following(r) if r==run) {
                if let Err(error) = result {
                    c.thread.append(Line {
                        speaker: Speaker::Activity,
                        text: error,
                        partial: false,
                    });
                }
                self.follow_remote(id);
            }
        }
    }
    pub(super) fn complete_feed(&mut self, id: &str, req: u64, result: Result<Outcome, String>) {
        if !matches!(self.remote.feeds.get(id),Some(Job::Busy(r)) if *r==req) {
            return;
        }
        self.remote.feeds.remove(id);
        let Some(c) = self.remote.saved.checkouts.get_mut(id) else {
            return;
        };
        match result {
            Ok(Outcome::Accepted(run))
                if c.thread.turn == Turn::Submitting
                    && c.thread
                        .conversation
                        .as_ref()
                        .is_none_or(|v| v == &run.conversation) =>
            {
                c.thread.conversation = Some(run.conversation.clone());
                c.thread.turn = Turn::Following(run);
                self.follow_remote(id);
            }
            Ok(Outcome::Rejected(error)) if c.thread.turn == Turn::Submitting => {
                c.thread.turn = Turn::Idle;
                if let Some(line) = c
                    .thread
                    .lines
                    .last()
                    .filter(|l| l.speaker == Speaker::You && c.thread.draft.is_empty())
                {
                    c.thread.draft.clone_from(&line.text);
                }
                self.remote.feeds.insert(id.into(), Job::Failed(error));
            }
            Ok(Outcome::Page(page)) if matches!(&c.thread.turn, Turn::Following(_)) => {
                for line in page.lines {
                    c.thread.append(line);
                }
                c.thread.cursor = page.cursor;
                if let Some(status) = page.finished {
                    if let Some(last) = c.thread.lines.last_mut() {
                        last.partial = false;
                    }
                    c.thread.turn = Turn::Idle;
                    if status != "finished" {
                        c.thread.append(Line {
                            speaker: Speaker::Activity,
                            text: status,
                            partial: false,
                        });
                    }
                } else {
                    let delay = if page.more {
                        Duration::ZERO
                    } else {
                        Duration::from_secs(1)
                    };
                    self.remote
                        .feeds
                        .insert(id.into(), Job::Wait(Instant::now() + delay));
                }
            }
            other => {
                self.remote.feeds.insert(
                    id.into(),
                    Job::Failed(
                        other
                            .err()
                            .unwrap_or_else(|| "Unexpected remote conversation response".into()),
                    ),
                );
            }
        }
    }
    pub fn feed_deadline(&mut self, now: Instant) -> Option<Instant> {
        let due: Vec<_> = self
            .remote
            .feeds
            .iter()
            .filter(|(_, j)| matches!(j,Job::Wait(at) if *at<=now))
            .map(|(k, _)| k.clone())
            .collect();
        for id in due {
            self.follow_remote(&id);
        }
        self.remote
            .feeds
            .values()
            .filter_map(|j| match j {
                Job::Wait(at) => Some(*at),
                _ => None,
            })
            .min()
    }
}
