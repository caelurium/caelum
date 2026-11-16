use crate::{Workspace, live::live_mut};
use caelum_core::{
    application::{Event, Task},
    updates::{Phase, Progress},
};

pub(crate) fn scene(s: &mut Workspace, name: &str) {
    crate::evaluation::scene(s, "about-update");
    let app = &mut live_mut(s).app;
    let release = app.services.update.clone().unwrap();
    if name == "updated" {
        app.services.update = None;
        app.services.updater.phase = Phase::Updated(release.latest_version);
        return;
    }
    let job = app.services.updater.download(&release).unwrap();
    let id = job.id;
    app.task(Task::Update(job));
    if name == "failed" {
        for (request, command) in app.commands() {
            if matches!(
                command,
                caelum_core::application::Command::Task(Task::Update(_))
            ) {
                app.apply(Event::Completed(
                    request,
                    Err("Cannot save the update: disk full. Free some space and try again.".into()),
                ));
            }
        }
    } else {
        app.services.updater.progress(
            id,
            match name {
                "verifying" => Progress::Verifying,
                "preparing" => Progress::Preparing,
                _ => Progress::Downloading {
                    bytes: 18 * 1024 * 1024,
                    total: Some(42 * 1024 * 1024),
                },
            },
        );
        if name == "cancelling" {
            app.services.updater.cancel();
        }
    }
}
