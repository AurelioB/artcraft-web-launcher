# Applications

Survey of the official releases, 12 October 2026 (the first seven were also surveyed in detail
on 8 October). The built-in manifest is `host/src/manifest.toml`; names, taglines, colors and
app pages come from the ArtCraft Launcher catalogue vendored in `host/assets/vendor/craft-launcher`.
Apps released later are discovered at run time from the ArtCraft feed and assumed to share the
layout described below; their install runs the same checksum, archive and serving checks (see
[`[discovery]`](configuration.md#discovery)).

| App | Group | Repository | Web artifact | Version checked | Archive contents | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| PhotoCraft | classic | `storytold/photocraft` | `photocraft-web-{v}.zip` | 0.7.0 | 25 MB | content-hashed asset names |
| VectorCraft | classic | `storytold/vectorcraft` | `vectorcraft-web-{v}.zip` | 0.10.0 | 63 MB | content-hashed asset names |
| FilmCraft | classic | `storytold/filmcraft` | `filmcraft-web-{v}.zip` | 0.6.0 | 44 MB | assets loaded with `?v=<build>` |
| LightCraft | classic | `storytold/lightcraft` | `lightcraft-web-{v}.zip` | 0.6.0 | 203 MB, 425 of 452 entries an unused Cargo target dir | precompressed `.gz`/`.br` copies |
| PdfCraft | classic | `storytold/pdfcraft` (formerly `printcraft`, which redirects) | `pdfcraft-web-{v}.zip` (≤ 0.2.1: `printcraft-web-{v}.zip`) | 0.7.1 | 38 MB | |
| EffectCraft | classic | `storytold/effectcraft` | `effectcraft-web-{v}.zip` | 0.8.0 | 61 MB | service worker, web manifest |
| DesignCraft | classic | `storytold/designcraft` | `designcraft-web-{v}.zip` | 0.6.0 | 43 MB | content-hashed asset names |
| WordCraft | new | `storytold/wordcraft` | `wordcraft-web-{v}.zip` | 0.6.1 | 40 MB | content-hashed favicon |
| DeckCraft | new | `storytold/deckcraft` | `deckcraft-web-{v}.zip` | 0.6.1 | 24 MB | content-hashed favicon |
| GridCraft | new | `storytold/gridcraft` | `gridcraft-web-{v}.zip` | 0.6.1 | 19 MB | content-hashed favicon |
| CADCraft | new | `storytold/cadcraft` | `cadcraft-web-{v}.zip` | 0.6.2 | 21 MB | no favicon link |
| SoundCraft | new | `storytold/soundcraft` | `soundcraft-web-{v}.zip` | 0.6.1 | 15 MB | WebAudio; no `SharedArrayBuffer` |
| FigCraft | new | `storytold/figcraft` | `figcraft-web-{v}.zip` | 0.5.1 (13 Oct) | 22 MB | no favicon link |

Common to all thirteen (FigCraft checked on 13 October: same layout, checksum and serving checks
passed on install):

- Each release also publishes desktop packages and `SHA256SUMS.txt`; the GitHub API reports a
  SHA-256 digest for every asset. The web archives of the versions above matched their
  `SHA256SUMS.txt`.
- The archive holds one top-level directory `<artifact>-web-<version>/` with `index.html`,
  wasm-bindgen glue and one WebAssembly module, plus `HOSTING.md`, `_headers` and `.htaccess`.
  None ships native executables outside LightCraft's excluded Cargo target directory.
- `index.html` uses relative URLs only, so every app works under a path prefix such as
  `/photocraft/` (stable URL) or `/photocraft/0.5.0/` (a retained release).
- Requirements from upstream `HOSTING.md` (read for WordCraft and SoundCraft; the first seven
  were read on 8 October) and `_headers` (the twelve apps of 12 October): `.wasm` as `application/wasm`, `.js` as
  `text/javascript`, gzip or Brotli compression, HTTPS (or localhost) for WebGPU, clipboard and
  storage. No cross-origin isolation headers are needed.
- Some releases reuse the same asset names across versions (EffectCraft, LightCraft). At the
  stable URL every response is revalidated, and entity tags carry the release, so a browser never
  keeps a file from the previous release; date validators are ignored because two releases can
  ship a file with the same size and time. Versioned URLs are cached as immutable, except entry
  pages, `sw.js` and manifests.
- Tag `v0.1.1-rc.5` of PhotoCraft is not flagged as a pre-release on GitHub; the updater treats
  semver pre-release suffixes as pre-releases regardless of the flag.

## SolidCraft (community app)

SolidCraft is published by Tangency Labs, not ArtCraft: ArtCraft Launcher 0.10.0 added it to its
catalogue as an approved community app, and the ArtCraft release feed does not list it, so it is
built in rather than discovered. Checked on 29 October 2026 with v0.26.1:

- Repository `tangency-labs/solidcraft`; the web archive follows the suite's naming
  (`solidcraft-web-{v}.zip`, 33 MB) and is listed in `SHA256SUMS.txt`, which matched, as did the
  GitHub API digest.
- One top-level directory `solidcraft-web-<version>/` with `index.html` (relative URLs only),
  wasm-bindgen glue and one WebAssembly module, `manifest.webmanifest`, `sw.js`, fonts, icons and
  licence files; no `HOSTING.md` or `_headers`. It installed, passed the archive and serving
  checks, and loaded under `/solidcraft/`.
- Like the suite apps it needs WebGPU or WebGL2, so HTTPS (or localhost).
- Its launcher logo is the 128 px PNG ArtCraft Launcher embeds (getartcraft.com has no page for
  it), MIT OR Apache-2.0 from the SolidCraft contributors; both licences are served next to it.

## LightCraft archive contents

`lightcraft-web-0.4.0.zip` contains 572 entries, 549 of which are a Cargo target directory
(`build/`, `deps/`, `.fingerprint/`, `examples/`, `incremental/`, `.cargo-*lock`) with native
build scripts, `.so` and `.rlib` files; in 0.6.0 it is 425 of 452 entries. The site never
references them. The manifest excludes
these paths (documented configuration, not patching); without the exclusion the updater rejects
the release because it contains native executables. The remaining files, including the
precompressed copies, are published unchanged; shipped `.gz`/`.br` copies are verified to
decompress to their originals because the server sends them in place of the originals.

## Service workers

EffectCraft registers `sw.js` with scope `./`, i.e. the release directory
(`/effectcraft/0.6.0/`). Each release therefore has its own service-worker scope; a newer release
does not take over tabs of an older one. On activation it deletes caches of other EffectCraft
versions; tabs still on an older release fall back to the network, which keeps serving retained
releases. SolidCraft registers `sw.js` relative to its release directory the same way.

## Optional server projects

The disabled-by-default [document store](cloud-storage.md) separates server projects by signed-in
account. It does not grant applications access to arbitrary server folders. Native Save and
server Recents need the documented editor bridge; official builds without it keep their normal
local behaviour. The server option does not migrate or isolate existing browser storage.

## Browser storage

All apps on one host share an origin. Observed on 8 October after using the seven classic apps
on one origin (the five new apps have not been surveyed for storage names yet):

| Kind | Names |
| --- | --- |
| `localStorage` | `photocraft.preferences`, `pdfcraft`, `egui_memory_ron` (PdfCraft) |
| IndexedDB | `effectcraft-handles` |
| OPFS root | `effectcraft/`, `effectcraft-cache/`, `library/`, `thumbs/` (LightCraft), `recovery/` |
| Cache Storage | `effectcraft-<build>` |

No collision was observed, but `library/`, `thumbs/`, `recovery/` (strings present in both
FilmCraft and VectorCraft) and `egui_memory_ron` are generic names, so future versions or
additional apps could collide. Give an app its own origin with the `origin` setting
([operations](operations.md#separate-origins)) if that matters for your deployment.

LightCraft keeps its photo library in browser storage and allows one tab at a time per origin
(upstream behaviour). Browsers may evict storage that is not persisted; the apps provide their
own export and backup functions.
