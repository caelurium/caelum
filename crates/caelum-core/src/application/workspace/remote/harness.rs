use super::{Lane, Pending, Workspace};
use crate::{application::Command, remote::*};
use std::time::{Duration, Instant};

impl Workspace {
    pub fn send_remote_draft(&mut self, id: &str, text: &str) -> Result<bool, String> {
        let expanded = self
            .templates
            .expand(&crate::templates::Target::Boat(id.into()), text)?;
        Ok(self.send_remote(id, expanded.as_deref().unwrap_or(text)))
    }

    pub fn send_remote(&mut self, id: &str, text: &str) -> bool {
        if self.publishing(id) {
            return false;
        }
        let Some(c) = self.remote.saved.checkouts.get_mut(id) else {
            return false;
        };
        if text.trim().is_empty()
            || c.thread.awaiting_native()
            || !c.thread.can_prompt_provider()
            || text.len() > Thread::MAX_MESSAGE_BYTES
            || !matches!(c.phase, Phase::Ready { .. })
            || self.remote.jobs.contains_key(id)
            || c.thread.turn() != &Turn::Idle
            || self.remote.interrupts.contains_key(id)
        {
            return false;
        }
        let op = Operation::Prompt(
            c.target().unwrap(),
            Prompt {
                provider: c.thread.provider,
                model: c.thread.model.clone(),
                effort: c
                    .thread
                    .model
                    .as_ref()
                    .and(c.thread.effort.as_ref())
                    .cloned(),
                conversation: c.thread.conversation().map(str::to_owned),
                text: c.thread.prompt(text),
            },
        );
        c.thread.append(Line::new(Speaker::You, text));
        c.thread.record_activity();
        c.thread.begin_submission();
        c.thread.draft.clear();
        self.feed_request(id, op);
        true
    }

    pub fn recover_remote(&mut self, id: &str, prompt: &str) {
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        if let (Turn::Submitting, Some(target)) = (c.thread.turn(), c.target()) {
            self.feed_request(
                id,
                Operation::Recover(
                    target,
                    c.thread.provider,
                    c.thread.conversation().map(str::to_owned),
                    prompt.trim().into(),
                ),
            );
        }
    }

    /// After inspecting Boat, the user can discard the uncertain submission. No network effect.
    pub fn dismiss_remote_submission(&mut self, id: &str) {
        if matches!(self.remote.feeds.get(id), Some(Job::Busy(_)))
            || matches!(self.remote.jobs.get(id), Some(Job::Deleting(_)))
        {
            return;
        }
        if let Some(c) = self.remote.saved.checkouts.get_mut(id) {
            if c.thread.turn() == &Turn::Submitting {
                c.thread.finish_turn();
                c.thread.append(Line::new(
                    Speaker::Activity,
                    "Unconfirmed submission dismissed",
                ));
                self.remote.feeds.remove(id);
            }
        }
    }

    pub fn follow_remote(&mut self, id: &str) {
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        if let (Some(target), Some(run)) = (c.target(), c.thread.turn().run()) {
            self.feed_request(
                id,
                Operation::Follow(target, run.clone(), c.thread.cursor().map(str::to_owned)),
            );
        }
    }

    fn feed_request(&mut self, id: &str, op: Operation) {
        if matches!(self.remote.feeds.get(id), Some(Job::Busy(_)))
            || matches!(self.remote.jobs.get(id), Some(Job::Deleting(_)))
        {
            return;
        }
        let request = self.request(Command::Remote(op), Pending::Remote(Lane::Feed(id.into())));
        self.remote.feeds.insert(id.into(), Job::Busy(request));
    }

    pub fn interrupt_remote(&mut self, id: &str) {
        if self.remote.interrupts.contains_key(id)
            || matches!(self.remote.jobs.get(id), Some(Job::Deleting(_)))
        {
            return;
        }
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        if matches!(c.thread.turn(), Turn::Stopping(_) | Turn::Finishing(_)) {
            return;
        }
        if let (Some(target), Some(run)) = (c.target(), c.thread.turn().run()) {
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
            if c.thread.turn().run() == Some(run) {
                match result {
                    Ok(Outcome::Interrupted) => c.thread.stop_run(run),
                    Err(error) => c.thread.append(Line::new(Speaker::Activity, error)),
                    _ => {}
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
                if c.other_threads
                    .iter()
                    .any(|t| t.conversation() == Some(run.conversation.as_str())) =>
            {
                self.remote.feeds.insert(
                    id.into(),
                    Job::Failed("That prompt belongs to another conversation on this VM.".into()),
                );
            }
            Ok(Outcome::Accepted(run)) => {
                if c.thread.accept_run(run) {
                    self.follow_remote(id);
                } else {
                    self.remote.feeds.insert(
                        id.into(),
                        Job::Failed("Unexpected VM conversation response".into()),
                    );
                }
            }
            Ok(Outcome::Rejected(error)) if c.thread.turn() == &Turn::Submitting => {
                c.thread.finish_turn();
                if c.thread
                    .lines
                    .last()
                    .is_some_and(|l| l.speaker == Speaker::You)
                {
                    let line = c.thread.lines.pop().unwrap();
                    if c.thread.draft.is_empty() {
                        c.thread.draft = line.text;
                    } else {
                        c.thread.append(Line::new(
                            Speaker::Activity,
                            format!("Not sent: {}", line.text),
                        ));
                    }
                }
                self.remote.feeds.insert(id.into(), Job::Failed(error));
            }
            Ok(Outcome::Page(page)) if c.thread.turn().run().is_some() => {
                for line in page.lines {
                    c.thread.append(line);
                }
                c.thread.record_page(page.cursor, &page.status);
                if let RunStatus::Ended(status) = page.status {
                    let usage = crate::telemetry::boat(
                        id,
                        &c.thread,
                        page.model,
                        crate::application::presentation::now(),
                    );
                    c.thread.record_activity();
                    for line in c
                        .thread
                        .lines
                        .iter_mut()
                        .rev()
                        .take_while(|l| l.speaker != Speaker::You)
                    {
                        line.partial = false;
                        if line.speaker == Speaker::Tool(ToolState::Running) {
                            line.speaker = Speaker::Tool(ToolState::Reported);
                        }
                    }
                    c.thread.finish_turn();
                    c.thread
                        .append(Line::new(Speaker::Outcome(status), status.label()));
                    self.templates_changed(crate::templates::Target::Boat(id.into()));
                    if self.remote.saved.selected.as_deref() == Some(id)
                        && self.remote.changes.contains_key(id)
                    {
                        self.review_remote(id);
                    }
                    if let Some(row) = usage {
                        self.record_usage(row);
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
                self.refresh_open_vm_tool(id);
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
