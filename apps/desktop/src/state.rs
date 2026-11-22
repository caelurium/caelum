use std::time::Duration;

use rune::{TextEdit, Tx};
use rune_kit::{
    diff::{self, DiffState},
    shell::PaneState,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub title: String,
    pub secondary_title: String,
    pub prompt: String,
    pub command: String,
    pub output: String,
    pub response: String,
    pub draft: String,
    pub patch: String,
}

pub struct Turn {
    pub prompt: String,
    phase: Response,
    pub tool: Option<Disclosure>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Disclosure {
    Closed,
    Open,
}

enum Response {
    Complete(String),
    Streaming {
        text: &'static str,
        visible: String,
        clock: StreamClock,
    },
    Stopped(String),
}

enum StreamClock {
    NotDrawn,
    Running { since: f64 },
}

impl Turn {
    pub fn response(&self) -> &str {
        match &self.phase {
            Response::Complete(text) | Response::Stopped(text) => text,
            Response::Streaming { visible, .. } => visible,
        }
    }

    pub fn working(&self) -> bool {
        matches!(self.phase, Response::Streaming { .. })
    }

    pub fn stopped(&self) -> bool {
        matches!(self.phase, Response::Stopped(_))
    }

    pub fn toggle_tool(&mut self) {
        self.tool = self.tool.map(|disclosure| match disclosure {
            Disclosure::Open => Disclosure::Closed,
            Disclosure::Closed => Disclosure::Open,
        });
    }

    pub fn expanded(&self) -> bool {
        self.tool == Some(Disclosure::Open)
    }
}

pub struct Session {
    pub title: String,
    pub draft: TextEdit,
    pub turns: Vec<Turn>,
}

impl Session {
    fn new(title: String) -> Self {
        Self {
            title,
            draft: TextEdit::new(""),
            turns: Vec::new(),
        }
    }

    pub fn working(&self) -> bool {
        self.turns.last().is_some_and(Turn::working)
    }

    pub fn stop(&mut self) {
        if let Some(turn) = self.turns.last_mut()
            && let Response::Streaming { visible, .. } = &mut turn.phase
        {
            turn.phase = Response::Stopped(std::mem::take(visible));
        }
    }
}

pub struct Workspace {
    pub handle: Option<rune::AppHandle<Self>>,
    pub live: Option<crate::live::Live>,
    pub sample: Sample,
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub diff: DiffState,
}

impl Workspace {
    pub fn new() -> Self {
        let sample: Sample =
            serde_json::from_str(include_str!("../../../docs/design/workspace.json"))
                .expect("valid shared workspace fixture");
        let mut first = Session::new(sample.title.clone());
        first.draft = TextEdit::new(sample.draft.clone());
        first.turns.push(Turn {
            prompt: sample.prompt.clone(),
            phase: Response::Complete(sample.response.clone()),
            tool: Some(Disclosure::Closed),
        });
        let second = Session::new(sample.secondary_title.clone());
        let diff = DiffState::new(diff::parse(&sample.patch));
        Self {
            handle: None,
            live: None,
            sample,
            sessions: vec![first, second],
            selected: 0,
            sidebar: PaneState::new(238.0).range(200.0, 360.0),
            inspector: PaneState::new(460.0).range(280.0, 560.0).collapsed(),
            diff,
        }
    }

    pub fn live(project: Option<String>) -> Self {
        let mut workspace = Self::new();
        workspace.live = Some(crate::live::Live::new(project));
        workspace
    }

    pub fn title(&self) -> &str {
        if let Some(live) = &self.live {
            if live.panels.screen == crate::panels::Screen::Home {
                return "Home";
            }
            if live.panels.screen == crate::panels::Screen::Remote {
                return live
                    .app
                    .remote
                    .saved
                    .selected
                    .as_ref()
                    .and_then(|id| live.app.remote.saved.checkouts.get(id))
                    .map_or("Remote checkout", |c| {
                        c.thread.heading().unwrap_or_else(|| c.title())
                    });
            }
            live.app
                .selected
                .map(|i| live.app.sessions[i].title.as_str())
                .unwrap_or("Caelum")
        } else {
            &self.active().title
        }
    }

    pub fn working(&self) -> bool {
        self.live
            .as_ref()
            .map_or_else(|| self.active().working(), |live| live.working())
    }

    pub fn stop(&mut self) {
        if let Some(live) = &mut self.live {
            live.stop();
        } else {
            self.active_mut().stop();
        }
    }

    pub fn focus(&mut self) {
        if let Some(live) = &mut self.live {
            live.focus();
        } else {
            self.active_mut().draft.focused = true;
        }
    }

    pub fn active(&self) -> &Session {
        &self.sessions[self.selected]
    }

    pub fn active_mut(&mut self) -> &mut Session {
        &mut self.sessions[self.selected]
    }

    pub fn new_session(&mut self) {
        if let Some(live) = &mut self.live {
            if live.panels.screen == crate::panels::Screen::Remote {
                if let Some(id) = live.app.remote.saved.selected.clone()
                    && live.panels.overlay().is_none()
                    && live.app.can_open_vm_session(&id)
                {
                    crate::remote::new_session(self, &id);
                } else {
                    crate::panels::open_launcher(self);
                }
                return;
            }
            live.new_session();
            return;
        }
        self.selected = self.sessions.len();
        let mut session = Session::new("New session".into());
        session.draft.focused = true;
        self.sessions.push(session);
    }

    pub fn send(&mut self, tx: &mut Tx) {
        if crate::home::is_home(self) {
            crate::home::send(self, tx);
            return;
        }
        if self.live.is_some() && crate::commands::dispatch(self, tx) {
            return;
        }
        if let Some(live) = &mut self.live {
            live.send(tx);
            return;
        }
        let session = self.active_mut();
        let prompt = session.draft.text.trim().to_owned();
        if prompt.is_empty() || session.working() {
            return;
        }
        if session.turns.is_empty() {
            session.title = prompt
                .lines()
                .next()
                .unwrap_or(&prompt)
                .chars()
                .take(48)
                .collect();
        }
        session.turns.push(Turn {
            prompt,
            phase: Response::Streaming {
                text: "## Reviewing the details\n\nThis is a **simulated response** so you can try Rune’s interaction and rendering.\n\n- Resize the sidebar and changes inspector.\n- Switch sessions while this reply arrives; each draft stays with its session.\n- Select or copy text, then try a multiline message.\n\n```css\n.workspace {\n  --radius: 12px;\n  --space: 16px;\n  --motion: 160ms;\n}\n```\n\nOpen **Changes** to compare the sample patch.",
                visible: String::new(),
                clock: StreamClock::NotDrawn,
            },
            tool: None,
        });
        session.draft.clear();
        session.draft.focused = true;
        tx.scroll_to_bottom(format!("transcript-{}", self.selected));
        tx.wake_after(Duration::from_millis(25));
    }
}

pub fn poll(s: &mut Workspace, now: f64, tx: &mut Tx) -> bool {
    if let Some(live) = &mut s.live {
        if !s.sidebar.is_open() {
            live.feed.hidden();
        }
        let changed = live.poll(tx);
        if !live.app.remote.removed.is_empty() {
            return crate::remote::finish_removals(s);
        }
        if live.panels.overlay() == Some(crate::panels::Screen::VmHandoff)
            && live.app.enter_confirmed_vm()
        {
            crate::remote::enter_new(s);
            return true;
        }
        return crate::context::sync(live, tx) || changed;
    }
    let mut changed = false;
    for session in &mut s.sessions {
        let Some(turn) = session.turns.last_mut() else {
            continue;
        };
        let Response::Streaming {
            text,
            visible,
            clock,
        } = &mut turn.phase
        else {
            continue;
        };
        let start = match clock {
            StreamClock::NotDrawn => {
                *clock = StreamClock::Running { since: now };
                now
            }
            StreamClock::Running { since } => *since,
        };
        let count = ((now - start).max(0.0) * 90.0) as usize;
        let next: String = text.chars().take(count).collect();
        if next.len() == text.len() {
            turn.phase = Response::Complete(next);
            changed = true;
        } else {
            changed |= *visible != next;
            *visible = next;
            tx.wake_after(Duration::from_millis(25));
        }
    }
    changed
}
