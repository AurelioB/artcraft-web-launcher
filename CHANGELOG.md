# Changelog

All notable changes to Craft Apps Host are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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

[0.3.0]: https://github.com/AurelioB/craft-server/releases/tag/v0.3.0
[0.2.4]: https://github.com/AurelioB/craft-server/releases/tag/v0.2.4
[0.2.3]: https://github.com/AurelioB/craft-server/releases/tag/v0.2.3
[0.2.2]: https://github.com/AurelioB/craft-server/releases/tag/v0.2.2
[0.2.1]: https://github.com/AurelioB/craft-server/releases/tag/v0.2.1
[0.2.0]: https://github.com/AurelioB/craft-server/releases/tag/v0.2.0
[0.1.1]: https://github.com/AurelioB/craft-server/releases/tag/v0.1.1
[0.1.0]: https://github.com/AurelioB/craft-server/releases/tag/v0.1.0
