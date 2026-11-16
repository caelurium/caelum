use super::*;
use crate::updates::tests::{available, Directory};

pub(in crate::updates) fn prepared(kind: Kind) -> (Directory, Arc<Prepared>) {
    let directory = Directory::new();
    let target = directory.0.join(if kind == Kind::Mac {
        "Caelum.app"
    } else {
        "Caelum.AppImage"
    });
    if kind == Kind::Mac {
        fs::create_dir(&target).unwrap();
        fs::write(target.join("version"), "old").unwrap();
    } else {
        fs::write(&target, "old").unwrap();
    }
    let destination = Destination {
        identity: Identity::read(&target).unwrap(),
        path: target,
        kind,
    };
    let stage = Stage::open(&destination).unwrap();
    let payload = stage.root.join("payload");
    if kind == Kind::Mac {
        fs::create_dir(&payload).unwrap();
        fs::write(payload.join("version"), "new").unwrap();
    } else {
        fs::write(&payload, "new").unwrap();
    }
    (
        directory,
        Arc::new(Prepared::new(available(), destination, stage).unwrap()),
    )
}

fn content(p: &Prepared, path: &Path) -> String {
    fs::read_to_string(if p.destination.kind == Kind::Mac {
        path.join("version")
    } else {
        path.to_path_buf()
    })
    .unwrap()
}

#[test]
fn file_and_bundle_replacement_is_atomic_and_can_be_rolled_back() {
    for kind in [Kind::Mac, Kind::AppImage] {
        let (_directory, prepared) = prepared(kind);
        let old = Identity::read(&prepared.destination.path).unwrap();
        let installed = install(prepared.clone()).unwrap();
        assert_eq!(content(&prepared, &prepared.destination.path), "new");
        assert_eq!(
            content(&prepared, &prepared.stage.root.join("payload")),
            "old"
        );
        assert_eq!(
            Identity::read(&prepared.stage.root.join("payload")).unwrap(),
            old
        );
        assert!(
            install(prepared.clone()).is_err(),
            "installation must never exchange twice"
        );
        installed.rollback().unwrap();
        assert_eq!(content(&prepared, &prepared.destination.path), "old");
        assert!(!prepared.stage.root.join("payload").exists());
    }
}

#[test]
fn a_replaced_target_or_payload_is_not_overwritten() {
    for replace_target in [false, true] {
        let (_directory, prepared) = prepared(Kind::AppImage);
        let path = if replace_target {
            prepared.destination.path.clone()
        } else {
            prepared.stage.root.join("payload")
        };
        fs::rename(&path, path.with_extension("elsewhere")).unwrap();
        fs::write(&path, "external replacement").unwrap();
        assert!(install(prepared.clone()).unwrap_err().contains("changed"));
        assert_eq!(fs::read_to_string(path).unwrap(), "external replacement");
    }
}

#[test]
fn staging_is_private_exclusive_and_released_after_drop() {
    let (_directory, prepared) = prepared(Kind::AppImage);
    let path = prepared.destination.path.clone();
    assert!(Stage::open(&prepared.destination)
        .unwrap_err()
        .contains("Another"));
    drop(prepared);
    let destination = Destination {
        identity: Identity::read(&path).unwrap(),
        path,
        kind: Kind::AppImage,
    };
    let stage = Stage::open(&destination).unwrap();
    assert_eq!(fs::metadata(&stage.root).unwrap().mode() & 0o777, 0o700);
}

#[test]
fn symlink_staging_directory_cannot_redirect_update_writes() {
    let directory = Directory::new();
    let path = directory.0.join("Caelum.AppImage");
    fs::write(&path, "old").unwrap();
    let destination = Destination {
        identity: Identity::read(&path).unwrap(),
        path,
        kind: Kind::AppImage,
    };
    let other = directory.0.join("other");
    fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&other, destination.staging()).unwrap();
    assert!(Stage::open(&destination).is_err());
    assert_eq!(fs::read_dir(other).unwrap().count(), 0);
}

#[test]
fn previous_version_is_retained_until_matching_new_version_acknowledges_startup() {
    let (_directory, prepared) = prepared(Kind::AppImage);
    let installed = install(prepared.clone()).unwrap();
    let destination = Destination {
        path: prepared.destination.path.clone(),
        identity: prepared.payload_identity,
        kind: Kind::AppImage,
    };
    let stage = prepared.stage.root.clone();
    drop(installed);
    drop(prepared);
    assert_eq!(finish_at(&destination, "1.0.0").unwrap(), None);
    assert_eq!(fs::read_to_string(stage.join("payload")).unwrap(), "old");
    assert_eq!(
        finish_at(&destination, "1.2.0").unwrap(),
        Some("1.2.0".into())
    );
    assert!(!stage.join("payload").exists());
    assert!(!stage.join("receipt.json").exists());
    assert_eq!(fs::read_to_string(&destination.path).unwrap(), "new");
    assert_eq!(finish_at(&destination, "1.2.0").unwrap(), None);
}

#[test]
fn cancellation_discards_payload_without_changing_the_app() {
    let (_directory, prepared) = prepared(Kind::Mac);
    prepared.discard().unwrap();
    assert_eq!(content(&prepared, &prepared.destination.path), "old");
    assert!(!prepared.stage.root.join("payload").exists());
}

#[test]
fn another_download_never_deletes_an_unacknowledged_rollback_copy() {
    let (_directory, prepared) = prepared(Kind::AppImage);
    install(prepared.clone()).unwrap();
    let current = Destination {
        path: prepared.destination.path.clone(),
        identity: prepared.payload_identity,
        kind: Kind::AppImage,
    };
    assert!(prepared
        .stage
        .begin(&current)
        .unwrap_err()
        .contains("restart"));
    assert_eq!(
        fs::read_to_string(prepared.stage.root.join("payload")).unwrap(),
        "old"
    );
    assert_eq!(fs::read_to_string(current.path).unwrap(), "new");
}

#[test]
fn interrupted_before_exchange_can_be_retried_without_touching_the_app() {
    let (_directory, prepared) = prepared(Kind::AppImage);
    let receipt = Receipt {
        target: prepared.destination.path.clone(),
        version: "1.2.0".into(),
        before: prepared.destination.identity,
        after: prepared.payload_identity,
    };
    fs::write(
        prepared.stage.root.join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    prepared.stage.begin(&prepared.destination).unwrap();
    assert_eq!(
        fs::read_to_string(&prepared.destination.path).unwrap(),
        "old"
    );
    assert!(!prepared.stage.root.join("payload").exists());
}

#[test]
fn an_update_that_exits_before_startup_restores_and_launches_the_old_app() {
    let (_directory, prepared) = prepared(Kind::Mac);
    let old = prepared
        .destination
        .path
        .join("Contents/MacOS/caelum-desktop");
    let new = prepared
        .stage
        .root
        .join("payload/Contents/MacOS/caelum-desktop");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    fs::create_dir_all(new.parent().unwrap()).unwrap();
    let started = prepared.stage.root.join("old-started");
    crate::test_support::write_executable(
        &old,
        format!("#!/bin/sh\nprintf started > '{}'\n", started.display()),
    );
    crate::test_support::write_executable(&new, "#!/bin/sh\nexit 1\n");
    let installed = install(prepared.clone()).unwrap();
    assert!(installed
        .relaunch(None)
        .unwrap_err()
        .contains("restored and launched"));
    assert_eq!(content(&prepared, &prepared.destination.path), "old");
    assert!(!prepared.stage.root.join("receipt.json").exists());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !started.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "previous app did not relaunch"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn installation_locations_distinguish_managed_portable_and_installed_layouts() {
    let directory = Directory::new();
    let data = directory.0.join("data");
    fs::create_dir_all(data.join("caelum")).unwrap();
    let installed = data.join("caelum/Caelum.AppImage");
    fs::write(&installed, "image").unwrap();
    assert_eq!(
        Destination::locate(
            "linux",
            Path::new("/image/usr/bin/caelum-desktop"),
            Some(&installed),
            Some(&data)
        )
        .unwrap()
        .path,
        installed
    );
    assert!(Destination::locate(
        "linux",
        Path::new("/usr/bin/caelum-desktop"),
        None,
        Some(&data)
    )
    .unwrap_err()
    .contains("package manager"));
    let portable = directory.0.join("download.AppImage");
    fs::write(&portable, "image").unwrap();
    assert!(Destination::locate(
        "linux",
        Path::new("/image/usr/bin/caelum-desktop"),
        Some(&portable),
        Some(&data)
    )
    .unwrap_err()
    .contains("--install"));
    let managed = directory
        .0
        .join("Caskroom/caelum/1.0/Caelum.app/Contents/MacOS/caelum-desktop");
    fs::create_dir_all(managed.parent().unwrap()).unwrap();
    fs::write(&managed, "executable").unwrap();
    assert!(Destination::locate("macos", &managed, None, None)
        .unwrap_err()
        .contains("package manager"));
    assert!(Destination::locate("macos", Path::new("/dev/caelum-desktop"), None, None).is_err());
}
