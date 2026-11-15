use crate::{Workspace, evaluation, home, latency, live, notifications, spec};

pub(crate) fn run() {
    let mut fixtures = false;
    let mut evaluation = false;
    let mut design = false;
    let mut project = None;
    let mut latency_workload = None;
    let mut args = Vec::new();
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            #[cfg(target_os = "linux")]
            "--install" => {
                super::install::run().unwrap_or_else(|error| fail(&error));
                return;
            }
            "--version" | "-V" => {
                println!("Caelum {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--fixtures" => fixtures = true,
            "--evaluation-workload" => evaluation = true,
            "--design-fixture" => design = true,
            "--latency-workload" => {
                latency_workload = Some(match input.next().as_deref() {
                    Some("small") => false,
                    Some("streaming") => true,
                    _ => fail("--latency-workload requires small or streaming"),
                });
            }
            "--project" | "--open-url" => {
                let Some(value) = input.next() else {
                    fail("--project/--open-url requires a directory or project link");
                };
                project = Some(project_argument(&value).unwrap_or_else(|error| fail(&error)));
            }
            _ if args.is_empty() && project.is_none() && !arg.starts_with('-') => {
                project = Some(project_argument(&arg).unwrap_or_else(|error| fail(&error)));
            }
            _ => args.push(arg),
        }
    }
    let _capture = std::env::var_os("CAELUM_LATENCY_TRACE")
        .filter(|path| !path.is_empty())
        .and_then(|path| {
            match caelum_core::diagnostics::Capture::start(std::path::Path::new(&path)) {
                Ok(capture) => Some(capture),
                Err(error) => {
                    eprintln!("Cannot start latency trace: {error}");
                    None
                }
            }
        });
    #[cfg(target_os = "linux")]
    rune::set_app_id("dev.caelum.desktop");
    let app = super::configure(
        rune::App::new(spec())
            .compositor(home::art::Compositor::default())
            .font_smoothing(rune::FontSmoothing::Off)
            .on_frame_drawn(live::frame_drawn),
    );
    let app = if caelum_core::diagnostics::enabled() {
        app.on_frame_encoded(latency::encoded)
    } else {
        app
    };
    let handle = app.handle();
    let production = !fixtures && !evaluation && !design && latency_workload.is_none();
    #[cfg(target_os = "linux")]
    let _instance = if production {
        match super::linux::start(handle.clone(), project.clone()) {
            Ok(super::linux::Launch::Primary(instance)) => Some(instance),
            Ok(super::linux::Launch::Forwarded) => return,
            Err(error) => fail(&error),
        }
    } else {
        None
    };
    let notifications = notifications::State::connect(handle.clone(), production);
    app.init(move || {
        let mut state = if let Some(streaming) = latency_workload {
            latency::workspace(streaming)
        } else if design {
            evaluation::design()
        } else if evaluation {
            evaluation::workspace()
        } else if fixtures {
            Workspace::new()
        } else {
            home::workspace(project)
        };
        if let Some(live) = &mut state.live {
            live.notifications = notifications;
        }
        state.handle = Some(handle);
        state
    })
    .args(args)
    .chrome(super::chrome())
    .run();
}

fn project_argument(value: &str) -> Result<String, String> {
    let path = if value.starts_with("caelum:") {
        caelum_core::desktop::project_from_url(value)?
    } else {
        std::path::absolute(value).map_err(|error| error.to_string())?
    };
    path.to_str()
        .filter(|value| !value.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(|| "Expected a UTF-8 project path without NUL characters".into())
}

fn fail(message: &str) -> ! {
    eprintln!("Caelum: {message}");
    std::process::exit(2);
}

#[cfg(test)]
mod tests;
