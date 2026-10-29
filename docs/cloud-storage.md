# Optional server document storage

This is an opt-in server component and a common browser client. It is disabled by default.
It does not modify or replace verified app releases, and it does not add native hooks to
official editor binaries. Native Save and server Recents depend on the bridge below.

## Enable or disable the entire feature

Use one of the launcher's existing sign-in methods, protect the apps and opt in:

```toml
[auth]
methods = ["local"]
apps = "signed-in"

[cloud]
enabled = true
max_file = "1GiB"
quota = "10GiB"
```

Configure local accounts, OIDC, Basic or a trusted proxy as described in [admin.md](admin.md).
Then restart the server. There is no additional identity provider, secret, public service or
Python process to configure. Use HTTPS for normal browser use.

Set `enabled = false` and restart to disable the entire feature. In this mode the server
does not open or create a document catalog, inject the cloud client, or accept file, project
or upload operations. The reserved configuration endpoint returns only `{"enabled":false}`
so an older loaded client can stop. Existing documents and browser data are preserved.
There is no per-user switch that can override this server policy.

## Browser behaviour

The same floating cloud button is used on the launcher and compatible editors. It can be
moved with a pointer or arrow keys and remembers a normalized position on that browser.
The panel stays beside the button, changes sides at viewport edges, and remains below the
button so the control stays reachable. It does not reserve space below the editor.

The panel lists the signed-in account's projects, with search, open, download, pending-copy
recovery and account actions. It uses the launcher's existing sign-out endpoint when the
sign-in method supports sessions. Proxy and Basic sign-in are managed by their provider or
browser and are not presented as a locally revocable session.

With a compatible editor, each Save asks for **server**, **device** or **cancel**. Choosing
device uses the native save path. The server never marks a browser download as a durable
server save, and a download request does not prove the operating system retained a file.
Server Recents are supplied separately; the editor keeps its existing local Recents.

Opening a project from the launcher passes an explicit project intent to the target editor
and waits for the native bridge. It does not stop at the editor's home page or consume that
intent before the editor confirms opening. Failed imports leave a visible error.

## Account isolation

Every catalog query, version lookup, file download, upload part, completion and cancellation
is checked against the existing authenticated principal. A database account uses its stable
account ID, so renaming it preserves ownership; deleting and recreating an account does not
reuse its ID. Proxy accounts use the exact provider-supplied user name. Providers must keep
that identity stable and must not recycle it for a different person.

An administrator does not receive access to another account's documents through this API.
The operator with filesystem access can still read and back up the server's stored data.

The client partitions its pending-copy records by an opaque account namespace. Mutations
must echo that namespace as well as a CSRF token. A changed session therefore cannot silently
submit the previous account's captured bytes into the new account. Background/focus checks
suspend the old integration, clear its displayed server list and revoke its download URLs.
Restored pending copies require an explicit retry in the same account.

The namespace is a routing guard, not encryption of browser data. This does **not** isolate the
client's pending-copy IndexedDB, official editors' IndexedDB/OPFS, browser caches, open documents
or downloads from another person using the same browser profile. Signing out does not erase those local
copies. Use a separate or temporary browser profile on a shared computer and close local
documents before leaving it. App code served on the same origin is trusted with that
origin's signed-in data, as in the existing launcher/admin origin model.

## Storage and backups

Documents live in `STATE_DIR/cloud/`: `catalog.sqlite3`, `blobs/`, `parts/` and a process lock.
The directory is private to the service account. No document is stored inside an installed
app release, and app updater retention never deletes document versions.

The server accepts 4MiB parts and verifies the declared total size and SHA-256 before a
transaction commits a version. Blob files and directories are synced before the durable ACK.
Operation IDs are idempotent; replay returns the originally committed result even if a
later edit has advanced the project. A stale base version returns a conflict instead of
overwriting another edit. Identical saves against the same head reuse that capture.

`quota` is global across users and includes all document versions, full pending reservations
and orphan data files left by an interrupted operation. SQLite metadata is not included.
At most four pending uploads per account and sixteen per server may be admitted. Assembly
also checks the physical free space needed for its temporary copy and `min_free_space`.
Completed document versions are never removed automatically. Only pending uploads can be
cancelled; this API does not provide permanent document deletion or automatic retention.

Back up the complete `STATE_DIR` with the server stopped, together with `CONFIG_DIR`.
Account state and the cloud catalog must come from the same consistent backup; restoring a
catalog without the account database can assign numeric identities incorrectly. A live copy
of a WAL database requires SQLite's backup API and a coherent strategy for its blobs and
pending files. Copying only the database is insufficient.

Turning the feature off preserves these files. A pending or conflicted capture stays pending
in the client instead of being reported as saved. Local export remains the recovery path.
External media, fonts, linked documents and editable format fidelity remain the editor's
responsibility: the server stores opaque native bytes and does not invent missing resources.

## Native editor bridge, protocol 1

The server component intentionally does not intercept global Blob downloads or patch menus,
canvas labels or dirty indicators. Editors without a compatible bridge keep their normal
local behaviour. The panel can still list/download that user's already stored files, but it
does not pretend that their Save command is integrated.

The client recognizes `window.artcraftPhotoCraft`, `artcraftVectorCraft`, `artcraftFilmCraft`,
`artcraftLightCraft`, `artcraftPdfCraft`, `artcraftEffectCraft`, `artcraftDesignCraft`,
`artcraftWordCraft`, `artcraftDeckCraft`, `artcraftGridCraft`, `artcraftCadCraft`,
`artcraftSoundCraft` and `artcraftFigCraft`. Each advertises `protocolVersion: 1`, its app ID,
and the following methods:

| Method | Contract |
| --- | --- |
| `connect()` | Explicitly claims the bridge; true only when supported. |
| `setCloudEnabled(bool)` | Enables/disables server destinations without deleting native local data. |
| `snapshot()` | Protocol/app identity, documents and captured pending requests; IDs and revisions are decimal strings. |
| `setProjects([{id,name,updatedAt}])` | Updates only server Recents, preserving local Recents. |
| `resolveSaveDialog(requestId,target,filename)` | Resolves the native dialog with `cloud`, `device` or `cancel`. |
| `openProject(projectId,filename,Uint8Array)` | Imports the exact editable bytes using native unsaved-work/import prompts. |
| `saveResult(requestId,status,projectId)` | Acknowledges only that captured native request/revision. |

`blobProjects: true` may additionally advertise `openProjectBlob` for native media bundles.
Events are delivered on the window's `artcraft-<app-id>` event with an object in `detail`:

- `ready`: the native API is available.
- `saveDialog`: request ID, filename and intent (`save`, `saveAs` or `copy`).
- `save`: request/document/revision IDs, filename, intent, destination and captured bytes
  (`Uint8Array`, or an explicitly advertised Blob).
- `saveUnchanged`: informative no-op after the user selected the server.
- `openRequest`: project ID selected in native server Recents.
- `opened`, `openFailed`, `openCancelled`: native import outcome, project and document IDs.

The native implementation owns the document/revision check. It must never mark the document
clean before `saveResult(..., "saved", ...)` for its durable capture. Editing while that
capture uploads must keep the later revision dirty; a closed or retired request must ignore
late ACKs. `download`, `failed` and `cancel` are distinct outcomes. The host must not resolve
a future or different document by reusing a request ID.

This contribution needs compatible upstream editor implementations before native Save/Recents
can be enabled for stock releases. It contains the shared host/client and protocol tests, not
thirteen silently patched official distributions. That dependency is a review/merge gate.

## Separately validated companion adapters

All thirteen adapted versions below were validated in a separate private desktop-PC
web deployment. Its canonical native protocol, codec and capture selections passed
260 cases, with no failures or ignored cases. Browser checks covered server save,
durable acknowledgement, reload and native server Recents, separate local Recents,
and physical device copies. These results concern the adapted builds only.

The companion source adaptations and binaries are **not included in this PR**. This
is not an end-to-end run against this community server or certification of unmodified
official binaries. This contribution remains the shared optional host/client and
native bridge protocol 1. Compatible upstream editor implementations remain a
review/merge dependency. The feature remains disabled by default, with storage scoped
to the authenticated account when enabled.

| Adapted editor | Version | Checked format and scope limits |
| --- | --- | --- |
| PhotoCraft | 0.17.0 | PSD with editable layers and a brush stroke. Other formats and PSD features were not certified. |
| WordCraft | 0.12.0 | DOCX with text and an embedded image. This does not certify every document feature or import/export format. |
| GridCraft | 0.13.0 | XLSX with two sheets, a formula and a pending cell edit committed through the visible Save button. A focused cell editor can consume Ctrl+S before the global shortcut. |
| DeckCraft | 0.12.0 | Native `.deckcraft` with text and an image. PPTX and real-device IME composition were not certified. |
| VectorCraft | 0.18.0 | Native `.vectorcraft` with an editable rectangle and embedded PNG. The reopened internal tab title can remain `Untitled-1`; other linked resources and external fonts were not certified. |
| FilmCraft | 0.14.0 | `.fcproj.zip` with PNG/WAV bytes retained and the PNG displayed in Source and Program. Audible playback, waveforms and video were not certified. Local use extracts the ZIP beside its project/assets. |
| LightCraft | 0.15.0 | Whole-library ZIP with PNG originals and an exposure adjustment. Large libraries and all image formats were not certified. |
| PdfCraft | 0.15.0 | One-page PDF with five form fields. Images, encryption and signatures were not certified. |
| EffectCraft | 0.18.0 | `.ecproj.zip` with a restored PNG and renamed layer. Direct local ZIP opening was not certified; the documented desktop route extracts the project/assets. Custom external OCIO configurations remain unsupported by the complete-bundle guard. |
| DesignCraft | 0.14.0 | Native `.designcraft` with one page and an embedded PNG frame. External fonts and other import/export formats were not certified. |
| CadCraft | 0.14.0 | DXF with one line and two circles. DWG, XREFs, images and numeric coordinate entry were not certified. |
| SoundCraft | 0.14.0 | `.scraftz` with a one-second mono WAV and track mute retained. Decoded Float32 samples matched the PCM16 reference exactly. Audible playback, hardware capture and live external plugins were not certified. |
| FigCraft | 0.12.1 | Native `.figcraft` with two editable rectangles and an embedded PNG. Experimental `.fig` interchange was not certified. |

Local Recents retain each editor's official browser provider. A listed local path
may require choosing the file again when the browser has no durable file permission.
The media examples above do not establish completeness for every external dependency.
Safari/iPad and Apple Pencil Scribble validation remain deferred. The host/client
development checks below are separate from this adapted-editor deployment evidence.

## HTTP API

All paths are under `/__cloud/api` on the existing listener and require the signed-in account
when enabled. Responses are not publicly cached. No CORS access is granted.

| Route | Operation |
| --- | --- |
| `GET /config` | Feature policy, limits, app links, opaque namespace and in-memory CSRF token. |
| `GET /projects?app=<id>&limit=<n>&offset=<n>` | Owner-scoped catalog, optionally filtered by app; 1–1000 entries per page. |
| `GET /projects/<id>` | Owner-scoped current head. |
| `GET /projects/<id>/versions` | Owner-scoped retained captures (up to 1000 returned). |
| `GET /files/<id>` | Owner-checked attachment download. |
| `POST /uploads` | Admit an idempotent native snapshot with filename, app, size, SHA, intent and optional project/base IDs. |
| `GET /uploads/<id>` | Owner-checked upload status and completed parts. |
| `PUT /uploads/<id>/parts/<index>` | Idempotent, size-checked part upload. |
| `POST /uploads/<id>/complete` | Verify, compare base and durably commit; return project/capture ACK. |
| `DELETE /uploads/<id>` | Cancel only that owner's uncommitted upload. |

Mutations require `X-Craft-CSRF` and `X-Craft-Cloud-Namespace`. The client's sign-out POST also
includes its captured namespace; the existing logout route checks that optional header before
revoking a session, while callers without it keep their existing behaviour. The CSRF cookie is separate from
the administration cookie, HttpOnly, SameSite=Strict and Secure according to the existing
cookie policy. The token returned by `/config` is held in memory. A namespace header on a
read is also checked, so an in-flight request from an old session is rejected.

## Development checks

```sh
cd host
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cd ..
node --test host/tests/cloud-client.cjs
```

The client tests use a simulated DOM/native editor and network, with independent SHA vectors.
HTTP tests use the actual router/authentication and temporary directories. These checks do
not prove browser behaviour in thirteen stock editors or Safari/iPad. No live deployment or
personal documents are required to run them.
