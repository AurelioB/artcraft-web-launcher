//! Launcher assets compiled into the binary: pages, script, styles, the official app logos, the
//! fonts used by https://getartcraft.com (Archivo and Geist Mono, SIL Open Font License) and the
//! catalogue and translations vendored from ArtCraft Launcher (MIT OR Apache-2.0).

pub struct Asset {
    pub bytes: &'static [u8],
    pub content_type: &'static str,
}

macro_rules! asset {
    ($path:literal, $ct:literal) => {
        Asset {
            bytes: include_bytes!(concat!("../assets/", $path)),
            content_type: $ct,
        }
    };
}

macro_rules! translation {
    ($code:literal) => {
        (
            concat!("i18n/", $code, ".tsv"),
            Asset {
                bytes: include_bytes!(concat!(
                    "../assets/vendor/craft-launcher/i18n/",
                    $code,
                    ".tsv"
                )),
                content_type: "text/tab-separated-values; charset=utf-8",
            },
        )
    };
}

/// `(path under /launcher/, asset)`.
const LAUNCHER: &[(&str, Asset)] = &[
    (
        "launcher.js",
        asset!("launcher.js", "text/javascript; charset=utf-8"),
    ),
    (
        "launcher-i18n.js",
        asset!("launcher-i18n.js", "text/javascript; charset=utf-8"),
    ),
    (
        "launcher.css",
        asset!("launcher.css", "text/css; charset=utf-8"),
    ),
    (
        "installing.html",
        asset!("installing.html", "text/html; charset=utf-8"),
    ),
    (
        "icons/artcraft.svg",
        asset!("icons/artcraft.svg", "image/svg+xml"),
    ),
    (
        "icons/photocraft.webp",
        asset!("icons/photocraft.webp", "image/webp"),
    ),
    (
        "icons/vectorcraft.webp",
        asset!("icons/vectorcraft.webp", "image/webp"),
    ),
    (
        "icons/filmcraft.webp",
        asset!("icons/filmcraft.webp", "image/webp"),
    ),
    (
        "icons/lightcraft.webp",
        asset!("icons/lightcraft.webp", "image/webp"),
    ),
    (
        "icons/pdfcraft.webp",
        asset!("icons/pdfcraft.webp", "image/webp"),
    ),
    (
        "icons/effectcraft.webp",
        asset!("icons/effectcraft.webp", "image/webp"),
    ),
    (
        "icons/designcraft.webp",
        asset!("icons/designcraft.webp", "image/webp"),
    ),
    (
        "icons/wordcraft.webp",
        asset!("icons/wordcraft.webp", "image/webp"),
    ),
    (
        "icons/deckcraft.webp",
        asset!("icons/deckcraft.webp", "image/webp"),
    ),
    (
        "icons/gridcraft.webp",
        asset!("icons/gridcraft.webp", "image/webp"),
    ),
    (
        "icons/cadcraft.webp",
        asset!("icons/cadcraft.webp", "image/webp"),
    ),
    (
        "icons/soundcraft.webp",
        asset!("icons/soundcraft.webp", "image/webp"),
    ),
    (
        "catalog.json",
        asset!("vendor/craft-launcher/catalog.json", "application/json"),
    ),
    (
        "i18n/languages.json",
        asset!(
            "vendor/craft-launcher/i18n/languages.json",
            "application/json"
        ),
    ),
    translation!("cs"),
    translation!("de"),
    translation!("es"),
    translation!("fr"),
    translation!("hu"),
    translation!("ja"),
    translation!("pl"),
    translation!("pt-BR"),
    translation!("ru"),
    translation!("zh-CN"),
    (
        "fonts/archivo-latin.woff2",
        asset!("fonts/archivo-latin.woff2", "font/woff2"),
    ),
    (
        "fonts/geist-mono-latin.woff2",
        asset!("fonts/geist-mono-latin.woff2", "font/woff2"),
    ),
    (
        "fonts/Archivo-OFL.txt",
        asset!("fonts/Archivo-OFL.txt", "text/plain; charset=utf-8"),
    ),
    (
        "fonts/GeistMono-OFL.txt",
        asset!("fonts/GeistMono-OFL.txt", "text/plain; charset=utf-8"),
    ),
];

pub fn launcher(path: &str) -> Option<&'static Asset> {
    LAUNCHER.iter().find(|(p, _)| *p == path).map(|(_, a)| a)
}

/// Launcher URL (relative to the site root) of the official logo for a built-in app.
pub fn logo(app_id: &str) -> Option<String> {
    let path = format!("icons/{app_id}.webp");
    launcher(&path).map(|_| format!("launcher/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_app_has_a_logo() {
        let manifest: toml::Table = toml::from_str(crate::config::MANIFEST).unwrap();
        for id in manifest["apps"].as_table().unwrap().keys() {
            let url = logo(id).unwrap_or_else(|| panic!("no logo for {id}"));
            let asset = launcher(url.strip_prefix("launcher/").unwrap()).unwrap();
            assert!(asset.bytes.starts_with(b"RIFF"), "{id} logo is WebP");
        }
        assert!(logo("examplecraft").is_none());
    }
}
