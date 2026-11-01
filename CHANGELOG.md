# Changelog

All notable changes to ArtCraft Web Launcher (formerly Craft Apps Host) are documented here.
This project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.5.2] - 2026-11-01

### Changed

- Without a GitHub token, the defaults now fit GitHub's limit of 60 API requests per hour per IP
  address, which other unauthenticated clients behind the same address share:
  - scheduled checks need at most 30 requests per hour: `check_interval` is stretched to at
    least 2 minutes per enabled app (unchanged for the 15 built-in apps at the default `1h`);
  - scheduled checks pause for the rest of the window once GitHub reports 10 or fewer requests
    left, keeping them for manual checks and other clients.
- A scheduled check that GitHub refuses for its rate limit waits for the reset instead of
  recording an error, and the rest of that round no longer asks GitHub. The quota GitHub reports
  is kept in the API cache, so the server and CLI commands share it. Manual checks still report
  the refusal.

## [0.5.1] - 2026-11-01

### Added

- **Craft Anything** (`storytold/craft-anything`), added to the ArtCraft Launcher catalogue in
  its 0.12.0, is built in and serves at `/craft-anything/`. The ArtCraft release feed does not
  list it yet, so discovery could not find it.

### Fixed

- EffectCraft 0.19.0 failed validation ("sw.js references "/effectcraft-config.json" with a
  root-absolute URL") and stayed on 0.18.0. Its service worker only compares request paths with
  that name to leave an optional config file alone; literals passed to `endsWith`,
  `startsWith`, `includes`, `indexOf` or `lastIndexOf` are no longer treated as asset
  references. Root-absolute paths that are requested are still refused.

### Changed

- The vendored ArtCraft Launcher catalogue and translations are refreshed to upstream
  `ffa3440`; the sync script reads its new `mac_team` field.

## [0.5.0] - 2026-10-29

### Added

- **SolidCraft**, the community app ArtCraft Launcher 0.10.0 added to its catalogue, is built in:
  it installs from `tangency-labs/solidcraft` like the suite apps (`solidcraft-web-<v>.zip`,
  checked against `SHA256SUMS.txt`) and serves at `/solidcraft/`. The ArtCraft release feed does
  not list it, so discovery could not find it.
- The launcher shows who makes each catalogue app, as ArtCraft Launcher does: "by ArtCraft" for
  official apps, "by Tangency Labs" in muted text for community apps.

### Changed

- Discovery accepts apps from any GitHub account, not only `storytold`: a feed entry becomes an
  app when its latest release has a `<id>-web-<version>.zip` archive. A discovered app keeps the
  repository it was first found with; ArtCraft Launcher's own entry is never an app.
- The vendored ArtCraft Launcher catalogue and translations are refreshed to upstream
  `c1d0d88` (PdfCraft's red brand colour, FigCraft and SoundCraft earlier in the suite order).
  `status.json` replaces each app's `group` ("classic"/"new", dropped upstream) with `kind`
  ("official" or "community") and `publisher`.
- Built-in logos may be PNG; SolidCraft's is the icon ArtCraft Launcher embeds, served with its
  MIT and Apache-2.0 licences under `/launcher/icons/`.

## [0.4.0] - 2026-10-14

### Changed

- Renamed to **ArtCraft Web Launcher**: the launcher header, page titles and sign-in realm, the
  repository (now `AurelioB/artcraft-web-launcher`; the old URL redirects) and the container
  image, now published as `ghcr.io/aureliob/artcraft-web-launcher`. The binary (`craft-host`),
  its environment variables, `config.toml`, the Compose service and the data layout are
  unchanged.
- The Unraid template is `unraid/artcraft-web-launcher.xml`, with the container and appdata
  directory named `artcraft-web-launcher` for new installations.
- README rewritten; installation details (Unraid, `docker run`, standalone binary) moved to
  `docs/install.md`. The original design plan is removed from the documentation.

### Upgrading

- `ghcr.io/aureliob/craft-host` receives no further releases, and the new name starts at 0.4.0.
  If `.env` sets `IMAGE`, change it to `ghcr.io/aureliob/artcraft-web-launcher` with `latest`,
  `0.4` or a later tag; the default already uses the new name. On Unraid, change only the
  container's **Repository** and keep its paths.

## [0.3.1] - 2026-10-13

### Added

- **Refresh app info** in `/admin` reads the ArtCraft feed at once and every discovered app's
  page and logo again, and drops their cached screenshots. The page shows when the feed was
  last read.
- `[discovery] interval` (default `"1d"`, at least `"1h"`).

### Changed

- The ArtCraft feed is read once a day instead of once per `check_interval` (hourly), and the
  time of the last read is kept, so restarts no longer read it again; a failed read is retried
  an hour later. Pages, logos and screenshots were already fetched only once.
- The feed, app pages and pictures are requested gzipped (the feed shrinks from 384 KB to
  about 95 KB); size limits apply to the decompressed data.

## [0.3.0] - 2026-10-13

### Added

- New Craft apps are discovered at run time from the ArtCraft release feed (at startup, then
  once per `check_interval`) and installed like the built-in ones, without a new image or a
  config change. Their name, description, logo and screenshots come from the app's page on
  getartcraft.com (schema.org data and `og:` tags; only that site's URLs, also across
  redirects, with bounded downloads). Discovered apps are kept in `STATE_DIR/discovered`;
  apps from the manifest or `config.toml` always take precedence. New `[discovery]` settings
  `feed_url` and `metadata_url` (`""` turns either off).
- The launcher shows app pages for discovered apps, with their screenshots served through the
  server like the catalogue's.

### Changed

- `/launcher/shots/<app>/<file>` also serves PNG and WebP pictures (discovered apps).

## [0.2.4] - 2026-10-13

### Added

- FigCraft (interface design and prototyping), the newest Craft app, installed automatically
  like the others. The vendored ArtCraft Launcher catalogue and translations are refreshed to
  upstream `cbae640`.

## [0.2.3] - 2026-10-13

### Changed

- The launcher's Open button is outlined, as in ArtCraft Launcher, instead of filled.

## [0.2.2] - 2026-10-12

### Changed

- The launcher shows every enabled app in one list: no Classic/New groups and no
  Installed/Not installed filters. Apps are numbered in display order; only apps that are not
  ready show a status ("Installing…", "Not available"), and the table shows App, Version and
  Released. Which apps appear is decided by enabling or disabling them in `/admin`.

## [0.2.1] - 2026-10-12

### Security

- With `[auth] apps = "signed-in"`, launcher screenshots (`/launcher/shots/…`) now need a
  signed-in account like the apps. In 0.2.0 they were open to anyone: the pictures are public
  upstream, but their availability showed which apps are enabled and an anonymous visitor could
  make the server fetch them.

## [0.2.0] - 2026-10-12

### Added

- WordCraft, DeckCraft, GridCraft, CADCraft and SoundCraft, the five new apps of the Craft App
  Suite. Like the seven classic apps, they are installed and kept up to date automatically.
- Administrators can disable and enable apps in `/admin`. A disabled app is no longer served,
  listed or updated (its releases stay on disk); enabling it brings it up to date at once. The
  choice survives restarts and takes precedence over `enabled` in `config.toml`.
- A launcher in the style of the official ArtCraft Launcher: grid and table views, filters,
  sorting, search, the suite's groups, app pages with features and screenshots, and the
  launcher's translations (Czech, German, Spanish, French, Hungarian, Japanese, Polish, Brazilian
  Portuguese, Russian and Simplified Chinese). Its catalogue, texts and translations are vendored
  from `storytold/craft-launcher` (MIT OR Apache-2.0) by `scripts/sync-craft-launcher.py`.
- `[launcher] screenshots_url`: the server fetches app-page screenshots once from the pinned
  upstream tag and caches them; browsers never contact GitHub. `""` turns them off.
- `status.json` reports each app's repository, product page, suite group, text colors, active
  release tag and publication date, and whether `/admin` overrides `enabled`.

### Changed

- Apps follow the suite's naming conventions by default: repository `storytold/<id>` and archive
  `<id>-web-{version}.zip` (plus former names). A Craft app added in `config.toml` needs only
  `name`; `repository` and `artifact_patterns` are optional.
- Names, taglines and colors of built-in apps come from the vendored catalogue; taglines are
  now the launcher's short descriptions.
- PdfCraft is fetched from `storytold/pdfcraft`, the repository's new name.
- Without a configured `icon`, the launcher shows a release's own favicon (found in its
  `index.html` at install time) instead of a monogram.

## [0.1.1] - 2026-10-10

### Fixed

- OpenID Connect sign-in started on another host name for the server (for example a LAN alias
  behind a local reverse proxy) now continues on the host of `redirect_url`, so the browser-bound
  state and the session cookie land where the provider returns. Previously such a sign-in was
  refused as "not started in this browser".

## [0.1.0] - 2026-10-08

### Added

- A single Rust host that serves the Craft browser apps and launcher, installs releases from their official GitHub repositories, and checks for updates on a schedule.
- Operator commands for configuration and health checks, status, update history, manual updates, pins, rollbacks, and user management.
- An administration page on the apps' port, with optional local-password and OpenID Connect sign-in, user and admin roles, and SQLite-backed accounts.
- Docker Compose deployment with configurable host directories, identity, listener, and optional persistent logs.

### Changed

- App URLs now stay at `/<app>/` when a release changes; retained versions remain available at `/<app>/<version>/`.
- The default retention policy keeps one release per app. Scheduled checks retain their timing across host restarts, and one app's retry no longer triggers checks for every app.
- The host serves app assets and adds precompressed copies to previously installed releases without a separate nginx service.

### Security

- Administration can be restricted to a separate host name; sign-in and proxy identity headers are subject to configured host and trusted-proxy boundaries.
- Container deployment uses a non-root, read-only distroless runtime without privileged mode or a Docker socket.

[0.5.2]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.5.2
[0.5.1]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.5.1
[0.5.0]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.5.0
[0.4.0]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.4.0
[0.3.1]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.3.1
[0.3.0]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.3.0
[0.2.4]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.2.4
[0.2.3]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.2.3
[0.2.2]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.2.2
[0.2.1]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.2.1
[0.2.0]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.2.0
[0.1.1]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.1.1
[0.1.0]: https://github.com/AurelioB/artcraft-web-launcher/releases/tag/v0.1.0
