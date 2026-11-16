use super::*;
use caelum_core::{
    application::{Command, Event, Task},
    updates::{Job, Phase, Progress},
};

fn request(rt: &mut Runtime<Workspace>) -> (u64, Job) {
    rt.state
        .live
        .as_mut()
        .unwrap()
        .app
        .commands()
        .into_iter()
        .find_map(|(id, command)| {
            if let Command::Task(Task::Update(job)) = command {
                Some((id, job))
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn download_cancellation_retry_and_navigation_keep_the_correct_attempt() {
    let mut rt = design_runtime("about-update", (800.0, 620.0));
    let draft = rt.state.live.as_ref().unwrap().drafts[0].text.clone();
    click(&mut rt, "download-update");
    let (first, job) = request(&mut rt);
    assert!(rt.find("install-update").is_none());
    click(&mut rt, "settings-providers");
    rt.update(|s, _| {
        let live = s.live.as_mut().unwrap();
        assert!(!live.apply_events(vec![Event::UpdateProgress(job.id, Progress::Verifying)]));
    });
    click(&mut rt, "settings-about");
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Verifying update"))
    );
    click(&mut rt, "cancel-update");
    assert!(matches!(
        rt.state.live.as_ref().unwrap().app.services.updater.phase,
        Phase::Cancelling
    ));
    rt.update(|s, _| {
        s.live
            .as_mut()
            .unwrap()
            .app
            .apply(Event::Completed(first, Err("cancelled".into())))
    });
    advance(&mut rt, 0.3);
    click(&mut rt, "download-update");
    let (second, retry) = request(&mut rt);
    rt.update(|s, _| {
        let live = s.live.as_mut().unwrap();
        assert!(!live.apply_events(vec![Event::UpdateProgress(job.id, Progress::Preparing)]));
        live.app.apply(Event::Completed(
            second,
            Err("Connection interrupted".into()),
        ));
    });
    advance(&mut rt, 0.3);
    assert!(rt.find("manual-update").is_some());
    click(&mut rt, "retry-update");
    let (_, current) = request(&mut rt);
    assert!(current.id > retry.id);
    assert_eq!(rt.state.live.as_ref().unwrap().drafts[0].text, draft);
}

#[test]
fn updater_status_and_controls_fit_the_compact_dialog() {
    for scene in [
        "update-downloading",
        "update-verifying",
        "update-preparing",
        "update-cancelling",
        "update-failed",
        "update-success",
    ] {
        let mut rt = design_runtime("about-update", (800.0, 620.0));
        let (_, initialize) = crate::evaluation::SCENES
            .iter()
            .find(|(name, _)| *name == scene)
            .unwrap();
        rt.update(|s, _| initialize(s));
        advance(&mut rt, 0.4);
        assert!(
            rt.elements().iter().any(|e| e.role == "status"
                && e.path.contains("about-updates")
                && e.label.is_some()),
            "{scene}"
        );
        for control in rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.focusable && e.path.contains("about-updates"))
        {
            assert!(
                control
                    .rect
                    .iter()
                    .zip(control.visible)
                    .all(|(a, b)| (*a - b).abs() < 1.0),
                "{scene}: {control:?}"
            );
        }
    }
}
