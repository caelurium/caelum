use super::{
    installation::{self, Destination, Prepared, Stage},
    worker::Cancellation,
    Progress, RELEASES_URL,
};
use caelum_protocol::{UpdateCheck, UpdateStatus};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::{Duration, Instant},
};

const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
const MAX_CHECKSUMS: usize = 64 * 1024;

pub(super) fn prepare(
    release: &UpdateCheck,
    cancel: &Cancellation,
    closed: &Cancellation,
    progress: impl Fn(Progress),
) -> Result<Prepared, String> {
    let (url, checksums, name) = urls(release)?;
    let destination = Destination::discover()?;
    let stage = Stage::open(&destination)?;
    stage.begin(&destination)?;
    let cancelled = || cancel.is_cancelled() || closed.is_cancelled();
    let result = (|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let client = Client::builder()
                .user_agent(concat!("Caelum/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(5))
                .read_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(600))
                .https_only(true)
                .build()
                .map_err(|e| e.to_string())?;
            let path = stage.root.join("download");
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err("Update cancelled".to_owned()),
                _ = closed.cancelled() => Err("Update cancelled".to_owned()),
                result = fetch(&client, &url, &checksums, &name, &path, &progress) => result,
            }
        })?;
        if cancelled() {
            return Err("Update cancelled".into());
        }
        progress(Progress::Preparing);
        installation::prepare_payload(&destination, &stage, &release.latest_version, &cancelled)?;
        if cancelled() {
            return Err("Update cancelled".into());
        }
        let _ = fs::remove_file(stage.root.join("download"));
        Ok(())
    })();
    if let Err(error) = result {
        let _ = stage.clear();
        return Err(error);
    }
    Prepared::new(release.clone(), destination, stage)
}

pub(super) async fn fetch(
    client: &Client,
    url: &str,
    checksums: &str,
    name: &str,
    path: &std::path::Path,
    progress: impl Fn(Progress),
) -> Result<(), String> {
    let mut response = client
        .get(checksums)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Cannot download release checksums: {e}"))?;
    let mut manifest = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if manifest.len() + chunk.len() > MAX_CHECKSUMS {
            return Err("The release checksum manifest is too large.".into());
        }
        manifest.extend_from_slice(&chunk);
    }
    let expected = checksum(
        std::str::from_utf8(&manifest).map_err(|e| e.to_string())?,
        name,
    )?;
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Cannot download the update: {e}"))?;
    let total = response.content_length();
    if total.is_some_and(|n| n > MAX_DOWNLOAD) {
        return Err("The update exceeds the 1 GiB download limit.".into());
    }
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut transfer = Transfer::new(file, total);
    progress(Progress::Downloading { bytes: 0, total });
    let mut reported = Instant::now();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Update download interrupted: {e}"))?
    {
        transfer.write(&chunk)?;
        if reported.elapsed() >= Duration::from_millis(200) {
            progress(Progress::Downloading {
                bytes: transfer.bytes,
                total,
            });
            reported = Instant::now();
        }
    }
    progress(Progress::Verifying);
    transfer
        .finish(&expected)?
        .sync_all()
        .map_err(|e| e.to_string())
}

fn urls(release: &UpdateCheck) -> Result<(String, String, String), String> {
    let version = semver::Version::parse(&release.latest_version).map_err(|e| e.to_string())?;
    if release.status != UpdateStatus::Available
        || !version.pre.is_empty()
        || !matches!(std::env::consts::ARCH, "aarch64" | "x86_64")
    {
        return Err("Check for a stable update for this architecture first.".into());
    }
    let tag = release
        .release_url
        .strip_prefix(&format!("{RELEASES_URL}/tag/"))
        .ok_or("The update is not an official Caelum release.")?;
    if tag != version.to_string() && tag != format!("v{version}") {
        return Err("The update release tag does not match its version.".into());
    }
    let extension = if cfg!(target_os = "macos") {
        "dmg"
    } else {
        "AppImage"
    };
    let name = format!("Caelum-v{version}-{}.{}", std::env::consts::ARCH, extension);
    let base = format!("{RELEASES_URL}/download/{tag}");
    let url = format!("{base}/{name}");
    if release.download_url.as_ref() != Some(&url) {
        return Err("The update asset does not match this platform and architecture.".into());
    }
    Ok((url, format!("{base}/SHA256SUMS"), name))
}

fn checksum(manifest: &str, name: &str) -> Result<String, String> {
    let mut matches = manifest.lines().filter_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file
            .trim_start()
            .strip_prefix('*')
            .unwrap_or(file.trim_start())
            == name)
            .then_some(hash)
    });
    let hash = matches
        .next()
        .ok_or("The release checksums do not contain this update.")?;
    if matches.next().is_some() || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("The release checksum entry is invalid or duplicated.".into());
    }
    Ok(hash.to_ascii_lowercase())
}

struct Transfer<W> {
    output: W,
    hash: Sha256,
    bytes: u64,
    total: Option<u64>,
}

impl<W: Write> Transfer<W> {
    fn new(output: W, total: Option<u64>) -> Self {
        Self {
            output,
            hash: Sha256::new(),
            bytes: 0,
            total,
        }
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.bytes += bytes.len() as u64;
        if self.bytes > MAX_DOWNLOAD {
            return Err("The update exceeds the 1 GiB download limit.".into());
        }
        self.output
            .write_all(bytes)
            .map_err(|e| format!("Cannot save the update: {e}"))?;
        self.hash.update(bytes);
        Ok(())
    }
    fn finish(self, expected: &str) -> Result<W, String> {
        if self.total.is_some_and(|size| size != self.bytes)
            || format!("{:x}", self.hash.finalize()) != expected
        {
            return Err("Update verification failed. The download is incomplete or does not match SHA256SUMS. Retry the download.".into());
        }
        Ok(self.output)
    }
}

#[cfg(test)]
mod tests;
