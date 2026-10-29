# Architecture and design decisions

## Components

| Path | Contents |
| --- | --- |
| `compose.yaml`, `compose.logs.yaml` | The `host` service: identity, mounts, health check; optional log mount |
| `host/` | Rust crate `craft-host` (server, updater, admin, CLI) and its `Dockerfile` |
| `host/assets/` | Launcher, admin and sign-in pages, compiled into the binary |
| `host/assets/vendor/craft-launcher/` | ArtCraft Launcher catalogue, translations and logos, vendored |
| `host/src/manifest.toml` | Built-in application manifest, compiled into the binary |
| `unraid/` | Unraid container template |
| `tests/integration/` | Compose override used by the Docker integration test |

Modules of `craft-host`:

| Module | Role |
| --- | --- |
| `server`, `serve` | HTTP routing, launcher, `/status.json`, health, app paths, static file serving |
| `login`, `auth`, `oidc`, `users`, `access` | sign-in (`/auth/`), roles, apps gate, sessions, throttling, CSRF, proxy identities, OIDC, accounts in SQLite |
| `admin` | `/admin` pages and API, background admin jobs |
| `assets`, `catalog`, `shots` | embedded launcher files, the vendored catalogue, the screenshot proxy and cache |
| `discovery` | new apps from the ArtCraft release feed, their page metadata and logos |
| `daemon`, `activity` | process start, updater, discovery and heartbeat threads, idle activation, request activity |
| `ops` | update lifecycle, activation policy, pins, rollback, retention |
| `github`, `archive`, `validate`, `precompress`, `candidate` | release lists and downloads, zip safety, content checks, `.br`/`.gz` copies, serving check |
| `layout`, `store`, `config`, `status`, `doctor` | data layout and reconciliation, state and lock, configuration, status, diagnostics |

One process: the HTTP server runs on a Tokio runtime; the updater, discovery, the heartbeat and
admin actions run on their own threads and never block serving. Discovered apps join the running
configuration, which is replaced as a whole, so each request works on one consistent snapshot.

## Publication model

```mermaid
sequenceDiagram
  participant U as updater thread
  participant FS as DATA_DIR
  participant C as candidate listener (127.0.0.1)
  participant S as HTTP server
  U->>FS: extract to .staging/<app>-<v>-<rand>/site, validate, add .br/.gz
  U->>C: GET /candidate/<staging>/site/… (MIME, compression, references)
  U->>FS: write .craft-release.json; rename site → releases/<app>/<v> (atomic)
  alt immediate, or idle long enough
    U->>FS: symlink .current.tmp → <v>; rename over current (atomic)
  else app recently requested
    U->>FS: record <v> as pending; activate later
  end
  S->>FS: per request to /<app>/: read current, serve releases/<app>/<v>/…
```

- A release directory either does not exist or is complete: it appears through one `rename(2)`
  inside `DATA_DIR`. The `current` pointer is replaced with `rename(2)` of a new symlink and read
  on every request to the stable path, so activation needs no restart or reload.
- `/<app>/` serves the active release itself; `/<app>/<version>/` serves a retained one. Versioned
  URLs are immutable and cached for a year; stable URLs, entry pages, `sw.js` and manifests
  revalidate with release-scoped entity tags.
- The serving check uses the production file-serving code (`serve::serve_dir`) on a loopback
  listener that serves only `.staging`, so a candidate is tested exactly as it will be served
  without ever being reachable from outside.
- Only release files are served. Paths are percent-decoded and refused if any segment is hidden
  (`.craft-release.json`, `.htaccess`, `..`) or contains a backslash or NUL; the version segment
  must name a directory with a release marker; `current` is never a valid version.

## Design decisions

- **One Rust binary** serves, updates and administers, written in Rust like the Craft apps.
  Consequences:
  - the serving process can write releases and read the GitHub token and admin secrets; the
    path checks above, a read-only root filesystem, dropped capabilities and a non-root identity
    are the boundary;
  - an updater fault cannot stop the server (the health check only probes `/healthz`), but
    updating the binary restarts serving briefly;
  - static serving, precompression and caching are implemented in the binary (tower-http's
    `ServeDir`).
- **Activation policy.** The server only observes requests, so `idle` activation is a
  request-inactivity heuristic; immediate activation is the default.
- **Stable app URLs, one release by default.** `/<app>/` serves the active release directly
  instead of redirecting to `/<app>/<version>/`. That gives up the guarantee that an open tab
  keeps loading files of its own release; the server narrows the gap by revalidating every
  response with release-scoped entity tags and by serving files missing from the active release
  from retained ones. Retention keeps only the active release by default.
- **One port, roles.** Launcher, apps, sign-in and `/admin` share one listener. `[auth]` signs
  people in for the whole site; the admin role manages updates, and `[auth] apps = "signed-in"`
  optionally limits the apps to accounts with the user or admin role. `/admin` therefore shares
  the apps' origin unless `[admin] host` gives it a host name of its own; the trade-off (app code
  could use an administrator's session) is documented in [admin.md](admin.md).
- **Discovery reads only machine-readable data.** New apps come from the release feed ArtCraft
  Launcher uses; their presentation from the schema.org data and `og:` tags of their pages, and
  only from URLs on that site. They install through the same verified path as built-in apps.
- **LightCraft exclusions.** The manifest's `exclude` list removes the Cargo build directory that
  LightCraft ships inside its web archive. Native executables anywhere else cause rejection. No
  application file is modified; precompressed copies are added alongside.
- **Separate origins** are a link-level option (`origin`), because the server answers on any host
  name and all apps work under a path prefix.
- **Document storage is opt-in.** The default remains a static app host. With `[cloud] enabled`,
  `cloud` stores immutable document versions in a separate private state directory and uses the
  existing authenticated principal on every request. Verified app releases are not modified.
  A runtime-injected common client connects only to an explicit native editor bridge. See
  [the storage and bridge contract](cloud-storage.md).
