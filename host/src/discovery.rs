//! Runtime discovery of Craft apps that this build does not know yet.
//!
//! The ArtCraft release feed (`[discovery] feed_url`, the service ArtCraft Launcher reads) lists
//! every app of the suite with its repository and latest release. An entry becomes an app here
//! when its repository is `storytold/<id>` and its latest release has a `<id>-web-<version>.zip`
//! web archive; installing it then goes through the normal update path (GitHub release lists,
//! checksums, archive and serving checks). Its launcher presentation comes from the app's page
//! on the ArtCraft site (`[discovery] metadata_url`/`<id>`): the schema.org `SoftwareApplication`
//! data for this app (name, description, logo, screenshots) and the page's `og:` title and
//! description. Only URLs on that site are used, and every download is bounded.
//!
//! Discovered apps are kept in `STATE_DIR/discovered/apps.json` (logos beside it), so they
//! survive restarts; apps from the manifest or config.toml always take precedence.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, valid_id};
use crate::fsutil::{atomic_write, atomic_write_json, read_json};
use crate::github::{fetch_public, origin_of};
use crate::timeutil::now_iso;

/// GitHub owner of every Craft app; the feed's other entries are ignored.
const OWNER: &str = "storytold";
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
    /// `storytold/<id>`.
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
    /// When the page was read; `None` until it was found (retried at every refresh).
    pub metadata_at: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    apps: Vec<Discovered>,
}

/// An entry of the feed that qualifies as a Craft app.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedApp {
    pub id: String,
    pub name: String,
}

pub fn dir(state: &Path) -> PathBuf {
    state.join("discovered")
}

/// The discovered apps saved earlier; an unreadable record is reported and ignored (the apps
/// are found again by the next refresh).
pub fn load(state: &Path) -> Vec<Discovered> {
    match read_json::<Saved>(&dir(state).join("apps.json")) {
        Ok(saved) => saved.map(|s| s.apps).unwrap_or_default(),
        Err(e) => {
            log::warn!("discovered apps: {e:#}");
            Vec::new()
        }
    }
}

fn save(state: &Path, apps: &[Discovered]) -> Result<()> {
    atomic_write_json(
        &dir(state).join("apps.json"),
        &Saved {
            apps: apps.to_vec(),
        },
    )
}

/// The feed's Craft apps: entries whose repository is `storytold/<id>` and whose latest release
/// has the web archive `<id>-web-<version>.zip`. Anything else (the launcher itself, desktop-only
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
            if !valid_id(id) || entry["repo"].as_str() != Some(&format!("{OWNER}/{id}")) {
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
/// is its GitHub repository or whose `url` is the page; the site's own object is ignored) and the
/// page's `og:` tags. URLs that are not on `origin` are dropped.
pub fn parse_page(html: &str, id: &str, page_url: &str, origin: &str) -> PageMeta {
    let repo_url = format!("https://github.com/{OWNER}/{id}");
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

/// Ask the feed for new apps and fill in missing page metadata. Returns the full list of
/// discovered apps when it changed (the caller then rebuilds its configuration), `None` when
/// nothing changed or discovery is off. Network failures leave everything as it was.
pub fn refresh(cfg: &Config) -> Result<Option<Vec<Discovered>>> {
    let Some(feed_url) = cfg.discovery.feed_url.as_deref() else {
        return Ok(None);
    };
    let timeout = cfg.updater.http_timeout_secs;
    let state = &cfg.paths.state;
    let mut apps = load(state);
    let mut changed = false;
    // Apps that became built in or were defined in config.toml are no longer "discovered". The
    // ones left out of the configuration for colliding with another app stay recorded.
    let before = apps.len();
    apps.retain(|d| !cfg.apps.iter().any(|a| a.id == d.id && !a.discovered));
    changed |= apps.len() != before;

    let feed = match fetch_public(feed_url, MAX_FEED, timeout, None) {
        Ok(Some(bytes)) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|e| anyhow::anyhow!("{feed_url}: not JSON: {e}"))?,
        Ok(None) => anyhow::bail!("{feed_url}: not found"),
        Err(e) => anyhow::bail!(e),
    };
    for app in parse_feed(&feed) {
        if cfg.apps.iter().any(|a| a.id == app.id) || apps.iter().any(|d| d.id == app.id) {
            continue;
        }
        log::info!("{}: discovered in the ArtCraft feed", app.id);
        apps.push(Discovered {
            repository: format!("{OWNER}/{}", app.id),
            id: app.id,
            name: app.name,
            discovered_at: now_iso(),
            ..Discovered::default()
        });
        changed = true;
    }

    if let Some(base) = cfg.discovery.metadata_url.as_deref() {
        let origin = origin_of(base).unwrap_or_default();
        for app in apps.iter_mut().filter(|d| d.metadata_at.is_none()) {
            let page_url = format!("{base}/{}", app.id);
            let html = match fetch_public(&page_url, MAX_PAGE, timeout, Some(&origin)) {
                Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
                Ok(None) => {
                    log::info!("{}: no page at {page_url} yet", app.id);
                    continue;
                }
                Err(e) => {
                    log::warn!("{}: {e}", app.id);
                    continue;
                }
            };
            let meta = parse_page(&html, &app.id, &page_url, &origin);
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
            match fetch_public(&logo_url, MAX_LOGO, timeout, Some(&origin)) {
                Ok(Some(bytes)) => match picture_kind(&bytes) {
                    Some(ext) => {
                        let file = format!("{}.{ext}", app.id);
                        atomic_write(&dir(state).join(&file), &bytes)?;
                        app.logo = Some(file);
                    }
                    None => log::warn!("{}: {logo_url} is not a picture", app.id),
                },
                Ok(None) => log::info!("{}: no logo at {logo_url}", app.id),
                Err(e) => {
                    // Keep the page's text; the next refresh reads the page and logo again.
                    log::warn!("{}: {e}", app.id);
                    changed = true;
                    continue;
                }
            }
            app.metadata_at = Some(now_iso());
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    save(state, &apps)?;
    Ok(Some(apps))
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
    fn only_storytold_apps_with_a_web_archive_qualify() {
        let feed = json!({"release_info": {"apps": {
            "figcraft": entry("storytold/figcraft", "0.5.1", &["figcraft-0.5.1-linux.tar.gz", "figcraft-web-0.5.1.zip"]),
            // The launcher's repository is not storytold/<id>.
            "artcraft-launcher": entry("storytold/craft-launcher", "0.4.0", &["artcraft-launcher-web-0.4.0.zip"]),
            "desktoponly": entry("storytold/desktoponly", "1.0.0", &["desktoponly-1.0.0.dmg"]),
            "stale": entry("storytold/stale", "2.0.0", &["stale-web-1.0.0.zip"]),
            "elsewhere": entry("someone/elsewhere", "1.0.0", &["elsewhere-web-1.0.0.zip"]),
            "Bad_Id": entry("storytold/Bad_Id", "1.0.0", &["Bad_Id-web-1.0.0.zip"]),
            "broken": {"repo": "storytold/broken"},
        }}});
        assert_eq!(
            parse_feed(&feed),
            [FeedApp {
                id: "figcraft".into(),
                name: "X".into()
            }]
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
            "figcraft",
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
            "deckcraft",
            "https://getartcraft.com/apps/deckcraft",
            "https://getartcraft.com",
        );
        assert_eq!((other.name.as_str(), other.logo), ("", None));
        // Logos off the site are refused.
        let elsewhere = parse_page(
            PAGE,
            "figcraft",
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
