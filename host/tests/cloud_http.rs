//! Opt-in isolation and end-to-end persistence through the existing authenticated router.
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use craft_host::{
    activity::Activity,
    config::{LiveConfig, Paths, load_config},
    server::{Shared, router},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

fn hex_sha(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct Fixture {
    _dir: tempfile::TempDir,
    shared: Arc<Shared>,
}
struct Browser {
    user: &'static str,
    namespace: String,
    csrf: String,
    cookie: String,
}

impl Fixture {
    fn new(enabled: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let paths = Paths {
            config_file: p.join("config/config.toml"),
            data: p.join("data"),
            state: p.join("state"),
            cache: p.join("cache"),
            work: p.join("work"),
            logs: None,
        };
        for path in [
            &paths.data,
            &paths.state,
            &paths.cache,
            &paths.work,
            paths.config_file.parent().unwrap(),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(
            &paths.config_file,
            format!(
                r#"
[server]
trusted_proxies = ["127.0.0.1"]
[auth]
methods = ["proxy"]
apps = "signed-in"
[limits]
min_free_space = 0
[discovery]
feed_url = ""
metadata_url = ""
[cloud]
enabled = {enabled}
max_file = "16MiB"
quota = "64MiB"
"#
            ),
        )
        .unwrap();
        let cfg = load_config(paths).unwrap();
        if enabled {
            std::fs::remove_dir(&cfg.paths.state).unwrap();
        }
        let shared =
            Shared::new(Arc::new(LiveConfig::new(cfg)), Arc::new(Activity::new())).unwrap();
        Self { _dir: dir, shared }
    }
    async fn req(
        &self,
        method: &str,
        path: &str,
        user: Option<&str>,
        browser: Option<&Browser>,
        body: Vec<u8>,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header("Host", "localhost")
            .header("Origin", "http://localhost");
        if let Some(user) = user {
            b = b.header("Remote-User", user);
        }
        if let Some(browser) = browser {
            b = b
                .header("Cookie", &browser.cookie)
                .header("X-Craft-CSRF", &browser.csrf)
                .header("X-Craft-Cloud-Namespace", &browser.namespace);
        }
        let mut req = b.body(Body::from(body)).unwrap();
        req.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        let res = router(self.shared.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let data = to_bytes(res.into_body(), 32 << 20).await.unwrap().to_vec();
        (status, headers, data)
    }
    async fn browser(&self, user: &'static str) -> Browser {
        let (status, headers, data) = self
            .req("GET", "/__cloud/api/config", Some(user), None, vec![])
            .await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_slice(&data).unwrap();
        let cookie = headers
            .get_all("set-cookie")
            .iter()
            .map(|v| v.to_str().unwrap())
            .find(|v| v.starts_with("craft_cloud_csrf="))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        Browser {
            user,
            namespace: v["namespace"].as_str().unwrap().into(),
            csrf: v["csrfToken"].as_str().unwrap().into(),
            cookie,
        }
    }
    async fn upload(&self, b: &Browser, op: &str, body: &[u8], project: Option<&Value>) -> Value {
        let metadata = json!({"operation_id":op,"app":"photocraft","filename":"Drawing.psd","size":body.len(),"sha256":hex_sha(body),"intent":"save","project_id":project.map(|p|p["id"].clone()),"base_id":project.map(|p|p["head"]["id"].clone())});
        let (status, _, bytes) = self
            .req(
                "POST",
                "/__cloud/api/uploads",
                Some(b.user),
                Some(b),
                serde_json::to_vec(&metadata).unwrap(),
            )
            .await;
        assert!(
            status.is_success(),
            "{}: {}",
            status,
            String::from_utf8_lossy(&bytes)
        );
        let upload: Value = serde_json::from_slice(&bytes).unwrap();
        let id = upload["id"].as_str().unwrap();
        let (status, _, data) = self
            .req(
                "PUT",
                &format!("/__cloud/api/uploads/{id}/parts/0"),
                Some(b.user),
                Some(b),
                body.to_vec(),
            )
            .await;
        assert!(
            status.is_success(),
            "{}: {}",
            status,
            String::from_utf8_lossy(&data)
        );
        let (status, _, data) = self
            .req(
                "POST",
                &format!("/__cloud/api/uploads/{id}/complete"),
                Some(b.user),
                Some(b),
                vec![],
            )
            .await;
        assert!(
            status.is_success(),
            "{}: {}",
            status,
            String::from_utf8_lossy(&data)
        );
        serde_json::from_slice(&data).unwrap()
    }
}

#[tokio::test]
async fn disabled_mode_has_no_widget_no_storage_and_no_file_endpoints() {
    let f = Fixture::new(false);
    assert!(f.shared.cloud.is_none());
    let (s, _, body) = f.req("GET", "/", Some("alice"), None, vec![]).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!String::from_utf8_lossy(&body).contains("data-craft-cloud-bootstrap"));
    for path in [
        "/__cloud/client.js",
        "/__cloud/api/projects",
        "/__cloud/api/files/00000000-0000-4000-8000-000000000001",
    ] {
        assert_eq!(
            f.req("GET", path, Some("alice"), None, vec![]).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (_, _, data) = f
        .req("GET", "/__cloud/api/config", Some("alice"), None, vec![])
        .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&data).unwrap(),
        json!({"enabled":false})
    );
    for entry in std::fs::read_dir(&f.shared.cfg().paths.state).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("cloud")
        );
    }
}

#[tokio::test]
async fn enabled_pages_are_injected_without_modifying_verified_release_files() {
    let f = Fixture::new(true);
    let cfg = f.shared.cfg();
    let path = cfg
        .release_root(cfg.app("photocraft").unwrap())
        .join("1.0.0");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join(".craft-release.json"), "{}").unwrap();
    let original = "<!doctype html><html><head></head><body>Editor</body></html>";
    std::fs::write(path.join("index.html"), original).unwrap();
    let (status, headers, body) = f
        .req("GET", "/photocraft/1.0.0/", Some("alice"), None, vec![])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("../../__cloud/client.js"));
    assert!(html.contains("data-cloud-app=\"photocraft\""));
    assert_eq!(
        std::fs::read_to_string(path.join("index.html")).unwrap(),
        original
    );
    let (status, _, _) = f
        .req("GET", "/__cloud/client.js", Some("alice"), None, vec![])
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn persistence_is_per_user_and_mutations_reject_changed_identity() {
    let f = Fixture::new(true);
    let a = f.browser("alice").await;
    let b = f.browser("bob").await;
    assert_ne!(a.namespace, b.namespace);
    let stored = f
        .upload(
            &a,
            "00000000-0000-4000-8000-000000000001",
            b"editable data",
            None,
        )
        .await;
    let project = &stored["project"];
    let file = &stored["file"];
    for url in [
        format!("/__cloud/api/projects/{}", project["id"].as_str().unwrap()),
        format!(
            "/__cloud/api/projects/{}/versions",
            project["id"].as_str().unwrap()
        ),
        format!("/__cloud/api/files/{}", file["id"].as_str().unwrap()),
    ] {
        let (status, _, data) = f.req("GET", &url, Some("bob"), Some(&b), vec![]).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{}: {}",
            url,
            String::from_utf8_lossy(&data)
        );
    }
    let (_, _, data) = f
        .req(
            "GET",
            "/__cloud/api/projects",
            Some("bob"),
            Some(&b),
            vec![],
        )
        .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&data).unwrap()["projects"],
        json!([])
    );
    let (status, headers, data) = f
        .req(
            "GET",
            &format!("/__cloud/api/files/{}", file["id"].as_str().unwrap()),
            Some("alice"),
            None,
            vec![],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data, b"editable data");
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    assert_eq!(
        f.req(
            "GET",
            "/__cloud/api/projects",
            Some("bob"),
            Some(&a),
            vec![]
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, _, data) = f
        .req(
            "POST",
            "/__cloud/api/uploads",
            Some("bob"),
            Some(&a),
            b"old account payload".to_vec(),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(String::from_utf8_lossy(&data).contains("identity_changed"));
    let (status, _, _) = f
        .req("POST", "/__cloud/api/uploads", Some("alice"), None, vec![])
        .await;
    assert!(status == StatusCode::FORBIDDEN || status == StatusCode::CONFLICT);
    let (status, _, _) = f
        .req("GET", "/__cloud/api/projects", None, None, vec![])
        .await;
    // The existing proxy auth gate denies missing identity before the cloud handler.
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        f.req("POST", "/auth/logout", Some("bob"), Some(&a), vec![])
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.req("POST", "/auth/logout", Some("bob"), None, vec![])
            .await
            .0,
        StatusCode::OK
    );
    let mut disabled = (*f.shared.cfg()).clone();
    disabled.cloud.enabled = false;
    f.shared.live.replace(disabled);
    assert_eq!(
        f.req(
            "GET",
            &format!("/__cloud/api/files/{}", file["id"].as_str().unwrap()),
            Some("alice"),
            Some(&a),
            vec![]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn durable_save_conflicts_do_not_overwrite_another_edit() {
    let f = Fixture::new(true);
    let a = f.browser("alice").await;
    let first = f
        .upload(&a, "00000000-0000-4000-8000-000000000002", b"first", None)
        .await;
    let second = f
        .upload(
            &a,
            "00000000-0000-4000-8000-000000000003",
            b"second",
            Some(&first["project"]),
        )
        .await;
    let third = f
        .upload(
            &a,
            "00000000-0000-4000-8000-000000000004",
            b"second",
            Some(&second["project"]),
        )
        .await;
    assert_eq!(
        second["file"]["id"], third["file"]["id"],
        "identical clean saves reuse their durable capture"
    );
    let project = first["project"]["id"].as_str().unwrap();
    let metadata = json!({"operation_id":"00000000-0000-4000-8000-000000000005","app":"photocraft","filename":"Drawing.psd","size":5,"sha256":hex_sha(b"stale"),"intent":"save","project_id":project,"base_id":first["file"]["id"]});
    let (status, _, data) = f
        .req(
            "POST",
            "/__cloud/api/uploads",
            Some("alice"),
            Some(&a),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .await;
    if status.is_success() {
        let up: Value = serde_json::from_slice(&data).unwrap();
        let id = up["id"].as_str().unwrap();
        assert!(
            f.req(
                "PUT",
                &format!("/__cloud/api/uploads/{id}/parts/0"),
                Some("alice"),
                Some(&a),
                b"stale".to_vec()
            )
            .await
            .0
            .is_success()
        );
        assert_eq!(
            f.req(
                "POST",
                &format!("/__cloud/api/uploads/{id}/complete"),
                Some("alice"),
                Some(&a),
                vec![]
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    } else {
        assert_eq!(status, StatusCode::CONFLICT);
    }
    let (_, _, data) = f
        .req(
            "GET",
            &format!("/__cloud/api/projects/{project}"),
            Some("alice"),
            Some(&a),
            vec![],
        )
        .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&data).unwrap()["head"]["id"],
        second["file"]["id"]
    );
}
