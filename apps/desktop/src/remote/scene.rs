use super::{checkout, conversation};
use crate::{Workspace, live::live_mut, panels::Screen, settings::Page};
use caelum_core::remote::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn scene(s: &mut Workspace, name: &str) {
    let l = live_mut(s);
    let source = Source {
        project: l.app.projects[0].path.clone(),
        account: "example-account".into(),
        repository: "example/project".into(),
        base: "main".into(),
        environment: Some("project".into()),
    };
    let machine = Machine {
        key: MachineId {
            host: Host::Boat,
            account: source.account.clone(),
            id: "vm-example".into(),
        },
        name: "dev-01".into(),
        state: "idle".into(),
        auto_stop_unix: Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 42 * 60,
        ),
    };
    l.app.remote.inventory = Some(Inventory {
        account: source.account.clone(),
        machines: vec![machine.clone()],
    });
    if name == "remote-settings" {
        l.panels.settings_page = Page::Remote;
        l.panels.overlay = Some(Screen::Settings);
        return;
    }
    if name == "remote-picker" {
        l.app.remote.picker = Some(Picker::Ready(source));
        l.panels.overlay = Some(Screen::RemotePicker);
        return;
    }
    let id = "checkout-example".to_owned();
    l.app.leave_conversation();
    l.app.active_project = None;
    l.app.remote.saved.selected = Some(id.clone());
    l.app.remote.saved.checkouts.insert(id.clone(),Checkout{source,
        phase:Phase::Ready{machine:machine.clone(),branch:"feat/remote-checkout".into()},pinned_stats:true,
        thread:Thread{conversation:Some("thread-example".into()),lines:vec![
            Line{speaker:Speaker::You,text:"Add a health check and show me the changes.".into(),partial:false},
            Line{speaker:Speaker::OpenCode,text:"I added a health check to src/server.rs. The response is ready to review on this branch.".into(),partial:false},
        ],..Default::default()}});
    l.app.remote.monitor = Monitor::Showing {
        owner: id.clone(),
        sample: Sample {
            machine,
            vcpu: Some(4),
            memory_gb: Some(8),
            activity: Activity::Live(Some(Process {
                name: "cargo".into(),
                cpu_percent: 82.,
                memory_mb: Some(610.),
            })),
        },
        next: Instant::now() + Duration::from_secs(60),
    };
    l.remote.restored = true;
    conversation::sync_editor(l);
    checkout::enter(s);
    let l = live_mut(s);
    l.remote.details = name == "remote-machine";
    if name == "remote-changes" {
        l.app.remote.changes.insert(id,Changes::Ready(Review{revision:Revision{branch:"feat/remote-checkout".into(),head:"a".repeat(40),index:"b".repeat(40),tree:"c".repeat(40)},
            status:"## feat/remote-checkout\n M src/server.rs".into(),patch:"diff --git a/src/server.rs b/src/server.rs\n--- a/src/server.rs\n+++ b/src/server.rs\n@@ -1,3 +1,4 @@\n fn routes() {\n+    add_health_check();\n     start_server();\n }\n".into()}));
        super::changes::sync(l);
        s.inspector.set_open(true);
    }
}
