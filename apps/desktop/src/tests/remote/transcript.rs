use super::*;
use crate::live::{live, live_mut};
use caelum_core::remote::{
    Line, Page, RunStatus, Speaker, ToolCall, ToolLoad, ToolOutput, ToolSource, ToolState, TurnEnd,
};

const VM: &str = "checkout-example";

fn complete(rt: &mut Runtime<Workspace>, r: u64, result: Result<Outcome, String>) {
    rt.update(|s, _| live_mut(s).apply(Event::Completed(r, result.map(Reply::Remote))));
    advance(rt, 0.15);
}

fn tool_read(rt: &mut Runtime<Workspace>) -> u64 {
    let ops = live_mut(&mut rt.state).app.commands();
    assert!(
        !ops.iter()
            .any(|(_, c)| matches!(c, Command::Remote(Operation::Prompt(..))))
    );
    let (req, Command::Remote(Operation::InspectTool(machine, source))) = ops
        .iter()
        .find(|(_, c)| matches!(c, Command::Remote(Operation::InspectTool(..))))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(machine.id, "vm-example");
    assert_eq!(source.conversation, "thread-example");
    *req
}

fn result(text: &str) -> Outcome {
    Outcome::ToolOutput(ToolOutput {
        command: "cargo test".into(),
        output: text.into(),
        state: ToolState::Complete,
        clipped: false,
    })
}

fn has(rt: &Runtime<Workspace>, text: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| !e.exiting && e.text.as_deref().is_some_and(|s| s.contains(text)))
}

#[test]
fn expanded_tool_results_stay_with_their_remote_thread_and_do_not_enter_workspace_history() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "remote-trace"));
    advance(&mut rt, 0.3);
    let tool = "vm-tool-checkout-example-test-call";
    assert!(has(&rt, "1 passed; 0 failed"));
    assert!(
        !serde_json::to_string(&live(&rt.state).app.snapshot())
            .unwrap()
            .contains("cargo test health_check")
    );
    click(&mut rt, tool);
    assert!(!has(&rt, "1 passed; 0 failed"));
    click(&mut rt, tool);
    let old = tool_read(&mut rt);
    click(&mut rt, "vm-threads");
    click(&mut rt, "new-vm-session");
    complete(&mut rt, old, Ok(result("obsolete read")));
    assert!(!has(&rt, "obsolete read"));
    click(&mut rt, "vm-threads");
    click(&mut rt, "vm-thread-checkout-example");
    click(&mut rt, tool);
    let req = tool_read(&mut rt);
    complete(&mut rt, req, Err("Event unavailable".into()));
    assert!(has(&rt, "Event unavailable"));
    click(&mut rt, tool);
    click(&mut rt, tool);
    let req = tool_read(&mut rt);
    complete(&mut rt, req, Ok(result("remote tool output")));
    assert!(has(&rt, "remote tool output"));
}

fn page(rt: &mut Runtime<Workspace>, lines: Vec<Line>, status: RunStatus) {
    let app = &mut live_mut(&mut rt.state).app;
    app.follow_remote(VM);
    let (req, _) = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Remote(Operation::Follow(..))))
        .unwrap();
    complete(
        rt,
        req,
        Ok(Outcome::Page(Page {
            lines,
            cursor: Some("more".into()),
            more: false,
            model: None,
            status,
        })),
    );
}

#[test]
fn quiet_thinking_and_tool_phases_use_real_activity_and_refresh_an_open_result() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| {
        crate::remote::scene(s, "remote-working");
        live_mut(s)
            .app
            .remote
            .saved
            .checkouts
            .get_mut(VM)
            .unwrap()
            .thread
            .lines
            .truncate(1);
    });
    advance(&mut rt, 0.2);
    assert!(rt.find("remote-progress-checkout-example").is_some());
    assert!(!has(&rt, "Thinking"));
    let summary = |partial| Line {
        key: Some("thought".into()),
        partial,
        ..Line::new(
            Speaker::ReasoningSummary,
            "Checking the tests.\n\nA second paragraph.",
        )
    };
    page(&mut rt, vec![summary(true)], RunStatus::Running);
    assert_eq!(
        element(&rt, "remote-turn-status").label.as_deref(),
        Some("Thinking…")
    );
    assert!(rt.find("remote-progress-checkout-example").is_none());
    click(&mut rt, "vm-summary-checkout-example-thought");
    assert!(has(&rt, "A second paragraph."));
    click(&mut rt, "vm-summary-checkout-example-thought");
    let call = |state| Line {
        key: Some("call".into()),
        tool_detail: Some("cargo".into()),
        tool_source: Some(Box::new(ToolSource {
            conversation: "thread-example".into(),
            prompt: "prompt-example".into(),
            event: "event".into(),
            before: None,
            call: ToolCall::Id("call".into()),
        })),
        ..Line::new(Speaker::Tool(state), "Bash")
    };
    page(
        &mut rt,
        vec![summary(false), call(ToolState::Running)],
        RunStatus::Running,
    );
    click(&mut rt, "vm-tool-checkout-example-call");
    let first = tool_read(&mut rt);
    page(
        &mut rt,
        vec![call(ToolState::Complete)],
        RunStatus::Ended(TurnEnd::Finished),
    );
    let second = tool_read(&mut rt);
    complete(&mut rt, first, Ok(result("old excerpt")));
    assert!(
        matches!(live(&rt.state).app.remote.tool.as_ref().unwrap().load,ToolLoad::Loading(n) if n==second)
    );
    complete(&mut rt, second, Ok(result("completed test output")));
    assert!(has(&rt, "completed test output"));
    assert!(rt.find("remote-progress-checkout-example").is_none());
    assert_eq!(
        element(&rt, "remote-turn-status").label.as_deref(),
        Some("Ready")
    );
}
