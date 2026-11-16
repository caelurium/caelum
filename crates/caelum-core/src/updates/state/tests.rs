use super::*;
use crate::updates::{
    installation::{tests::prepared, Kind},
    tests::available,
};

#[test]
fn cancellation_and_retries_reject_old_progress_and_completions() {
    let mut state = State::default();
    let first = state.download(&available()).unwrap();
    assert!(state.download(&available()).is_none());
    state.cancel();
    assert!(first.cancel.is_cancelled());
    assert!(state.download(&available()).is_none());
    state.complete(&first, Err("cancelled".into()));
    let second = state.download(&available()).unwrap();
    assert!(!state.progress(first.id, Progress::Verifying));
    state.complete(&first, Err("old download failed".into()));
    assert!(matches!(state.phase, Phase::Downloading { .. }));
    assert!(state.progress(second.id, Progress::Verifying));
    assert!(!state.progress(second.id, Progress::Verifying));
    state.complete(&second, Err("connection lost".into()));
    assert!(matches!(state.phase, Phase::Failed { .. }));
    let retry = state.retry().unwrap();
    assert!(retry.id > second.id);
}

#[test]
fn a_download_that_finishes_after_cancellation_is_discarded_on_a_worker() {
    let (_directory, prepared) = prepared(Kind::Mac);
    let mut state = State::default();
    let download = state.download(&available()).unwrap();
    state.cancel();
    let discard = state
        .complete(&download, Ok(Reply::Prepared(prepared.clone())))
        .unwrap();
    assert!(matches!(discard.action, Action::Discard(_)));
    assert!(
        prepared.stage.root.join("payload").exists(),
        "no filesystem cleanup on the UI thread"
    );
    assert!(matches!(state.phase, Phase::Cancelling));
    state.complete(&discard, Ok(Reply::Discarded));
    assert!(matches!(state.phase, Phase::Idle));
}

#[test]
fn only_ready_payloads_can_install_and_restart_keeps_the_installed_attempt() {
    let (_directory, prepared) = prepared(Kind::AppImage);
    let mut state = State::default();
    assert!(state.install().is_none());
    let download = state.download(&available()).unwrap();
    state.complete(&download, Ok(Reply::Prepared(prepared.clone())));
    let install = state.install().unwrap();
    assert!(!state.can_check());
    assert!(state.cancel().is_none());
    state.complete(&install, Err("write failed".into()));
    let retry = state.retry().unwrap();
    state.complete(
        &retry,
        Ok(Reply::Installed(Arc::new(Installed { prepared }))),
    );
    state.restart();
    state.restart_failed("draft save failed".into());
    assert!(matches!(
        state.phase,
        Phase::Failed {
            retry: Retry::Restart(_),
            ..
        }
    ));
    state.retry();
    assert!(matches!(state.phase, Phase::Restarting(_)));
}
