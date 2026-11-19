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
