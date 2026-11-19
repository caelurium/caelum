use super::*;
use serde_json::json;
use wiremock::{
    matchers::{header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

async fn fixture() -> (MockServer, Boat) {
    let server = MockServer::start().await;
    let config = boat_sdk::Config::new("fixture-key")
        .unwrap()
        .with_base_url(&server.uri())
        .unwrap();
    Mock::given(method("GET"))
        .and(path("/me"))
        .and(header("authorization", "Bearer fixture-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"type":"me","user":{"id":"user-1"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let boat = Boat::authenticate(Client::from_config(config).unwrap())
        .await
        .unwrap();
    (server, boat)
}

#[tokio::test]
async fn lists_every_page_without_exposing_sandbox_secrets() {
    let (server, boat) = fixture().await;
    for (rank, cursor, id) in [(2, None, "vm-1"), (1, Some("second"), "vm-2")] {
        let mut mock = Mock::given(method("GET")).and(path("/sandboxes"));
        if let Some(cursor) = cursor {
            mock = mock.and(query_param("cursor", cursor));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"type":"list","sandboxes":[{"id":id,"name":"dev","state":"running","desktopAvailable":true,
            "snapshotAvailable":true,"desktopUrl":"secret-url"}],
            "pageInfo":{"limit":100,"hasMore":cursor.is_none(),"nextCursor":cursor.is_none().then_some("second")}
        }))).with_priority(rank).expect(1).mount(&server).await;
    }
    let list = boat.inventory().await.unwrap();
    assert_eq!(list.machines.len(), 2);
    assert_ne!(list.machines[0].key, list.machines[1].key);
    assert!(!format!("{list:?}").contains("secret-url"));
    assert!(
        !format!("{:?}", super::super::Credential::new("never-print".into()))
            .contains("never-print")
    );
}

fn sandbox(state: &str) -> serde_json::Value {
    json!({"id":"vm-1","name":"dev","state":state,"desktopAvailable":false,"snapshotAvailable":true,
           "environment":"project-env","environmentVersion":2})
}

async fn environment(server: &MockServer) {
    Mock::given(method("GET")).and(path("/environments")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "environments":[{"id":"env-1","name":"project-env","isDefault":true,"latestVersionNumber":2,
        "safeForThirdParties":false,"passGithub":true,"passSecrets":false,"passSandboxCredentials":false,
        "passAgentsCredentials":true,"envContents":"","secretFiles":[],"versions":[],"selectedRepositories":[{"fullName":"example/project"}]}]
    }))).mount(server).await;
}

#[tokio::test]
async fn create_retry_reuses_body_and_idempotency_key_without_changing_shared_environment() {
    use crate::remote::Size;
    let (server, boat) = fixture().await;
    environment(&server).await;
    let source = Source {
        project: "/local/project".into(),
        account: boat.account.clone(),
        repository: "example/project".into(),
        base: "main".into(),
        environment: Some("project-env".into()),
    };
    let plan = Plan {
        token: uuid::Uuid::new_v4().to_string(),
        branch: "feat/new".into(),
        size: Size::Small,
        minutes: 30,
    };
    Mock::given(method("POST")).and(path("/sandboxes")).and(header("idempotency-key",plan.token.as_str()))
        .and(wiremock::matchers::body_partial_json(json!({"environment":"project-env","type":"small","ttlSeconds":1800})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"sandbox","status":"provisioning","sandbox":sandbox("provisioning")})))
        .expect(2).mount(&server).await;
    assert_eq!(
        boat.create(&source, &plan).await.unwrap(),
        boat.create(&source, &plan).await.unwrap()
    );
    let requests = server.received_requests().await.unwrap();
    let posts: Vec<_> = requests
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .collect();
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].body, posts[1].body);
}

#[tokio::test]
async fn opencode_uses_exact_machine_conversation_cursor_and_terminal_run() {
    use crate::remote::harness::Harness;
    use wiremock::matchers::body_json;
    let (server, boat) = fixture().await;
    let target = Target {
        machine: MachineId {
            host: Host::Boat,
            account: "user-1".into(),
            id: "vm-1".into(),
        },
        repository: "example/project".into(),
    };
    let run = json!({"id":"r","promptId":"p","sandboxId":"vm-1","conversationId":"c","status":"finished","done":true});
    Mock::given(method("POST")).and(path("/sandboxes/vm-1/commands"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"command","success":true,"exitCode":0,"stdout":"git@github.com:example/project.git\n","stderr":"","timedOut":false}))).expect(2).mount(&server).await;
    for (scope, message) in [
        (json!({"new":true}), "Start"),
        (json!({"conversationId":"c"}), "Continue"),
    ] {
        let mut body = scope;
        body["provider"] = "opencode".into();
        body["prompt"] = message.into();
        Mock::given(method("POST")).and(path("/sandboxes/vm-1/prompt")).and(body_json(body))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({"ok":true,"type":"queued","id":"vm-1","promptId":"p","conversationId":"c","promptRun":run,"status":"queued","provider":"opencode"}))).expect(1).mount(&server).await;
    }
    let Outcome::Accepted(first) = boat.send(&target, None, "Start").await.unwrap() else {
        panic!()
    };
    Mock::given(path("/sandboxes/vm-1/prompts/p"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"type":"status","id":"vm-1","promptRun":run})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/sandboxes/vm-1/events")).and(query_param("conversation","c")).and(query_param("cursor","before")).and(query_param("sort","asc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"events","id":"vm-1","events":[
            {"type":"response","id":"1","conversationId":"other","timestamp":10,"data":{"content":"another session"}},
            {"type":"response","id":"2","conversationId":"c","timestamp":11,"data":{"content":"Edited the file."}}
        ],"pageInfo":{"hasMore":false,"nextCursor":"after","limit":100}}))).expect(1).mount(&server).await;
    let page = boat.follow(&target, &first, Some("before")).await.unwrap();
    assert_eq!(page.lines.len(), 1);
    assert_eq!(page.lines[0].text, "Edited the file.");
    assert_eq!(page.cursor.as_deref(), Some("after"));
    assert_eq!(page.finished.as_deref(), Some("finished"));
    let Outcome::Accepted(next) = boat
        .send(&target, Some(&first.conversation), "Continue")
        .await
        .unwrap()
    else {
        panic!()
    };
    Mock::given(method("POST"))
        .and(path("/sandboxes/vm-1/interrupt"))
        .and(query_param("conversation", "c"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"type":"interrupted","id":"vm-1","status":"ok"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    boat.interrupt(&target, &next).await.unwrap();
}
