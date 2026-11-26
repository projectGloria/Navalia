//! Checks GitHub Releases for a newer GloriaSetup installer, verifies it
//! against `SHA256SUMS.txt`, and launches it silently with `/S /RELAUNCH`.

use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/projectGloria/Navalia/releases/latest";
const RELEASES_PREFIX: &str = "https://github.com/projectGloria/Navalia/releases/";
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SUMS_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
    pub installer_url: String,
    pub sums_url: String,
    pub installer_name: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn is_release_page(url: &str) -> bool {
    url.starts_with(RELEASES_PREFIX) && !url.contains(char::is_whitespace)
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split('-').next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse_semver(current), parse_semver(candidate)) {
        (Some(cur), Some(cand)) => cand > cur,
        _ => false,
    }
}

fn parse_sha256_for(sums_text: &str, filename: &str) -> Option<String> {
    for line in sums_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let hash = parts.next()?.trim();
        let name = parts.next()?.trim().trim_start_matches('*');
        if name == filename && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn find_release_update(release: &Release, current_version: &str) -> Option<AvailableUpdate> {
    if release.draft || release.prerelease {
        return None;
    }
    let version = release.tag_name.trim().trim_start_matches('v').to_owned();
    if !is_newer(current_version, &version) {
        return None;
    }
    let installer_name = format!("Navalia_{version}_x64-setup.exe");
    let installer = release
        .assets
        .iter()
        .find(|asset| asset.name == installer_name)?;
    let sums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS.txt")?;
    if !installer.browser_download_url.starts_with("https://")
        || !sums.browser_download_url.starts_with("https://")
    {
        return None;
    }
    Some(AvailableUpdate {
        version,
        notes: is_release_page(&release.html_url).then(|| release.html_url.clone()),
        installer_url: installer.browser_download_url.clone(),
        sums_url: sums.browser_download_url.clone(),
        installer_name,
    })
}

fn build_client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || attempt.url().scheme() != "https" {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .user_agent(concat!("Navalia/", env!("CARGO_PKG_VERSION"), " (updater)"))
        .build()
        .map_err(|error| error.to_string())
}

pub fn check() -> Result<Option<AvailableUpdate>, String> {
    let client = build_client(Duration::from_secs(20))?;
    let response = client
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("update check failed ({})", response.status()));
    }
    let body = response.text().map_err(|error| error.to_string())?;
    let release: Release = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    Ok(find_release_update(&release, env!("CARGO_PKG_VERSION")))
}

fn download_and_verify(
    update: &AvailableUpdate,
    destination: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
    mut should_abort: impl FnMut() -> bool,
) -> Result<bool, String> {
    if should_abort() {
        return Ok(false);
    }
    let client = build_client(Duration::from_secs(120))?;
    let sums_resp = client
        .get(&update.sums_url)
        .send()
        .map_err(|error| error.to_string())?;
    if !sums_resp.status().is_success() {
        return Err(format!(
            "could not fetch SHA256SUMS.txt ({})",
            sums_resp.status()
        ));
    }
    let mut sums_buf = Vec::new();
    sums_resp
        .take(MAX_SUMS_BYTES)
        .read_to_end(&mut sums_buf)
        .map_err(|error| error.to_string())?;
    let sums_text = String::from_utf8(sums_buf).map_err(|error| error.to_string())?;
    let expected_sha = parse_sha256_for(&sums_text, &update.installer_name)
        .ok_or_else(|| format!("missing checksum for {}", update.installer_name))?;

    let mut resp = client
        .get(&update.installer_url)
        .send()
        .map_err(|error| error.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("installer download failed ({})", resp.status()));
    }
    let total = resp.content_length();
    if total.is_some_and(|len| len > MAX_INSTALLER_BYTES) {
        return Err("installer exceeds maximum allowed size".into());
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut downloaded = 0u64;
    let mut buf = [0u8; 32 * 1024];
    loop {
        if should_abort() {
            return Ok(false);
        }
        let read = resp.read(&mut buf).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        downloaded += read as u64;
        if downloaded > MAX_INSTALLER_BYTES {
            return Err("installer exceeds maximum allowed size".into());
        }
        file.write_all(&buf[..read])
            .map_err(|error| error.to_string())?;
        digest.update(&buf[..read]);
        on_progress(downloaded, total);
    }
    file.flush()
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    drop(file);

    let actual_sha = format!("{:x}", digest.finalize());
    if actual_sha != expected_sha {
        return Err("installer checksum verification failed".into());
    }
    if should_abort() {
        return Ok(false);
    }
    Ok(true)
}

fn temp_installer_path(version: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "Navalia-update-{version}-{}.exe",
        std::process::id()
    ))
}

/// Downloads the installer, verifies its SHA-256 against `SHA256SUMS.txt`,
/// and launches it with `/S /RELAUNCH` unless `should_abort` returns `true`.
/// Returns `Ok(true)` if the installer was launched, `Ok(false)` if aborted.
pub fn download_and_install(
    update: &AvailableUpdate,
    on_progress: impl FnMut(u64, Option<u64>),
    should_abort: impl FnMut() -> bool,
) -> Result<bool, String> {
    let path = temp_installer_path(&update.version);
    match download_and_verify(update, &path, on_progress, should_abort) {
        Ok(true) => {
            Command::new(&path)
                .args(["/S", "/RELAUNCH"])
                .creation_flags(CREATE_NEW_PROCESS_GROUP)
                .spawn()
                .map_err(|error| {
                    let _ = fs::remove_file(&path);
                    error.to_string()
                })?;
            Ok(true)
        }
        Ok(false) => {
            let _ = fs::remove_file(&path);
            Ok(false)
        }
        Err(error) => {
            let _ = fs::remove_file(&path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_v_prefix_and_ordering() {
        assert!(is_newer("0.1.25", "0.1.26"));
        assert!(is_newer("0.1.25", "v0.2.0"));
        assert!(!is_newer("0.1.25", "0.1.25"));
        assert!(!is_newer("0.1.25", "0.1.24"));
        assert!(!is_newer("0.1.25", "not-a-version"));
    }

    #[test]
    fn sha256sums_parser_matches_exact_filename() {
        let hash = "a".repeat(64);
        let sums = format!(
            "{}  other.txt\n{} *Navalia_0.1.26_x64-setup.exe\n",
            "b".repeat(64),
            hash.to_ascii_uppercase()
        );
        assert_eq!(
            parse_sha256_for(&sums, "Navalia_0.1.26_x64-setup.exe").as_deref(),
            Some(hash.as_str())
        );
        assert_eq!(parse_sha256_for(&sums, "missing.exe"), None);
    }

    #[test]
    fn find_release_update_requires_https_installer_and_checksums() {
        let release = Release {
            tag_name: "v0.1.26".into(),
            html_url: "https://github.com/projectGloria/Navalia/releases/tag/v0.1.26".into(),
            draft: false,
            prerelease: false,
            assets: vec![
                Asset {
                    name: "Navalia_0.1.26_x64-setup.exe".into(),
                    browser_download_url:
                        "https://github.com/projectGloria/Navalia/releases/download/v0.1.26/Navalia_0.1.26_x64-setup.exe"
                            .into(),
                },
                Asset {
                    name: "SHA256SUMS.txt".into(),
                    browser_download_url:
                        "https://github.com/projectGloria/Navalia/releases/download/v0.1.26/SHA256SUMS.txt"
                            .into(),
                },
            ],
        };
        let found = find_release_update(&release, "0.1.25");
        assert!(found.is_some());
        assert_eq!(find_release_update(&release, "0.1.26"), None);
    }

    #[test]
    fn abort_before_download_skips_install_without_network() -> Result<(), String> {
        let update = AvailableUpdate {
            version: "0.1.26".into(),
            notes: None,
            installer_url: "https://127.0.0.1/unused.exe".into(),
            sums_url: "https://127.0.0.1/SHA256SUMS.txt".into(),
            installer_name: "Navalia_0.1.26_x64-setup.exe".into(),
        };
        let launched = download_and_install(&update, |_, _| {}, || true)?;
        assert!(!launched);
        Ok(())
    }
}
