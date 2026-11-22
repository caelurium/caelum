use super::*;
use crate::application::{ExportFormat, Options, Provider, Scope, SessionKey};
use crate::remote::native::{Binding, NativeTarget};
use crate::remote::{Host, MachineId};
use std::sync::mpsc as blocking;

#[test]
fn losing_a_link_retires_the_send_and_only_admits_new_work_after_its_connected_notice() {
    let (attempt, rx) = blocking::channel();
    let (wake, notices) = blocking::channel();
    let mut client = Client::with_connection(
        "/vm".into(),
        move || {
            let _ = wake.send(());
        },
        async move |attempt_state, out| {
            let (lost, signal) = tokio::sync::oneshot::channel::<()>();
            let (active, writer) = attempt_state.activate();
            let (sent, observed) = blocking::channel();
            if attempt.send((lost, observed)).is_err() {
                return "test receiver dropped".into();
            }
            out.publish(Message::Connected(writer)).await;
            tokio::pin!(signal);
            loop {
                tokio::select! {
                    _=&mut signal=>return "tunnel closed".into(),
                    cmd=active.requests.recv()=>if let Some(cmd)=cmd {
                        active.pending.insert(cmd.0);
                        if sent.send(cmd).is_err() {
                            return "test command receiver dropped".into();
                        }
                    }
                }
            }
        },
    )
    .unwrap();
    let wait = |client: &mut Client, accept: fn(&Notice) -> bool| loop {
        if client.drain().iter().any(accept) {
            return;
        }
        notices.recv_timeout(Duration::from_secs(5)).unwrap();
    };
    let connected = |n: &Notice| matches!(n, Notice::Status(Status::Connected));
    let (lost, first) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait(&mut client, connected);
    let key = SessionKey {
        scope: Scope {
            project: "/vm".into(),
            provider: Provider::Codex,
        },
        id: "thread".into(),
    };
    client
        .submit(
            1,
            Command::Send(key.clone(), "Once".into(), Options::default()),
        )
        .unwrap();
    assert!(matches!(first.recv().unwrap(), (1, Command::Send(..))));
    lost.send(()).unwrap();
    wait(
        &mut client,
        |n| matches!(n,Notice::Event(e) if matches!(e.as_ref(),Event::Completed(1,Err(e)) if e.contains("unconfirmed"))),
    );
    let (_lost, second) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(client.submit(2, Command::Resume(key.clone())).is_err());
    wait(&mut client, connected);
    client.submit(2, Command::Resume(key)).unwrap();
    assert!(matches!(second.recv().unwrap(), (2, Command::Resume(_))));
}

#[test]
fn dropping_client_cancels_and_joins_its_ssh_supervisor() {
    struct Cancelled(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for Cancelled {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }

    let (started, wait) = blocking::channel();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = cancelled.clone();
    let client = Client::with_connection(
        "/vm".into(),
        || {},
        async move |_, _| {
            let _cancelled = Cancelled(observed.clone());
            if started.send(()).is_err() {
                return "test receiver dropped".into();
            }
            std::future::pending::<String>().await
        },
    )
    .unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    let before = std::time::Instant::now();
    drop(client);
    assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
    assert!(before.elapsed() < Duration::from_secs(2));
}

#[test]
fn native_codex_export_is_routed_to_its_vm_without_opencode_fallback() {
    let target = NativeTarget {
        machine: MachineId {
            host: Host::Boat,
            account: "account".into(),
            id: "machine".into(),
        },
        repository: "example/project".into(),
        checkout_root: "/vm/project".into(),
    };
    let binding = Binding::new("checkout", &target).unwrap();
    let key = SessionKey {
        scope: Scope {
            project: binding.project.clone(),
            provider: Provider::Codex,
        },
        id: "thread".into(),
    };
    let command = Command::Export(
        key,
        "VM conversation".into(),
        ExportFormat::Markdown,
        "/tmp/vm-conversation.md".into(),
    );
    assert!(matches!(
        binding.outgoing(command).unwrap(),
        Command::Export(key, _, _, _) if key.scope.project == target.checkout_root
    ));

    let key = SessionKey {
        scope: Scope {
            project: binding.project.clone(),
            provider: Provider::OpenCode,
        },
        id: "legacy".into(),
    };
    assert!(binding
        .outgoing(Command::Export(
            key,
            "Legacy conversation".into(),
            ExportFormat::Markdown,
            "/tmp/legacy.md".into(),
        ))
        .is_err());
}
