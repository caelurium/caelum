use super::*;
use crate::live::{live, live_mut};
use caelum_core::application::{Command, Event, Reply};
use caelum_protocol::{ApprovalDecision, ApprovalRule};

fn settle(rt: &mut Runtime<Workspace>) {
    rt.update(|s, _| {
        let app = &mut live_mut(s).app;
        let (id, _) = app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Approve(_, ApprovalDecision::AllowOnce)))
            .unwrap_or_else(|| panic!("missing decision for {:?}", app.approvals));
        app.apply(Event::Approvals(vec![]));
        app.apply(Event::Completed(id, Ok(Reply::Done)));
    });
    advance(rt, 0.2);
}

#[test]
fn approval_controls_show_matches_in_the_transcript_and_revoke_from_the_heading() {
    let mut rt = design_runtime("approval", (800., 620.));
    let mut p = live(&rt.state).app.approvals[0].clone();
    click(&mut rt, "allow-similar-fixture");
    settle(&mut rt);
    assert_eq!(
        element(&rt, "session-approval-rules").label.as_deref(),
        Some("1 auto-allow rule")
    );
    p.attempt = "automatic".into();
    rt.update(|s, _| live_mut(s).app.apply(Event::Approvals(vec![p.clone()])));
    settle(&mut rt);
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Auto-approved · Session rule"))
    );
    p.repeatable = Some(ApprovalRule::ExactCommand("git status".into()));
    p.attempt = "inspect".into();
    rt.update(|s, _| live_mut(s).app.apply(Event::Approvals(vec![p.clone()])));
    advance(&mut rt, 0.2);
    reveal(&mut rt, "allow-similar-fixture");
    click(&mut rt, "allow-similar-fixture");
    settle(&mut rt);
    click(&mut rt, "session-approval-rules");
    assert!(rt.elements().iter().any(|e| e.text.as_deref()
        == Some("Run `git status` in the checkout root")
        && e.visible[3] > 0.));
    click(&mut rt, "revoke-rule-0");
    key(&mut rt, Key::Escape, Mods::default());
    assert_eq!(
        element(&rt, "session-approval-rules").label.as_deref(),
        Some("1 auto-allow rule")
    );
    click(&mut rt, "session-approval-rules");
    click(&mut rt, "clear-approval-rules");
    assert!(rt.find("session-approval-rules").is_none());
    assert!(rt.find("approval-rules-popover").is_none());
    p.repeatable = None;
    rt.update(|s, _| live_mut(s).app.apply(Event::Approvals(vec![p])));
    advance(&mut rt, 0.2);
    assert!(!element(&rt, "allow-similar-fixture").focusable);
    assert!(element(&rt, "allow-once-fixture").focusable);
}
