//! Opt-in, authenticated document storage. App releases remain read-only and separate from
//! documents. Every metadata lookup, blob and upload belongs to the authenticated principal.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{auth, fsutil, login, serve, server::Shared, timeutil};

pub const CHUNK_BYTES: u64 = 4 << 20;
const MAX_PENDING_PER_USER: i64 = 4;
const MAX_PENDING_TOTAL: i64 = 16;
const NAMESPACE_HEADER: &str = "x-craft-cloud-namespace";
const CLOUD_CSRF_COOKIE: &str = "craft_cloud_csrf";

#[derive(Debug)]
struct CloudError(StatusCode, &'static str);
type Result<T> = std::result::Result<T, CloudError>;

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}
impl std::error::Error for CloudError {}
impl From<std::io::Error> for CloudError {
    fn from(_: std::io::Error) -> Self {
        internal()
    }
}
impl From<rusqlite::Error> for CloudError {
    fn from(_: rusqlite::Error) -> Self {
        internal()
    }
}
fn internal() -> CloudError {
    CloudError(StatusCode::INTERNAL_SERVER_ERROR, "storage_unavailable")
}
fn invalid() -> CloudError {
    CloudError(StatusCode::BAD_REQUEST, "invalid_request")
}
fn missing() -> CloudError {
    CloudError(StatusCode::NOT_FOUND, "not_found")
}
fn conflict() -> CloudError {
    CloudError(StatusCode::CONFLICT, "conflict")
}
fn quota() -> CloudError {
    CloudError(StatusCode::INSUFFICIENT_STORAGE, "storage_limit")
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}
fn digest(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}
fn row_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
fn owner_key(principal: &auth::Principal) -> Option<String> {
    match principal.user_id {
        Some(id) => Some(format!("db:{id}")),
        None if principal.method == "proxy" => Some(format!("proxy:{}", principal.user)),
        _ => None,
    }
}

/// Shared by the optional cloud client and its guarded sign-out operation.
pub(crate) fn principal_namespace(principal: &auth::Principal) -> Option<String> {
    owner_key(principal).map(|owner| digest(owner.as_bytes()))
}

fn secure_hex(bytes: usize) -> Result<String> {
    let mut data = vec![0; bytes];
    // IDs, unlike temporary-file counters, must never fall back to a predictable value.
    File::open("/dev/urandom")?.read_exact(&mut data)?;
    Ok(data.iter().map(|b| format!("{b:02x}")).collect())
}
fn new_id() -> Result<String> {
    let mut h = secure_hex(16)?.into_bytes();
    h[12] = b'4';
    h[16] = b'8';
    let h = String::from_utf8(h).map_err(|_| internal())?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}
fn valid_id(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
}
fn valid_name(s: &str) -> bool {
    !s.trim().is_empty()
        && s.len() <= 240
        && s != "."
        && s != ".."
        && !s.chars().any(|c| c.is_control() || c == '/' || c == '\\')
}
fn regular(path: &FsPath) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() {
        return Err(internal());
    }
    Ok(m)
}
fn private_dir(path: &FsPath) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && m.permissions().mode() & 0o077 == 0 => Ok(()),
        Ok(_) => Err(internal()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            DirBuilder::new().mode(0o700).create(path)?;
            fsutil::fsync_dir(path.parent().ok_or_else(internal)?)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
fn read_file(path: &FsPath) -> Result<File> {
    regular(path)?;
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?)
}
fn fresh_file(path: &FsPath) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub id: String,
    pub filename: String,
    pub app: String,
    pub size: u64,
    pub sha256: String,
    pub created_at: String,
    pub url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub app: String,
    pub created_at: String,
    pub updated_at: String,
    pub head: Capture,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Complete {
    project: Project,
    file: Capture,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadRequest {
    operation_id: String,
    app: String,
    filename: String,
    size: u64,
    sha256: String,
    intent: String,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    base_id: Option<String>,
}
#[derive(Debug, Serialize)]
struct Upload {
    id: String,
    chunk_bytes: u64,
    parts: Vec<u64>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<Project>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<Capture>,
}
struct UploadRow {
    id: String,
    request: UploadRequest,
    status: String,
    result: Option<Complete>,
}

/// A single server process owns this database and its private document directories. The lock
/// serializes quota reservations, parts and SQLite commits; another process cannot open the
/// same store. Slow work runs on a blocking worker, never on an async executor thread.
pub struct CloudStore {
    root: PathBuf,
    blobs: PathBuf,
    parts: PathBuf,
    db: Mutex<Connection>,
    max_file: u64,
    quota: u64,
    min_free: u64,
    _lock: File,
}

impl CloudStore {
    pub fn open(state: &FsPath, max_file: u64, quota: u64, min_free: u64) -> anyhow::Result<Self> {
        anyhow::ensure!(
            max_file > 0 && max_file <= quota && quota <= i64::MAX as u64,
            "invalid cloud limits"
        );
        fsutil::make_dirs(state)?;
        let root = state.join("cloud");
        private_dir(&root)?;
        let blobs = root.join("blobs");
        let parts = root.join("parts");
        private_dir(&blobs)?;
        private_dir(&parts)?;
        let lock_path = root.join("lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)?;
        anyhow::ensure!(lock.metadata()?.is_file(), "invalid cloud lock");
        // SAFETY: the file descriptor is live and flock does not access user memory.
        anyhow::ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "cloud storage is already open by another server"
        );
        let db_path = root.join("catalog.sqlite3");
        for suffix in ["", "-wal", "-shm"] {
            let path = root.join(format!("catalog.sqlite3{suffix}"));
            match fs::symlink_metadata(path) {
                Ok(m) => anyhow::ensure!(m.is_file(), "invalid cloud database file"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        let db_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&db_path)?;
        db_file.sync_all()?;
        let db = Connection::open(db_path)?;
        db.busy_timeout(std::time::Duration::from_secs(15))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS captures (
                id TEXT PRIMARY KEY, owner TEXT NOT NULL, filename TEXT NOT NULL, app TEXT NOT NULL,
                size INTEGER NOT NULL, sha256 TEXT NOT NULL, created_at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL, app TEXT NOT NULL,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL, head_id TEXT NOT NULL REFERENCES captures(id));
            CREATE INDEX IF NOT EXISTS projects_owner ON projects(owner,app,updated_at);
            CREATE TABLE IF NOT EXISTS versions (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL REFERENCES projects(id),
                capture_id TEXT NOT NULL REFERENCES captures(id), UNIQUE(project_id,capture_id));
            CREATE TABLE IF NOT EXISTS uploads (
                id TEXT PRIMARY KEY, owner TEXT NOT NULL, operation_id TEXT NOT NULL,
                request_json TEXT NOT NULL, size INTEGER NOT NULL, status TEXT NOT NULL,
                result_json TEXT, UNIQUE(owner,operation_id));")?;
        fsutil::fsync_dir(&root)?;
        let store = Self {
            root,
            blobs,
            parts,
            db: Mutex::new(db),
            max_file,
            quota,
            min_free,
            _lock: lock,
        };
        // Only parts already proven complete/cancelled may be recovered automatically. Pending
        // uploads and orphan blobs remain intact and count against quota after a crash.
        let finished: Vec<String> = {
            let db = store.db.lock();
            let mut q = db.prepare("SELECT id FROM uploads WHERE status!='pending'")?;
            q.query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        for id in finished {
            store.cleanup_parts(&id)?;
        }
        store.usage(&store.db.lock())?;
        Ok(store)
    }

    fn part_dir(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(missing());
        }
        let p = self.parts.join(id);
        match fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() && m.permissions().mode() & 0o077 == 0 => Ok(p),
            Ok(_) => Err(internal()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(missing()),
            Err(e) => Err(e.into()),
        }
    }
    fn cleanup_parts(&self, id: &str) -> Result<()> {
        let dir = match self.part_dir(id) {
            Ok(p) => p,
            Err(e) if e.0 == StatusCode::NOT_FOUND => return Ok(()),
            Err(e) => return Err(e),
        };
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err(internal());
            }
            fs::remove_file(entry.path())?;
        }
        fs::remove_dir(dir)?;
        fsutil::fsync_dir(&self.parts)?;
        Ok(())
    }
    fn parts_bytes(&self, id: &str) -> Result<u64> {
        let dir = match self.part_dir(id) {
            Ok(p) => p,
            Err(e) if e.0 == StatusCode::NOT_FOUND => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut size = 0u64;
        for entry in fs::read_dir(dir)? {
            size = size
                .checked_add(regular(&entry?.path())?.len())
                .ok_or_else(quota)?;
        }
        Ok(size)
    }
    fn usage(&self, db: &Connection) -> Result<u64> {
        let mut size = 0u64;
        for entry in fs::read_dir(&self.blobs)? {
            size = size
                .checked_add(regular(&entry?.path())?.len())
                .ok_or_else(quota)?;
        }
        let mut pending = BTreeSet::new();
        let mut q = db.prepare("SELECT id,size FROM uploads WHERE status='pending'")?;
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, row_u64(r, 1)?)))? {
            let (id, reserved) = row?;
            size = size
                .checked_add(reserved.max(self.parts_bytes(&id)?))
                .ok_or_else(quota)?;
            pending.insert(id);
        }
        for entry in fs::read_dir(&self.parts)? {
            let entry = entry?;
            let id = entry.file_name().into_string().map_err(|_| internal())?;
            if !entry.file_type()?.is_dir() || !valid_id(&id) {
                return Err(internal());
            }
            if !pending.contains(&id) {
                size = size.checked_add(self.parts_bytes(&id)?).ok_or_else(quota)?;
            }
        }
        Ok(size)
    }
    fn space(&self, additional: u64) -> Result<()> {
        let required = self.min_free.checked_add(additional).ok_or_else(quota)?;
        if fsutil::free_bytes(&self.root)? < required {
            return Err(quota());
        }
        Ok(())
    }
    fn upload_row(db: &Connection, owner: &str, id: &str) -> Result<UploadRow> {
        if !valid_id(id) {
            return Err(missing());
        }
        let row: Option<(String, String, Option<String>)> = db
            .query_row(
                "SELECT request_json,status,result_json FROM uploads WHERE owner=?1 AND id=?2",
                params![owner, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (request, status, result) = row.ok_or_else(missing)?;
        Ok(UploadRow {
            id: id.into(),
            request: serde_json::from_str(&request).map_err(|_| internal())?,
            status,
            result: result
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|_| internal())?,
        })
    }
    fn upload_view(&self, row: UploadRow) -> Result<Upload> {
        let mut parts = Vec::new();
        if row.status == "pending" {
            for entry in fs::read_dir(self.part_dir(&row.id)?)? {
                let entry = entry?;
                regular(&entry.path())?;
                let name = entry.file_name();
                if let Some(n) = name.to_str().and_then(|s| s.parse::<u64>().ok()) {
                    parts.push(n);
                }
            }
            parts.sort_unstable();
        }
        Ok(Upload {
            id: row.id,
            chunk_bytes: CHUNK_BYTES,
            parts,
            status: row.status,
            project: row.result.as_ref().map(|r| r.project.clone()),
            file: row.result.map(|r| r.file),
        })
    }
    fn init(&self, owner: &str, r: UploadRequest) -> Result<Upload> {
        if !valid_id(&r.operation_id)
            || !valid_name(&r.filename)
            || !crate::config::valid_id(&r.app)
            || r.sha256.len() != 64
            || !r
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !matches!(r.intent.as_str(), "save" | "save_as")
            || r.project_id.as_deref().is_some_and(|id| !valid_id(id))
            || r.base_id.as_deref().is_some_and(|id| !valid_id(id))
            || (r.intent == "save_as" && (r.project_id.is_some() || r.base_id.is_some()))
            || (r.project_id.is_some() != r.base_id.is_some())
        {
            return Err(invalid());
        }
        if r.size > self.max_file {
            return Err(CloudError(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large"));
        }
        let sql_size = i64::try_from(r.size).map_err(|_| invalid())?;
        let db = self.db.lock();
        let previous: Option<String> = db
            .query_row(
                "SELECT id FROM uploads WHERE owner=?1 AND operation_id=?2",
                params![owner, r.operation_id],
                |row| row.get(0),
            )
            .optional()?;
        let request_json = serde_json::to_string(&r).map_err(|_| internal())?;
        if let Some(id) = previous {
            let row = Self::upload_row(&db, owner, &id)?;
            if serde_json::to_string(&row.request).map_err(|_| internal())? != request_json {
                return Err(conflict());
            }
            return self.upload_view(row);
        }
        if let Some(id) = &r.project_id {
            let project = Self::project_db(&db, owner, id)?;
            if project.app != r.app || Some(&project.head.id) != r.base_id.as_ref() {
                return Err(conflict());
            }
        }
        let (total, personal): (i64, i64) = db.query_row(
            "SELECT count(*),coalesce(sum(owner=?1),0) FROM uploads WHERE status='pending'",
            [owner],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if total >= MAX_PENDING_TOTAL || personal >= MAX_PENDING_PER_USER {
            return Err(CloudError(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_uploads",
            ));
        }
        if self
            .usage(&db)?
            .checked_add(r.size)
            .is_none_or(|n| n > self.quota)
        {
            return Err(quota());
        }
        let reserved: u64 = db.query_row(
            "SELECT coalesce(sum(size),0) FROM uploads WHERE status='pending'",
            [],
            |row| row_u64(row, 0),
        )?;
        self.space(
            r.size
                .checked_mul(2)
                .and_then(|n| n.checked_add(reserved))
                .ok_or_else(quota)?,
        )?;
        let id = new_id()?;
        private_dir(&self.parts.join(&id))?;
        db.execute("INSERT INTO uploads(id,owner,operation_id,request_json,size,status) VALUES(?1,?2,?3,?4,?5,'pending')",
            params![id,owner,r.operation_id,request_json,sql_size])?;
        self.upload_view(Self::upload_row(&db, owner, &id)?)
    }
    fn status(&self, owner: &str, id: &str) -> Result<Upload> {
        self.upload_view(Self::upload_row(&self.db.lock(), owner, id)?)
    }
    fn put_part(&self, owner: &str, id: &str, index: u64, data: &[u8]) -> Result<Upload> {
        let db = self.db.lock();
        let row = Self::upload_row(&db, owner, id)?;
        if row.status != "pending" {
            return Err(conflict());
        }
        let start = index.checked_mul(CHUNK_BYTES).ok_or_else(invalid)?;
        if start >= row.request.size
            || data.len() as u64 != CHUNK_BYTES.min(row.request.size - start)
        {
            return Err(invalid());
        }
        let dir = self.part_dir(id)?;
        let dest = dir.join(index.to_string());
        if fs::symlink_metadata(&dest).is_ok() {
            let mut old = read_file(&dest)?;
            let mut hash = Sha256::new();
            let mut total = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                let n = old.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
                total += n as u64;
            }
            if total != data.len() as u64 || hex(&hash.finalize()) != digest(data) {
                return Err(conflict());
            }
        } else {
            self.space(data.len() as u64)?;
            let tmp = dir.join(format!("part-{}", secure_hex(12)?));
            let result = (|| -> Result<()> {
                let mut f = fresh_file(&tmp)?;
                f.write_all(data)?;
                f.sync_all()?;
                // Linking never replaces an immutable named part, even if a second writer
                // somehow bypasses the store's process lock.
                fs::hard_link(&tmp, &dest)?;
                fs::remove_file(&tmp)?;
                fsutil::fsync_dir(&dir)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(tmp);
            }
            result?;
        }
        self.upload_view(row)
    }
    fn capture_row(row: &Row<'_>) -> rusqlite::Result<Capture> {
        let id: String = row.get(0)?;
        Ok(Capture {
            url: format!("/__cloud/api/files/{id}"),
            id,
            filename: row.get(1)?,
            app: row.get(2)?,
            size: row_u64(row, 3)?,
            sha256: row.get(4)?,
            created_at: row.get(5)?,
        })
    }
    fn capture_db(db: &Connection, owner: &str, id: &str) -> Result<Capture> {
        if !valid_id(id) {
            return Err(missing());
        }
        db.query_row(
            "SELECT id,filename,app,size,sha256,created_at FROM captures WHERE owner=?1 AND id=?2",
            params![owner, id],
            Self::capture_row,
        )
        .optional()?
        .ok_or_else(missing)
    }
    fn project_db(db: &Connection, owner: &str, id: &str) -> Result<Project> {
        if !valid_id(id) {
            return Err(missing());
        }
        let row: Option<(String,String,String,String,String)> = db.query_row(
            "SELECT name,app,created_at,updated_at,head_id FROM projects WHERE owner=?1 AND id=?2",params![owner,id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let (name, app, created_at, updated_at, head_id) = row.ok_or_else(missing)?;
        Ok(Project {
            id: id.into(),
            name,
            app,
            created_at,
            updated_at,
            head: Self::capture_db(db, owner, &head_id)?,
        })
    }
    fn project(&self, owner: &str, id: &str) -> Result<Project> {
        Self::project_db(&self.db.lock(), owner, id)
    }
    fn projects(
        &self,
        owner: &str,
        app: Option<&str>,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<Project>> {
        let limit = i64::try_from(limit).map_err(|_| invalid())?;
        let offset = i64::try_from(offset).map_err(|_| invalid())?;
        let db = self.db.lock();
        let mut q=db.prepare("SELECT id FROM projects WHERE owner=?1 AND (?2 IS NULL OR app=?2) ORDER BY updated_at DESC,rowid DESC LIMIT ?3 OFFSET ?4")?;
        let ids = q
            .query_map(params![owner, app, limit, offset], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter()
            .map(|id| Self::project_db(&db, owner, id))
            .collect()
    }
    fn versions(&self, owner: &str, id: &str) -> Result<Vec<Capture>> {
        let db = self.db.lock();
        Self::project_db(&db, owner, id)?;
        let mut q=db.prepare("SELECT c.id,c.filename,c.app,c.size,c.sha256,c.created_at FROM versions v JOIN captures c ON c.id=v.capture_id WHERE v.project_id=?1 AND c.owner=?2 ORDER BY v.seq DESC LIMIT 1000")?;
        Ok(q.query_map(params![id, owner], Self::capture_row)?
            .collect::<std::result::Result<_, _>>()?)
    }
    fn download(&self, owner: &str, id: &str) -> Result<Capture> {
        let c = Self::capture_db(&self.db.lock(), owner, id)?;
        if regular(&self.blobs.join(id))?.len() != c.size {
            return Err(internal());
        }
        Ok(c)
    }
    fn complete(&self, owner: &str, id: &str) -> Result<Complete> {
        let mut db = self.db.lock();
        let row = Self::upload_row(&db, owner, id)?;
        if let Some(done) = row.result {
            return Ok(done);
        }
        if row.status != "pending" {
            return Err(conflict());
        }
        // A crash can leave a durable but unassociated blob. Replays must account for it,
        // rather than repeatedly materializing another blob outside the configured quota.
        if self.usage(&db)? > self.quota {
            return Err(quota());
        }
        let old = row
            .request
            .project_id
            .as_deref()
            .map(|id| Self::project_db(&db, owner, id))
            .transpose()?;
        if old.as_ref().is_some_and(|p| {
            Some(&p.head.id) != row.request.base_id.as_ref() || p.app != row.request.app
        }) {
            return Err(conflict());
        }
        self.space(row.request.size)?;
        let dir = self.part_dir(id)?;
        let assembled = dir.join(format!("assembled-{}", secure_hex(12)?));
        let result = (|| -> Result<Complete> {
            let mut out = fresh_file(&assembled)?;
            let mut hash = Sha256::new();
            let mut written = 0u64;
            let mut buffer = [0u8; 65536];
            for index in 0..row.request.size.div_ceil(CHUNK_BYTES) {
                let path = dir.join(index.to_string());
                let mut part = read_file(&path).map_err(|e| {
                    if e.0 == StatusCode::INTERNAL_SERVER_ERROR && !path.exists() {
                        CloudError(StatusCode::CONFLICT, "missing_parts")
                    } else {
                        e
                    }
                })?;
                let expected = CHUNK_BYTES.min(row.request.size - index * CHUNK_BYTES);
                if part.metadata()?.len() != expected {
                    return Err(internal());
                }
                loop {
                    let n = part.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    out.write_all(&buffer[..n])?;
                    hash.update(&buffer[..n]);
                    written += n as u64;
                }
            }
            if written != row.request.size || hex(&hash.finalize()) != row.request.sha256 {
                return Err(CloudError(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "checksum_mismatch",
                ));
            }
            out.sync_all()?;
            let when = timeutil::now_iso();
            let unchanged = old.as_ref().is_some_and(|p| {
                p.head.sha256 == row.request.sha256
                    && p.head.size == row.request.size
                    && p.head.filename == row.request.filename
            });
            let capture = if unchanged {
                old.as_ref().unwrap().head.clone()
            } else {
                let capture_id = new_id()?;
                fs::hard_link(&assembled, self.blobs.join(&capture_id))?;
                fsutil::fsync_dir(&self.blobs)?;
                Capture {
                    id: capture_id.clone(),
                    filename: row.request.filename.clone(),
                    app: row.request.app.clone(),
                    size: row.request.size,
                    sha256: row.request.sha256.clone(),
                    created_at: when.clone(),
                    url: format!("/__cloud/api/files/{capture_id}"),
                }
            };
            let project = if let Some(mut p) = old {
                p.name = row.request.filename.clone();
                p.updated_at = if unchanged {
                    p.updated_at
                } else {
                    when.clone()
                };
                p.head = capture.clone();
                p
            } else {
                Project {
                    id: new_id()?,
                    name: row.request.filename.clone(),
                    app: row.request.app.clone(),
                    created_at: when.clone(),
                    updated_at: when,
                    head: capture.clone(),
                }
            };
            let done = Complete {
                project,
                file: capture,
            };
            let result_json = serde_json::to_string(&done).map_err(|_| internal())?;
            let sql_size = i64::try_from(done.file.size).map_err(|_| internal())?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if !unchanged {
                tx.execute("INSERT INTO captures(id,owner,filename,app,size,sha256,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![done.file.id,owner,done.file.filename,done.file.app,sql_size,done.file.sha256,done.file.created_at])?;
            }
            if row.request.project_id.is_some() {
                let n=tx.execute("UPDATE projects SET name=?1,updated_at=?2,head_id=?3 WHERE owner=?4 AND id=?5 AND head_id=?6",
                    params![done.project.name,done.project.updated_at,done.file.id,owner,done.project.id,row.request.base_id])?;
                if n != 1 {
                    return Err(conflict());
                }
            } else {
                tx.execute("INSERT INTO projects(id,owner,name,app,created_at,updated_at,head_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![done.project.id,owner,done.project.name,done.project.app,done.project.created_at,done.project.updated_at,done.file.id])?;
            }
            tx.execute(
                "INSERT OR IGNORE INTO versions(project_id,capture_id) VALUES(?1,?2)",
                params![done.project.id, done.file.id],
            )?;
            tx.execute(
                "UPDATE uploads SET status='complete',result_json=?1 WHERE id=?2 AND owner=?3",
                params![result_json, id, owner],
            )?;
            tx.commit()?;
            Ok(done)
        })();
        let _ = fs::remove_file(&assembled);
        if result.is_ok() {
            self.cleanup_parts(id)?;
        }
        result
    }
    fn cancel(&self, owner: &str, id: &str) -> Result<()> {
        let db = self.db.lock();
        let row = Self::upload_row(&db, owner, id)?;
        if row.status == "complete" {
            return Err(conflict());
        }
        db.execute(
            "UPDATE uploads SET status='cancelled' WHERE id=?1 AND owner=?2",
            params![id, owner],
        )?;
        self.cleanup_parts(id)
    }
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    let mut res = Response::new(Body::from(value.to_string()));
    *res.status_mut() = status;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    serve::set_common_headers(&mut res);
    res
}
fn fail(e: CloudError) -> Response<Body> {
    json_response(e.0, json!({"error":e.1}))
}
fn query(query: Option<&str>) -> Result<BTreeMap<String, String>> {
    fn decode(s: &str) -> Result<String> {
        let mut bytes = Vec::new();
        let mut i = 0;
        while i < s.len() {
            let b = s.as_bytes()[i];
            if b == b'%' {
                let hex = s.get(i + 1..i + 3).ok_or_else(invalid)?;
                bytes.push(u8::from_str_radix(hex, 16).map_err(|_| invalid())?);
                i += 3;
            } else {
                bytes.push(if b == b'+' { b' ' } else { b });
                i += 1;
            }
        }
        String::from_utf8(bytes).map_err(|_| invalid())
    }
    let mut out = BTreeMap::new();
    if let Some(query) = query {
        if query.len() > 2048 {
            return Err(invalid());
        }
        for pair in query.split('&').filter(|s| !s.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if out.insert(decode(k)?, decode(v)?).is_some() {
                return Err(invalid());
            }
        }
    }
    Ok(out)
}

/// This route is distinct from the admin API. It never treats the auth-free anonymous admin
/// as a document owner, even when the rest of the launcher is public.
pub async fn request(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let cfg = s.cfg();
    let method = req.method().clone();
    if !cfg.cloud.enabled {
        return if path == "/__cloud/api/config" && method == Method::GET {
            json_response(StatusCode::OK, json!({"enabled":false}))
        } else {
            fail(missing())
        };
    }
    let context = login::ctx(&s, req.headers(), peer);
    let principal = match login::authenticate(&s, req.headers(), &context) {
        Ok(p) if p.method != "none" => p,
        _ => {
            return fail(CloudError(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
            ));
        }
    };
    let owner = match owner_key(&principal) {
        Some(owner) => owner,
        None => {
            return fail(CloudError(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
            ));
        }
    };
    let namespace = digest(owner.as_bytes());
    if path != "/__cloud/api/config"
        && req
            .headers()
            .get(NAMESPACE_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v != namespace)
    {
        return fail(CloudError(StatusCode::CONFLICT, "identity_changed"));
    }
    if method != Method::GET && method != Method::HEAD {
        if req
            .headers()
            .get(NAMESPACE_HEADER)
            .and_then(|s| s.to_str().ok())
            != Some(namespace.as_str())
        {
            return fail(CloudError(StatusCode::CONFLICT, "identity_changed"));
        }
        let csrf_cookie = auth::cookie(req.headers(), CLOUD_CSRF_COOKIE);
        let csrf_header = req
            .headers()
            .get(auth::CSRF_HEADER)
            .and_then(|v| v.to_str().ok());
        if !csrf_cookie.as_deref().is_some_and(|c| {
            c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()) && Some(c) == csrf_header
        }) || !auth::same_origin(req.headers(), context.host.as_deref())
        {
            return fail(CloudError(StatusCode::FORBIDDEN, "csrf_failed"));
        }
    }
    if path == "/__cloud/api/config" && method == Method::GET {
        let csrf = match auth::cookie(req.headers(), CLOUD_CSRF_COOKIE)
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            Some(s) => s,
            None => match secure_hex(32) {
                Ok(s) => s,
                Err(e) => return fail(e),
            },
        };
        let apps: BTreeMap<_, _> = cfg
            .enabled_apps()
            .map(|a| {
                (
                    a.id.clone(),
                    json!({"entry":a.entry,"label":a.name,"origin":a.origin}),
                )
            })
            .collect();
        let mut res = json_response(
            StatusCode::OK,
            json!({"enabled":true,"maxFileBytes":cfg.cloud.max_file_bytes,"chunkBytes":CHUNK_BYTES,"namespace":namespace,"csrfToken":csrf,"apps":apps}),
        );
        let secure = if auth::cookie_secure(&cfg.auth, context.https) {
            "; Secure"
        } else {
            ""
        };
        if let Ok(v) = HeaderValue::from_str(&format!(
            "{CLOUD_CSRF_COOKIE}={csrf}; Path=/; HttpOnly; SameSite=Strict{secure}"
        )) {
            res.headers_mut().insert(header::SET_COOKIE, v);
        }
        return res;
    }
    let Some(store) = s.cloud.clone() else {
        return fail(internal());
    };
    let parts: Vec<&str> = path
        .strip_prefix("/__cloud/api/")
        .unwrap_or("")
        .split('/')
        .collect();
    if method == Method::GET && parts.len() == 2 && parts[0] == "files" {
        let id = parts[1].to_string();
        let owner = owner.clone();
        let st = store.clone();
        let capture = match tokio::task::spawn_blocking(move || st.download(&owner, &id)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return fail(e),
            Err(_) => return fail(internal()),
        };
        let mut res = serve::serve_dir(
            store.blobs.clone(),
            &format!("/{}", capture.id),
            req,
            serve::CachePolicy::NoStore,
        )
        .await;
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        let filename = auth::query_escape(&capture.filename);
        if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{filename}")) {
            res.headers_mut().insert(header::CONTENT_DISPOSITION, v);
        }
        return res;
    }
    let query = match query(req.uri().query()) {
        Ok(q) => q,
        Err(e) => return fail(e),
    };
    let is_part =
        method == Method::PUT && parts.len() == 4 && parts[0] == "uploads" && parts[2] == "parts";
    let body_limit = if is_part {
        CHUNK_BYTES as usize
    } else {
        64 << 10
    };
    let body = match to_bytes(req.into_body(), body_limit).await {
        Ok(b) => b,
        Err(_) => return fail(CloudError(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large")),
    };
    let parts: Vec<String> = parts.into_iter().map(str::to_string).collect();
    let allowed: BTreeSet<String> = cfg.enabled_apps().map(|a| a.id.clone()).collect();
    let result=tokio::task::spawn_blocking(move||->Result<Value>{
        match (method.as_str(),parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice()) {
            ("GET",["projects"])=>{
                let app=query.get("app");if app.is_some_and(|a|!allowed.contains(a)){return Err(invalid());}
                let limit=query.get("limit").map(|v|v.parse::<u64>().map_err(|_|invalid())).transpose()?.unwrap_or(100);
                let offset=query.get("offset").map(|v|v.parse::<u64>().map_err(|_|invalid())).transpose()?.unwrap_or(0);
                if !(1..=1000).contains(&limit)||offset>1_000_000{return Err(invalid());}
                Ok(json!({"projects":store.projects(&owner,app.map(String::as_str),limit,offset)?,"limit":limit,"offset":offset}))
            }
            ("GET",["projects",id])=>Ok(json!(store.project(&owner,id)?)),
            ("GET",["projects",id,"versions"])=>Ok(json!({"files":store.versions(&owner,id)?})),
            ("POST",["uploads"])=>{let r:UploadRequest=serde_json::from_slice(&body).map_err(|_|invalid())?;if !allowed.contains(&r.app){return Err(invalid());}Ok(json!(store.init(&owner,r)?))}
            ("GET",["uploads",id])=>Ok(json!(store.status(&owner,id)?)),
            ("PUT",["uploads",id,"parts",index])=>{let index=index.parse().map_err(|_|invalid())?;Ok(json!(store.put_part(&owner,id,index,&body)?))}
            ("POST",["uploads",id,"complete"]) if body.is_empty() || body.as_ref()==b"{}"=>Ok(json!(store.complete(&owner,id)?)),
            ("DELETE",["uploads",id])=>{store.cancel(&owner,id)?;Ok(json!({"cancelled":true}))}
            _=>Err(missing()),
        }
    }).await;
    match result {
        Ok(Ok(v)) => json_response(StatusCode::OK, v),
        Ok(Err(e)) => fail(e),
        Err(_) => fail(internal()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(data: &[u8]) -> UploadRequest {
        UploadRequest {
            operation_id: new_id().unwrap(),
            app: "photocraft".into(),
            filename: "Project.psd".into(),
            size: data.len() as u64,
            sha256: digest(data),
            intent: "save".into(),
            project_id: None,
            base_id: None,
        }
    }
    fn save(s: &CloudStore, owner: &str, r: UploadRequest, data: &[u8]) -> Complete {
        let u = s.init(owner, r).unwrap();
        for (i, p) in data.chunks(CHUNK_BYTES as usize).enumerate() {
            s.put_part(owner, &u.id, i as u64, p).unwrap();
        }
        s.complete(owner, &u.id).unwrap()
    }
    fn store(tmp: &tempfile::TempDir, quota: u64) -> CloudStore {
        CloudStore::open(tmp.path(), quota, quota, 0).unwrap()
    }

    #[test]
    fn durable_versions_cas_noop_and_operation_replay() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(&tmp, 1 << 20);
        let r = request(b"one");
        let first = save(&s, "db:1", r.clone(), b"one");
        assert_eq!(s.versions("db:1", &first.project.id).unwrap().len(), 1);
        let mut next = request(b"two");
        next.project_id = Some(first.project.id.clone());
        next.base_id = Some(first.file.id.clone());
        let competing = s.init("db:1", next.clone()).unwrap();
        s.put_part("db:1", &competing.id, 0, b"two").unwrap();
        let mut next2 = next.clone();
        next2.operation_id = new_id().unwrap();
        let second = save(&s, "db:1", next2, b"two");
        assert_eq!(
            s.complete("db:1", &competing.id).unwrap_err().0,
            StatusCode::CONFLICT
        );
        let replay = s.init("db:1", r).unwrap();
        assert_eq!(replay.project.unwrap().head.id, first.file.id);
        assert_eq!(
            s.project("db:1", &first.project.id).unwrap().head.id,
            second.file.id
        );
        let mut noop = request(b"two");
        noop.project_id = Some(first.project.id.clone());
        noop.base_id = Some(second.file.id.clone());
        assert_eq!(save(&s, "db:1", noop, b"two").file.id, second.file.id);
        assert_eq!(s.versions("db:1", &first.project.id).unwrap().len(), 2);
        drop(s);
        let reopened = store(&tmp, 1 << 20);
        assert_eq!(
            reopened.project("db:1", &first.project.id).unwrap().head.id,
            second.file.id
        );
        assert_eq!(
            fs::read(reopened.blobs.join(second.file.id)).unwrap(),
            b"two"
        );
    }
    #[test]
    fn every_object_and_operation_is_scoped_to_its_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(&tmp, 1 << 20);
        let r = request(b"secret");
        let u = s.init("db:1", r.clone()).unwrap();
        assert_eq!(
            s.status("db:2", &u.id).unwrap_err().0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            s.put_part("db:2", &u.id, 0, b"secret").unwrap_err().0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            s.cancel("db:2", &u.id).unwrap_err().0,
            StatusCode::NOT_FOUND
        );
        let first = save(&s, "db:1", r.clone(), b"secret");
        let other = save(&s, "db:2", r, b"secret");
        assert_ne!(first.file.id, other.file.id);
        assert_ne!(first.project.id, other.project.id);
        for id in [&first.project.id, &first.file.id] {
            assert_eq!(s.project("db:2", id).unwrap_err().0, StatusCode::NOT_FOUND);
        }
        assert_eq!(
            s.download("db:2", &first.file.id).unwrap_err().0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            s.versions("db:2", &first.project.id).unwrap_err().0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(s.projects("db:2", None, 100, 0).unwrap().len(), 1);
    }
    #[test]
    fn parts_hash_quota_and_cancel_preserve_committed_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(&tmp, 20);
        let r = request(b"1234567890");
        let u = s.init("db:1", r.clone()).unwrap();
        assert_eq!(
            s.init("db:2", request(b"abcdefghijkl")).unwrap_err().0,
            StatusCode::INSUFFICIENT_STORAGE
        );
        assert!(s.put_part("db:1", &u.id, 0, b"short").is_err());
        s.put_part("db:1", &u.id, 0, b"1234567890").unwrap();
        s.put_part("db:1", &u.id, 0, b"1234567890").unwrap();
        assert_eq!(
            s.put_part("db:1", &u.id, 0, b"abcdefghij").unwrap_err().0,
            StatusCode::CONFLICT
        );
        let done = s.complete("db:1", &u.id).unwrap();
        assert_eq!(s.cancel("db:1", &u.id).unwrap_err().0, StatusCode::CONFLICT);
        let mut changed = r;
        changed.filename = "Other.psd".into();
        assert_eq!(s.init("db:1", changed).unwrap_err().0, StatusCode::CONFLICT);
        let pending = s.init("db:1", request(b"1234567890")).unwrap();
        s.cancel("db:1", &pending.id).unwrap();
        assert!(s.download("db:1", &done.file.id).is_ok());
        let mut wrong = request(b"abc");
        wrong.sha256 = digest(b"xyz");
        let u = s.init("db:1", wrong).unwrap();
        s.put_part("db:1", &u.id, 0, b"abc").unwrap();
        assert_eq!(
            s.complete("db:1", &u.id).unwrap_err().1,
            "checksum_mismatch"
        );
    }
    #[test]
    fn invalid_names_symlinks_and_a_second_writer_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(&tmp, 100);
        for name in ["../x", "x/y", "x\\y", "x\nY", ""] {
            let mut r = request(b"a");
            r.filename = name.into();
            assert!(s.init("db:1", r).is_err());
        }
        assert!(CloudStore::open(tmp.path(), 100, 100, 0).is_err());
        let u = s.init("db:1", request(b"a")).unwrap();
        let path = s.part_dir(&u.id).unwrap().join("0");
        let outside = tmp.path().join("outside");
        fs::write(&outside, b"safe").unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(s.put_part("db:1", &u.id, 0, b"a").is_err());
        assert_eq!(fs::read(outside).unwrap(), b"safe");
    }
    #[test]
    fn completing_a_pending_upload_accounts_for_crash_orphans() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(&tmp, 20);
        let u = s.init("db:1", request(b"abc")).unwrap();
        s.put_part("db:1", &u.id, 0, b"abc").unwrap();
        fs::write(s.blobs.join(new_id().unwrap()), [0; 18]).unwrap();
        let before = fs::read_dir(&s.blobs).unwrap().count();
        assert_eq!(
            s.complete("db:1", &u.id).unwrap_err().0,
            StatusCode::INSUFFICIENT_STORAGE
        );
        assert_eq!(fs::read_dir(&s.blobs).unwrap().count(), before);
        assert_eq!(s.status("db:1", &u.id).unwrap().status, "pending");
    }

    #[test]
    fn negative_sqlite_sizes_are_rejected_without_unsigned_wrapping() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(&tmp, 100);
        let done = save(&s, "db:1", request(b"abc"), b"abc");
        s.db.lock()
            .execute("UPDATE captures SET size=-1 WHERE id=?1", [&done.file.id])
            .unwrap();
        assert_eq!(
            s.download("db:1", &done.file.id).unwrap_err().0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        let pending = s.init("db:1", request(b"xyz")).unwrap();
        s.db.lock()
            .execute("UPDATE uploads SET size=-1 WHERE id=?1", [&pending.id])
            .unwrap();
        assert_eq!(
            s.usage(&s.db.lock()).unwrap_err().0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
