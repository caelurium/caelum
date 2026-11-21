use super::super::{connect, Endpoint};
use crate::{
    application::{Command, Event, Options, Provider, Reply, Scope, SessionKey},
    remote::{
        native::{Binding, Client, NativeTarget, Notice, Status},
        Host, MachineId,
    },
};
use caelum_protocol::ApprovalPrompt;
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

struct Desktop {
    binding: Binding,
    client: Client,
    wake: mpsc::Receiver<()>,
    notices: VecDeque<Notice>,
    approvals: Vec<ApprovalPrompt>,
    id: u64,
    tunnels: mpsc::Receiver<u32>,
}

impl Desktop {
    fn submit(&self, id: u64, cmd: Command) -> Result<(), String> {
        self.client.submit(id, self.binding.outgoing(cmd)?)
    }
    fn interrupt_link(&mut self) {
        let tunnel = self.tunnels.recv_timeout(Duration::from_secs(5)).unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(tunnel as i32).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        self.until(|n| matches!(n, Notice::Status(Status::Disconnected(_))).then_some(()));
        self.connected();
    }
    #[track_caller]
    fn until<T>(&mut self, mut accept: impl FnMut(Notice) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            self.notices.extend(self.client.drain());
            while let Some(n) = self.notices.pop_front() {
                let n = match n {
                    Notice::Event(e) => match self.binding.incoming(*e) {
                        Some(e) => Notice::Event(Box::new(e)),
                        None => continue,
                    },
                    other => other,
                };
                if let Notice::Event(e) = &n {
                    if let Event::Approvals(a) = e.as_ref() {
                        self.approvals.clone_from(a);
                    }
                }
                if let Some(value) = accept(n) {
                    return value;
                }
            }
            self.wake
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("timed out waiting for the SSH fixture");
        }
    }
    #[track_caller]
    fn connected(&mut self) {
        self.until(|n| match n {
            Notice::Status(Status::Connected) => Some(()),
            Notice::Status(Status::Disconnected(e)) => panic!("SSH connection failed: {e}"),
            _ => None,
        })
    }
    #[track_caller]
    fn request(&mut self, cmd: Command) -> Reply {
        self.try_request(cmd).unwrap()
    }
    fn try_request(&mut self, cmd: Command) -> Result<Reply, String> {
        self.id += 1;
        let id = self.id;
        self.submit(id, cmd).unwrap();
        self.until(|n| {
            if let Notice::Event(e) = n {
                if let Event::Completed(i, r) = *e {
                    assert_eq!(i, id);
                    return Some(r);
                }
                None
            } else {
                None
            }
        })
    }
}

fn attach() -> (Desktop, PathBuf) {
    let fixture =
        PathBuf::from(std::env::var_os("CAELUM_NATIVE_SSH_FIXTURE").expect("SSH fixture"));
    let root = std::env::var("CAELUM_NATIVE_SSH_ROOT").unwrap();
    let repo = "example/project";
    let binding = Binding::new(
        "fixture",
        &NativeTarget {
            machine: MachineId {
                host: Host::Boat,
                account: "example".into(),
                id: "vm-fixture".into(),
            },
            repository: repo.into(),
            checkout_root: root.clone(),
        },
    )
    .unwrap();
    let (wake, events) = mpsc::channel();
    let (children, pids) = mpsc::channel();
    let project = PathBuf::from(&root);
    let mut attempts = 0;
    let client = Client::with_tunnel(
        root.clone(),
        move || {
            let _ = wake.send(());
        },
        async move || {
            attempts += 1;
            let mut endpoint: boat_sdk::access::models::SshKeyResponse =
                serde_json::from_slice(&fs::read(fixture.join("endpoint.json")).unwrap()).unwrap();
            if attempts == 1 {
                endpoint.host_key = Some(
                    fs::read_to_string(fixture.join("client.pub"))
                        .unwrap()
                        .trim()
                        .into(),
                );
            }
            let tunnel = connect(
                fixture.join("client"),
                Endpoint::new(endpoint)?,
                repo,
                &root,
            )
            .await?;
            let _ = children.send(tunnel.child.id().unwrap());
            Ok(tunnel)
        },
    )
    .unwrap();
    let mut desktop = Desktop {
        binding,
        client,
        wake: events,
        notices: VecDeque::new(),
        approvals: vec![],
        id: 0,
        tunnels: pids,
    };
    desktop.until(|n| matches!(n, Notice::Status(Status::Disconnected(_))).then_some(()));
    desktop.client.reconnect();
    desktop.connected();
    (desktop, project)
}

#[test]
#[ignore = "run scripts/dev/smoke_native_workspace.py with the loopback SSH fixture"]
fn native_workspace_over_openssh() {
    let (mut desktop, _) = attach();
    parity::checkout(&mut desktop);
    desktop.interrupt_link();
    parity::checkout(&mut desktop);
}

#[test]
#[ignore = "run scripts/dev/smoke_native_workspace.py with the fake Codex fixture"]
fn native_fake_codex_over_openssh() {
    let (mut desktop, _) = attach();
    let scope = Scope {
        project: desktop.binding.project.clone(),
        provider: Provider::Codex,
    };
    assert!(matches!(
        desktop.request(Command::Choices(scope.clone())),
        Reply::Choices(_)
    ));
    let Reply::Session(session) = desktop.request(Command::Start(scope, Options::default())) else {
        panic!("could not create a native Codex session")
    };
    desktop.request(Command::Send(
        session.key.clone(),
        "Run over the VM WebSocket.".into(),
        Options::default(),
    ));
    let mut streamed = String::new();
    desktop.until(|notice| match notice {
        Notice::Event(event) => match *event {
            Event::Codex(event) if event.method == "item/agentMessage/delta" => {
                streamed.push_str(event.params["delta"].as_str().unwrap());
                None
            }
            Event::Codex(event) if event.method == "turn/completed" => Some(()),
            Event::Completed(_, Err(error)) => panic!("native Codex request failed: {error}"),
            _ => None,
        },
        _ => None,
    });
    assert!(streamed.contains("Codex ran over the VM WebSocket."));
}

mod parity;

#[test]
#[ignore = "run scripts/dev/smoke_native_workspace.py --real-codex BINARY --approval"]
fn native_pending_approval_over_openssh() {
    let (mut desktop, fixture) = attach();
    let scope = Scope {
        project: desktop.binding.project.clone(),
        provider: Provider::Codex,
    };
    let Reply::Session(s) = desktop.request(Command::Start(scope, Options::default())) else {
        panic!()
    };
    fs::write(fixture.join("first-send"), "").unwrap();
    desktop.id += 1;
    desktop
        .submit(
            desktop.id,
            Command::Send(
                s.key.clone(),
                "Create the fixture directory.".into(),
                Options::default(),
            ),
        )
        .unwrap();
    let pending = |n: Notice| match n {
        Notice::Event(e) => match *e {
            Event::Approvals(a) => a.first().cloned(),
            Event::Completed(_, Err(e)) => panic!("Failed to send: {e}"),
            _ => None,
        },
        _ => None,
    };
    let before = desktop.until(pending);
    fs::write(fixture.join("reconnecting"), "").unwrap();
    desktop.interrupt_link();
    desktop.approvals.clear(); // Require the new Codex connection to produce the approval.
    desktop.request(Command::Resume(s.key.clone()));
    let after = desktop
        .approvals
        .iter()
        .find(|a| a.session_id == before.session_id)
        .cloned()
        .unwrap_or_else(|| desktop.until(pending));
    assert_eq!(after.session_id.as_deref(), Some(s.key.id.as_str()));
    assert_eq!(after.summary, before.summary);
    assert!(!fixture.join("approval-fixture").exists());
}

#[test]
#[ignore = "run scripts/dev/smoke_native_workspace.py --real-codex BINARY"]
fn native_new_session_over_openssh() {
    let (mut desktop, fixture) = attach();
    let scope = Scope {
        project: desktop.binding.project.clone(),
        provider: Provider::Codex,
    };
    parity::checkout(&mut desktop);
    assert!(matches!(
        desktop.request(Command::Choices(scope.clone())),
        Reply::Choices(_)
    ));
    let Reply::Sessions(before) = desktop.request(Command::List(scope.clone())) else {
        panic!("could not list sessions")
    };
    assert!(
        before.is_empty(),
        "isolated VM checkout already contained sessions"
    );
    let Reply::Session(s) = desktop.request(Command::Start(scope, Options::default())) else {
        panic!("could not create a native session")
    };
    {
        fs::write(fixture.join("first-send"), "").unwrap();
        desktop.id += 1;
        desktop
            .submit(
                desktop.id,
                Command::Send(
                    s.key.clone(),
                    "Confirm the checkout.".into(),
                    Options::default(),
                ),
            )
            .unwrap();
        let mut text = String::new();
        desktop.until(|n| match n {
            Notice::Event(e) => match *e {
                Event::Codex(e) if e.method == "item/agentMessage/delta" => {
                    text.push_str(e.params["delta"].as_str().unwrap());
                    None
                }
                Event::Codex(e) if e.method == "turn/completed" => {
                    assert_eq!(
                        e.params["turn"]["status"], "completed",
                        "{:?}",
                        e.params["turn"]["error"]
                    );
                    Some(())
                }
                Event::Completed(_, Err(e)) => panic!("Native send failed: {e}"),
                _ => None,
            },
            _ => None,
        });
        assert!(
            text.contains("Confirmed in the VM checkout."),
            "native text was not streamed"
        );
    }
    fs::write(
        fixture.join("native-session.json"),
        serde_json::to_vec(&s.key).unwrap(),
    )
    .unwrap();
    fs::write(fixture.join("reconnecting"), "").unwrap();
    desktop.interrupt_link();
    assert!(
        matches!(desktop.request(Command::List(s.key.scope.clone())),Reply::Sessions(rows) if rows.iter().any(|r|r.key==s.key))
    );
}

#[test]
#[ignore = "run scripts/dev/smoke_native_workspace.py --real-codex BINARY"]
fn native_resume_after_vm_stop() {
    let (mut desktop, fixture) = attach();
    let key: SessionKey =
        serde_json::from_slice(&fs::read(fixture.join("native-session.json")).unwrap()).unwrap();
    assert!(
        matches!(desktop.request(Command::List(key.scope.clone())),Reply::Sessions(s) if s.len()==1 && s[0].key==key)
    );
    assert!(
        matches!(desktop.request(Command::Resume(key.clone())), Reply::Session(s) if s.key == key)
    );
    let Reply::History(p) = desktop.request(Command::History(key, None)) else {
        panic!("missing Codex history")
    };
    assert!(p
        .data
        .iter()
        .any(|turn| turn["items"].as_array().is_some_and(|items| items
            .iter()
            .any(|item| item["type"] == "agentMessage"
                && item["text"] == "Confirmed in the VM checkout."))));
}
