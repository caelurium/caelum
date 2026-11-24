use crate::Workspace;
use crate::context::EditMenu;
use crate::live::{Live, live, live_mut};
use crate::panels::{Panels, Screen, Target};
use caelum_core::application::{Review, Task, Workspace as Application};
use caelum_protocol::{GitHubDiffSide, GitHubReviewWrite};
use rune::{El, TextEdit, text_input};
use rune_kit::Kit;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::panels) struct Owner {
    pub(super) key: Target,
    head: String,
    revision: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Field {
    Summary,
    Line {
        path: String,
        side: GitHubDiffSide,
        line: u32,
    },
    Reply(String),
}

pub(in crate::panels) struct Editor {
    pub(in crate::panels) owner: Owner,
    pub(super) field: Field,
    pub(super) text: TextEdit,
}

impl Owner {
    pub(in crate::panels) fn current(l: &Live) -> Option<Self> {
        let key = l.panels.review.as_ref()?;
        let r = l.app.services.reviews.get(key)?;
        Some(Self {
            key: key.clone(),
            head: r.writable_head()?.into(),
            revision: r.draft_revision(),
        })
    }

    pub(in crate::panels) fn matches(&self, panels: &Panels, app: &Application) -> bool {
        panels.screen == Screen::Review
            && panels.review.as_ref() == Some(&self.key)
            && app.services.reviews.get(&self.key).is_some_and(|r| {
                r.writable_head() == Some(&self.head) && r.draft_revision() == self.revision
            })
    }

    pub(in crate::panels) fn busy(&self, l: &Live) -> bool {
        l.app.services.review_mutating(&self.key.0, &self.key.1)
    }

    pub(super) fn editable<'a>(&self, l: &'a mut Live) -> Option<&'a mut Review> {
        if self.matches(&l.panels, &l.app) && !self.busy(l) {
            l.app.services.reviews.get_mut(&self.key)
        } else {
            None
        }
    }

    pub(super) fn open(&self, s: &mut Workspace, field: Field) {
        let l = live_mut(s);
        let Some(r) = self.editable(l) else { return };
        let initial = match &field {
            Field::Summary => &r.draft.body,
            Field::Reply(id) => r.reply_drafts.get(id).map_or("", String::as_str),
            Field::Line { .. } => "",
        };
        let mut text = TextEdit::new(initial);
        text.focused = true;
        l.panels.review_editor = Some(Editor {
            owner: self.clone(),
            field,
            text,
        });
    }

    pub(super) fn send(&self, s: &mut Workspace, action: GitHubReviewWrite) {
        let l = live_mut(s);
        if let Some(base) = self
            .editable(l)
            .and_then(|r| r.expected_base())
            .map(str::to_owned)
        {
            l.app.task(Task::WriteReview {
                project: self.key.0.clone(),
                url: self.key.1.clone(),
                head: self.head.clone(),
                base,
                action,
            });
            if self.busy(l) {
                close(s);
            }
        }
    }
}

pub(super) fn close(s: &mut Workspace) {
    live_mut(s).panels.review_editor = None;
}

impl Editor {
    pub(super) fn active(s: &Workspace) -> Option<&Self> {
        let l = live(s);
        l.panels
            .review_editor
            .as_ref()
            .filter(|e| e.owner.matches(&l.panels, &l.app))
    }

    pub(super) fn input(&self, key: &str, label: &str, k: &Kit) -> El {
        let owner = self.owner.clone();
        let field = self.field.clone();
        text_input(&self.text, k.ty.ui)
            .label(label)
            .placeholder(label, k.c.text_muted)
            .color(k.c.text_base)
            .selection_color(k.selection())
            .w_full()
            .min_w(0.0)
            .h(68.0)
            .padding(8.0)
            .radius(6.0)
            .fill(k.c.bg_base)
            .border(
                1.0,
                if self.text.focused {
                    k.c.border_focus
                } else {
                    k.c.border_base
                },
            )
            .edit_menu_changed(
                key,
                move |s| {
                    let l = live_mut(s);
                    let editor = l.panels.review_editor.as_mut()?;
                    (editor.owner == owner && editor.field == field).then_some(&mut editor.text)
                },
                |s, _| {
                    let l = live_mut(s);
                    let Some(editor) = &l.panels.review_editor else {
                        return;
                    };
                    if !editor.owner.matches(&l.panels, &l.app) {
                        return;
                    }
                    let r = l.app.services.reviews.get_mut(&editor.owner.key).unwrap();
                    match &editor.field {
                        Field::Summary => r.draft.body.clone_from(&editor.text.text),
                        Field::Reply(id) if editor.text.text.is_empty() => {
                            r.reply_drafts.remove(id);
                        }
                        Field::Reply(id) => {
                            r.reply_drafts.insert(id.clone(), editor.text.text.clone());
                        }
                        Field::Line { .. } => {}
                    }
                },
            )
    }
}
