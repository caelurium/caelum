use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Directory(std::path::PathBuf);

impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "caelum-activation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn one_owner_per_workspace_forwards_launches_and_releases_ownership_on_exit() {
    assert!(
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        "run Linux desktop tests under dbus-run-session"
    );
    let directory = Directory::new();
    let state = directory.0.join("workspace.json");
    let handle = rune::App::new(crate::spec()).handle();
    let Launch::Primary(first) = start_for_state(handle.clone(), None, &state).unwrap() else {
        panic!("first owner");
    };
    assert!(matches!(
        start_for_state(handle.clone(), Some("/projects/café #1".into()), &state).unwrap(),
        Launch::Forwarded
    ));
    assert!(start_for_state(handle.clone(), Some("relative".into()), &state).is_err());
    let other = start_for_state(handle.clone(), None, &directory.0.join("other.json")).unwrap();
    assert!(
        matches!(other, Launch::Primary(_)),
        "isolated profiles have independent owners"
    );
    drop(first);
    assert!(matches!(
        start_for_state(handle, None, &state).unwrap(),
        Launch::Primary(_)
    ));
}

#[test]
fn workspace_identity_is_stable_before_and_after_save_and_through_parent_symlinks() {
    let directory = Directory::new();
    let state = directory.0.join("real/workspace.json");
    let original = instance_name(&state).unwrap();
    std::fs::write(&state, "{}").unwrap();
    std::os::unix::fs::symlink(directory.0.join("real"), directory.0.join("alias")).unwrap();
    assert_eq!(instance_name(&state).unwrap(), original);
    assert_eq!(
        instance_name(&directory.0.join("alias/workspace.json")).unwrap(),
        original
    );
}

struct ExitingOwner {
    connection: Connection,
    name: String,
}

#[zbus::interface(name = "dev.caelum.Desktop")]
impl ExitingOwner {
    async fn activate(&self, _project: &str) -> zbus::fdo::Result<()> {
        self.connection
            .release_name(self.name.as_str())
            .await
            .unwrap();
        Err(zbus::fdo::Error::NoReply(
            "The previous owner exited".into(),
        ))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn launch_takes_ownership_when_the_previous_owner_exits_during_forwarding() {
    let directory = Directory::new();
    let name = instance_name(&directory.0.join("workspace.json")).unwrap();
    let previous = Connection::session().await.unwrap();
    previous
        .object_server()
        .at(
            OBJECT,
            ExitingOwner {
                connection: previous.clone(),
                name: name.clone(),
            },
        )
        .await
        .unwrap();
    previous.request_name(name.as_str()).await.unwrap();
    let handle = rune::App::new(crate::spec()).handle();
    let current = tokio::time::timeout(Duration::from_secs(5), connect(handle, &name, None))
        .await
        .unwrap()
        .unwrap()
        .expect("new primary after owner disappearance");
    let (released, removed) = tokio::join!(
        current.release_name(name.as_str()),
        previous.object_server().remove::<ExitingOwner, _>(OBJECT),
    );
    assert!(released.unwrap());
    removed.unwrap();
}
