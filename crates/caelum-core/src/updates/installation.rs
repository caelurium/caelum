//! Same-filesystem staging and atomic exchange keep an installed app launchable.
#[cfg(test)]
mod packaged;
mod platform;
mod restart;
#[cfg(test)]
pub(super) mod tests;

use caelum_protocol::UpdateCheck;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn read(path: &Path) -> Result<Self, String> {
        let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if m.file_type().is_symlink() {
            return Err(
                "The application path changed to a symlink. Check for updates again.".into(),
            );
        }
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Mac,
    AppImage,
}

#[derive(Debug)]
pub(super) struct Destination {
    pub path: PathBuf,
    pub kind: Kind,
    identity: Identity,
}

impl Destination {
    pub fn discover() -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let image = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        Self::locate(
            std::env::consts::OS,
            &executable,
            image.as_deref(),
            data.as_deref(),
        )
    }

    fn locate(
        platform: &str,
        executable: &Path,
        image: Option<&Path>,
        data: Option<&Path>,
    ) -> Result<Self, String> {
        let (path, kind) = match platform {
            "macos" => {
                let path = executable
                    .ancestors()
                    .find(|p| p.extension().is_some_and(|e| e == "app"))
                    .ok_or("Install Caelum.app in Applications to use in-app updates.")?;
                (path.to_path_buf(), Kind::Mac)
            }
            "linux" => {
                let image = image.ok_or("This installation is managed externally. Update it using your package manager or install the official AppImage.")?;
                let expected = data
                    .ok_or("Set HOME or XDG_DATA_HOME to locate the installed AppImage.")?
                    .join("caelum/Caelum.AppImage");
                if image.canonicalize().ok() != expected.canonicalize().ok() || !expected.exists() {
                    return Err("Install the downloaded AppImage with --install before using in-app updates.".into());
                }
                (image.to_path_buf(), Kind::AppImage)
            }
            _ => return Err("In-app installation is available on macOS and Linux.".into()),
        };
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if path.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some("Caskroom" | "Cellar" | ".nix-profile")
            )
        }) || path.starts_with("/nix/store")
        {
            return Err("This installation is managed by a package manager. Update Caelum using that package manager.".into());
        }
        if path.starts_with("/Volumes")
            || path
                .components()
                .any(|c| c.as_os_str() == "AppTranslocation")
        {
            return Err(
                "Move Caelum.app to Applications and launch it there before installing updates."
                    .into(),
            );
        }
        let identity = Identity::read(&path)?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.uid()
            != rustix::process::geteuid().as_raw()
        {
            return Err("This application is owned by another user. Ask its owner to update it, or install Caelum in a writable directory you own.".into());
        }
        Ok(Self {
            path,
            kind,
            identity,
        })
    }

    fn staging(&self) -> PathBuf {
        self.path
            .parent()
            .expect("installed application parent")
            .join(format!(
                ".{}.update",
                self.path.file_name().unwrap().to_string_lossy()
            ))
    }
}

#[derive(Debug)]
pub(super) struct Stage {
    pub root: PathBuf,
    lock: File,
}

impl Stage {
    /// An exchanged payload is the rollback copy, not an abandoned download.
    pub fn begin(&self, destination: &Destination) -> Result<(), String> {
        if let Some(receipt) = self.receipt()? {
            if receipt.target != destination.path || receipt.before != destination.identity {
                return Err(format!("A previous update is waiting for a successful restart. Restart Caelum before trying again. The recovery copy is in {}.", self.root.join("payload").display()));
            }
        }
        self.clear()
    }

    fn receipt(&self) -> Result<Option<Receipt>, String> {
        let file = match File::open(self.root.join("receipt.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(16 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map(Some).map_err(|e| format!("Cannot read the update recovery receipt: {e}. The previous version has been retained."))
    }

    pub fn open(destination: &Destination) -> Result<Self, String> {
        let root = destination.staging();
        match fs::DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => {},
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(e) => return Err(format!("Cannot write beside the installed app: {e}. Move Caelum to a writable installation directory.")),
        }
        let metadata = fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err("The update staging directory must be a private directory owned by the current user.".into());
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(root.join("lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|_| "Another Caelum instance is preparing an update. Finish or cancel that update first.".to_owned())?;
        Ok(Self { root, lock })
    }

    pub fn clear(&self) -> Result<(), String> {
        platform::unmount(self)?;
        for name in [
            "payload",
            "download",
            "probe",
            "receipt.json",
            "receipt.tmp",
        ] {
            let path = self.root.join(name);
            match fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => fs::remove_dir_all(path),
                Ok(_) => fs::remove_file(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
            .map_err(|e| format!("Cannot clear the previous update: {e}"))?;
        }
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}

#[derive(Debug)]
pub struct Prepared {
    pub release: UpdateCheck,
    pub(super) destination: Destination,
    pub(super) stage: Stage,
    payload_identity: Identity,
}

impl Prepared {
    pub(super) fn new(
        release: UpdateCheck,
        destination: Destination,
        stage: Stage,
    ) -> Result<Self, String> {
        let payload_identity = Identity::read(&stage.root.join("payload"))?;
        Ok(Self {
            release,
            destination,
            stage,
            payload_identity,
        })
    }
    pub(super) fn discard(&self) -> Result<(), String> {
        self.stage.clear()
    }
}

#[derive(Debug)]
pub struct Installed {
    pub(super) prepared: Arc<Prepared>,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    target: PathBuf,
    version: String,
    before: Identity,
    after: Identity,
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn exchange(a: &Path, b: &Path) -> Result<(), String> {
    rustix::fs::renameat_with(rustix::fs::CWD, a, rustix::fs::CWD, b, rustix::fs::RenameFlags::EXCHANGE)
        .map_err(|e| format!("Could not atomically replace the application: {e}. The existing installation has been retained."))
}

pub(super) fn install(prepared: Arc<Prepared>) -> Result<Installed, String> {
    let target = &prepared.destination;
    let payload = prepared.stage.root.join("payload");
    if Identity::read(&target.path)? != target.identity
        || Identity::read(&payload)? != prepared.payload_identity
    {
        return Err("The application changed while the update was downloading. Cancel this update and check again.".into());
    }
    let receipt = Receipt {
        target: target.path.clone(),
        version: prepared.release.latest_version.clone(),
        before: target.identity,
        after: prepared.payload_identity,
    };
    let bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
    let mut file =
        File::create(prepared.stage.root.join("receipt.tmp")).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(
        prepared.stage.root.join("receipt.tmp"),
        prepared.stage.root.join("receipt.json"),
    )
    .map_err(|e| e.to_string())?;
    sync_directory(&prepared.stage.root)?;
    exchange(&payload, &target.path)?;

    let _ = sync_directory(&prepared.stage.root);
    let _ = sync_directory(target.path.parent().unwrap());
    Ok(Installed { prepared })
}

impl Installed {
    pub fn version(&self) -> &str {
        &self.prepared.release.latest_version
    }

    fn rollback(&self) -> Result<(), String> {
        let p = &self.prepared;
        let payload = p.stage.root.join("payload");
        if Identity::read(&p.destination.path)? != p.payload_identity
            || Identity::read(&payload)? != p.destination.identity
        {
            return Err("The installation changed after updating; the previous version has been retained for recovery.".into());
        }
        exchange(&payload, &p.destination.path)?;
        sync_directory(p.destination.path.parent().unwrap())?;
        p.stage.clear()
    }
}

/// Acknowledgment runs only after the new application has drawn and loaded its state.
pub(super) fn finish() -> Result<Option<String>, String> {
    let Ok(destination) = Destination::discover() else {
        return Ok(None);
    };
    finish_at(&destination, env!("CARGO_PKG_VERSION"))
}

fn finish_at(destination: &Destination, version: &str) -> Result<Option<String>, String> {
    if !destination.staging().join("receipt.json").is_file() {
        return Ok(None);
    }
    let stage = Stage::open(destination)?;
    let Some(receipt) = stage.receipt()? else {
        return Ok(None);
    };
    if receipt.target == destination.path
        && receipt.after == destination.identity
        && receipt.version == version
    {
        stage.clear()?;
        return Ok(Some(receipt.version));
    }
    Ok(None)
}

pub(super) fn prepare_payload(
    destination: &Destination,
    stage: &Stage,
    version: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    platform::prepare(destination, stage, version, cancelled)
}
