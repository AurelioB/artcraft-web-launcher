//! Runtime discovery of Craft apps that this build does not know yet.
//!
//! The ArtCraft release feed (`[discovery] feed_url`, the service ArtCraft Launcher reads) lists
//! apps with their GitHub repository and latest release. An entry becomes an app here when its
//! repository is any GitHub `owner/name` (ArtCraft's own `storytold/<id>` or a community
//! publisher's) and its latest release has a `<id>-web-<version>.zip` web archive; installing it
//! then goes through the normal update path (GitHub release lists, checksums, archive and serving
//! checks). Its launcher presentation comes from the app's page on the ArtCraft site
//! (`[discovery] metadata_url`/`<id>`): the schema.org `SoftwareApplication` data for this app
//! (name, description, logo, screenshots) and the page's `og:` title and description. Only URLs
//! on that site are used, and every download is bounded.
//!
//! Discovered apps are kept in `STATE_DIR/discovered/apps.json` (logos beside it), so they
//! survive restarts; apps from the manifest or config.toml always take precedence. The feed is
//! read once per `[discovery] interval` (the time of the last read is kept there too, so restarts
//! cost no request), each page and logo once; an administrator's refresh reads them all again.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::Result;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, valid_id, valid_repository};
use crate::fsutil::{atomic_write, atomic_write_json, read_json};
use crate::github::{fetch_public, origin_of};
use crate::timeutil::{now_epoch, now_iso, parse_iso};

/// The feed's entry for ArtCraft Launcher itself, a desktop program rather than a Craft app.
const LAUNCHER_ID: &str = "artcraft-launcher";
const MAX_FEED: u64 = 4 << 20;
const MAX_PAGE: u64 = 2 << 20;
const MAX_LOGO: u64 = 512 << 10;
const MAX_SCREENSHOTS: usize = 12;

static JSON_LD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]*type\s*=\s*["']application/ld\+json["'][^>]*>(.*?)</script>"#)
        .unwrap()
});
static META: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<meta\b[^>]*>").unwrap());
static ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)\s(property|name|content)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
});

/// A discovered app and what its page told us about it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Discovered {
    pub id: String,
    pub name: String,
    /// GitHub `owner/name`, as the feed names it.
    pub repository: String,
    /// The page's `og:description`: one line for the launcher.
    pub tagline: String,
    /// The page's `og:title`.
    pub headline: String,
    pub description: String,
    /// The app's page, once it was found.
    pub website: String,
    /// Screenshot URLs on the ArtCraft site, fetched (and cached) by the server on demand.
    pub screenshots: Vec<String>,
    /// File name of the logo in the discovery directory.
    pub logo: Option<String>,
    pub discovered_at: String,
    /// When the page was last read; `None` until it was found (retried at every feed read).
    pub metadata_at: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    /// The last successful read of the feed.
    feed_read_at: Option<String>,
    apps: Vec<Discovered>,
}

/// One refresh at a time: the scheduled round and an administrator's refresh both rewrite
/// `apps.json` and the logos.
static REFRESH: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// What a refresh reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refresh {
    /// The feed if `[discovery] interval` has passed since its last read, and the pages of apps
    /// whose page was never read.
    Scheduled,
    /// The feed now, and every discovered app's page and logo again.
    Everything,
}

/// What a refresh did.
#[derive(Debug, Default)]
pub struct Refreshed {
    /// Every discovered app, when the list or what is known about them changed (the caller
    /// then rebuilds its configuration).
    pub apps: Option<Vec<Discovered>>,
    /// Apps first seen in this read of the feed.
    pub new: Vec<String>,
    /// Apps whose page was read.
    pub pages: Vec<String>,
}

/// An entry of the feed that qualifies as a Craft app.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedApp {
    pub id: String,
    pub name: String,
    /// GitHub `owner/name`.
    pub repository: String,
}

pub fn dir(state: &Path) -> PathBuf {
    state.join("discovered")
}

/// The discovered apps saved earlier; an unreadable record is reported and ignored (the apps
/// are found again by the next refresh).
pub fn load(state: &Path) -> Vec<Discovered> {
    load_saved(state).apps
}

fn load_saved(state: &Path) -> Saved {
    match read_json::<Saved>(&dir(state).join("apps.json")) {
        Ok(saved) => saved.unwrap_or_default(),
        Err(e) => {
            log::warn!("discovered apps: {e:#}");
            Saved::default()
        }
    }
}

/// When the feed was last read successfully.
pub fn feed_read_at(state: &Path) -> Option<String> {
    load_saved(state).feed_read_at
}

/// When the feed is due again: `[discovery] interval` after its last read, at once if never.
pub fn next_due(cfg: &Config) -> i64 {
    feed_read_at(&cfg.paths.state)
        .as_deref()
        .and_then(parse_iso)
        .map_or_else(now_epoch, |t| t + cfg.discovery.interval_secs as i64)
}

/// The feed's Craft apps: entries with a GitHub `owner/name` repository whose latest release has
/// the web archive `<id>-web-<version>.zip`. Anything else (the launcher itself, desktop-only
/// apps, malformed entries) is skipped.
pub fn parse_feed(feed: &Value) -> Vec<FeedApp> {
    let Some(apps) = feed
        .pointer("/release_info/apps")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut out: Vec<FeedApp> = apps
        .iter()
        .filter_map(|(id, entry)| {
            let repository = entry["repo"].as_str()?;
            if id == LAUNCHER_ID || !valid_id(id) || !valid_repository(repository) {
                return None;
            }
            let release = &entry["latest_release"];
            let version = release["version"].as_str()?;
            let web = format!("{id}-web-{version}.zip");
            release["assets"]
                .as_array()?
                .iter()
                .any(|a| a["name"].as_str() == Some(&web))
                .then(|| FeedApp {
                    id: id.clone(),
                    name: clean(entry["name"].as_str().unwrap_or(id), 60),
                    repository: repository.to_string(),
                })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// What an app's page says about it.
#[derive(Debug, Default, PartialEq)]
pub struct PageMeta {
    pub name: String,
    pub tagline: String,
    pub headline: String,
    pub description: String,
    pub logo: Option<String>,
    pub screenshots: Vec<String>,
}

/// Read the schema.org `SoftwareApplication` object describing this app (the one whose `sameAs`
/// is its GitHub `repository` or whose `url` is the page; the site's own object is ignored) and
/// the page's `og:` tags. URLs that are not on `origin` are dropped.
pub fn parse_page(html: &str, repository: &str, page_url: &str, origin: &str) -> PageMeta {
    let repo_url = format!("https://github.com/{repository}");
    let app = JSON_LD
        .captures_iter(html)
        .filter_map(|c| serde_json::from_str::<Value>(c[1].trim()).ok())
        .flat_map(|v| match v {
            Value::Array(items) => items,
            Value::Object(ref o) if o.contains_key("@graph") => {
                o["@graph"].as_array().cloned().unwrap_or_default()
            }
            other => vec![other],
        })
        .find(|v| {
            v["@type"] == "SoftwareApplication"
                && (strings(&v["sameAs"]).any(|s| s.trim_end_matches('/') == repo_url)
                    || v["url"].as_str().map(|u| u.trim_end_matches('/'))
                        == Some(page_url.trim_end_matches('/')))
        })
        .unwrap_or_default();
    let on_site = |u: &str| {
        let u = u.trim();
        (u.starts_with(&format!("{origin}/")) && !u.contains(['"', '<', '>', ' ', '\\']))
            .then(|| u.to_string())
    };
    let (mut og_title, mut og_description) = (String::new(), String::new());
    for tag in META.find_iter(html) {
        let (mut key, mut content) = (String::new(), String::new());
        for c in ATTR.captures_iter(tag.as_str()) {
            let value = c.get(2).or(c.get(3)).map_or("", |m| m.as_str());
            match c[1].to_ascii_lowercase().as_str() {
                "content" => content = decode_entities(value),
                _ => key = value.to_ascii_lowercase(),
            }
        }
        match key.as_str() {
            "og:title" if og_title.is_empty() => og_title = content,
            "og:description" if og_description.is_empty() => og_description = content,
            _ => {}
        }
    }
    PageMeta {
        name: clean(app["name"].as_str().unwrap_or(""), 60),
        tagline: clean(&og_description, 160),
        headline: clean(&og_title, 160),
        description: clean(app["description"].as_str().unwrap_or(""), 1000),
        logo: app["thumbnailUrl"].as_str().and_then(on_site),
        screenshots: strings(&app["screenshot"])
            .filter_map(on_site)
            .take(MAX_SCREENSHOTS)
            .collect(),
    }
}

fn strings(v: &Value) -> Box<dyn Iterator<Item = &str> + '_> {
    match v {
        Value::String(s) => Box::new(std::iter::once(s.as_str())),
        Value::Array(items) => Box::new(items.iter().filter_map(Value::as_str)),
        _ => Box::new(std::iter::empty()),
    }
}

/// Single-line text of at most `max` characters, without control characters.
fn clean(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&end| end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .map_or_else(
                        || name.strip_prefix('#').and_then(|d| d.parse().ok()),
                        |h| u32::from_str_radix(h, 16).ok(),
                    )
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// File extension for a picture's bytes (WebP, PNG or JPEG), or `None` for anything else.
pub fn picture_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

/// Read the feed for new apps and the pages of discovered apps (see [`Refresh`]). `None` when
/// discovery is off or the feed is not due. A failed feed read changes nothing; a page or logo
/// that cannot be read keeps what was known before.
pub fn refresh(cfg: &Config, what: Refresh) -> Result<Option<Refreshed>> {
    let Some(feed_url) = cfg.discovery.feed_url.as_deref() else {
        return Ok(None);
    };
    let _one = REFRESH.lock();
    let state = &cfg.paths.state;
    let mut saved = load_saved(state);
    let interval = cfg.discovery.interval_secs as i64;
    if what == Refresh::Scheduled
        && saved
            .feed_read_at
            .as_deref()
            .and_then(parse_iso)
            .is_some_and(|t| now_epoch() < t + interval)
    {
        return Ok(None);
    }
    let timeout = cfg.updater.http_timeout_secs;
    let feed = match fetch_public(feed_url, MAX_FEED, timeout, None) {
        Ok(Some(bytes)) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|e| anyhow::anyhow!("{feed_url}: not JSON: {e}"))?,
        Ok(None) => anyhow::bail!("{feed_url}: not found"),
        Err(e) => anyhow::bail!(e),
    };
    saved.feed_read_at = Some(now_iso());
    let mut report = Refreshed::default();
    let apps = &mut saved.apps;
    // Apps that became built in or were defined in config.toml are no longer "discovered". The
    // ones left out of the configuration for colliding with another app stay recorded.
    let before = apps.len();
    apps.retain(|d| !cfg.apps.iter().any(|a| a.id == d.id && !a.discovered));
    let mut changed = apps.len() != before;
    for app in parse_feed(&feed) {
        // A known app keeps the repository it was discovered with: the feed cannot point an
        // installed app at another account later.
        if cfg.apps.iter().any(|a| a.id == app.id) || apps.iter().any(|d| d.id == app.id) {
            continue;
        }
        log::info!(
            "{}: discovered in the ArtCraft feed ({})",
            app.id,
            app.repository
        );
        report.new.push(app.id.clone());
        apps.push(Discovered {
            id: app.id,
            name: app.name,
            repository: app.repository,
            discovered_at: now_iso(),
            ..Discovered::default()
        });
        changed = true;
    }
    if let Some(base) = cfg.discovery.metadata_url.as_deref() {
        let origin = origin_of(base).unwrap_or_default();
        for app in apps
            .iter_mut()
            .filter(|d| what == Refresh::Everything || d.metadata_at.is_none())
        {
            if read_page(app, base, &origin, timeout, state)? {
                report.pages.push(app.id.clone());
                changed = true;
            }
        }
    }
    // Saved even when nothing changed: the time of this read schedules the next one.
    atomic_write_json(&dir(state).join("apps.json"), &saved)?;
    report.apps = changed.then_some(saved.apps);
    Ok(Some(report))
}

/// Read an app's page and logo into `app`; `false` when the page could not be read (nothing
/// changes then). A logo that fails to download keeps the previous one and leaves
/// `metadata_at` as it was, so a scheduled refresh tries again.
fn read_page(
    app: &mut Discovered,
    base: &str,
    origin: &str,
    timeout: u64,
    state: &Path,
) -> Result<bool> {
    let page_url = format!("{base}/{}", app.id);
    let html = match fetch_public(&page_url, MAX_PAGE, timeout, Some(origin)) {
        Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(None) => {
            log::info!("{}: no page at {page_url} yet", app.id);
            return Ok(false);
        }
        Err(e) => {
            log::warn!("{}: {e}", app.id);
            return Ok(false);
        }
    };
    let meta = parse_page(&html, &app.repository, &page_url, origin);
    if !meta.name.is_empty() {
        app.name = meta.name;
    }
    app.tagline = meta.tagline;
    app.headline = meta.headline;
    app.description = meta.description;
    app.screenshots = meta.screenshots;
    app.website = page_url;
    // The site's naming convention when the page names no logo.
    let logo_url = meta
        .logo
        .unwrap_or_else(|| format!("{origin}/images/apps/{}/icon.webp", app.id));
    let remove = |file: &str| {
        let _ = std::fs::remove_file(dir(state).join(file));
    };
    match fetch_public(&logo_url, MAX_LOGO, timeout, Some(origin)) {
        Ok(Some(bytes)) => match picture_kind(&bytes) {
            Some(ext) => {
                let file = format!("{}.{ext}", app.id);
                atomic_write(&dir(state).join(&file), &bytes)?;
                if let Some(old) = app.logo.replace(file.clone())
                    && old != file
                {
                    remove(&old);
                }
            }
            None => log::warn!("{}: {logo_url} is not a picture", app.id),
        },
        Ok(None) => {
            log::info!("{}: no logo at {logo_url}", app.id);
            if let Some(old) = app.logo.take() {
                remove(&old);
            }
        }
        Err(e) => {
            log::warn!("{}: {e}", app.id);
            return Ok(true);
        }
    }
    app.metadata_at = Some(now_iso());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(repo: &str, version: &str, assets: &[&str]) -> Value {
        json!({
            "name": "X",
            "repo": repo,
            "latest_release": {
                "version": version,
                "assets": assets.iter().map(|n| json!({"name": n})).collect::<Vec<_>>(),
            },
        })
    }

    #[test]
    fn apps_from_any_repository_with_a_web_archive_qualify() {
        let feed = json!({"release_info": {"apps": {
            "figcraft": entry("storytold/figcraft", "0.5.1", &["figcraft-0.5.1-linux.tar.gz", "figcraft-web-0.5.1.zip"]),
            // A community app lives under its publisher's account.
            "solidcraft": entry("tangency-labs/solidcraft", "0.26.1", &["solidcraft-0.26.1-linux-x86_64.AppImage", "solidcraft-web-0.26.1.zip"]),
            // The launcher is never an app, whatever its release carries.
            "artcraft-launcher": entry("storytold/craft-launcher", "0.10.0", &["artcraft-launcher-0.10.0-linux-x86_64.AppImage", "artcraft-launcher-web-0.10.0.zip"]),
            "desktoponly": entry("storytold/desktoponly", "1.0.0", &["desktoponly-1.0.0.dmg"]),
            "stale": entry("storytold/stale", "2.0.0", &["stale-web-1.0.0.zip"]),
            "Bad_Id": entry("storytold/Bad_Id", "1.0.0", &["Bad_Id-web-1.0.0.zip"]),
            "noowner": entry("noowner", "1.0.0", &["noowner-web-1.0.0.zip"]),
            "nested": entry("a/b/nested", "1.0.0", &["nested-web-1.0.0.zip"]),
            "spaced": entry("some one/spaced", "1.0.0", &["spaced-web-1.0.0.zip"]),
            "broken": {"repo": "storytold/broken"},
        }}});
        assert_eq!(
            parse_feed(&feed),
            [
                FeedApp {
                    id: "figcraft".into(),
                    name: "X".into(),
                    repository: "storytold/figcraft".into()
                },
                FeedApp {
                    id: "solidcraft".into(),
                    name: "X".into(),
                    repository: "tangency-labs/solidcraft".into()
                }
            ]
        );
        assert!(parse_feed(&json!({"success": false})).is_empty());
    }

    const PAGE: &str = r#"<html><head>
<meta property="og:title" content="FigCraft: Interface design, rebuilt in pure Rust."/>
<meta content="Open-source &amp; free interface design &#8212; in Rust." property="og:description"/>
<script type="application/ld+json">{"@type":"SoftwareApplication","name":"ArtCraft","description":"The site","url":"https://getartcraft.com","thumbnailUrl":"https://getartcraft.com/site.webp"}</script>
<script type="application/ld+json">{"@context":"https://schema.org","@type":"SoftwareApplication","name":"FigCraft",
 "description":"Frames, auto layout\nand components.","url":"https://getartcraft.com/apps/figcraft",
 "thumbnailUrl":"https://getartcraft.com/images/apps/figcraft/icon.webp",
 "screenshot":["https://getartcraft.com/images/apps/figcraft/hero.webp","https://evil.example/x.webp","javascript:alert(1)"],
 "sameAs":"https://github.com/storytold/figcraft"}</script>
<script type="application/ld+json">{not json</script>
</head></html>"#;

    #[test]
    fn page_metadata_comes_from_this_apps_object_and_the_site_only() {
        let meta = parse_page(
            PAGE,
            "storytold/figcraft",
            "https://getartcraft.com/apps/figcraft",
            "https://getartcraft.com",
        );
        assert_eq!(
            meta,
            PageMeta {
                name: "FigCraft".into(),
                tagline: "Open-source & free interface design \u{2014} in Rust.".into(),
                headline: "FigCraft: Interface design, rebuilt in pure Rust.".into(),
                description: "Frames, auto layout and components.".into(),
                logo: Some("https://getartcraft.com/images/apps/figcraft/icon.webp".into()),
                screenshots: vec!["https://getartcraft.com/images/apps/figcraft/hero.webp".into()],
            }
        );
        // A page without this app's data yields nothing rather than the site's.
        let other = parse_page(
            PAGE,
            "storytold/deckcraft",
            "https://getartcraft.com/apps/deckcraft",
            "https://getartcraft.com",
        );
        assert_eq!((other.name.as_str(), other.logo), ("", None));
        // Logos off the site are refused.
        let elsewhere = parse_page(
            PAGE,
            "storytold/figcraft",
            "https://getartcraft.com/apps/figcraft",
            "https://example.net",
        );
        assert_eq!((elsewhere.logo, elsewhere.screenshots.len()), (None, 0));
    }

    #[test]
    fn pictures_are_recognized_by_their_bytes() {
        assert_eq!(picture_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(picture_kind(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(picture_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(picture_kind(b"<html>"), None);
    }
}
