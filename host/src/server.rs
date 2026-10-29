//! The HTTP server: launcher, `/status.json`, health endpoints, sign-in (`/auth/`), the admin
//! interface and app releases under stable per-app paths, all on one port.
//!
//! `/<entry>/` redirects (relative 302) to the active immutable release `/<entry>/<version>/`;
//! tabs that loaded a release keep requesting files from it after an update.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::routing::{any, get, post};

use crate::activity::Activity;
use crate::admin::{self, Jobs};
use crate::auth::{Sessions, Throttle, request_host};
use crate::config::{AppConfig, Config, LiveConfig};
use crate::layout::{self, MARKER, valid_release_name};
use crate::login;
use crate::oidc::Oidc;
use crate::serve::{
    CachePolicy, fallback_allowed, redirect, release_path, serve_dir, set_common_headers, simple,
};
use crate::status;
use crate::store::Store;
use crate::users::UserDb;

const LAUNCHER_HTML: &str = include_str!("../assets/index.html");
const INSTALLING_HTML: &str = include_str!("../assets/installing.html");

pub struct Shared {
    /// Replaced when apps are discovered; read it with [`Shared::cfg`].
    pub live: Arc<LiveConfig>,
    pub activity: Arc<Activity>,
    pub users: UserDb,
    pub sessions: Sessions,
    pub throttle: Throttle,
    pub oidc: Option<Oidc>,
    pub jobs: Jobs,
    pub shots: crate::shots::Shots,
    pub cloud: Option<Arc<crate::cloud::CloudStore>>,
}

impl Shared {
    pub fn new(live: Arc<LiveConfig>, activity: Arc<Activity>) -> anyhow::Result<Arc<Self>> {
        let cfg = live.get();
        let oidc = cfg
            .auth
            .oidc
            .clone()
            .map(|o| Oidc::new(o, cfg.updater.http_timeout_secs));
        Ok(Arc::new(Self {
            cloud: if cfg.cloud.enabled {
                Some(Arc::new(crate::cloud::CloudStore::open(
                    &cfg.paths.state,
                    cfg.cloud.max_file_bytes,
                    cfg.cloud.quota_bytes,
                    cfg.limits.min_free_bytes,
                )?))
            } else {
                None
            },
            users: UserDb::open(&cfg.paths.state)?,
            sessions: Sessions::default(),
            throttle: Throttle::default(),
            oidc,
            jobs: Jobs::default(),
            shots: crate::shots::Shots::default(),
            live,
            activity,
        }))
    }

    /// The current configuration snapshot.
    pub fn cfg(&self) -> Arc<Config> {
        self.live.get()
    }
}

fn static_page(body: &'static str, content_type: &'static str) -> Response<Body> {
    let mut res = Response::new(Body::from(body));
    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    set_common_headers(&mut res);
    res
}

async fn launcher(State(s): State<Arc<Shared>>, req: Request) -> Response<Body> {
    if s.cfg().cloud.enabled {
        cloud_page(
            inject_cloud(LAUNCHER_HTML, "", req.uri().path()),
            req.method() == Method::HEAD,
        )
    } else {
        static_page(LAUNCHER_HTML, "text/html; charset=utf-8")
    }
}

/// Runtime integration is injected only for the opt-in server configuration. Immutable
/// release files and their published checksums remain untouched.
fn inject_cloud(html: &str, app: &str, path: &str) -> String {
    let script = format!(
        r#"<script defer src="{}__cloud/client.js" data-cloud-enabled="true" data-cloud-app="{}" data-craft-cloud-bootstrap></script>"#,
        crate::auth::to_root(path),
        app,
    );
    let mut page = html.to_owned();
    if let Some(at) = html.to_ascii_lowercase().rfind("</head>") {
        page.insert_str(at, &script);
    } else {
        page.push_str(&script);
    }
    page
}

fn cloud_page(page: String, head: bool) -> Response<Body> {
    let mut res = Response::new(if head {
        Body::empty()
    } else {
        Body::from(page.clone())
    });
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(size) = HeaderValue::from_str(&page.len().to_string()) {
        res.headers_mut().insert(header::CONTENT_LENGTH, size);
    }
    set_common_headers(&mut res);
    res
}

async fn cloud_client(State(s): State<Arc<Shared>>) -> Response<Body> {
    if !s.cfg().cloud.enabled {
        return simple(StatusCode::NOT_FOUND, "not found\n");
    }
    let mut res = static_page(
        include_str!("../assets/cloud.js"),
        "text/javascript; charset=utf-8",
    );
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

async fn serve_with_cloud(
    s: &Shared,
    app: &AppConfig,
    root: std::path::PathBuf,
    rest: &str,
    req: Request<Body>,
    policy: CachePolicy,
) -> Response<Body> {
    let path = rest.split('?').next().unwrap_or(rest);
    if s.cfg().cloud.enabled && (req.method() == Method::GET || req.method() == Method::HEAD) {
        let index = if path.ends_with('/') {
            format!("{path}index.html")
        } else {
            path.to_string()
        };
        if index.ends_with(".html")
            && let Some(file) = release_path(&root, &index)
            && tokio::fs::metadata(&file)
                .await
                .is_ok_and(|m| m.is_file() && m.len() <= 2 << 20)
            && let Ok(html) = tokio::fs::read_to_string(file).await
        {
            return cloud_page(
                inject_cloud(&html, &app.id, req.uri().path()),
                req.method() == Method::HEAD,
            );
        }
    }
    serve_dir(root, rest, req, policy).await
}

async fn launcher_asset(Path(path): Path<String>) -> Response<Body> {
    let Some(asset) = crate::assets::launcher(&path) else {
        return simple(StatusCode::NOT_FOUND, "not found\n");
    };
    let mut res = Response::new(Body::from(asset.bytes));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    // Logos and fonts only change with a new image; pages and script revalidate.
    let cache = if path.starts_with("icons/") || path.starts_with("fonts/") {
        "public, max-age=86400"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    set_common_headers(&mut res);
    res
}

fn picture(bytes: Vec<u8>, content_type: &'static str) -> Response<Body> {
    let mut res = Response::new(Body::from(bytes));
    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    set_common_headers(&mut res);
    res
}

/// `/launcher/shots/<app>/<file>`: a catalogued or discovered app's screenshot, fetched once
/// and cached.
async fn launcher_shot(
    State(s): State<Arc<Shared>>,
    Path((app, file)): Path<(String, String)>,
) -> Response<Body> {
    let shared = s.clone();
    let found = tokio::task::spawn_blocking(move || shared.shots.get(&shared.cfg(), &app, &file))
        .await
        .ok()
        .flatten();
    match found {
        Some(p) => picture(p.bytes, p.content_type),
        None => simple(StatusCode::NOT_FOUND, "not found\n"),
    }
}

/// `/launcher/discovered/<file>`: the logo of an enabled discovered app.
async fn discovered_logo(State(s): State<Arc<Shared>>, Path(file): Path<String>) -> Response<Body> {
    let cfg = s.cfg();
    let known = cfg
        .enabled_apps()
        .any(|a| a.discovered && a.logo.as_deref() == Some(file.as_str()));
    let content_type = file
        .rsplit_once('.')
        .and_then(|(_, ext)| crate::shots::content_type(ext));
    let (true, Some(content_type)) = (known, content_type) else {
        return simple(StatusCode::NOT_FOUND, "not found\n");
    };
    let path = crate::discovery::dir(&cfg.paths.state).join(&file);
    match tokio::fs::read(path).await {
        Ok(bytes) => picture(bytes, content_type),
        Err(_) => simple(StatusCode::NOT_FOUND, "not found\n"),
    }
}
async fn healthz() -> Response<Body> {
    let mut r = simple(StatusCode::OK, "ok\n");
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn find_entry<'a>(cfg: &'a Config, entry: &str) -> Option<&'a AppConfig> {
    cfg.enabled_apps().find(|a| a.entry == entry)
}

async fn readyz(State(s): State<Arc<Shared>>, Path(entry): Path<String>) -> Response<Body> {
    let cfg = s.cfg();
    match find_entry(&cfg, &entry) {
        Some(app) if layout::active_version(&cfg, app).is_some() => healthz().await,
        Some(_) => simple(StatusCode::SERVICE_UNAVAILABLE, "not ready\n"),
        None => simple(StatusCode::NOT_FOUND, "not found\n"),
    }
}

async fn status_json(State(s): State<Arc<Shared>>) -> Response<Body> {
    let cfg = s.cfg();
    let built = tokio::task::spawn_blocking(move || {
        serde_json::to_vec(&status::build(&cfg, &Store::new(&cfg.paths.state), true))
    })
    .await;
    let Ok(Ok(body)) = built else {
        return simple(StatusCode::INTERNAL_SERVER_ERROR, "internal error\n");
    };
    let mut res = Response::new(Body::from(body));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    set_common_headers(&mut res);
    res
}

fn installing() -> Response<Body> {
    let mut res = static_page(INSTALLING_HTML, "text/html; charset=utf-8");
    *res.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    res
}

/// A retained release directory named by the first path segment, e.g. `0.5.0`.
fn retained_release(cfg: &Config, app: &AppConfig, segment: &str) -> Option<std::path::PathBuf> {
    let root = cfg.release_root(app).join(segment);
    (valid_release_name(segment) && root.join(MARKER).is_file()).then_some(root)
}

/// Everything under `/<entry>/…`:
/// - `/<entry>/<path>` serves the active release at the app's stable URL;
/// - `/<entry>/<version>/<path>` serves that retained release (immutable, cached for a year).
///
/// A file missing from the active release is looked up in the other retained releases, newest
/// first: a tab opened before an update may still load content-hashed files of its release.
async fn app_request(State(s): State<Arc<Shared>>, req: Request) -> Response<Body> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return simple(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n");
    }
    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let entry = parts.next().unwrap_or("");
    let cfg = s.cfg();
    let Some(app) = find_entry(&cfg, entry) else {
        return simple(StatusCode::NOT_FOUND, "not found\n");
    };
    // /photocraft
    let Some(inner) = parts.next() else {
        return redirect(StatusCode::MOVED_PERMANENTLY, &format!("{entry}/{query}"));
    };
    let (first, after) = match inner.split_once('/') {
        Some((f, a)) => (f, Some(a)),
        None => (inner, None),
    };
    if let Some(root) = retained_release(&cfg, app, first) {
        return match after {
            // /photocraft/0.5.0
            None => redirect(StatusCode::MOVED_PERMANENTLY, &format!("{first}/{query}")),
            // /photocraft/0.5.0/…
            Some(rest) => {
                s.activity.record(&app.id, Some(first));
                serve_with_cloud(
                    &s,
                    app,
                    root,
                    &format!("/{rest}{query}"),
                    req,
                    CachePolicy::Immutable,
                )
                .await
            }
        };
    }
    // /photocraft/… from the active release.
    let Some(active) = layout::active_version(&cfg, app) else {
        return if inner.is_empty() {
            installing()
        } else {
            simple(StatusCode::NOT_FOUND, "not found\n")
        };
    };
    let rest = format!("/{inner}{query}");
    let releases = cfg.release_root(app);
    let mut version = active;
    if fallback_allowed(&rest)
        && release_path(&releases.join(&version), &rest).is_some_and(|p| !p.exists())
    {
        let state = Store::new(&cfg.paths.state)
            .load(&app.id)
            .unwrap_or_default();
        let mut others: Vec<_> = state
            .installed
            .iter()
            .filter(|(v, _)| **v != version)
            .collect();
        others.sort_by(|a, b| b.1.installed_at.cmp(&a.1.installed_at));
        if let Some((v, _)) = others.into_iter().find(|(v, _)| {
            retained_release(&cfg, app, v).is_some()
                && release_path(&releases.join(v.as_str()), &rest).is_some_and(|p| p.exists())
        }) {
            version = v.clone();
        }
    }
    s.activity.record(&app.id, Some(&version));
    let root = releases.join(&version);
    serve_with_cloud(&s, app, root, &rest, req, CachePolicy::Current(version)).await
}

/// /admin answers only when enabled. With `[admin] host`, it lives only on that host name and the
/// apps are not served there; sign-in (`/auth/`) and `/healthz` work on every host.
async fn host_gate(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path();
    let is_admin = path == "/admin" || path.starts_with("/admin/");
    if let Some(admin_host) = &s.cfg().admin.host {
        let host = request_host(req.headers(), peer.ip(), &s.cfg().server);
        // Ports are ignored unless the configured host names one.
        let on_admin_host = host.as_deref().is_some_and(|h| {
            h == admin_host
                || (!admin_host.contains(':')
                    && h.rsplit_once(':')
                        .is_some_and(|(name, _)| name == admin_host))
        });
        if is_admin != on_admin_host && path != "/healthz" && !path.starts_with("/auth/") {
            return simple(StatusCode::NOT_FOUND, "not found\n");
        }
    }
    if is_admin && !s.cfg().admin.enabled {
        return simple(StatusCode::NOT_FOUND, "not found\n");
    }
    next.run(req).await
}

pub fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/", get(launcher))
        .route("/__cloud/client.js", get(cloud_client))
        .route("/__cloud/api/config", any(crate::cloud::request))
        .route("/__cloud/api/{*path}", any(crate::cloud::request))
        .route("/launcher/shots/{app}/{file}", get(launcher_shot))
        .route("/launcher/discovered/{file}", get(discovered_logo))
        .route("/launcher/{*path}", get(launcher_asset))
        .route("/status.json", get(status_json))
        .route("/healthz", get(healthz))
        .route("/readyz/{entry}", get(readyz))
        .route(
            "/auth/login",
            get(login::login_page).post(login::login_submit),
        )
        .route("/auth/style.css", get(login::style))
        .route("/auth/logout", post(login::logout))
        .route("/auth/me", get(login::me))
        .route("/auth/options", get(login::options))
        .route("/auth/oidc/login", get(login::oidc_login))
        .route("/auth/oidc/callback", get(login::oidc_callback))
        .route("/admin", get(admin::root_redirect))
        .route("/admin/", get(admin::index))
        .route("/admin/api/status", get(admin::api_status))
        .route("/admin/api/apps/{app}/{action}", post(admin::api_action))
        .route(
            "/admin/api/discovery/refresh",
            post(admin::api_refresh_discovery),
        )
        .route(
            "/admin/api/users",
            get(admin::users_list).post(admin::users_create),
        )
        .route("/admin/api/users/{id}", post(admin::users_update))
        .route("/admin/api/users/{id}/{action}", post(admin::users_action))
        .route("/admin/{name}", get(admin::asset))
        .fallback(app_request)
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            login::apps_gate,
        ))
        .layer(middleware::from_fn_with_state(shared.clone(), host_gate))
        .with_state(shared)
}

/// Serve until the process is stopped.
pub async fn serve(shared: Arc<Shared>) -> anyhow::Result<()> {
    let addr = shared.cfg().server.listen;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    log::info!("listening on http://{addr}");
    if shared.cfg().admin.enabled {
        match &shared.cfg().admin.host {
            Some(h) => log::info!("admin interface on host {h}, path /admin/"),
            None => log::info!(
                "admin interface at /admin/, on the apps' browser origin (set [admin] host to separate it; see docs/admin.md)"
            ),
        }
    }
    axum::serve(
        listener,
        router(shared).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    })
    .await?;
    Ok(())
}
