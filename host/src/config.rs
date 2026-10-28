//! Configuration: the built-in manifest, the operator's config.toml and container paths from the
//! environment. Every problem is collected so `doctor` can report them all at once.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use regex::Regex;
use serde::Deserialize;

use crate::access::{
    AdminSettings, AuthSettings, RawAdmin, RawAuth, RawServer, ServerSettings, build_admin,
    build_auth, build_server,
};
use crate::version::parse_tag;

pub const MANIFEST: &str = include_str!("manifest.toml");
pub const STAGING_DIR: &str = ".staging";
/// Paths the server answers itself; an app entry must not shadow them.
pub const RESERVED_ENTRIES: &[&str] = &["admin", "auth", "launcher", "healthz", "readyz", "status"];

#[derive(Debug)]
pub struct ConfigError(pub Vec<String>);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid configuration:")?;
        for p in &self.0 {
            write!(f, "\n  - {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub work: PathBuf,
    pub logs: Option<PathBuf>,
}

impl Paths {
    pub fn from_env() -> Self {
        let get = |k: &str, d: &str| {
            std::env::var(k)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| d.to_string())
        };
        Self {
            config_file: get("CRAFT_CONFIG", "/config/config.toml").into(),
            data: get("CRAFT_DATA_DIR", "/srv/data").into(),
            state: get("CRAFT_STATE_DIR", "/srv/state").into(),
            cache: get("CRAFT_CACHE_DIR", "/srv/cache").into(),
            work: get("CRAFT_WORK_DIR", "/srv/work").into(),
            logs: std::env::var("CRAFT_LOG_DIR")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn staging(&self) -> PathBuf {
        self.data.join(STAGING_DIR)
    }

    pub fn config_dir(&self) -> PathBuf {
        self.config_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

/// When a downloaded release becomes the one new visitors get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// As soon as it is installed. Open tabs keep their release either way.
    Immediate,
    /// After the app has received no requests for `idle_after_secs` (a heuristic: a long-open
    /// tab may make no requests).
    Idle,
}

#[derive(Debug, Clone)]
pub struct UpdaterSettings {
    pub check_on_startup: bool,
    pub check_interval_secs: u64,
    pub http_timeout_secs: u64,
    pub download_timeout_secs: u64,
    pub max_retries: u32,
    pub retry_backoff_secs: u64,
    pub retry_backoff_max_secs: u64,
    pub github_api_url: String,
    pub github_token_file: String,
    pub activation: Activation,
    pub idle_after_secs: u64,
    pub heartbeat_secs: u64,
    pub lock_wait_secs: u64,
    pub log_max_bytes: u64,
    pub log_backups: u32,
}

impl Default for UpdaterSettings {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            check_interval_secs: 3600,
            http_timeout_secs: 30,
            download_timeout_secs: 900,
            max_retries: 3,
            retry_backoff_secs: 5,
            retry_backoff_max_secs: 300,
            github_api_url: "https://api.github.com".into(),
            github_token_file: String::new(),
            activation: Activation::Immediate,
            idle_after_secs: 30 * 60,
            heartbeat_secs: 30,
            lock_wait_secs: 600,
            log_max_bytes: 10 << 20,
            log_backups: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_download_bytes: u64,
    pub max_extracted_bytes: u64,
    pub max_files: u64,
    pub max_compression_ratio: u64,
    pub min_free_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_download_bytes: 512 << 20,
            max_extracted_bytes: 1 << 30,
            max_files: 5000,
            max_compression_ratio: 200,
            min_free_bytes: 256 << 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Retention {
    pub keep_latest: u32,
    pub keep_days: u32,
    /// Releases that served requests within this window are kept (open tabs may still use them).
    pub keep_recently_used_secs: u64,
    pub cache_max_bytes: u64,
}

impl Default for Retention {
    fn default() -> Self {
        // One release per app: the active one. `keep_latest`, `keep_days` and
        // `keep_recently_used` (a grace period for tabs opened before an update) keep more.
        Self {
            keep_latest: 1,
            keep_days: 0,
            keep_recently_used_secs: 0,
            cache_max_bytes: 2 << 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Prerelease,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub artifact_patterns: Vec<String>,
    pub checksum_assets: Vec<String>,
    pub entry: String,
    pub release_dir: String,
    pub enabled: bool,
    pub auto_update: bool,
    pub pinned_version: Option<String>,
    pub channel: Channel,
    pub exclude: Vec<String>,
    pub icon: String,
    pub notes: String,
    /// Launcher metadata: short category, one-line description, brand color (#rrggbb), the
    /// suite group ("classic", "new", or empty), the product page and the colors for text on
    /// light and dark backgrounds (#rrggbb or empty). Built-in apps take them from the vendored
    /// ArtCraft Launcher catalogue.
    pub category: String,
    pub tagline: String,
    pub color: String,
    pub group: String,
    pub website: String,
    pub ink_light: String,
    pub ink_dark: String,
    /// App page text and screenshot URLs of a discovered app (built-in apps' pages come from
    /// the catalogue), and its logo file in the discovery directory.
    pub headline: String,
    pub description: String,
    pub screenshots: Vec<String>,
    pub logo: Option<String>,
    /// Found in the ArtCraft feed at run time, not defined by the manifest or config.toml.
    pub discovered: bool,
    pub keep_latest: u32,
    pub keep_days: u32,
    /// Optional separate origin (scheme://host[:port]) for this app's links, giving it browser
    /// storage of its own. The server answers on any host name.
    pub origin: Option<String>,
    pub activation: Activation,
    pub idle_after_secs: u64,
}

/// Administrators' enable/disable choices, persisted in each app's state record. Shared by every
/// clone of a [`Config`], so serving and the updater see a change at once.
#[derive(Debug, Clone, Default)]
pub struct Switches(Arc<RwLock<BTreeMap<String, bool>>>);

#[derive(Debug, Clone)]
pub struct LauncherSettings {
    /// Base URL of the upstream screenshots (`<base>/<app>/<key>.jpg`); `None` disables them.
    pub screenshots_url: Option<String>,
}

/// Where and how often new apps are discovered, see [`crate::discovery`]; a `None` URL turns
/// that part off.
#[derive(Debug, Clone)]
pub struct DiscoverySettings {
    pub feed_url: Option<String>,
    pub metadata_url: Option<String>,
    /// Time between two reads of the feed, kept across restarts.
    pub interval_secs: u64,
}

pub const DEFAULT_FEED_URL: &str = "https://api.storyteller.ai/v1/craft_apps/release_info";
pub const DEFAULT_METADATA_URL: &str = "https://getartcraft.com/apps";

/// The configuration of a running server. Discovering an app replaces it as a whole, so each
/// request and update cycle works on one consistent snapshot.
#[derive(Debug)]
pub struct LiveConfig(RwLock<Arc<Config>>);

impl LiveConfig {
    pub fn new(cfg: Config) -> Self {
        Self(RwLock::new(Arc::new(cfg)))
    }

    pub fn get(&self) -> Arc<Config> {
        self.0.read().clone()
    }

    pub fn replace(&self, cfg: Config) {
        *self.0.write() = Arc::new(cfg);
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub paths: Paths,
    pub updater: UpdaterSettings,
    pub limits: Limits,
    pub retention: Retention,
    pub server: ServerSettings,
    pub auth: AuthSettings,
    pub admin: AdminSettings,
    pub launcher: LauncherSettings,
    pub cloud: CloudSettings,
    pub discovery: DiscoverySettings,
    pub apps: Vec<AppConfig>,
    pub config_file_found: bool,
    pub switches: Switches,
}

impl Config {
    /// Whether the app is served and updated: an administrator's choice, else config.toml.
    pub fn is_enabled(&self, app: &AppConfig) -> bool {
        self.switches
            .0
            .read()
            .get(&app.id)
            .copied()
            .unwrap_or(app.enabled)
    }

    /// "admin" when an administrator's choice overrides config.toml, else "config".
    pub fn enabled_by(&self, app: &AppConfig) -> &'static str {
        if self.switches.0.read().contains_key(&app.id) {
            "admin"
        } else {
            "config"
        }
    }

    /// Record an administrator's choice in memory; `None` follows config.toml again. The caller
    /// persists it in the app's state record.
    pub fn set_switch(&self, app_id: &str, enabled: Option<bool>) {
        let mut map = self.switches.0.write();
        match enabled {
            Some(on) => map.insert(app_id.to_string(), on),
            None => map.remove(app_id),
        };
    }

    pub fn enabled_apps(&self) -> impl Iterator<Item = &AppConfig> {
        self.apps.iter().filter(|a| self.is_enabled(a))
    }

    pub fn app(&self, id: &str) -> Result<&AppConfig, ConfigError> {
        self.apps.iter().find(|a| a.id == id).ok_or_else(|| {
            let known: Vec<_> = self.apps.iter().map(|a| a.id.as_str()).collect();
            ConfigError(vec![format!(
                "unknown app {id:?}; known apps: {}",
                known.join(", ")
            )])
        })
    }

    pub fn release_root(&self, app: &AppConfig) -> PathBuf {
        self.paths.data.join(&app.release_dir)
    }

    pub fn token_path(&self) -> Option<PathBuf> {
        let name = self.updater.github_token_file.trim();
        if name.is_empty() {
            return None;
        }
        let p = PathBuf::from(name);
        Some(if p.is_absolute() {
            p
        } else {
            self.paths.config_dir().join(p)
        })
    }

    /// Read the optional GitHub token. The value is never logged or printed.
    pub fn read_token(&self) -> Result<Option<String>, ConfigError> {
        let Some(path) = self.token_path() else {
            return Ok(None);
        };
        match std::fs::read_to_string(&path) {
            Ok(t) => Ok(Some(t.trim().to_string()).filter(|t| !t.is_empty())),
            Err(e) => Err(ConfigError(vec![format!(
                "[updater] github_token_file: cannot read {} ({})",
                path.display(),
                e.kind()
            )])),
        }
    }
}

// ---- raw TOML ------------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum Quantity {
    Int(i64),
    Text(String),
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawFile {
    updater: Option<RawUpdater>,
    limits: Option<RawLimits>,
    retention: Option<RawRetention>,
    server: Option<RawServer>,
    auth: Option<RawAuth>,
    admin: Option<RawAdmin>,
    launcher: Option<RawLauncher>,
    cloud: Option<RawCloud>,
    discovery: Option<RawDiscovery>,
    #[serde(default)]
    apps: toml::Table,
}

/// Optional authenticated, per-user document storage. Never creates storage when disabled.
#[derive(Debug, Clone)]
pub struct CloudSettings {
    pub enabled: bool,
    pub max_file_bytes: u64,
    pub quota_bytes: u64,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawCloud {
    enabled: Option<bool>,
    max_file: Option<Quantity>,
    quota: Option<Quantity>,
}

fn build_cloud(raw: RawCloud, problems: &mut Vec<String>) -> CloudSettings {
    let max_file_bytes = parse_size(raw.max_file, "[cloud] max_file", 1 << 30, problems);
    let quota_bytes = parse_size(raw.quota, "[cloud] quota", 10 << 30, problems);
    if max_file_bytes == 0 || quota_bytes == 0 || max_file_bytes > quota_bytes {
        problems.push("[cloud] max_file and quota must be positive, with max_file <= quota".into());
    }
    if max_file_bytes > (1u64 << 40) || quota_bytes > (1u64 << 50) {
        problems
            .push("[cloud] max_file must not exceed 1TiB and quota must not exceed 1PiB".into());
    }
    CloudSettings {
        enabled: raw.enabled.unwrap_or(false),
        max_file_bytes,
        quota_bytes,
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawLauncher {
    screenshots_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawDiscovery {
    feed_url: Option<String>,
    metadata_url: Option<String>,
    interval: Option<Quantity>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawUpdater {
    check_on_startup: Option<bool>,
    check_interval: Option<Quantity>,
    http_timeout: Option<Quantity>,
    download_timeout: Option<Quantity>,
    max_retries: Option<u32>,
    retry_backoff: Option<Quantity>,
    retry_backoff_max: Option<Quantity>,
    github_api_url: Option<String>,
    github_token_file: Option<String>,
    activation: Option<String>,
    idle_after: Option<Quantity>,
    heartbeat_interval: Option<Quantity>,
    lock_wait: Option<Quantity>,
    log_max_size: Option<Quantity>,
    log_backups: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    max_download: Option<Quantity>,
    max_extracted: Option<Quantity>,
    max_files: Option<u64>,
    max_compression_ratio: Option<u64>,
    min_free_space: Option<Quantity>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawRetention {
    keep_latest: Option<u32>,
    keep_days: Option<u32>,
    keep_recently_used: Option<Quantity>,
    cache_max_size: Option<Quantity>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
struct RawApp {
    // operator settings
    enabled: Option<bool>,
    auto_update: Option<bool>,
    pinned_version: Option<String>,
    channel: Option<String>,
    release_dir: Option<String>,
    entry: Option<String>,
    keep_latest: Option<u32>,
    keep_days: Option<u32>,
    origin: Option<String>,
    activation: Option<String>,
    idle_after: Option<Quantity>,
    // definition (built-in manifest, or operator-defined apps)
    name: Option<String>,
    repository: Option<String>,
    artifact_patterns: Option<Vec<String>>,
    checksum_assets: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    icon: Option<String>,
    notes: Option<String>,
    category: Option<String>,
    tagline: Option<String>,
    color: Option<String>,
}

impl RawApp {
    fn defines_app(&self) -> bool {
        self.name.is_some()
            || self.repository.is_some()
            || self.artifact_patterns.is_some()
            || self.checksum_assets.is_some()
            || self.exclude.is_some()
            || self.icon.is_some()
            || self.notes.is_some()
            || self.category.is_some()
            || self.tagline.is_some()
            || self.color.is_some()
    }

    fn overlay(self, o: RawApp) -> RawApp {
        RawApp {
            enabled: o.enabled.or(self.enabled),
            auto_update: o.auto_update.or(self.auto_update),
            pinned_version: o.pinned_version.or(self.pinned_version),
            channel: o.channel.or(self.channel),
            release_dir: o.release_dir.or(self.release_dir),
            entry: o.entry.or(self.entry),
            keep_latest: o.keep_latest.or(self.keep_latest),
            keep_days: o.keep_days.or(self.keep_days),
            origin: o.origin.or(self.origin),
            activation: o.activation.or(self.activation),
            idle_after: o.idle_after.or(self.idle_after),
            name: o.name.or(self.name),
            repository: o.repository.or(self.repository),
            artifact_patterns: o.artifact_patterns.or(self.artifact_patterns),
            checksum_assets: o.checksum_assets.or(self.checksum_assets),
            exclude: o.exclude.or(self.exclude),
            icon: o.icon.or(self.icon),
            notes: o.notes.or(self.notes),
            category: o.category.or(self.category),
            tagline: o.tagline.or(self.tagline),
            color: o.color.or(self.color),
        }
    }
}

fn parse_size(q: Option<Quantity>, key: &str, default: u64, problems: &mut Vec<String>) -> u64 {
    let re = Regex::new(r"(?i)^\s*(\d+)\s*(b|kb|mb|gb|tb|kib|mib|gib|tib)?\s*$").unwrap();
    match q {
        None => default,
        Some(Quantity::Int(n)) if n >= 0 => n as u64,
        Some(Quantity::Text(s)) if re.is_match(&s) => {
            let c = re.captures(&s).unwrap();
            let n: u64 = c[1].parse().unwrap_or(u64::MAX);
            let unit = c
                .get(2)
                .map(|m| m.as_str().to_ascii_lowercase())
                .unwrap_or_default();
            let mul: u64 = match unit.as_str() {
                "kb" => 1_000,
                "mb" => 1_000_000,
                "gb" => 1_000_000_000,
                "tb" => 1_000_000_000_000,
                "kib" => 1 << 10,
                "mib" => 1 << 20,
                "gib" => 1 << 30,
                "tib" => 1 << 40,
                _ => 1,
            };
            n.saturating_mul(mul)
        }
        Some(_) => {
            problems.push(format!(
                "{key}: expected bytes or a size such as \"512MiB\""
            ));
            default
        }
    }
}

pub(crate) fn parse_duration(
    q: Option<Quantity>,
    key: &str,
    default: u64,
    problems: &mut Vec<String>,
) -> u64 {
    let re = Regex::new(r"^\s*(\d+)\s*(s|m|h|d)?\s*$").unwrap();
    match q {
        None => default,
        Some(Quantity::Int(n)) if n >= 0 => n as u64,
        Some(Quantity::Text(s)) if re.is_match(&s) => {
            let c = re.captures(&s).unwrap();
            let n: u64 = c[1].parse().unwrap_or(u64::MAX);
            let mul = match c.get(2).map(|m| m.as_str()) {
                Some("m") => 60,
                Some("h") => 3600,
                Some("d") => 86_400,
                _ => 1,
            };
            n.saturating_mul(mul)
        }
        Some(_) => {
            problems.push(format!(
                "{key}: expected seconds or a duration such as \"1h\""
            ));
            default
        }
    }
}

pub fn valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// `http(s)://host[:port]` with DNS-style labels (or IPv4) and a port in 1..=65535.
/// IPv6 literals are not accepted.
fn valid_origin(o: &str) -> bool {
    let Some(rest) = o
        .strip_prefix("https://")
        .or_else(|| o.strip_prefix("http://"))
    else {
        return false;
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (rest, None),
    };
    let port_ok = port.is_none_or(|p| {
        p.parse::<u16>().is_ok_and(|n| n > 0) && p.bytes().all(|c| c.is_ascii_digit())
    });
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    port_ok && !host.is_empty() && host.len() <= 253 && host.split('.').all(label_ok)
}

fn valid_dir_component(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"._- ".contains(c))
}

fn relative_components(rel: &str) -> Option<Vec<String>> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    let parts: Vec<String> = Path::new(rel)
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (!parts.is_empty()).then_some(parts)
}

fn parse_activation(
    text: Option<&str>,
    key: &str,
    default: Activation,
    problems: &mut Vec<String>,
) -> Activation {
    match text {
        None => default,
        Some("immediate") => Activation::Immediate,
        Some("idle") => Activation::Idle,
        Some(other) => {
            problems.push(format!(
                "{key}: expected \"immediate\" or \"idle\", got {other:?}"
            ));
            default
        }
    }
}

fn build_app(
    id: &str,
    raw: RawApp,
    retention: &Retention,
    updater: &UpdaterSettings,
    problems: &mut Vec<String>,
) -> Option<AppConfig> {
    let w = |k: &str| format!("[apps.{id}] {k}");
    let cat = crate::catalog::app(id);
    let Some(name) = raw.name.or_else(|| cat.map(|c| c.name.clone())) else {
        problems.push(format!(
            "{}: required for apps that are not in the built-in manifest or the Craft App catalogue",
            w("name")
        ));
        return None;
    };
    // The suite's convention: repository `storytold/<id>`, web archive `<id>-web-<version>.zip`,
    // and the same for every former name of the app.
    let repository = raw
        .repository
        .unwrap_or_else(|| cat.map_or_else(|| format!("storytold/{id}"), |c| c.repository.clone()));
    let repo_re = Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").unwrap();
    if !repo_re.is_match(&repository) {
        problems.push(format!(
            "{}: expected \"owner/name\", got {repository:?}",
            w("repository")
        ));
    }
    let entry = raw.entry.unwrap_or_else(|| id.to_string());
    if !valid_id(&entry) || RESERVED_ENTRIES.contains(&entry.as_str()) {
        problems.push(format!(
            "{}: {entry:?} must be lowercase letters, digits and '-', and not one of {}",
            w("entry"),
            RESERVED_ENTRIES.join(", ")
        ));
    }
    let channel = match raw.channel.as_deref().unwrap_or("stable") {
        "stable" => Channel::Stable,
        "prerelease" => Channel::Prerelease,
        other => {
            problems.push(format!(
                "{}: expected \"stable\" or \"prerelease\", got {other:?}",
                w("channel")
            ));
            Channel::Stable
        }
    };
    let pinned_version = raw
        .pinned_version
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if let Some(p) = &pinned_version
        && parse_tag(p).is_none()
    {
        problems.push(format!(
            "{}: {p:?} is not a release version such as 0.5.0",
            w("pinned_version")
        ));
    }
    let release_dir = raw.release_dir.unwrap_or_else(|| format!("releases/{id}"));
    if !matches!(relative_components(&release_dir), Some(parts) if parts.iter().all(|p| valid_dir_component(p)))
    {
        problems.push(format!(
            "{}: {release_dir:?} must be a relative path inside DATA_DIR whose components use letters, digits, '.', '_', '-' or spaces and do not start with '.'",
            w("release_dir")
        ));
    }
    let release_dir = relative_components(&release_dir)
        .map(|p| p.join("/"))
        .unwrap_or(release_dir);
    let patterns = raw.artifact_patterns.unwrap_or_else(|| {
        std::iter::once(id)
            .chain(
                cat.iter()
                    .flat_map(|c| c.former_ids.iter().map(String::as_str)),
            )
            .map(|stem| format!("{stem}-web-{{version}}.zip"))
            .collect()
    });
    if patterns.is_empty() {
        problems.push(format!("{}: must not be empty", w("artifact_patterns")));
    }
    for p in &patterns {
        if p.matches("{version}").count() != 1 || p.contains('/') {
            problems.push(format!(
                "{}: {p:?} must be a file name containing {{version}} exactly once",
                w("artifact_patterns")
            ));
        } else if !p.ends_with(".zip") {
            problems.push(format!(
                "{}: {p:?}: only .zip web archives are supported",
                w("artifact_patterns")
            ));
        }
    }
    let exclude = raw.exclude.unwrap_or_default();
    for p in &exclude {
        if relative_components(p.trim_end_matches("/**")).is_none() && !p.starts_with('*') {
            problems.push(format!(
                "{}: {p:?} must be a relative pattern without '..'",
                w("exclude")
            ));
        } else if ["*", "**", "**/*"].contains(&p.as_str()) || p.starts_with("index.html") {
            problems.push(format!(
                "{}: {p:?} would exclude the entry page",
                w("exclude")
            ));
        }
    }
    let icon = raw.icon.unwrap_or_default();
    if !icon.is_empty() && relative_components(&icon).is_none() {
        problems.push(format!(
            "{}: must be a relative path inside the release",
            w("icon")
        ));
    }
    let color = raw.color.unwrap_or_default();
    if !color.is_empty()
        && !(color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|c| c.is_ascii_hexdigit()))
    {
        problems.push(format!("{}: {color:?} must be a #rrggbb color", w("color")));
    }
    let origin = raw
        .origin
        .map(|o| o.trim().trim_end_matches('/').to_string())
        .filter(|o| !o.is_empty());
    if let Some(o) = &origin
        && !valid_origin(o)
    {
        problems.push(format!(
                "{}: {o:?} must be http(s)://host[:port] without a path, e.g. \"https://lightcraft.example.net\"",
                w("origin")
            ));
    }
    let activation = parse_activation(
        raw.activation.as_deref(),
        &w("activation"),
        updater.activation,
        problems,
    );
    let idle_after_secs = parse_duration(
        raw.idle_after,
        &w("idle_after"),
        updater.idle_after_secs,
        problems,
    );
    let presentation =
        |own: Option<String>, upstream: fn(&crate::catalog::CatalogApp) -> &String| {
            own.or_else(|| cat.map(|c| upstream(c).clone()))
                .unwrap_or_default()
        };
    Some(AppConfig {
        id: id.to_string(),
        name,
        repository,
        artifact_patterns: patterns,
        checksum_assets: raw
            .checksum_assets
            .unwrap_or_else(|| vec!["SHA256SUMS.txt".into()]),
        entry,
        release_dir,
        enabled: raw.enabled.unwrap_or(true),
        auto_update: raw.auto_update.unwrap_or(true),
        pinned_version,
        channel,
        exclude,
        icon,
        notes: raw.notes.unwrap_or_default(),
        category: raw.category.unwrap_or_default(),
        tagline: presentation(raw.tagline, |c| &c.tagline),
        color: if color.is_empty() {
            presentation(None, |c| &c.color)
        } else {
            color
        },
        group: presentation(None, |c| &c.group),
        website: presentation(None, |c| &c.website),
        ink_light: presentation(None, |c| &c.ink_light),
        ink_dark: presentation(None, |c| &c.ink_dark),
        headline: String::new(),
        description: String::new(),
        screenshots: Vec::new(),
        logo: None,
        discovered: false,
        keep_latest: raw.keep_latest.unwrap_or(retention.keep_latest),
        keep_days: raw.keep_days.unwrap_or(retention.keep_days),
        origin,
        activation,
        idle_after_secs,
    })
}

fn check_overlaps(apps: &[AppConfig], problems: &mut Vec<String>) {
    let mut entries: BTreeMap<&str, &str> = BTreeMap::new();
    for a in apps {
        if let Some(other) = entries.insert(&a.entry, &a.id) {
            problems.push(format!(
                "[apps.{}] entry: {:?} is already used by {other}",
                a.id, a.entry
            ));
        }
    }
    for (i, a) in apps.iter().enumerate() {
        for b in &apps[i + 1..] {
            let (pa, pb): (Vec<_>, Vec<_>) = (
                a.release_dir.split('/').collect(),
                b.release_dir.split('/').collect(),
            );
            let n = pa.len().min(pb.len());
            if pa[..n] == pb[..n] {
                problems.push(format!(
                    "[apps.{}] and [apps.{}] release_dir overlap ({:?} vs {:?}); each app needs its own directory",
                    a.id, b.id, a.release_dir, b.release_dir
                ));
            }
        }
    }
}

fn check_roots(paths: &Paths, problems: &mut Vec<String>) {
    let mut roots: Vec<(&str, PathBuf)> = vec![
        ("DATA_DIR", paths.data.clone()),
        ("STATE_DIR", paths.state.clone()),
        ("CACHE_DIR", paths.cache.clone()),
        ("WORK_DIR", paths.work.clone()),
        ("CONFIG_DIR", paths.config_dir()),
    ];
    if let Some(l) = &paths.logs {
        roots.push(("LOG_DIR", l.clone()));
    }
    for (name, p) in &roots {
        if !p.is_absolute() {
            problems.push(format!(
                "{name}: container path {} must be absolute",
                p.display()
            ));
        }
    }
    for (i, (an, a)) in roots.iter().enumerate() {
        for (bn, b) in &roots[i + 1..] {
            // Logs kept inside the state directory are harmless.
            if *bn == "LOG_DIR" && *an == "STATE_DIR" && b.starts_with(a) && a != b {
                continue;
            }
            if a.starts_with(b) || b.starts_with(a) {
                problems.push(format!(
                    "{an} ({}) and {bn} ({}) overlap; they must be separate directories",
                    a.display(),
                    b.display()
                ));
            }
        }
    }
}

fn parse_apps_table(
    table: toml::Table,
    source: &str,
    problems: &mut Vec<String>,
) -> Vec<(String, RawApp)> {
    let mut out = Vec::new();
    for (id, value) in table {
        match value.try_into::<RawApp>() {
            Ok(raw) => out.push((id, raw)),
            Err(e) => problems.push(format!("{source} [apps.{id}]: {}", e.message())),
        }
    }
    out
}

/// Settings from earlier versions; name them in the error so an older config.toml is easy to
/// update (it is rejected either way, never read as "no sign-in").
fn moved_admin_keys(text: &str) -> Vec<String> {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return vec![];
    };
    let keys = |section: &str| -> Vec<String> {
        table
            .get(section)
            .and_then(|a| a.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    };
    let mut hints = Vec::new();
    let moved = [
        (
            "admin",
            "auth",
            "[admin] auth: replaced by [auth] methods (\"form\" becomes [\"local\"]) plus [admin] enabled = true",
        ),
        (
            "admin",
            "users_file",
            "[admin] users_file, session_ttl, cookie_secure, oidc and proxy: moved to [auth] (and [auth.oidc], [auth.proxy]); OIDC redirect_url now ends in /auth/oidc/callback",
        ),
        (
            "admin",
            "session_ttl",
            "[admin] users_file, session_ttl, cookie_secure, oidc and proxy: moved to [auth] (and [auth.oidc], [auth.proxy]); OIDC redirect_url now ends in /auth/oidc/callback",
        ),
        (
            "admin",
            "cookie_secure",
            "[admin] users_file, session_ttl, cookie_secure, oidc and proxy: moved to [auth] (and [auth.oidc], [auth.proxy]); OIDC redirect_url now ends in /auth/oidc/callback",
        ),
        (
            "admin",
            "oidc",
            "[admin] users_file, session_ttl, cookie_secure, oidc and proxy: moved to [auth] (and [auth.oidc], [auth.proxy]); OIDC redirect_url now ends in /auth/oidc/callback",
        ),
        (
            "admin",
            "proxy",
            "[admin] users_file, session_ttl, cookie_secure, oidc and proxy: moved to [auth] (and [auth.oidc], [auth.proxy]); OIDC redirect_url now ends in /auth/oidc/callback",
        ),
        (
            "admin",
            "allowed_users",
            "[admin] allowed_users/allowed_groups: replaced by [auth] admin_users/admin_groups",
        ),
        (
            "admin",
            "allowed_groups",
            "[admin] allowed_users/allowed_groups: replaced by [auth] admin_users/admin_groups",
        ),
        (
            "admin",
            "listen",
            "[admin] listen/shared_origin: removed; /admin is served on the apps' port (optionally only for [admin] host)",
        ),
        (
            "admin",
            "shared_origin",
            "[admin] listen/shared_origin: removed; /admin is served on the apps' port (optionally only for [admin] host)",
        ),
        (
            "auth",
            "method",
            "[auth] method: replaced by [auth] methods, a list: \"form\" becomes [\"local\"], \"oidc\" [\"oidc\"], \"basic\" [\"basic\"], \"proxy\" [\"proxy\"], \"none\" []",
        ),
        (
            "auth",
            "users_file",
            "[auth] users_file: removed; accounts live in STATE_DIR/users.sqlite3. Import the file with `craft-host user import FILE`, or add accounts with `craft-host user add`",
        ),
    ];
    for (section, key, hint) in moved {
        if keys(section).iter().any(|k| k == key) && !hints.iter().any(|h: &String| h == hint) {
            hints.push(hint.to_string());
        }
    }
    hints
}

pub fn load_config(paths: Paths) -> Result<Config, ConfigError> {
    let mut problems = Vec::new();
    let manifest: RawFile = toml::from_str(MANIFEST).expect("built-in manifest is valid");
    let manifest_apps = parse_apps_table(manifest.apps, "manifest", &mut problems);

    let (raw, found) = match std::fs::read_to_string(&paths.config_file) {
        Ok(text) => match toml::from_str::<RawFile>(&text) {
            Ok(raw) => (raw, true),
            Err(e) => {
                let mut problems =
                    vec![format!("{}: {}", paths.config_file.display(), e.message())];
                problems.extend(moved_admin_keys(&text));
                return Err(ConfigError(problems));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (RawFile::default(), false),
        Err(e) => {
            return Err(ConfigError(vec![format!(
                "{}: cannot read ({})",
                paths.config_file.display(),
                e.kind()
            )]));
        }
    };

    let d = UpdaterSettings::default();
    let u = raw.updater.unwrap_or_default();
    let updater = UpdaterSettings {
        check_on_startup: u.check_on_startup.unwrap_or(d.check_on_startup),
        check_interval_secs: parse_duration(
            u.check_interval,
            "[updater] check_interval",
            d.check_interval_secs,
            &mut problems,
        ),
        http_timeout_secs: parse_duration(
            u.http_timeout,
            "[updater] http_timeout",
            d.http_timeout_secs,
            &mut problems,
        ),
        download_timeout_secs: parse_duration(
            u.download_timeout,
            "[updater] download_timeout",
            d.download_timeout_secs,
            &mut problems,
        ),
        max_retries: u.max_retries.unwrap_or(d.max_retries),
        retry_backoff_secs: parse_duration(
            u.retry_backoff,
            "[updater] retry_backoff",
            d.retry_backoff_secs,
            &mut problems,
        ),
        retry_backoff_max_secs: parse_duration(
            u.retry_backoff_max,
            "[updater] retry_backoff_max",
            d.retry_backoff_max_secs,
            &mut problems,
        ),
        github_api_url: u.github_api_url.unwrap_or(d.github_api_url),
        github_token_file: u.github_token_file.unwrap_or(d.github_token_file),
        activation: parse_activation(
            u.activation.as_deref(),
            "[updater] activation",
            d.activation,
            &mut problems,
        ),
        idle_after_secs: parse_duration(
            u.idle_after,
            "[updater] idle_after",
            d.idle_after_secs,
            &mut problems,
        ),
        heartbeat_secs: parse_duration(
            u.heartbeat_interval,
            "[updater] heartbeat_interval",
            d.heartbeat_secs,
            &mut problems,
        ),
        lock_wait_secs: parse_duration(
            u.lock_wait,
            "[updater] lock_wait",
            d.lock_wait_secs,
            &mut problems,
        ),
        log_max_bytes: parse_size(
            u.log_max_size,
            "[updater] log_max_size",
            d.log_max_bytes,
            &mut problems,
        ),
        log_backups: u.log_backups.unwrap_or(d.log_backups),
    };
    let dl = Limits::default();
    let l = raw.limits.unwrap_or_default();
    let limits = Limits {
        max_download_bytes: parse_size(
            l.max_download,
            "[limits] max_download",
            dl.max_download_bytes,
            &mut problems,
        ),
        max_extracted_bytes: parse_size(
            l.max_extracted,
            "[limits] max_extracted",
            dl.max_extracted_bytes,
            &mut problems,
        ),
        max_files: l.max_files.unwrap_or(dl.max_files),
        max_compression_ratio: l.max_compression_ratio.unwrap_or(dl.max_compression_ratio),
        min_free_bytes: parse_size(
            l.min_free_space,
            "[limits] min_free_space",
            dl.min_free_bytes,
            &mut problems,
        ),
    };
    let dr = Retention::default();
    let r = raw.retention.unwrap_or_default();
    let retention = Retention {
        keep_latest: r.keep_latest.unwrap_or(dr.keep_latest),
        keep_days: r.keep_days.unwrap_or(dr.keep_days),
        keep_recently_used_secs: parse_duration(
            r.keep_recently_used,
            "[retention] keep_recently_used",
            dr.keep_recently_used_secs,
            &mut problems,
        ),
        cache_max_bytes: parse_size(
            r.cache_max_size,
            "[retention] cache_max_size",
            dr.cache_max_bytes,
            &mut problems,
        ),
    };

    if updater.check_interval_secs < 60 {
        problems.push(
            "[updater] check_interval: must be at least 60 seconds to respect GitHub rate limits"
                .into(),
        );
    }
    if updater.max_retries > 10 {
        problems.push("[updater] max_retries: at most 10".into());
    }
    if updater.http_timeout_secs == 0
        || updater.download_timeout_secs == 0
        || updater.heartbeat_secs == 0
    {
        problems.push("[updater] timeouts and heartbeat_interval must be greater than zero".into());
    }
    if !updater.github_api_url.starts_with("https://")
        && !updater.github_api_url.starts_with("http://")
    {
        problems.push("[updater] github_api_url: must be an http(s) URL".into());
    }
    if limits.max_files == 0 || limits.max_compression_ratio == 0 {
        problems
            .push("[limits] max_files and max_compression_ratio must be greater than zero".into());
    }
    if retention.keep_latest == 0 {
        problems.push("[retention] keep_latest: must be at least 1".into());
    }

    let overrides = parse_apps_table(raw.apps, "config", &mut problems);
    let mut apps = Vec::new();
    let mut seen = Vec::new();
    for (id, base) in &manifest_apps {
        let over = overrides
            .iter()
            .find(|(o, _)| o == id)
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        if over.defines_app() {
            problems.push(format!(
                "[apps.{id}]: built-in apps accept only enabled, auto_update, pinned_version, channel, release_dir, entry, origin, activation, idle_after, keep_latest and keep_days"
            ));
        }
        seen.push(id.clone());
        if let Some(app) = build_app(
            id,
            base.clone().overlay(over),
            &retention,
            &updater,
            &mut problems,
        ) {
            apps.push(app);
        }
    }
    for (id, raw_app) in overrides {
        if seen.contains(&id) {
            continue;
        }
        if !valid_id(&id) {
            problems.push(format!(
                "[apps.{id}]: app ids must be lowercase letters, digits and '-'"
            ));
            continue;
        }
        if let Some(app) = build_app(&id, raw_app, &retention, &updater, &mut problems) {
            apps.push(app);
        }
    }
    add_discovered(
        &mut apps,
        &crate::discovery::load(&paths.state),
        &retention,
        &updater,
    );
    check_overlaps(&apps, &mut problems);
    check_roots(&paths, &mut problems);
    let server = build_server(raw.server.unwrap_or_default(), &mut problems);
    let admin = build_admin(raw.admin.unwrap_or_default(), &mut problems);
    let auth = build_auth(
        raw.auth.unwrap_or_default(),
        &paths.config_dir(),
        &server,
        &admin,
        &mut problems,
    );
    let cloud = build_cloud(raw.cloud.unwrap_or_default(), &mut problems);
    if cloud.enabled
        && (matches!(auth.sign_in, crate::access::SignIn::None)
            || !matches!(auth.apps, crate::access::AppsAccess::SignedIn))
    {
        problems.push(
            "[cloud] enabled requires an [auth] sign-in method and apps = \"signed-in\"".into(),
        );
    }
    let launcher = build_launcher(raw.launcher.unwrap_or_default(), &mut problems);
    let discovery = build_discovery(raw.discovery.unwrap_or_default(), &mut problems);
    if !problems.is_empty() {
        return Err(ConfigError(problems));
    }
    let switches = Switches::default();
    {
        // An unreadable record shows up in status; its app follows config.toml meanwhile.
        let store = crate::store::Store::new(&paths.state);
        let mut map = switches.0.write();
        for app in &apps {
            if let Ok(crate::store::AppState {
                enabled: Some(on), ..
            }) = store.load(&app.id)
                && on != app.enabled
            {
                map.insert(app.id.clone(), on);
            }
        }
    }
    Ok(Config {
        paths,
        updater,
        limits,
        retention,
        server,
        auth,
        admin,
        launcher,
        cloud,
        discovery,
        apps,
        config_file_found: found,
        switches,
    })
}

fn build_launcher(raw: RawLauncher, problems: &mut Vec<String>) -> LauncherSettings {
    let url = raw
        .screenshots_url
        .unwrap_or_else(crate::catalog::default_screenshots_url)
        .trim()
        .trim_end_matches('/')
        .to_string();
    if !url.is_empty() && !url.starts_with("https://") && !url.starts_with("http://") {
        problems.push(format!(
            "[launcher] screenshots_url: {url:?} must be an http(s) URL, or \"\" to turn screenshots off"
        ));
    }
    LauncherSettings {
        screenshots_url: (!url.is_empty()).then_some(url),
    }
}

/// Add discovered apps that the manifest and config.toml do not define. One that cannot be
/// served beside the others (its URL path or release directory is taken) is reported and left
/// out; discovery never makes a configuration invalid.
fn add_discovered(
    apps: &mut Vec<AppConfig>,
    found: &[crate::discovery::Discovered],
    retention: &Retention,
    updater: &UpdaterSettings,
) {
    for d in found {
        if apps.iter().any(|a| a.id == d.id) {
            continue;
        }
        let raw = RawApp {
            name: Some(if d.name.is_empty() { &d.id } else { &d.name }.clone()),
            repository: Some(d.repository.clone()),
            tagline: (!d.tagline.is_empty()).then(|| d.tagline.clone()),
            ..RawApp::default()
        };
        let mut problems = Vec::new();
        if let Some(mut app) = build_app(&d.id, raw, retention, updater, &mut problems) {
            if !d.website.is_empty() {
                app.website = d.website.clone();
            }
            app.headline = d.headline.clone();
            app.description = d.description.clone();
            app.screenshots = d.screenshots.clone();
            app.logo = d.logo.clone();
            app.discovered = true;
            let mut all = apps.clone();
            all.push(app.clone());
            check_overlaps(&all, &mut problems);
            if problems.is_empty() {
                apps.push(app);
                continue;
            }
        }
        log::warn!("discovered app {} left out: {}", d.id, problems.join("; "));
    }
}

impl Config {
    /// This configuration with `found` as its discovered apps, replacing earlier ones.
    pub fn with_discovered(&self, found: &[crate::discovery::Discovered]) -> Config {
        let mut cfg = self.clone();
        cfg.apps.retain(|a| !a.discovered);
        add_discovered(&mut cfg.apps, found, &cfg.retention, &cfg.updater);
        cfg
    }
}

fn optional_url(value: String, key: &str, problems: &mut Vec<String>) -> Option<String> {
    let url = value.trim().trim_end_matches('/').to_string();
    if !url.is_empty() && !url.starts_with("https://") && !url.starts_with("http://") {
        problems.push(format!(
            "{key}: {url:?} must be an http(s) URL, or \"\" to turn it off"
        ));
    }
    (!url.is_empty()).then_some(url)
}

/// New apps appear a few times a month: one feed read a day is plenty and spares the source.
const DEFAULT_DISCOVERY_INTERVAL: u64 = 86_400;
const MIN_DISCOVERY_INTERVAL: u64 = 3600;

fn build_discovery(raw: RawDiscovery, problems: &mut Vec<String>) -> DiscoverySettings {
    let interval_secs = parse_duration(
        raw.interval,
        "[discovery] interval",
        DEFAULT_DISCOVERY_INTERVAL,
        problems,
    );
    if interval_secs < MIN_DISCOVERY_INTERVAL {
        problems.push("[discovery] interval: must be at least 1h".into());
    }
    DiscoverySettings {
        feed_url: optional_url(
            raw.feed_url.unwrap_or_else(|| DEFAULT_FEED_URL.into()),
            "[discovery] feed_url",
            problems,
        ),
        metadata_url: optional_url(
            raw.metadata_url
                .unwrap_or_else(|| DEFAULT_METADATA_URL.into()),
            "[discovery] metadata_url",
            problems,
        ),
        interval_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_with(config: &Path) -> Paths {
        Paths {
            config_file: config.to_path_buf(),
            data: "/srv/data".into(),
            state: "/srv/state".into(),
            cache: "/srv/cache".into(),
            work: "/srv/work".into(),
            logs: None,
        }
    }

    fn load(text: &str) -> Result<Config, ConfigError> {
        load_in(tempfile::tempdir().unwrap().path(), text)
    }

    fn load_in(dir: &Path, text: &str) -> Result<Config, ConfigError> {
        let mut p = paths_with(&dir.join("config/config.toml"));
        p.data = dir.join("data");
        p.state = dir.join("state");
        p.cache = dir.join("cache");
        p.work = dir.join("work");
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(&p.config_file, text).unwrap();
        load_config(p)
    }

    #[test]
    fn document_storage_is_off_by_default_and_requires_sign_in_when_enabled() {
        assert!(!load("").unwrap().cloud.enabled);
        assert!(!load("[cloud]\nenabled = false\n").unwrap().cloud.enabled);
        assert!(load("[cloud]\nenabled = true\n").is_err());
        let cfg = load("[auth]\nmethods = [\"local\"]\napps = \"signed-in\"\n[cloud]\nenabled = true\nmax_file = \"8MiB\"\nquota = \"32MiB\"\n").unwrap();
        assert!(cfg.cloud.enabled);
        assert_eq!(cfg.cloud.max_file_bytes, 8 << 20);
        assert_eq!(cfg.cloud.quota_bytes, 32 << 20);
        assert!(load("[cloud]\nmax_file = 0\n").is_err());
        assert!(load("[cloud]\nmax_file = \"20GiB\"\nquota = \"10GiB\"\n").is_err());
        assert!(load("[cloud]\nunknown_option = true\n").is_err());
    }

    #[test]
    fn built_in_apps_follow_the_suite_conventions_and_catalogue() {
        let cfg = load("").unwrap();
        let ids: Vec<_> = cfg.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "photocraft",
                "vectorcraft",
                "filmcraft",
                "lightcraft",
                "pdfcraft",
                "effectcraft",
                "designcraft",
                "wordcraft",
                "deckcraft",
                "gridcraft",
                "cadcraft",
                "soundcraft",
                "figcraft"
            ]
        );
        assert_eq!(cfg.enabled_apps().count(), 13);
        let pdf = cfg.app("pdfcraft").unwrap();
        assert_eq!(pdf.repository, "storytold/pdfcraft");
        assert_eq!(
            pdf.artifact_patterns,
            ["pdfcraft-web-{version}.zip", "printcraft-web-{version}.zip"]
        );
        assert_eq!(pdf.release_dir, "releases/pdfcraft");
        assert_eq!(pdf.tagline, "Reading, organizing and protecting PDFs");
        assert_eq!(
            (
                pdf.group.as_str(),
                pdf.color.as_str(),
                pdf.ink_dark.as_str()
            ),
            ("classic", "#12a58a", "#3fd6b8")
        );
        let word = cfg.app("wordcraft").unwrap();
        assert_eq!(
            (
                word.name.as_str(),
                word.repository.as_str(),
                word.group.as_str()
            ),
            ("WordCraft", "storytold/wordcraft", "new")
        );
        assert_eq!(word.artifact_patterns, ["wordcraft-web-{version}.zip"]);
        assert_eq!(word.website, "https://getartcraft.com/apps/wordcraft");
        assert_eq!(cfg.updater.check_interval_secs, 3600);
    }

    #[test]
    fn admin_choices_override_config_until_they_agree_again() {
        let dir = tempfile::tempdir().unwrap();
        let state = |app: &str, json: &str| {
            std::fs::create_dir_all(dir.path().join("state/apps")).unwrap();
            std::fs::write(dir.path().join(format!("state/apps/{app}.json")), json).unwrap();
        };
        state("cadcraft", r#"{"enabled": false}"#);
        state("lightcraft", r#"{"enabled": true}"#);
        // A choice equal to config.toml is no override.
        state("filmcraft", r#"{"enabled": true}"#);
        let cfg = load_in(dir.path(), "[apps.lightcraft]\nenabled = false\n").unwrap();
        let at = |id: &str| {
            let a = cfg.app(id).unwrap();
            (cfg.is_enabled(a), cfg.enabled_by(a))
        };
        assert_eq!(at("cadcraft"), (false, "admin"));
        assert_eq!(at("lightcraft"), (true, "admin"));
        assert_eq!(at("filmcraft"), (true, "config"));
        assert!(!cfg.enabled_apps().any(|a| a.id == "cadcraft"));
        // A change is visible to every clone (server and updater share it) at once.
        let shared = cfg.clone();
        cfg.set_switch("cadcraft", None);
        assert!(shared.is_enabled(shared.app("cadcraft").unwrap()));
    }

    #[test]
    fn operator_overrides_and_units_apply() {
        let cfg = load(
            r#"
[updater]
check_interval = "2h"
[limits]
max_download = "64MiB"
[retention]
keep_latest = 5
[apps.photocraft]
enabled = false
release_dir = "my releases/photo"
pinned_version = "0.3.0"
"#,
        )
        .unwrap();
        assert_eq!(cfg.updater.check_interval_secs, 7200);
        assert_eq!(cfg.limits.max_download_bytes, 64 << 20);
        let p = cfg.app("photocraft").unwrap();
        assert!(!p.enabled);
        assert_eq!(p.release_dir, "my releases/photo");
        assert_eq!(p.pinned_version.as_deref(), Some("0.3.0"));
        assert_eq!(cfg.app("vectorcraft").unwrap().keep_latest, 5);
    }

    #[test]
    fn escaping_overlapping_and_reserved_settings_are_rejected() {
        let err = load(
            r#"
[apps.photocraft]
release_dir = "../outside"
[apps.vectorcraft]
release_dir = ".staging/vc"
[apps.filmcraft]
release_dir = "shared"
[apps.lightcraft]
release_dir = "shared/light"
entry = "healthz"
[apps.pdfcraft]
entry = "effectcraft"
"#,
        )
        .unwrap_err();
        let all = err.0.join("\n");
        assert!(all.contains("[apps.photocraft] release_dir"), "{all}");
        assert!(all.contains("[apps.vectorcraft] release_dir"), "{all}");
        assert!(
            all.contains("[apps.filmcraft] and [apps.lightcraft] release_dir overlap"),
            "{all}"
        );
        assert!(all.contains("[apps.lightcraft] entry"), "{all}");
        assert!(all.contains("\"effectcraft\" is already used"), "{all}");
    }

    #[test]
    fn activation_auth_and_admin_settings_are_validated() {
        use crate::access::{AppsAccess, SignIn};
        let cfg = load(
            "[updater]\nactivation = \"idle\"\nidle_after = \"10m\"\n[apps.photocraft]\nactivation = \"immediate\"\n[auth]\nmethods = [\"local\"]\napps = \"signed-in\"\n[admin]\nenabled = true\nhost = \"Admin.Example.net\"\n",
        )
        .unwrap();
        assert_eq!(cfg.app("vectorcraft").unwrap().activation, Activation::Idle);
        assert_eq!(cfg.app("vectorcraft").unwrap().idle_after_secs, 600);
        assert_eq!(
            cfg.app("photocraft").unwrap().activation,
            Activation::Immediate
        );
        assert_eq!(cfg.admin.host.as_deref(), Some("admin.example.net"));
        assert_eq!(
            (cfg.auth.sign_in, cfg.auth.apps),
            (
                SignIn::Interactive {
                    local: true,
                    oidc: false
                },
                AppsAccess::SignedIn
            )
        );
        // Same port, no host: the default. Local and OIDC combine.
        assert!(load("[auth]\nmethods = [\"basic\"]\n[admin]\nenabled = true\n").is_ok());
        let both = load("[auth]\nmethods = [\"local\", \"oidc\"]\n[auth.oidc]\nissuer = \"https://id.example.net\"\nclient_id = \"c\"\nredirect_url = \"https://apps.example.net/auth/oidc/callback\"\n").unwrap();
        assert_eq!(
            both.auth.sign_in,
            SignIn::Interactive {
                local: true,
                oidc: true
            }
        );
        let o = both.auth.oidc.unwrap();
        assert!(o.create_users && !o.link_by_email, "safe defaults");
        let all = load("[auth]\nmethods = [\"local\", \"basic\"]\n")
            .unwrap_err()
            .0
            .join("\n");
        assert!(all.contains("must stand alone"), "{all}");
        // Older files are rejected with a hint per moved key, never read as "no sign-in".
        let all = load("[admin]\nenabled = true\nauth = \"form\"\nallowed_groups = [\"a\"]\n[auth]\nmethod = \"form\"\nusers_file = \"users\"\n")
            .unwrap_err()
            .0
            .join("\n");
        assert!(
            all.contains("replaced by [auth] methods")
                && all.contains("admin_users/admin_groups")
                && all.contains("craft-host user import"),
            "{all}"
        );

        let all = load("[auth]\nmethods = [\"proxy\"]\n[admin]\nenabled = true\n")
            .unwrap_err()
            .0
            .join("\n");
        assert!(all.contains("needs [server] trusted_proxies"), "{all}");
        assert!(
            all.contains("admin_groups or admin_users: required"),
            "{all}"
        );
        let all = load("[auth]\napps = \"signed-in\"\n")
            .unwrap_err()
            .0
            .join("\n");
        assert!(all.contains("needs a sign-in method"), "{all}");
        let all = load("[auth]\nmethods = [\"oidc\"]\n[admin]\nhost = \"admin.example.net\"\n")
            .unwrap_err()
            .0
            .join("\n");
        assert!(all.contains("needs an [auth.oidc] section"), "{all}");
        assert!(all.contains("cannot be combined"), "{all}");
        let all = load("[auth]\nmethods = [\"oidc\"]\n[auth.oidc]\nissuer = \"https://id.example.net\"\nredirect_url = \"https://x/admin/oidc/callback\"\n").unwrap_err().0.join("\n");
        assert!(
            all.contains("client_id: required") && all.contains("/auth/oidc/callback"),
            "{all}"
        );
        let all = load("[auth]\nmethods = [\"magic\"]\napps = \"some\"\n[server]\nlisten = \"nowhere\"\ntrusted_proxies = [\"10.0.0.0/99\"]\n").unwrap_err().0.join("\n");
        assert!(
            all.contains("[auth] methods")
                && all.contains("[auth] apps")
                && all.contains("[server] listen")
                && all.contains("trusted_proxies"),
            "{all}"
        );
        let all = load("[apps.photocraft]\nactivation = \"later\"\n")
            .unwrap_err()
            .0
            .join("\n");
        assert!(all.contains("[apps.photocraft] activation"), "{all}");
    }

    #[test]
    fn unknown_keys_and_builtin_redefinitions_are_rejected() {
        let err = load("[updater]\ncheck_intervall = 60\n").unwrap_err();
        assert!(err.0[0].contains("unknown field"), "{:?}", err.0);
        let err = load("[apps.photocraft]\nrepository = \"evil/fork\"\n").unwrap_err();
        assert!(
            err.0.join("").contains("built-in apps accept only"),
            "{:?}",
            err.0
        );
    }

    #[test]
    fn separate_origins_are_normalized_and_validated() {
        let cfg =
            load("[apps.lightcraft]\norigin = \"https://lightcraft.example.net/\"\n").unwrap();
        assert_eq!(
            cfg.app("lightcraft").unwrap().origin.as_deref(),
            Some("https://lightcraft.example.net")
        );
        let err =
            load("[apps.lightcraft]\norigin = \"https://example.net/lightcraft\"\n").unwrap_err();
        assert!(err.0[0].contains("[apps.lightcraft] origin"), "{:?}", err.0);
        for bad in [
            "https://.",
            "https://host:99999",
            "https://host:0",
            "ftp://host",
            "https://-x.net",
            "https://[::1]",
        ] {
            assert!(!valid_origin(bad), "{bad}");
        }
        assert!(valid_origin("http://192.168.1.10:8080"));
    }

    #[test]
    fn custom_apps_need_a_name_and_default_to_the_suite_conventions() {
        let err = load("[apps.newcraft]\nenabled = true\n").unwrap_err();
        assert!(
            err.0.join("\n").contains("[apps.newcraft] name: required"),
            "{:?}",
            err.0
        );
        let cfg = load("[apps.newcraft]\nname = \"NewCraft\"\n").unwrap();
        let app = cfg.app("newcraft").unwrap();
        assert_eq!(
            (
                app.repository.as_str(),
                app.entry.as_str(),
                app.group.as_str()
            ),
            ("storytold/newcraft", "newcraft", "")
        );
        assert_eq!(app.artifact_patterns, ["newcraft-web-{version}.zip"]);
        let cfg = load(
            "[apps.mine]\nname = \"Mine\"\nrepository = \"me/mine\"\nartifact_patterns = [\"site-{version}.zip\"]\ncolor = \"#123456\"\n",
        )
        .unwrap();
        let app = cfg.app("mine").unwrap();
        assert_eq!(
            (app.repository.as_str(), app.color.as_str()),
            ("me/mine", "#123456")
        );
        assert_eq!(app.artifact_patterns, ["site-{version}.zip"]);
        let err = load("[apps.mine]\nname = \"Mine\"\nartifact_patterns = []\n").unwrap_err();
        assert!(
            err.0[0].contains("artifact_patterns: must not be empty"),
            "{:?}",
            err.0
        );
    }

    #[test]
    fn overlapping_root_directories_are_rejected() {
        let mut p = paths_with(Path::new("/config/config.toml"));
        p.cache = "/srv/data/cache".into();
        let err = load_config(p).unwrap_err();
        assert!(
            err.0
                .iter()
                .any(|e| e.contains("DATA_DIR") && e.contains("CACHE_DIR")),
            "{:?}",
            err.0
        );
    }
}
