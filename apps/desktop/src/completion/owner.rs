use crate::{live::Live, panels::Screen};
use caelum_core::templates::Target;
use rune::TextEdit;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum EditorOwner {
    Composer(usize),
    SideQuestion(usize),
    Home(String),
    Boat { id: String, thread: String },
}

impl EditorOwner {
    pub(super) fn index(&self) -> Option<usize> {
        match self {
            Self::Composer(i) | Self::SideQuestion(i) => Some(*i),
            _ => None,
        }
    }

    pub(super) fn current(l: &Live) -> Option<Self> {
        match l.panels.screen {
            Screen::Home => Some(Self::Home(l.app.home.draft.project.clone()?)),
            Screen::Remote => {
                let id = l.app.remote.saved.selected.as_ref()?;
                let c = l.app.remote.saved.checkouts.get(id)?;
                if !l.app.can_open_vm_session(id) || !c.thread.can_prompt_provider() {
                    return None;
                }
                Some(Self::Boat {
                    id: id.clone(),
                    thread: c.thread.editor_key(id).into(),
                })
            }
            Screen::Conversation => {
                let i = l.app.selected?;
                if !l.app.sessions[i].scope.provider_available() {
                    return None;
                }
                Some(if l.commands.side_owner == Some(i) {
                    Self::SideQuestion(i)
                } else {
                    Self::Composer(i)
                })
            }
            _ => None,
        }
    }
    pub(super) fn target(&self, l: &Live) -> Option<Target> {
        match self {
            Self::Composer(i) => Some(Target::Checkout(l.app.sessions[*i].scope.project.clone())),
            Self::Home(p) => Some(Target::Checkout(p.clone())),
            Self::Boat { id, .. } => Some(Target::Boat(id.clone())),
            _ => None,
        }
    }
    pub(super) fn editor<'a>(&self, l: &'a Live) -> Option<&'a TextEdit> {
        match self {
            Self::Composer(i) => l.drafts.get(*i),
            Self::SideQuestion(i) => {
                (l.commands.side_owner == Some(*i)).then_some(&l.commands.side_editor)
            }
            Self::Home(_) => Some(&l.home.editor),
            Self::Boat { thread, .. } => crate::remote::draft_editor(l, thread),
        }
    }
    pub(super) fn replace(&self, l: &mut Live, text: String, caret: usize) {
        let editor = match self {
            Self::Composer(i) => &mut l.drafts[*i],
            Self::Home(_) => &mut l.home.editor,
            Self::Boat { id, thread } => {
                l.app
                    .remote
                    .saved
                    .checkouts
                    .get_mut(id)
                    .unwrap()
                    .thread
                    .draft
                    .clone_from(&text);
                let Some(e) = crate::remote::draft_editor_mut(l, thread) else {
                    return;
                };
                e
            }
            Self::SideQuestion(_) => return,
        };
        editor.replace_text(text);
        editor.set_caret(caret);
        editor.focused = true;
    }
}
