use super::Live;
use rune::{ScrollPos, Tx};
use std::{cell::Cell, rc::Rc};

pub(crate) struct Bookmark {
    owner: usize,
    entry: String,
    inset: f32,
    settled: bool,
}

pub(super) struct Place {
    pub(super) position: Rc<Cell<ScrollPos>>,
    bookmark: Option<Bookmark>,
}

impl Place {
    pub(super) fn new(offset: f32) -> Self {
        let mut position = ScrollPos::default();
        if offset >= 0.0 {
            position.offset[1] = offset;
            position.max[1] = offset + 10.0;
        }
        Self {
            position: Rc::new(Cell::new(position)),
            bookmark: None,
        }
    }
}

impl Live {
    pub(crate) fn reading_position(&self, index: usize) -> ScrollPos {
        self.reading[index].position.get()
    }

    pub(crate) fn reading_bookmark_for(&self, owner: usize) -> Option<Bookmark> {
        let position = self.reading.get(owner)?.position.get();
        if position.offset[1] >= position.max[1] - 1.0 {
            return None;
        }
        let viewport = rune::memo::window_layout(&format!("live-transcript-{owner}"));
        self.visible_entries
            .iter()
            .filter(|(i, _)| *i == owner)
            .filter_map(|(_, id)| {
                let rect = rune::memo::window_layout(&format!("live-entry-{owner}-{id}"))?;
                let viewport = viewport?;
                let inset = rect[1] - viewport[1];
                (inset < viewport[3] && inset + rect[3] > 0.0).then_some((
                    inset.abs(),
                    Bookmark {
                        owner,
                        entry: id.clone(),
                        inset,
                        settled: false,
                    },
                ))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, mark)| mark)
    }

    pub(crate) fn park_reading(&mut self, mark: Bookmark) {
        let owner = mark.owner;
        if let Some(place) = self.reading.get_mut(owner) {
            place.bookmark = Some(mark);
        }
    }

    pub(crate) fn clear_visible_entries(&mut self) {
        self.visible_entries.clear();
    }

    pub(crate) fn restore_reading(&self, index: usize, tx: &mut Tx) {
        if let Some(mark) = &self.reading[index].bookmark
            && self.app.sessions[index]
                .conversation
                .entries
                .iter()
                .find(|e| e.id == mark.entry)
                .is_some_and(caelum_core::application::presentation::entry_visible)
        {
            let row = format!("live-entry-{index}-{}", mark.entry);
            tx.scroll_center_in_instant(format!("live-transcript-{index}"), &row);
            tx.scroll_key_to(row, mark.inset);
        } else {
            let position = self.reading_position(index);
            let key = format!("live-transcript-{index}");
            if position.offset[1] >= position.max[1] - 1.0 {
                tx.scroll_to_bottom_instant(key);
            } else {
                tx.scroll_to_y(key, position.offset[1]);
            }
        }
    }

    pub(super) fn enter_transcript(&mut self, index: usize, tx: &mut Tx) {
        self.restore_reading(index, tx);
        if let Some(mark) = &mut self.reading[index].bookmark {
            mark.settled = true;
        }
    }

    pub(super) fn settle_reading(&mut self, tx: &mut Tx) -> bool {
        if self.panels.screen != crate::panels::Screen::Conversation {
            return false;
        }
        let Some(index) = self.app.selected else {
            return false;
        };
        let Some(mark) = self.reading.get(index).and_then(|p| p.bookmark.as_ref()) else {
            return false;
        };
        if !mark.settled {
            return false;
        }
        if !self.visible_entries.contains(&(index, mark.entry.clone())) {
            return false;
        }
        if let Some(mark) = self.reading[index].bookmark.take()
            && self.app.sessions[index]
                .conversation
                .entries
                .iter()
                .find(|e| e.id == mark.entry)
                .is_some_and(caelum_core::application::presentation::entry_visible)
        {
            let row = format!("live-entry-{index}-{}", mark.entry);
            tx.scroll_key_to(row, mark.inset);
            return true;
        }
        false
    }
}
