//! Over-the-air updates.
//!
//! CI publishes each version as a GitHub Release of this repository: `latest.json` plus the new
//! `Nova.exe`. Nova reads the manifest over HTTPS from github.com and accepts an update only if
//! its version is newer than the running one and the downloaded file's SHA-256 matches the
//! manifest (the same scheme lagless uses).
//!
//! The new exe is staged in Nova's data folder and swapped in when Nova quits (or on "Restart
//! to update"). That works for the installed and the portable copy alike, without admin
//! rights. lagless is linked into Nova, so its engine updates with Nova.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where releases are published (public, binaries only). Overridable at build time.
const DEFAULT_FEED: &str = "https://github.com/leoble123/nova/releases/latest/download/latest.json";
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;
pub const PLATFORM: &str = "windows-x86_64";

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum State {
    Idle,
    Checking,
    UpToDate { checked_at: i64 },
    #[serde(rename_all = "camelCase")]
    Downloading { version: String, progress: f32 },
    #[serde(rename_all = "camelCase")]
    Ready { version: String, notes: String },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub pub_date: String,
    pub platforms: std::collections::HashMap<String, Asset>,
}

pub struct Updater {
    feed: String,
    dir: PathBuf,
    state: Mutex<State>,
    staged: Mutex<Option<(String, PathBuf)>>,
    busy: Mutex<()>,
    on_change: Box<dyn Fn(&State) + Send + Sync>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.trim().trim_start_matches('v').split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    parse(candidate) > parse(current)
}

impl Updater {
    pub fn new(dir: PathBuf, on_change: impl Fn(&State) + Send + Sync + 'static) -> Arc<Updater> {
        let mut feed = option_env!("NOVA_UPDATE_FEED").unwrap_or(DEFAULT_FEED).to_string();
        // Test hook, debug builds only: a release build only ever reads the compiled-in feed.
        if cfg!(debug_assertions) {
            if let Ok(f) = std::env::var("NOVA_UPDATE_FEED") {
                feed = f;
            }
        }
        let u = Arc::new(Updater { feed, dir, state: Mutex::new(State::Idle), staged: Mutex::new(None), busy: Mutex::new(()), on_change: Box::new(on_change) });
        u.clean_stale();
        u
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    fn set(&self, s: State) {
        *self.state.lock().unwrap() = s.clone();
        (self.on_change)(&s);
    }

    /// Staged files from older runs (already installed, or superseded).
    fn clean_stale(&self) {
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Check the feed; download and verify a newer version. Blocking; safe to call repeatedly.
    pub fn check(&self) {
        let Ok(_guard) = self.busy.try_lock() else { return };
        if matches!(self.state(), State::Ready { .. }) {
            return;
        }
        self.set(State::Checking);
        match self.check_inner() {
            Ok(Some((version, notes))) => self.set(State::Ready { version, notes }),
            Ok(None) => self.set(State::UpToDate { checked_at: crate::models::now_ms() }),
            Err(e) => self.set(State::Error { message: format!("{e:#}") }),
        }
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).user_agent(format!("Nova/{VERSION}")).build().into()
    }

    fn check_inner(&self) -> Result<Option<(String, String)>> {
        let m: Manifest = Self::agent()
            .get(&self.feed)
            .call()
            .context("couldn't reach the update server")?
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_json()
            .context("the update feed isn't valid")?;
        if !is_newer(&m.version, VERSION) {
            return Ok(None);
        }
        let asset = m.platforms.get(PLATFORM).context("no build for this platform in the update")?;
        if unhex(&asset.sha256).map(|h| h.len()) != Some(32) {
            bail!("the update feed has no valid checksum");
        }
        if asset.size > MAX_DOWNLOAD {
            bail!("the update is unexpectedly large");
        }
        std::fs::create_dir_all(&self.dir)?;
        let part = self.dir.join(format!("Nova-{}.exe.part", m.version));
        let staged = self.dir.join(format!("Nova-{}.exe", m.version));
        let resp = Self::agent().get(&asset.url).call().context("couldn't download the update")?;
        let mut reader = resp.into_body().into_with_config().limit(MAX_DOWNLOAD).reader();
        let mut file = std::fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut done: u64 = 0;
        let mut last_emit = 0.0f32;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            done += n as u64;
            if asset.size > 0 {
                let p = (done as f32 / asset.size as f32).min(1.0);
                if p - last_emit >= 0.05 {
                    last_emit = p;
                    self.set(State::Downloading { version: m.version.clone(), progress: p });
                }
            }
        }
        file.sync_all()?;
        drop(file);
        let got = hex(&hasher.finalize());
        if !got.eq_ignore_ascii_case(asset.sha256.trim()) {
            let _ = std::fs::remove_file(&part);
            bail!("the downloaded update was corrupted (checksum mismatch)");
        }
        std::fs::rename(&part, &staged)?;
        *self.staged.lock().unwrap() = Some((m.version.clone(), staged));
        Ok(Some((m.version, m.notes)))
    }

    /// Swap the staged exe in place of `target` (normally the running exe).
    pub fn install_into(&self, target: Option<&Path>) -> Result<Option<String>> {
        let Some((version, staged)) = self.staged.lock().unwrap().take() else { return Ok(None) };
        match target {
            Some(t) => {
                std::fs::copy(&staged, t)?;
            }
            None => self_replace::self_replace(&staged)?,
        }
        let _ = std::fs::remove_file(&staged);
        Ok(Some(version))
    }

    pub fn has_staged(&self) -> bool {
        self.staged.lock().unwrap().is_some()
    }

    /// Background loop: first check shortly after start, then every 6 hours.
    pub fn spawn_loop(self: &Arc<Self>, enabled: impl Fn() -> bool + Send + 'static) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("updater".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_secs(20));
                loop {
                    if enabled() {
                        me.check();
                    }
                    std::thread::sleep(Duration::from_secs(6 * 3600));
                }
            })
            .ok();
    }
}

/// A fresh process started right after an update waits for the old one to exit first
/// (single-instance would otherwise hand control back to the exiting copy).
pub fn wait_for_previous() {
    let mut args = std::env::args().skip_while(|a| a != "--after-update");
    if args.next().is_none() {
        return;
    }
    let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok()) else { return };
    let Ok(mut sys) = sysinfo::System::new() else { return };
    let spid = sysinfo::Pid::from_u32(pid);
    for _ in 0..100 {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[spid]), true);
        if sys.process(spid).is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(is_newer("v2.0.0", "1.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    /// Serves `routes` (path -> body) over HTTP on localhost.
    fn serve(routes: Vec<(String, Vec<u8>)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                }
                let mut s = s;
                match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(body);
                    }
                    None => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            }
        });
        addr
    }

    fn release(version: &str, payload: &[u8], tamper: bool) -> (Arc<Updater>, tempfile::TempDir) {
        let sha = hex(&Sha256::digest(payload));
        let served = if tamper { b"evil payload".to_vec() } else { payload.to_vec() };
        let base = serve(vec![("/Nova.exe".into(), served)]);
        let manifest = serde_json::json!({ "version": version, "notes": "Faster", "platforms": { PLATFORM: { "url": format!("{base}/Nova.exe"), "sha256": sha, "size": payload.len() } } }).to_string();
        let feed = serve(vec![("/latest.json".into(), manifest.into_bytes())]);
        let dir = tempfile::tempdir().unwrap();
        let u = Arc::new(Updater { feed: format!("{feed}/latest.json"), dir: dir.path().join("update"), state: Mutex::new(State::Idle), staged: Mutex::new(None), busy: Mutex::new(()), on_change: Box::new(|_| {}) });
        (u, dir)
    }

    #[test]
    fn downloads_verifies_and_installs() {
        let payload = vec![42u8; 700_000];
        let (u, dir) = release("99.0.0", &payload, false);
        u.check();
        assert_eq!(u.state(), State::Ready { version: "99.0.0".into(), notes: "Faster".into() });
        let target = dir.path().join("Nova.exe");
        std::fs::write(&target, b"old").unwrap();
        assert_eq!(u.install_into(Some(&target)).unwrap().as_deref(), Some("99.0.0"));
        assert_eq!(std::fs::read(&target).unwrap(), payload);
    }

    #[test]
    fn rejects_corrupted_download() {
        let (u, _d) = release("99.0.0", b"genuine", true);
        u.check();
        assert!(matches!(u.state(), State::Error { message } if message.contains("checksum")));
        assert!(!u.has_staged());
    }

    #[test]
    fn ignores_same_or_older_versions() {
        let (u, _d) = release(VERSION, b"x", false);
        u.check();
        assert!(matches!(u.state(), State::UpToDate { .. }));
        let (u2, _d2) = release("0.0.1", b"x", false);
        u2.check();
        assert!(matches!(u2.state(), State::UpToDate { .. }));
    }

    #[test]
    fn default_feed_is_this_repository() {
        assert!(DEFAULT_FEED.starts_with("https://github.com/leoble123/nova/releases/"));
    }
}
