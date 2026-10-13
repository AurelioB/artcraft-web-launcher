//! Screenshots for the launcher's app pages. The server fetches each picture once and keeps it in
//! `CACHE_DIR/shots`; visitors' browsers never contact a third party. Only two kinds of picture
//! are ever requested: those named in the vendored catalogue, from `[launcher] screenshots_url`
//! (by default the pinned tag of storytold/craft-launcher-assets), and those a discovered app's
//! page lists on the ArtCraft site (see [`crate::discovery`]).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::config::{AppConfig, Config};
use crate::discovery::picture_kind;
use crate::fsutil::atomic_write;
use crate::timeutil::now_epoch;

/// The largest picture accepted; upstream's are 60 to 150 KB.
pub const MAX_BYTES: u64 = 2 << 20;
/// How long a failed picture is not asked for again.
const RETRY_SECS: i64 = 600;

#[derive(Default)]
pub struct Shots {
    /// `app/file` → when it may be fetched again.
    failures: Mutex<HashMap<String, i64>>,
}

pub struct Picture {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

impl Shots {
    /// One screenshot of an enabled app, `file` being `<key>.<ext>`: a catalogued picture
    /// (`<key>.jpg`), or the `n`th picture of a discovered app's page (`<n>.<ext>`, from 1).
    /// `None` when unknown, turned off or unavailable. Blocking: call from a blocking task.
    pub fn get(&self, cfg: &Config, app_id: &str, file: &str) -> Option<Picture> {
        let app = cfg.enabled_apps().find(|a| a.id == app_id)?;
        let (key, ext) = file.rsplit_once('.')?;
        // Pictures named by a discovered app's page must stay on the page's site.
        let (url, path, origin) = if app.discovered {
            let base = cfg.discovery.metadata_url.as_deref()?;
            let n: usize = key.parse().ok().filter(|n| *n >= 1)?;
            let url = app.screenshots.get(n - 1)?;
            (url_ext(url)? == ext).then_some(())?;
            (
                url.clone(),
                cache_path(cfg, url, app_id, file),
                Some(crate::github::origin_of(base)?),
            )
        } else {
            if ext != "jpg" || !crate::catalog::has_shot(app_id, key) {
                return None;
            }
            let base = cfg.launcher.screenshots_url.as_deref()?;
            (
                format!("{base}/{app_id}/{key}.jpg"),
                cache_path(cfg, base, app_id, file),
                None,
            )
        };
        let content_type = content_type(ext)?;
        if let Ok(bytes) = fs::read(&path) {
            return Some(Picture {
                bytes,
                content_type,
            });
        }
        let id = format!("{app_id}/{file}");
        let now = now_epoch();
        if self.failures.lock().get(&id).is_some_and(|t| *t > now) {
            return None;
        }
        let timeout = cfg.updater.http_timeout_secs;
        let fetched = crate::github::fetch_public(&url, MAX_BYTES, timeout, origin.as_deref())
            .and_then(|b| b.ok_or_else(|| format!("{url}: not found")))
            .and_then(|b| {
                (picture_kind(&b) == Some(ext))
                    .then_some(b)
                    .ok_or_else(|| format!("{url}: not a .{ext} picture"))
            });
        match fetched {
            Ok(bytes) => {
                self.failures.lock().remove(&id);
                if let Err(e) = atomic_write(&path, &bytes) {
                    log::warn!("screenshot {id}: cannot cache: {e:#}");
                }
                Some(Picture {
                    bytes,
                    content_type,
                })
            }
            Err(e) => {
                log::warn!("screenshot {id}: {e}");
                self.failures.lock().insert(id, now + RETRY_SECS);
                None
            }
        }
    }

    /// Drop the cached screenshots of these apps and their failed fetches, so the next view
    /// fetches them again (after their page was read again).
    pub fn forget(&self, cfg: &Config, app_ids: &[String]) {
        self.failures
            .lock()
            .retain(|id, _| !app_ids.iter().any(|a| id.starts_with(&format!("{a}/"))));
        let Ok(sources) = fs::read_dir(cfg.paths.cache.join("shots")) else {
            return;
        };
        for source in sources.flatten() {
            for app in app_ids {
                let path = source.path().join(app);
                if path.is_dir()
                    && let Err(e) = fs::remove_dir_all(&path)
                {
                    log::warn!(
                        "screenshots of {app}: cannot remove {}: {e}",
                        path.display()
                    );
                }
            }
        }
    }
}

/// Launcher URLs of a discovered app's screenshots (empty for other apps).
pub fn discovered_urls(cfg: &Config, app: &AppConfig) -> Vec<String> {
    if !app.discovered || cfg.discovery.metadata_url.is_none() {
        return Vec::new();
    }
    app.screenshots
        .iter()
        .enumerate()
        .filter_map(|(i, url)| {
            Some(format!(
                "launcher/shots/{}/{}.{}",
                app.id,
                i + 1,
                url_ext(url)?
            ))
        })
        .collect()
}

/// Picture type of a URL by its file extension.
fn url_ext(url: &str) -> Option<&'static str> {
    let path = url.split(['?', '#']).next()?;
    match path.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "webp" => Some("webp"),
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        _ => None,
    }
}

pub fn content_type(ext: &str) -> Option<&'static str> {
    match ext {
        "webp" => Some("image/webp"),
        "jpg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        _ => None,
    }
}

/// Pictures from another source are cached apart, so changing it never serves stale ones.
fn cache_path(cfg: &Config, source: &str, app: &str, file: &str) -> PathBuf {
    let source = crate::github::hex(&Sha256::digest(source.as_bytes())[..8]);
    cfg.paths
        .cache
        .join("shots")
        .join(source)
        .join(app)
        .join(file)
}
