use super::{Destination, Kind, Stage};
use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) fn prepare(
    destination: &Destination,
    stage: &Stage,
    version: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    match destination.kind {
        Kind::Mac => mac(stage, version, cancelled),
        Kind::AppImage => image(stage, version, cancelled),
    }
}

fn checked(command: &mut Command, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    run(command, cancelled, Stdio::null())
}

fn output(
    command: &mut Command,
    stage: &Stage,
    cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    let path = stage.root.join("probe");
    let file = File::create(&path).map_err(|e| e.to_string())?;
    run(command, cancelled, Stdio::from(file))?;
    let mut bytes = Vec::new();
    File::open(&path)
        .and_then(|f| f.take(4097).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    fs::remove_file(path).map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("Unexpected update version output".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn run(command: &mut Command, cancelled: &dyn Fn() -> bool, stdout: Stdio) -> Result<(), String> {
    if cancelled() {
        return Err("Update cancelled".into());
    }
    let name = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{name}: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!("Update preparation failed ({name}: {status})."))
            }
            Ok(None) if Instant::now() < deadline && !cancelled() => {
                thread::sleep(Duration::from_millis(25))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Update preparation could not finish ({name}: {result:?})."
                ));
            }
        }
    }
}

fn mac(stage: &Stage, version: &str, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let mount = stage.root.join("mount");
    fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let attach = checked(
        Command::new("/usr/bin/hdiutil")
            .arg("attach")
            .arg(stage.root.join("download"))
            .args(["-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
            .arg(&mount),
        cancelled,
    );
    if let Err(error) = attach {
        let _ = checked(
            Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount),
            &|| false,
        );
        return Err(error);
    }
    let result = (|| {
        let app = mount.join("Caelum.app");
        let plist = app.join("Contents/Info.plist");
        // Compare individual plist values without trusting shell interpolation.
        for (key, expected) in [
            ("CFBundleIdentifier", "dev.caelum.desktop"),
            ("CFBundleExecutable", "caelum-desktop"),
            ("CFBundleShortVersionString", version),
        ] {
            let value = output(
                Command::new("/usr/bin/plutil")
                    .args(["-extract", key, "raw", "-o", "-"])
                    .arg(&plist),
                stage,
                cancelled,
            )?;
            if value.trim() != expected {
                return Err(format!(
                    "The downloaded application has an unexpected {key}."
                ));
            }
        }
        checked(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&app),
            cancelled,
        )?;
        checked(
            Command::new("/usr/bin/lipo")
                .arg(app.join("Contents/MacOS/caelum-desktop"))
                .args([
                    "-verify_arch",
                    if std::env::consts::ARCH == "aarch64" {
                        "arm64"
                    } else {
                        "x86_64"
                    },
                ]),
            cancelled,
        )?;
        checked(
            Command::new("/usr/bin/ditto")
                .arg(&app)
                .arg(stage.root.join("payload")),
            cancelled,
        )?;
        checked(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(stage.root.join("payload")),
            cancelled,
        )?;
        sync_tree(&stage.root.join("payload"), cancelled)
    })();
    let detached = checked(
        Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount),
        &|| false,
    );
    if detached.is_ok() {
        let _ = fs::remove_dir(&mount);
    }
    result.and(detached)
}

fn image(stage: &Stage, version: &str, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let download = stage.root.join("download");
    let mut header = [0; 20];
    File::open(&download)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| e.to_string())?;
    let machine = u16::from_le_bytes([header[18], header[19]]);
    let expected = if std::env::consts::ARCH == "aarch64" {
        183
    } else {
        62
    };
    if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || &header[8..11] != b"AI\x02"
        || machine != expected
    {
        return Err("The download is not a type-2 AppImage for this architecture.".into());
    }
    fs::set_permissions(&download, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    fs::rename(&download, stage.root.join("payload")).map_err(|e| e.to_string())?;
    let reported = output(
        Command::new(stage.root.join("payload"))
            .args(["--appimage-extract-and-run", "--version"])
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .env_remove("ARGV0"),
        stage,
        cancelled,
    )?;
    if reported.trim() != format!("Caelum {version}") {
        return Err("The downloaded AppImage version does not match the release.".into());
    }
    File::open(stage.root.join("payload"))
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

pub(super) fn unmount(stage: &Stage) -> Result<(), String> {
    let mount = stage.root.join("mount");
    let metadata = match fs::symlink_metadata(&mount) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        return fs::remove_file(mount).map_err(|e| e.to_string());
    }
    if metadata.dev() != fs::metadata(&stage.root).map_err(|e| e.to_string())?.dev() {
        checked(
            Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount),
            &|| false,
        )?;
    }
    fs::remove_dir(mount).map_err(|e| format!("Cannot clear the update mount: {e}"))
}

fn sync_tree(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        return Err("Update cancelled".into());
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            sync_tree(&entry.map_err(|e| e.to_string())?.path(), cancelled)?;
        }
    }
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
