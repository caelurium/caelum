use super::{Connection, Connections};
use caelum_core::{
    application::{Command, Provider, Runtime, Scope, SessionKey, Workspace},
    remote::{
        Host, MachineId,
        native::{Binding, NativeTarget, Status},
    },
};
use std::collections::BTreeMap;

#[test]
fn codex_reads_wait_for_the_vm_connection_and_keep_the_verified_checkout_owner() {
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
    let connection = Connection {
        binding,
        status: Status::Connecting,
        client: None,
        reads: Vec::new(),
        initial: None,
    };
    let mut connections = Connections(BTreeMap::from([("checkout".into(), connection)]));
    let app = Workspace::default();
    let mut local = Runtime::new(|| {});
    let key = SessionKey {
        scope: Scope {
            project: "boat://checkout/checkout".into(),
            provider: Provider::Codex,
        },
        id: "thread".into(),
    };

    connections
        .submit(&mut local, &app, None, 17, Command::History(key, None))
        .unwrap();

    let [(request, Command::History(key, None))] = connections.0["checkout"].reads.as_slice()
    else {
        panic!("expected the request to stay queued until SSH connects")
    };
    assert_eq!(*request, 17);
    assert_eq!(key.scope.project, target.checkout_root);
}
