use super::{Installed, Kind};
use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

impl Installed {
    fn launch(&self, project: Option<&str>) -> Result<Child, String> {
        let destination = &self.prepared.destination;
        let executable = match destination.kind {
            Kind::Mac => destination.path.join("Contents/MacOS/caelum-desktop"),
            Kind::AppImage => destination.path.clone(),
        };
        let mut command = Command::new(executable);
        if destination.kind == Kind::AppImage {
            command.arg("--appimage-extract-and-run");
        }
        if let Some(project) = project {
            command.args(["--project", project]);
        }
        command
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .env_remove("ARGV0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())
    }

    /// After UI/runtime/store shutdown, supervise the new process until it has
    /// drawn and restored the workspace. An early exit restores the previous app.
    pub fn relaunch(&self, project: Option<&str>) -> Result<Child, String> {
        let stage = &self.prepared.stage;
        stage.lock.unlock().map_err(|e| e.to_string())?;
        let started = self.launch(project).and_then(|mut child| {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                if !stage.root.join("receipt.json").exists() {
                    return Ok(child);
                }
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    return Err(format!(
                        "The new application exited before startup ({status})"
                    ));
                }
                if Instant::now() >= deadline {
                    return Ok(child);
                }

                thread::sleep(Duration::from_millis(100));
            }
        });
        match started {
            Ok(child) => Ok(child),
            Err(error) => {
                stage
                    .lock
                    .try_lock()
                    .map_err(|_| "Could not lock the update for recovery".to_owned())?;
                self.rollback()?;
                stage.lock.unlock().map_err(|e| e.to_string())?;
                self.launch(project).map_err(|e| {
                    format!("The previous version was restored. Launch Caelum manually: {e}")
                })?;
                Err(format!("Could not restart the update ({error}); restored and launched the previous version."))
            }
        }
    }
}
