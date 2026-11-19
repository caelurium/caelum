use crate::live::Live;
use caelum_core::{
    application::{Command, Event, Reply},
    remote::{
        Checkout, Line, Operation, Outcome, Speaker, ToolCall, ToolOutput, ToolSource, ToolState,
    },
};

pub(super) fn setup(c: &mut Checkout) {
    c.pinned_stats = false;
    c.thread.provider = caelum_core::application::Provider::Codex;
    c.thread.lines = vec![
        Line::new(Speaker::You, "Run the health check tests."),
        Line {
            key: Some("summary".into()),
            ..Line::new(
                Speaker::ReasoningSummary,
                "Checking the route and its readiness test.\n\nThe test should exercise the same handler used by the health check.",
            )
        },
        Line {
            key: Some("test-call".into()),
            tool_detail: Some("cargo".into()),
            tool_source: Some(Box::new(ToolSource {
                conversation: c.thread.conversation.clone().unwrap(),
                prompt: "prompt-example".into(),
                event: "response-example".into(),
                before: None,
                call: ToolCall::Id("call-example".into()),
            })),
            ..Line::new(Speaker::Tool(ToolState::Complete), "Bash")
        },
    ];
}

pub(super) fn open(l: &mut Live, id: &str) {
    l.app.toggle_vm_tool(id, id, "test-call");
    let (req, _) = l
        .app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Remote(Operation::InspectTool(..))))
        .unwrap();
    l.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::ToolOutput(ToolOutput {
            command: "cargo test health_check".into(),
            output: "test routes::health_check ... ok\n\n1 passed; 0 failed".into(),
            state: ToolState::Complete,
            clipped: false,
        }))),
    ));
}
