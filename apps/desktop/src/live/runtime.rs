use super::Live;
use crate::Workspace;
use caelum_core::application::{Runtime, Store};
use rune::Waker;

pub fn setup(s: &mut Workspace, wake: Waker) {
    if let Some(live) = &mut s.live
        && !live.evaluation
        && !live.fixture
    {
        live.start(Store(Store::default_path()), wake);
    }
}

impl Live {
    pub(crate) fn start(&mut self, store: Store, wake: Waker) {
        match store.lock() {
            Ok(lock) => self.store_lock = Some(lock),
            Err(error) => {
                self.app.store_unavailable(error);
                return;
            }
        }
        self.wake = Some(wake.clone());
        self.media = crate::media::State::new(wake.clone());
        self.runtime = Some(Runtime::new(move || wake()));
        self.app.enable_store(store.0);
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.app.home.draft.text.clone_from(&self.home.editor.text);
        self.panels.remember(&mut self.app);
        for (i, editor) in self.drafts.iter().enumerate() {
            self.app.sessions[i].draft.clone_from(&editor.text);
        }
        drop(self.runtime.take());
        if let Err(error) = self.app.flush() {
            self.updates.abort_restart();
            eprintln!("{error}");
        }
    }
}
