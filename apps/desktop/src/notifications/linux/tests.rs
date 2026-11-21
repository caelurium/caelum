use super::*;
use caelum_core::application::{Provider, Scope};
use std::sync::{Arc, Mutex};
use zbus::OwnedVariant;

#[derive(Default)]
struct Received {
    title: String,
    body: String,
    actions: Vec<String>,
    desktop: String,
    silent: bool,
    closed: Vec<u32>,
}

struct Server(Arc<Mutex<Received>>);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, OwnedVariant>,
        expire_timeout: i32,
    ) -> u32 {
        assert_eq!(app_name, "Caelum");
        assert_eq!(replaces_id, 0);
        assert_eq!(app_icon, "dev.caelum.desktop");
        assert_eq!(expire_timeout, 10_000);
        let mut received = self.0.lock().unwrap();
        received.title = summary.into();
        received.body = body.into();
        received.actions = actions;
        received.desktop = <&str>::try_from(&hints["desktop-entry"]).unwrap().into();
        received.silent = bool::try_from(&hints["suppress-sound"]).unwrap();
        1000
    }

    fn close_notification(&self, id: u32) {
        self.0.lock().unwrap().closed.push(id);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn notification_wire_contract_preserves_targets_and_bounds_active_identifiers() {
    let received = Arc::new(Mutex::new(Received::default()));
    let (service, connection) =
        tokio::try_join!(Connection::session(), Connection::session()).unwrap();
    service
        .object_server()
        .at("/org/freedesktop/Notifications", Server(received.clone()))
        .await
        .unwrap();
    service
        .request_name("org.freedesktop.Notifications")
        .await
        .unwrap();
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await
    .unwrap();
    let handle = rune::App::new(crate::spec()).handle();
    let target = SessionKey {
        scope: Scope {
            project: "/projects/café #1".into(),
            provider: Provider::OpenCode,
        },
        id: "session-1".into(),
    };
    let mut targets = (0..LIMIT as u32).map(|id| (id, None)).collect();
    deliver(
        &handle,
        &proxy,
        &mut targets,
        (
            1,
            Notification {
                title: "Caelum · Question".into(),
                body: "literal <b>café</b> & text".into(),
                target: Some(target.clone()),
            },
        ),
    )
    .await;
    assert_eq!(targets.len(), LIMIT);
    assert_eq!(targets.get(&1000), Some(&Some(target)));
    assert!(!targets.contains_key(&0));
    let received = received.lock().unwrap();
    assert_eq!(received.title, "Caelum · Question");
    assert_eq!(received.body, "literal &lt;b&gt;café&lt;/b&gt; &amp; text");
    assert_eq!(received.actions, ["default", "Open Caelum"]);
    assert_eq!(received.desktop, "dev.caelum.desktop");
    assert!(received.silent);
    assert_eq!(received.closed, [0]);
}
