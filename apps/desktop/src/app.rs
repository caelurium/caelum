use crate::{Workspace, evaluation, live, on_key, state, view};
use rune::AppSpec;

pub(super) fn spec() -> AppSpec<Workspace> {
    AppSpec {
        min_size: Some((800.0, 620.0)),
        // Match Rune's macOS fallback order explicitly. Its primary-font probe
        // skips CSS generic families, so `system-ui` alone shapes SF text once
        // without its tracking correction, then shapes the whole run again.
        families: (
            if cfg!(target_os = "macos") {
                "System Font, '.SF NS', system-ui"
            } else {
                "system-ui"
            },
            "ui-monospace",
        ),
        poll: Some(state::poll),
        setup: Some(live::setup),
        on_key: Some(on_key),
        scenes: evaluation::SCENES,
        ..AppSpec::new("Caelum", (1280.0, 1000.0), Workspace::new, view)
    }
}
