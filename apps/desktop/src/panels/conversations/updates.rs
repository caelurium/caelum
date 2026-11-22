use super::Panels;
use std::{collections::HashSet, time::Duration};

#[derive(Default)]
pub(super) struct Frame {
    turns: HashSet<usize>,
    next_age: Option<u64>,
}

impl Frame {
    pub(super) fn reset(&mut self) {
        self.turns.clear();
        self.next_age = None;
    }

    pub(super) fn age(&mut self, start: u64, now: u64) -> Option<String> {
        if start == 0 {
            return None;
        }
        let elapsed = now.saturating_sub(start);
        let (unit, suffix) = if elapsed < 3_600_000 {
            (60_000, "m")
        } else {
            (3_600_000, "h")
        };
        let count = elapsed / unit;
        let next = start.saturating_add((count + 1).saturating_mul(unit));
        self.next_age = Some(self.next_age.map_or(next, |at| at.min(next)));
        (count > 0).then(|| format!("{count}{suffix}"))
    }
}

impl Panels {
    pub(super) fn watch_turn(&self, index: usize) {
        if let Some(state) = self.conversations() {
            state.frame.borrow_mut().turns.insert(index);
        }
    }

    pub(crate) fn overview_shows_turn(&self, index: usize) -> bool {
        self.conversations()
            .is_some_and(|s| s.frame.borrow().turns.contains(&index))
    }

    pub(crate) fn tick_overview(&mut self, now: u64, tx: &mut rune::Tx) -> bool {
        let Some(state) = self.conversations_mut() else {
            return false;
        };
        let frame = state.frame.get_mut();
        let next = frame.next_age;
        if next.is_some_and(|at| now >= at) {
            frame.next_age = None;
            state.wake_at = None;
            return true;
        }
        if state.wake_at != next {
            state.wake_at = next;
            if let Some(at) = next {
                tx.wake_after(Duration::from_millis(at - now));
            }
        }
        false
    }
}
