use super::*;
use serde_json::json;
use std::{io::Write, net::TcpListener, thread};

fn release(tag: &str) -> Value {
    json!({ "tag_name": tag, "draft": false, "prerelease": false, "assets": [] })
}

#[test]
fn release_assets_use_the_canonical_owner_after_the_repository_transfer() {
    let url =
        "https://github.com/caelurium/caelum/releases/download/v1.2.0/Caelum-v1.2.0-aarch64.dmg";
    let mut payload = release("v1.2.0");
    payload["assets"] = json!([{"name":"Caelum-v1.2.0-aarch64.dmg", "state":"uploaded", "browser_download_url":url}]);
    let update = compare_release(&payload, "1.0.0", "macos", "aarch64").unwrap();
    assert_eq!(update.download_url.as_deref(), Some(url));
    assert_eq!(
        update.release_url,
        "https://github.com/caelurium/caelum/releases/tag/v1.2.0"
    );
}

#[test]
fn compares_semantic_versions_including_prereleases_and_build_metadata() {
    for (current, tag, expected) in [
        ("0.1.9", "v0.1.10", UpdateStatus::Available),
        ("1.0.0-rc.1", "v1.0.0", UpdateStatus::Available),
        ("1.0.0+local.2", "1.0.0+release.1", UpdateStatus::UpToDate),
        ("1.0.0", "v1.0.0", UpdateStatus::UpToDate),
        ("1.1.0-dev.1", "v1.0.0", UpdateStatus::Ahead),
        ("1.1.0", "v1.0.0", UpdateStatus::Ahead),
    ] {
        let result = compare_release(&release(tag), current, "macos", "aarch64").unwrap();
        assert_eq!(result.status, expected);
    }
    for tag in [
        "nightly",
        "v1.0",
        "v01.0.0",
        "v1.1.0-beta.1",
        "v1.0.0/other",
    ] {
        assert!(compare_release(&release(tag), "1.0.0", "macos", "aarch64").is_err());
    }
    for field in ["draft", "prerelease"] {
        let mut value = release("v1.1.0");
        value[field] = json!(true);
        assert!(compare_release(&value, "1.0.0", "macos", "aarch64").is_err());
    }
}

#[test]
fn downloads_match_the_native_platform_architecture_and_published_asset() {
    let mut value = release("v1.1.0");
    value["assets"] = json!(["aarch64", "x86_64"].map(|arch| json!({
        "name": format!("Caelum-v1.1.0-{arch}.dmg"), "state": "uploaded",
        "browser_download_url": format!("{RELEASES_URL}/download/v1.1.0/Caelum-v1.1.0-{arch}.dmg"),
    })));
    for arch in ["aarch64", "x86_64"] {
        let result = compare_release(&value, "1.0.0", "macos", arch).unwrap();
        assert!(result
            .download_url
            .unwrap()
            .ends_with(&format!("-{arch}.dmg")));
    }
    assert!(compare_release(&value, "1.0.0", "linux", "aarch64")
        .unwrap()
        .download_url
        .is_none());
    value["assets"][0]["browser_download_url"] = json!("https://example.invalid/other.dmg");
    assert!(compare_release(&value, "1.0.0", "macos", "aarch64")
        .unwrap()
        .download_url
        .is_none());
    assert!(
        compare_release(&release("v1.1.0"), "1.0.0", "macos", "aarch64")
            .unwrap()
            .download_url
            .is_none()
    );
}

#[test]
fn linux_downloads_require_the_matching_uploaded_appimage_on_github() {
    let mut value = release("v1.1.0");
    value["assets"] = json!(["aarch64", "x86_64"].map(|arch| json!({
        "name": format!("Caelum-v1.1.0-{arch}.AppImage"), "state": "uploaded",
        "browser_download_url": format!("{RELEASES_URL}/download/v1.1.0/Caelum-v1.1.0-{arch}.AppImage"),
    })));
    for arch in ["aarch64", "x86_64"] {
        assert!(compare_release(&value, "1.0.0", "linux", arch)
            .unwrap()
            .download_url
            .unwrap()
            .ends_with(&format!("-{arch}.AppImage")));
        assert!(compare_release(&value, "1.0.0", "macos", arch)
            .unwrap()
            .download_url
            .is_none());
    }
    assert!(compare_release(&value, "1.0.0", "linux", "arm")
        .unwrap()
        .download_url
        .is_none());
    value["assets"][0]["state"] = json!("new");
    assert!(compare_release(&value, "1.0.0", "linux", "aarch64")
        .unwrap()
        .download_url
        .is_none());
    value["assets"][0]["state"] = json!("uploaded");
    value["assets"][0]["browser_download_url"] = json!("https://example.invalid/other.AppImage");
    assert!(compare_release(&value, "1.0.0", "linux", "aarch64")
        .unwrap()
        .download_url
        .is_none());
}

#[test]
fn handles_http_failures_malformed_responses_and_bounded_payloads() {
    for (status, body, expected) in [
        ("200 OK", release("v1.1.0").to_string(), None),
        ("403 Forbidden", "{}".to_owned(), Some("limiting")),
        ("429 Too Many Requests", "{}".to_owned(), Some("limiting")),
        ("404 Not Found", "{}".to_owned(), Some("No stable release")),
        ("503 Unavailable", "{}".to_owned(), Some("HTTP 503")),
        ("200 OK", "not JSON".to_owned(), Some("invalid release")),
        (
            "200 OK",
            " ".repeat(MAX_RESPONSE_BYTES as usize + 1),
            Some("too large"),
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .contains("application/vnd.github+json"));
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let result = fetch_release(&client, &url);
        if let Some(message) = expected {
            assert!(result.unwrap_err().contains(message));
        } else {
            assert_eq!(result.unwrap()["tag_name"], "v1.1.0");
        }
        server.join().unwrap();
    }
}
