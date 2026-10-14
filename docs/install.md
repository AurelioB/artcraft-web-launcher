# Installation and deployment

## Requirements

- Docker Engine with Compose v2 (Linux host recommended; see [permissions](permissions.md) for
  Docker Desktop and user-namespace notes), or a Linux machine for the standalone binary.
- Outbound HTTPS to `api.github.com`, `github.com` and GitHub's asset host
  (`release-assets.githubusercontent.com` as of October 2026) for updates, and to
  `api.storyteller.ai` and `getartcraft.com` for app discovery. Installed apps keep being served
  without them.
- An HTTPS reverse proxy for anything other than `localhost` (see [HTTPS](#https-and-the-reverse-proxy)).
- Disk: one release of each of the twelve apps of 12 October 2026 took about 730 MB in
  `DATA_DIR`, of which about 300 MB are `.br`/`.gz` copies (written at install time, or shipped
  by LightCraft). The downloaded archives take about 250 MB in `CACHE_DIR` (LightCraft's alone is
  100 MB). `keep_latest` can retain more releases per app; the archive cache is bounded by
  `cache_max_size`.

## 1. Choose identity and directories

Edit `.env` (copy `.env.example`). Host paths are bind-mount sources; container paths are fixed:

| Setting | Container path | Access | Holds |
| --- | --- | --- | --- |
| `CONFIG_DIR` | `/config` | read-only | `config.toml`, GitHub token, OIDC client secret |
| `DATA_DIR` | `/srv/data` | read-write | releases (`releases/<app>/<version>`, `current` pointers), `.staging/` |
| `STATE_DIR` | `/srv/state` | read-write | per-app state, history, lock, heartbeat, accounts (`users.sqlite3`), discovered apps |
| `CACHE_DIR` | `/srv/cache` | read-write | verified release archives, API response cache, screenshots |
| `WORK_DIR` | `/srv/work` | read-write | in-progress downloads |
| `LOG_DIR` (optional) | `/srv/logs` | read-write | rotated `updater.log` |
| `WORKING_DIR` | — | — | working directory of the process (default `/srv/work`) |

The other `.env` settings: `RUN_UID`/`RUN_GID` (required non-root identity), `SUPPLEMENTAL_GID`
(optional extra group), `FILE_UMASK`, `BIND_ADDRESS`/`PORT` (published listener, default
`0.0.0.0:8080`), `IMAGE` (default `ghcr.io/aureliob/artcraft-web-launcher:latest`), `LOG_LEVEL`
and `COMPOSE_FILE` (for [persistent logs](#persistent-logs)).

- Relative host paths resolve from the project directory; production setups should use absolute
  paths. Paths may contain spaces.
- The directories must be separate and not nested in each other; `doctor` checks this.
- `WORK_DIR` and `CACHE_DIR` may be on other filesystems. Staging always happens inside
  `DATA_DIR/.staging` so that publishing is one atomic rename on one filesystem.
- No document workspace is mounted: the browser apps cannot use server folders.

## 2. Prepare the directories

Compose uses `create_host_path: false`, so missing directories make `docker compose up` fail
instead of Docker creating root-owned ones. Nothing ever changes ownership. Create them once:

```sh
sudo install -d -o "$RUN_UID" -g "$RUN_GID" -m 2775 \
  /srv/craft-apps/config /srv/craft-apps/data /srv/craft-apps/state \
  /srv/craft-apps/cache /srv/craft-apps/work
sudo install -m 0640 -o "$RUN_UID" -g "$RUN_GID" config.example.toml /srv/craft-apps/config/config.toml
```

### GitHub token

Optional, but recommended: it raises the API limit from 60 to 5000 requests per hour, and
unchanged release lists then cost no quota. Without one, hourly checks use one request per app
(13 with the built-in apps), and the 60 are shared by everything behind the same public IP
address. A fine-grained token with read-only access to public repositories is enough:

```sh
install -m 0400 -o "$RUN_UID" /dev/stdin /srv/craft-apps/config/github-token <<<"github_pat_…"
# config.toml: [updater] github_token_file = "github-token"
```

Secrets in `CONFIG_DIR` are never logged, printed or served.

## 3. Check and start

```sh
docker compose pull
docker compose run --rm host doctor --offline
docker compose up -d
docker compose exec host craft-host doctor     # includes GitHub and OIDC connectivity
```

`doctor` exits non-zero on any `FAIL` and prints a preparation command for each problem. To
enable the administration page or require sign-in for the apps, follow [admin.md](admin.md).
`docker compose build` builds the image from the source checkout instead of pulling it.

## Unraid

Unraid manages containers from its Docker page. Prepare the five appdata directories once in the
Unraid terminal for the `nobody`/`users` identity (99:100) that the container runs as:

```sh
BASE=/mnt/user/appdata/artcraft-web-launcher
mkdir -p "$BASE"/{config,data,state,cache,work}
if [ ! -f "$BASE/config/config.toml" ]; then
  curl -fsSL -o "$BASE/config/config.toml" \
    https://raw.githubusercontent.com/AurelioB/artcraft-web-launcher/main/config.example.toml
fi
chown -R 99:100 "$BASE"
chmod 2775 "$BASE"/{config,data,state,cache,work}
chmod 0640 "$BASE/config/config.toml"
```

Add the template to the user templates on the flash drive:

```sh
curl -fsSL -o /boot/config/plugins/dockerMan/templates-user/my-artcraft-web-launcher.xml \
  https://raw.githubusercontent.com/AurelioB/artcraft-web-launcher/main/unraid/artcraft-web-launcher.xml
```

Then open **Docker → Add Container**, choose `artcraft-web-launcher` under **User templates**,
check the five paths (and the matching `CRAFT_HOST_*` variables under **Show more settings** if
you changed them) and press **Apply**. The launcher is at `http://<unraid-ip>:8080/`. The template
applies the same identity, read-only `/config`, read-only root filesystem, dropped capabilities
and `no-new-privileges` as the Compose file.

The image has no shell: run commands with `docker exec` and read output with `docker logs`:

```sh
docker exec artcraft-web-launcher craft-host doctor
docker exec -i artcraft-web-launcher craft-host user add NAME --role admin <<<'a long passphrase'
```

## docker run

After preparing the directories and `config.toml` as above (replace the sample identity and
paths with yours):

```sh
docker run -d --name artcraft-web-launcher --init --restart unless-stopped \
  --user 1000:10000 --group-add 10000 \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --workdir /srv/work --tmpfs /tmp:mode=1777,size=16m \
  -p 0.0.0.0:8080:8080 \
  -e FILE_UMASK=0002 -e CRAFT_LOG_LEVEL=info \
  -e RUN_UID=1000 -e RUN_GID=10000 \
  -e CRAFT_HOST_CONFIG_DIR=/srv/craft-apps/config \
  -e CRAFT_HOST_DATA_DIR=/srv/craft-apps/data \
  -e CRAFT_HOST_STATE_DIR=/srv/craft-apps/state \
  -e CRAFT_HOST_CACHE_DIR=/srv/craft-apps/cache \
  -e CRAFT_HOST_WORK_DIR=/srv/craft-apps/work \
  --mount type=bind,src=/srv/craft-apps/config,dst=/config,readonly \
  --mount type=bind,src=/srv/craft-apps/data,dst=/srv/data \
  --mount type=bind,src=/srv/craft-apps/state,dst=/srv/state \
  --mount type=bind,src=/srv/craft-apps/cache,dst=/srv/cache \
  --mount type=bind,src=/srv/craft-apps/work,dst=/srv/work \
  ghcr.io/aureliob/artcraft-web-launcher:latest
```

The image runs `craft-host serve` on port 8080. `RUN_UID`, `RUN_GID` and `CRAFT_HOST_*` are only
used by `doctor` to check the host side. For rotated log files, mount a writable directory at
`/srv/logs` and set `CRAFT_LOG_DIR=/srv/logs`.

## Without Docker

Download the static Linux binary for your architecture from
[Releases](https://github.com/AurelioB/artcraft-web-launcher/releases)
(`craft-host-X.Y.Z-x86_64-linux-musl.tar.gz` or `craft-host-X.Y.Z-aarch64-linux-musl.tar.gz`),
check it against the release's `SHA256SUMS`, and put `craft-host` in your `PATH`. Or build it:
`cd host && cargo build --release --locked` (output in `target/release/craft-host`).

Run it as a non-root user who can read the config and write the other directories. Without
overrides it expects `/config/config.toml`, `/srv/data`, `/srv/state`, `/srv/cache` and
`/srv/work`; set your own paths in an environment file, for example `/etc/craft-host/host.env`:

```sh
CRAFT_CONFIG=/etc/craft-host/config.toml
CRAFT_DATA_DIR=/var/lib/craft-host/data
CRAFT_STATE_DIR=/var/lib/craft-host/state
CRAFT_CACHE_DIR=/var/cache/craft-host
CRAFT_WORK_DIR=/var/lib/craft-host/work
FILE_UMASK=0002
```

Create the directories, copy `config.example.toml` to `CRAFT_CONFIG`, and run it as a systemd
service:

```ini
[Unit]
Description=ArtCraft Web Launcher

[Service]
User=craft-host
Group=craft-host
EnvironmentFile=/etc/craft-host/host.env
ExecStart=/usr/local/bin/craft-host serve
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

The [configuration reference](configuration.md#process-environment) lists every variable.

## HTTPS and the reverse proxy

The container publishes plain HTTP on `BIND_ADDRESS:PORT` (default `0.0.0.0:8080`, every host
interface). Docker-published ports bypass host firewalls such as ufw; set `BIND_ADDRESS` to one
address (for example the LAN address, or `127.0.0.1` behind a local proxy) to narrow it.

Plain HTTP from another machine is not a secure context. Observed in Chromium over
`http://<LAN address>`: the classic apps load and show their start screens, but `navigator.gpu`
(WebGPU) and the Origin Private File System are unavailable, as is the clipboard API. According
to the apps' own hosting notes, rendering then falls back to WebGL2, and EffectCraft and LightCraft
fall back from OPFS to IndexedDB (EffectCraft: or memory only, which keeps nothing after the tab
closes). Saving and reopening projects over plain HTTP has not been tested. For full
functionality on the LAN, serve it through an HTTPS reverse proxy with a certificate the clients
trust:

- Redirects are relative, so the site works under any host name and behind a path prefix.
- List the proxy in `[server] trusted_proxies` so `X-Forwarded-Proto/-Host` are honoured (secure
  cookies, admin host matching) and, with `[auth] methods = ["proxy"]`, its identity headers.
- `/admin` is on the apps' port; optionally give it a host name of its own (`[admin] host`), see
  [admin.md](admin.md#same-origin-as-the-apps).
- Browser storage is per origin: a LAN name and a public name for the same server have separate
  app libraries. Pick one canonical host name per app.
- `/healthz` (process alive) and `/readyz/<app>` (app installed) suit proxy health checks.

## Upgrading

App updates are automatic. To update the launcher itself:

```sh
docker compose pull
docker compose up -d
```

Installed releases, state and cache are kept. With a standalone binary, replace it and restart
the service.

Up to 0.3, the image was published as `ghcr.io/aureliob/craft-host`. That name gets no further
updates, and the new name starts at 0.4.0: if `.env` sets `IMAGE`, change it to
`ghcr.io/aureliob/artcraft-web-launcher` with `latest`, `0.4` or a later tag. On Unraid, edit the
existing container and set **Repository** to `ghcr.io/aureliob/artcraft-web-launcher:latest`;
keep its paths so it continues with the same data.

Configurations with older sign-in settings (`[auth] method`, `users_file`, or `[admin] auth`, …)
are rejected with a message per moved key; see
[admin.md](admin.md#upgrading-from-earlier-sign-in-settings). A users file is imported once with
`craft-host user import`.

## Persistent logs

Container logs (`docker compose logs host`) are the default. To also keep rotated files:

```dotenv
LOG_DIR=/srv/craft-apps/logs
COMPOSE_FILE=compose.yaml:compose.logs.yaml
```

Create `LOG_DIR` like the other directories. Size and count follow `log_max_size` and
`log_backups` in `config.toml`.
