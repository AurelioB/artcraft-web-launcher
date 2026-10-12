//! The Craft App Suite as ArtCraft Launcher describes it: names, taglines, brand colors, groups
//! and product pages, vendored from storytold/craft-launcher (MIT OR Apache-2.0) by
//! `scripts/sync-craft-launcher.py`. Built-in apps take their launcher presentation from here
//! unless the manifest says otherwise; which apps exist and how they install stays in the
//! manifest.

use std::sync::LazyLock;

use serde::Deserialize;

pub const CATALOG_JSON: &str = include_str!("../assets/vendor/craft-launcher/catalog.json");

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub source: Source,
    pub apps: Vec<CatalogApp>,
}

#[derive(Debug, Deserialize)]
pub struct Source {
    pub screenshots_base: String,
    pub screenshots_rev: String,
}

#[derive(Debug, Deserialize)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub group: String,
    pub color: String,
    pub ink_light: String,
    pub ink_dark: String,
    pub repository: String,
    pub former_ids: Vec<String>,
    pub website: String,
    pub details: Option<Details>,
}

#[derive(Debug, Deserialize)]
pub struct Details {
    pub shots: Vec<Shot>,
}

#[derive(Debug, Deserialize)]
pub struct Shot {
    pub key: String,
}

static CATALOG: LazyLock<Catalog> =
    LazyLock::new(|| serde_json::from_str(CATALOG_JSON).expect("vendored catalog is valid"));

pub fn catalog() -> &'static Catalog {
    &CATALOG
}

pub fn app(id: &str) -> Option<&'static CatalogApp> {
    catalog().apps.iter().find(|a| a.id == id)
}

/// Whether `key` names one of the app's upstream screenshots.
pub fn has_shot(app_id: &str, key: &str) -> bool {
    app(app_id)
        .and_then(|a| a.details.as_ref())
        .is_some_and(|d| d.shots.iter().any(|s| s.key == key))
}

/// Default `[launcher] screenshots_url`: the pinned tag of the upstream pictures repository.
pub fn default_screenshots_url() -> String {
    let s = &catalog().source;
    format!(
        "{}/{}",
        s.screenshots_base.trim_end_matches('/'),
        s.screenshots_rev
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_catalog_is_consistent() {
        let apps = &catalog().apps;
        let mut ids: Vec<_> = apps.iter().map(|a| a.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), apps.len(), "duplicate ids");
        let color = |c: &str| {
            c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
        };
        for a in apps {
            assert!(crate::config::valid_id(&a.id), "{}", a.id);
            assert!(["classic", "new"].contains(&a.group.as_str()), "{}", a.id);
            assert!(
                color(&a.color) && color(&a.ink_light) && color(&a.ink_dark),
                "{}",
                a.id
            );
            assert!(a.website.starts_with("https://"), "{}", a.id);
            for s in a.details.iter().flat_map(|d| &d.shots) {
                assert!(crate::config::valid_id(&s.key), "{}/{}", a.id, s.key);
            }
        }
        assert!(default_screenshots_url().starts_with("https://"));
    }
}
