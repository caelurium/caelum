use super::*;
use crate::updates::tests::{available, Directory};
use std::{io::Read, net::TcpListener, sync::Arc, thread};

#[test]
fn checksums_are_exact_unambiguous_and_validate_streamed_bytes() {
    let hash = format!("{:x}", Sha256::digest(b"payload"));
    for separator in ["  ", " *"] {
        assert_eq!(
            checksum(&format!("{hash}{separator}asset\n"), "asset").unwrap(),
            hash
        );
    }
    for manifest in [
        format!("{hash}  other"),
        format!("{hash}  asset\n{hash}  asset"),
        "invalid  asset".into(),
    ] {
        assert!(checksum(&manifest, "asset").is_err());
    }
    let mut transfer = Transfer::new(Vec::new(), Some(7));
    transfer.write(b"pay").unwrap();
    transfer.write(b"load").unwrap();
    assert_eq!(transfer.finish(&hash).unwrap(), b"payload");
    for (bytes, total) in [
        (&b"corrupt"[..], Some(7)),
        (b"pay", Some(7)),
        (b"payload", Some(8)),
    ] {
        let mut transfer = Transfer::new(Vec::new(), total);
        transfer.write(bytes).unwrap();
        assert!(transfer
            .finish(&hash)
            .unwrap_err()
            .contains("verification failed"));
    }
}

#[test]
fn disk_errors_and_download_limit_stop_the_transfer() {
    struct Full;
    impl Write for Full {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(Transfer::new(Full, None)
        .write(b"x")
        .unwrap_err()
        .contains("disk full"));
    let mut transfer = Transfer::new(Vec::new(), None);
    transfer.bytes = MAX_DOWNLOAD;
    assert!(transfer.write(b"x").unwrap_err().contains("1 GiB"));
}

#[test]
fn only_exact_official_platform_release_urls_are_installable() {
    assert!(urls(&available()).is_ok());
    for (url, tag) in [
        (
            "https://example.invalid/update",
            available().release_url.as_str(),
        ),
        (
            available().download_url.as_ref().unwrap(),
            "https://example.invalid/tag/v1.2.0",
        ),
        (
            available().download_url.as_ref().unwrap(),
            "https://github.com/caelurium/caelum/releases/tag/v1.2.0/../other",
        ),
    ] {
        let mut release = available();
        release.download_url = Some(url.into());
        release.release_url = tag.into();
        assert!(urls(&release).is_err());
    }
}

fn read_request(stream: &mut std::net::TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
}

#[test]
fn downloads_and_verifies_a_real_http_response() {
    let directory = Directory::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for body in [
            format!("{:x}  asset\n", Sha256::digest(b"payload")),
            "payload".into(),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        fetch(
            &Client::builder().no_proxy().build().unwrap(),
            &format!("{url}/asset"),
            &format!("{url}/checksums"),
            "asset",
            &directory.0.join("download"),
            |_| {},
        )
        .await
        .unwrap();
    });
    assert_eq!(fs::read(directory.0.join("download")).unwrap(), b"payload");
    server.join().unwrap();
}

#[test]
fn cancellation_interrupts_a_server_that_never_sends_headers() {
    let directory = Directory::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let cancel = Arc::new(Cancellation::default());
    let cancellation = cancel.clone();
    let (release, released) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        cancellation.cancel();
        released.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let start = Instant::now();
    runtime.block_on(async {
        let client = Client::builder().no_proxy().build().unwrap();
        let path = directory.0.join("download");
        tokio::select! {
            _ = cancel.cancelled() => {},
            result = fetch(&client, &url, &url, "asset", &path, |_| {}) => panic!("unexpected {result:?}"),
            _ = tokio::time::sleep(Duration::from_secs(2)) => panic!("cancellation stalled"),
        }
    });
    assert!(start.elapsed() < Duration::from_secs(2));
    release.send(()).unwrap();
    server.join().unwrap();
}
