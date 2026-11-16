use super::*;
use serde_json::json;

const OLD: &str = "https://git.example.com/previous/repo/pull/43";
const CANONICAL: &str = "https://git.example.com/team/renamed/pull/43";
const REPOSITORY: &str = "https://git.example.com/team/renamed";

#[test]
fn transferred_details_keep_the_requested_review_and_cache_identity() {
    let fixture = Fixture::new(
        "printf '%s\\n' \"$@\" >> arguments\ncase \"$1\" in pr) cat details.json;; api) cat repository.json;; *) exit 99;; esac",
    );
    fs::write(fixture.0.join("details.json"), details_response(CANONICAL)).unwrap();
    fs::write(
        fixture.0.join("repository.json"),
        json!({"html_url": REPOSITORY}).to_string(),
    )
    .unwrap();
    let details = query_details(&fixture.0.join("gh"), &fixture.0, OLD, TEST_TIMEOUT).unwrap();
    assert_eq!(details.pull_request.url, OLD);
    assert_eq!(details.pull_request.number, 43);
    let args = fs::read_to_string(fixture.0.join("arguments")).unwrap();
    assert!(args.starts_with(&format!("pr\nview\n{OLD}\n")));
    assert!(
        args.ends_with("api\n--hostname\ngit.example.com\n--method\nGET\nrepos/previous/repo\n")
    );

    let cache = cache::Cache::for_store(&fixture.0.join("workspace.json"));
    let files = caelum_protocol::GitHubPullRequestFiles {
        base_oid: details.base_ref_oid.clone(),
        files: vec![],
        next_page: None,
    };
    cache.save_details(&details, cache::timestamp());
    cache.save_page(OLD, &details.head_ref_oid, 1, &files);
    let restored = cache.load(OLD).unwrap();
    assert_eq!(restored.details, details);
    assert_eq!(restored.files, Some(files));
}

#[test]
fn redirects_require_confirmation_from_the_original_repository() {
    let fixture = Fixture::new("cat repository.json");
    for repository in [
        json!({"html_url": "https://git.example.com/previous/repo"}),
        json!({"html_url": "https://git.example.com/other/renamed"}),
        json!({"html_url": "https://other.example.com/team/renamed"}),
        json!({"html_url": format!("{REPOSITORY}?redirect=true")}),
        json!({"html_url": null}),
        json!({}),
    ] {
        fs::write(fixture.0.join("repository.json"), repository.to_string()).unwrap();
        let error = identity::check(
            &fixture.0.join("gh"),
            &fixture.0,
            OLD,
            Some(CANONICAL),
            TEST_TIMEOUT,
        )
        .unwrap_err();
        assert!(error.to_string().contains("different pull request"));
    }
    let unavailable = Fixture::new("printf 'Authentication required' >&2\nexit 1");
    assert!(identity::check(
        &unavailable.0.join("gh"),
        &unavailable.0,
        OLD,
        Some(CANONICAL),
        TEST_TIMEOUT,
    )
    .unwrap_err()
    .to_string()
    .contains("Authentication required"));
}

#[test]
fn a_repository_redirect_cannot_change_the_host_or_pull_request_number() {
    let fixture = Fixture::new("touch called");
    for returned in [
        None,
        Some("https://git.example.com/team/renamed/pull/44"),
        Some("https://other.example.com/team/renamed/pull/43"),
        Some("https://git.example.com:8443/team/renamed/pull/43"),
        Some("http://git.example.com/team/renamed/pull/43"),
        Some("https://git.example.com/team/renamed/pull/43?x=1"),
        Some("https://git.example.com/team/renamed/pull/43#issuecomment-1"),
        Some("https://user@git.example.com/team/renamed/pull/43"),
        Some("https://git.example.com/team/renamed/issues/43"),
    ] {
        assert!(identity::check(
            &fixture.0.join("gh"),
            &fixture.0,
            OLD,
            returned,
            TEST_TIMEOUT,
        )
        .unwrap_err()
        .to_string()
        .contains("different pull request"));
    }
    assert!(!fixture.0.join("called").exists());
    assert_eq!(
        identity::check(
            &fixture.0.join("missing"),
            &fixture.0,
            OLD,
            Some(OLD),
            TEST_TIMEOUT
        )
        .unwrap(),
        OLD
    );
}
