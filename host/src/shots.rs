//! Screenshots for the launcher's app pages. The server fetches each picture named in the
//! vendored catalogue once, from `[launcher] screenshots_url` (by default the pinned tag of
//! storytold/craft-launcher-assets), and keeps it in `CACHE_DIR/shots`; visitors' browsers never
//! contact a third party. Nothing outside the catalogue is ever requested.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::fsutil::atomic_write;
use crate::timeutil::now_epoch;

/// The largest picture accepted; upstream's are 60 to 150 KB.
pub const MAX_BYTES: u64 = 2 << 20;
/// How long a failed picture is not asked for again.
const RETRY_SECS: i64 = 600;

#[derive(Default)]
pub struct Shots {
    /// `app/key` → when it may be fetched again.
    failures: Mutex<HashMap<String, i64>>,
}

impl Shots {
    /// The JPEG bytes of one of an enabled app's catalogued screenshots, or `None` when unknown,
    /// turned off or unavailable. Blocking: call from a blocking task.
    pub fn get(&self, cfg: &Config, app: &str, key: &str) -> Option<Vec<u8>> {
        if !crate::catalog::has_shot(app, key) || !cfg.enabled_apps().any(|a| a.id == app) {
            return None;
        }
        let base = cfg.launcher.screenshots_url.as_deref()?;
        let path = cache_path(cfg, base, app, key);
        if let Ok(bytes) = fs::read(&path) {
            return Some(bytes);
        }
        let id = format!("{app}/{key}");
        let now = now_epoch();
        if self.failures.lock().get(&id).is_some_and(|t| *t > now) {
            return None;
        }
        match fetch(cfg, &format!("{base}/{app}/{key}.jpg")) {
            Ok(bytes) => {
                self.failures.lock().remove(&id);
                if let Err(e) = atomic_write(&path, &bytes) {
                    log::warn!("screenshot {id}: cannot cache: {e:#}");
                }
                Some(bytes)
            }
            Err(e) => {
                log::warn!("screenshot {id}: {e}");
                self.failures.lock().insert(id, now + RETRY_SECS);
                None
            }
        }
    }
}

/// Pictures from another source URL are cached apart, so changing it never serves stale ones.
fn cache_path(cfg: &Config, base: &str, app: &str, key: &str) -> PathBuf {
    let source = crate::github::hex(&Sha256::digest(base.as_bytes())[..8]);
    cfg.paths
        .cache
        .join("shots")
        .join(source)
        .join(app)
        .join(format!("{key}.jpg"))
}

fn fetch(cfg: &Config, url: &str) -> Result<Vec<u8>, String> {
    let secs = cfg.updater.http_timeout_secs;
    let mut resp = crate::github::agent(secs, secs)
        .get(url)
        .call()
        .map_err(|e| format!("cannot fetch: {e}"))?;
    if resp.status() != 200 {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let mut bytes = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("larger than {MAX_BYTES} bytes"));
    }
    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err("not a JPEG picture".into());
    }
    Ok(bytes)
}
