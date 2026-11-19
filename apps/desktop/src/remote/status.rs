use caelum_core::remote::{Checkout, Job, Phase, Speaker, State, ToolState, Turn, TurnEnd};
use rune::{Color, El, row, text};
use rune_kit::Kit;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Ready,
    Busy,
    Quiet,
    Attention,
    Error,
}

pub(super) struct Status {
    pub label: String,
    tone: Tone,
}

impl Status {
    fn new(tone: Tone, label: impl Into<String>) -> Self {
        Self {
            tone,
            label: label.into(),
        }
    }

    pub fn view(&self, k: &Kit) -> El {
        let color = match self.tone {
            Tone::Ready => k.c.success.fg,
            Tone::Busy => k.c.text_accent,
            Tone::Quiet => k.c.text_faint,
            Tone::Attention => k.c.warning.fg,
            Tone::Error => k.c.danger.fg,
        };
        row()
            .min_w(0.0)
            .min_h(22.0)
            .center_y()
            .gap(6.0)
            .role("status")
            .label(&self.label)
            .help(&self.label)
            .child(dot(color))
            .child(if self.tone == Tone::Busy {
                crate::ui::busy_text(&self.label, k.ty.small_medium, k)
                    .min_w(0.0)
                    .clip_rounded(0.0)
            } else {
                text(&self.label)
                    .font(k.ty.small_medium)
                    .color(k.c.text_muted)
                    .min_w(0.0)
                    .truncate()
            })
    }
}

fn dot(color: Color) -> El {
    rune::el()
        .size(5.0, 5.0)
        .radius(2.5)
        .fill(color)
        .a11y_hidden()
}

pub(super) fn status(r: &State, id: &str, c: &Checkout) -> Status {
    use Tone::*;
    if matches!(r.jobs.get(id), Some(Job::Deleting(_))) {
        return Status::new(Busy, "Deleting VM on Boat…");
    }
    if r.delete_errors.contains_key(id) {
        return Status::new(Error, "VM deletion failed");
    }
    if matches!(r.jobs.get(id), Some(Job::Failed(_))) {
        return Status::new(
            Error,
            if matches!(c.phase, Phase::Ready { .. }) {
                "Checkout needs attention"
            } else {
                "Setup needs attention"
            },
        );
    }
    if let Phase::Create(_) = c.phase {
        return if r.jobs.contains_key(id) {
            Status::new(Busy, "Creating VM…")
        } else {
            Status::new(Quiet, "Setup not finished")
        };
    }
    let stopped = c
        .machine()
        .is_some_and(|m| matches!(m.state.as_str(), "stopped" | "archived"));
    if stopped {
        return Status::new(Quiet, "Machine stopped");
    }
    if let Phase::Prepare { machine, .. } = &c.phase {
        return match machine.state.as_str() {
            "init" | "provisioning" | "provisioned" => Status::new(Busy, "Starting VM…"),
            "cloning" => Status::new(Busy, "Cloning repository…"),
            "archiving" => Status::new(Busy, "Stopping machine…"),
            "error" | "failed" | "cancelled" => Status::new(Error, "Machine unavailable"),
            _ => Status::new(Busy, "Preparing checkout…"),
        };
    }
    if r.interrupts.contains_key(id) {
        return Status::new(Attention, "Stopping turn…");
    }
    let feed = r.feeds.get(id);
    match &c.thread.turn {
        Turn::Submitting if matches!(feed, Some(Job::Busy(_))) => {
            Status::new(Busy, "Waiting for Boat to confirm…")
        }
        Turn::Submitting => Status::new(Attention, "Delivery unconfirmed"),
        t if t.run().is_some() && matches!(feed, Some(Job::Failed(_))) => {
            Status::new(Attention, "Live updates paused")
        }
        t if t.run().is_some() && feed.is_none() => {
            if matches!(r.jobs.get(id), Some(Job::Busy(_) | Job::Wait(_))) {
                Status::new(Busy, "Reconnecting to turn…")
            } else {
                Status::new(Quiet, "Open to reconnect")
            }
        }
        Turn::Queued(_) => Status::new(Busy, "Queued on Boat…"),
        Turn::Finishing(_) => Status::new(Busy, "Receiving final updates…"),
        Turn::Stopping(_) => Status::new(Attention, "Stopping turn…"),
        Turn::Following(_) => {
            for l in c
                .thread
                .lines
                .iter()
                .rev()
                .take_while(|l| l.speaker != Speaker::You)
            {
                if l.speaker == Speaker::Tool(ToolState::Running) {
                    if let Some(detail) = &l.tool_detail {
                        let verb = match l.text.to_ascii_lowercase().as_str() {
                            "read" => "Reading",
                            "edit" => "Editing",
                            "write" => "Writing",
                            _ => "Running",
                        };
                        return Status::new(Busy, format!("{verb} {detail}…"));
                    }
                    return Status::new(Busy, format!("Using {}…", l.text));
                }
                if l.speaker == Speaker::OpenCode && l.partial {
                    return Status::new(Busy, "Responding…");
                }
            }
            Status::new(Busy, "OpenCode is working…")
        }
        Turn::Idle if matches!(feed, Some(Job::Failed(_))) => {
            Status::new(Attention, "Message not sent")
        }
        Turn::Idle if matches!(r.jobs.get(id), Some(Job::Busy(_) | Job::Wait(_))) => {
            Status::new(Busy, "Checking machine…")
        }
        Turn::Idle
            if c.thread
                .lines
                .last()
                .is_some_and(|l| l.speaker == Speaker::Outcome(TurnEnd::Failed)) =>
        {
            Status::new(Error, "Last turn failed")
        }
        Turn::Idle => Status::new(Ready, "Ready"),
    }
}

pub(super) fn branch(c: &Checkout) -> Option<&str> {
    match &c.phase {
        Phase::Create(p) => Some(&p.branch),
        Phase::Prepare { new_branch, .. } => new_branch.as_deref(),
        Phase::Ready { branch, .. } => Some(branch),
    }
}
