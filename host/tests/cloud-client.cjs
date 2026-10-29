'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { createHash, webcrypto, randomUUID } = require('node:crypto');
const { install, panelGeometry, sha256Blob } = require('../assets/cloud.js');

const IDS = { project: '12345678-1234-4234-8234-123456789abc', file: '23456789-1234-4234-8234-123456789abc', upload: '34567890-1234-4234-8234-123456789abc', other: '45678901-1234-4234-8234-123456789abc' };
const NS = owner => 'namespace_' + owner + '_0123456789abcdef';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function project(app = 'photocraft', name = 'Example.psd', id = IDS.project) {
  const bytes = Buffer.from('abc');
  return { id, name, app, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', head: { id: IDS.file, filename: name, app, size: bytes.length, sha256: digest(bytes), created_at: '2026-01-01T00:00:00Z', url: '/__cloud/api/files/' + IDS.file } };
}
class Target {
  constructor() { this.listeners = new Map(); }
  addEventListener(name, callback) { if (!this.listeners.has(name)) this.listeners.set(name, []); this.listeners.get(name).push(callback); }
  dispatchEvent(event) { event.target ||= this; event.preventDefault ||= () => { event.defaultPrevented = true; }; for (const callback of this.listeners.get(event.type) || []) { callback(event); if (event.stopped) break; } return !event.defaultPrevented; }
}
class Element extends Target {
  constructor(tag, document) { super(); this.tagName = tag.toUpperCase(); this.document = document; this.children = []; this.style = {}; this.attributes = {}; this.dataset = {}; this.hidden = false; this.value = ''; this._text = ''; }
  appendChild(child) { child.parentNode = this; this.children.push(child); return child; }
  removeChild(child) { this.children.splice(this.children.indexOf(child), 1); child.parentNode = null; }
  get firstChild() { return this.children[0] || null; }
  remove() { if (this.parentNode) this.parentNode.removeChild(this); }
  set textContent(text) { this._text = String(text); this.children = []; }
  get textContent() { return this._text + this.children.map(c => c.textContent).join(''); }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  getAttribute(name) { return this.attributes[name] ?? null; }
  attachShadow() { this.shadowRoot = new Element('shadow-root', this.document); return this.shadowRoot; }
  getBoundingClientRect() { const width = this.className === 'bubble' ? 48 : 420; const height = this.className === 'bubble' ? 48 : 460; return { left: parseFloat(this.style.left) || 0, top: parseFloat(this.style.top) || 0, width, height }; }
  setPointerCapture(id) { this.pointerId = id; }
  focus() { this.document.activeElement = this; }
  click() { this.dispatchEvent({ type: 'click' }); if (this.tagName === 'A') this.document.downloads.push({ href: this.href, name: this.download }); }
}
function all(node) { return [node, ...node.children.flatMap(all), ...(node.shadowRoot ? all(node.shadowRoot) : [])]; }
function byText(document, text) { return all(document.body).find(n => n.tagName === 'BUTTON' && n.textContent === text); }
function byClass(document, className) { return all(document.body).find(n => n.className === className); }
function createIDB(records = new Map()) {
  return { records, open() { const req = {}; queueMicrotask(() => { req.result = { createObjectStore() {}, transaction() { const tx = { objectStore() { return { put(record) { records.set(record.key, record); queueMicrotask(() => tx.oncomplete?.()); return { result: record.key }; }, delete(key) { records.delete(key); queueMicrotask(() => tx.oncomplete?.()); return {}; }, getAll() { const result = { result: Array.from(records.values()) }; queueMicrotask(() => tx.oncomplete?.()); return result; } }; } }; return tx; } }; req.onupgradeneeded?.(); req.onsuccess?.(); }); return req; } };
}
function response(value, status = 200, binary = false) { return { ok: status >= 200 && status < 300, status, redirected: false, url: '', text: async () => JSON.stringify(value), json: async () => value, blob: async () => binary ? new Blob([value]) : new Blob([JSON.stringify(value)]) }; }
function harness(options = {}) {
  const d = new Target(); d.visibilityState = 'visible'; d.activeElement = null; d.downloads = [];
  d.body = new Element('body', d); d.createElement = tag => new Element(tag, d); d.createElementNS = (_, tag) => new Element(tag, d);
  d.querySelector = selector => selector === 'canvas' ? d.canvas : null; d.getElementById = id => all(d.body).find(x => x.id === id) || null;
  d.canvas = d.createElement('canvas'); d.canvas.id = 'photocraft_canvas'; d.body.appendChild(d.canvas);
  const prefix = options.prefix || '/';
  let location = new URL(options.url || 'https://apps.example.test' + prefix + (options.app === '' ? '' : 'pictures/'));
  const w = new Target(); w.document = d; w.innerWidth = 1200; w.innerHeight = 900; w.Blob = Blob;
  w.location = { get href() { return location.href; }, get origin() { return location.origin; }, get pathname() { return location.pathname; }, assign(value) { location = new URL(value, location); } };
  w.history = { replaceState(_, __, value) { location = new URL(value, location); } };
  w.opened = []; w.open = (url, target, features) => { w.opened.push({ url, target, features }); };
  w.crypto = { subtle: webcrypto.subtle, randomUUID, getRandomValues: webcrypto.getRandomValues.bind(webcrypto) };
  w.objectURLs = new Map(); let nextURL = 0; class LocalURL extends URL {};
  LocalURL.createObjectURL = blob => { const url = 'blob:local-' + ++nextURL; w.objectURLs.set(url, blob); return url; };
  LocalURL.revokeObjectURL = url => w.objectURLs.delete(url); w.URL = LocalURL;
  w.localStorage = { data: new Map(), getItem(key) { return this.data.get(key) || null; }, setItem(key, value) { this.data.set(key, value); } };
  w.indexedDB = createIDB(options.records); const timers = new Map(); let timerID = 0;
  w.setTimeout = callback => { timers.set(++timerID, callback); return timerID; }; w.clearTimeout = id => timers.delete(id);
  w.requestAnimationFrame = callback => queueMicrotask(callback);
  d.currentScript = { src: 'https://apps.example.test' + prefix + '__cloud/client.js', dataset: { cloudEnabled: options.flag === undefined ? 'true' : options.flag, cloudApp: options.app === undefined ? 'photocraft' : options.app } };
  const service = { owner: options.owner || 'a', enabled: options.enabled !== false, calls: [], projects: options.projects || [project()], binary: Buffer.from('abc'), metadata: null, wrongAck: false, firstPart: null, releasePart: null, auth: true, canLogout: options.canLogout !== false };
  w.fetch = async (url, init = {}) => {
    const path = new URL(url, w.location.href).pathname; const base = prefix + '__cloud/api'; const part = path.startsWith(base) ? path.slice(base.length) : path;
    service.calls.push({ url, path: part, method: init.method || 'GET', headers: init.headers || {}, body: init.body });
    if (part === '/auth/me' || path === prefix + 'auth/me') return response({ user: 'Example user', can_logout: service.canLogout });
    if (path === prefix + 'auth/logout') return response({ ok: true });
    if (part === '/config') {
      if (!service.auth) return response({ error: 'auth_required' }, 401);
      return response(service.enabled ? { enabled: true, namespace: NS(service.owner), csrfToken: 'csrf_0123456789abcdef0123456789abcdef', maxFileBytes: 64 * 1024 * 1024, chunkBytes: 4 * 1024 * 1024, apps: { photocraft: { entry: 'pictures', label: 'PhotoCraft', origin: options.editorOrigin || null }, gridcraft: { entry: 'sheets', label: 'GridCraft', origin: null } } } : { enabled: false });
    }
    if (!service.auth) return response({ error: 'auth_required' }, 401);
    const mutation = init.method && !['GET', 'HEAD'].includes(init.method);
    if ((mutation || init.headers['X-Craft-Cloud-Namespace']) && init.headers['X-Craft-Cloud-Namespace'] !== NS(service.owner)) return response({ error: 'identity_changed' }, 409);
    if (part === '/projects') { const app = new URL(url, w.location.href).searchParams.get('app'); return response({ projects: service.projects.filter(p => !app || p.app === app) }); }
    if (part === '/projects/' + IDS.project) return response(service.projects.find(p => p.id === IDS.project));
    if (part === '/files/' + IDS.file) return response(service.binary, 200, true);
    if (part === '/uploads' && init.method === 'POST') { service.metadata = JSON.parse(init.body); return response({ id: IDS.upload, chunk_bytes: 4 * 1024 * 1024, parts: [], status: 'pending' }); }
    if (part === '/uploads/' + IDS.upload + '/parts/0' && init.method === 'PUT') {
      service.binary = Buffer.from(await init.body.arrayBuffer()); service.firstPart?.(); if (service.releasePart) await service.releasePart;
      return response({ index: 0 });
    }
    if (part === '/uploads/' + IDS.upload + '/complete') {
      if (init.headers['X-Craft-Cloud-Namespace'] !== NS(service.owner)) return response({ error: 'identity_changed' }, 409);
      const file = { ...project().head, filename: service.metadata.filename, app: service.metadata.app, size: service.metadata.size, sha256: service.wrongAck ? '0'.repeat(64) : service.metadata.sha256 };
      const saved = { ...project(), name: file.filename, app: file.app, head: file }; service.projects = [saved]; return response({ project: saved, file });
    }
    throw Error('Unhandled test request ' + part);
  };
  const native = { protocolVersion: 1, app: options.app || 'photocraft', blobProjects: false, editableExtensions: ['psd'], connectCount: 0, cloudModes: [], resolved: [], results: [], recentLists: [], opens: [], availableSnapshot: options.snapshot !== false, doc: { documentId: '1', revision: '1', savedRevision: '0', dirty: true, pendingRequestId: null }, captures: new Map(), noop: false,
    connect() { this.connectCount++; return true; }, setCloudEnabled(value) { this.cloudModes.push(value); return true; }, snapshot() { return this.availableSnapshot ? { protocolVersion: 1, app: this.app, activeDocumentId: '1', cloudEnabled: this.cloudModes.at(-1) === true, documents: [{ ...this.doc }] } : null; }, setProjects(items) { this.recentLists.push(items); },
    resolveSaveDialog(requestId, destination, filename) { this.resolved.push({ requestId, destination, filename }); if (destination === 'cancel') return; if (this.noop && destination === 'cloud') { emit({ type: 'saveUnchanged', requestId, documentId: '1', revision: this.doc.revision }); return; } this.doc.pendingRequestId = requestId; this.captures.set(requestId, this.doc.revision); emit({ type: 'save', requestId, documentId: '1', revision: this.doc.revision, filename, intent: 'save', destination, bytes: new Uint8Array([97, 98, 99]) }); },
    saveResult(requestId, status, projectId) { this.results.push({ requestId, status, projectId }); if (status === 'saved') { this.doc.savedRevision = this.captures.get(requestId); this.doc.dirty = this.doc.revision !== this.doc.savedRevision; this.doc.pendingRequestId = null; } },
    openProject(id, filename, bytes) { this.opens.push({ id, filename, bytes }); }
  };
  if (options.native !== false) w.artcraftPhotoCraft = native;
  function emit(detail) { w.dispatchEvent({ type: 'artcraft-photocraft', detail }); }
  return { w, d, native, service, emit, timers };
}
async function until(predicate) { for (let i = 0; i < 200; i++) { if (predicate()) return; await new Promise(resolve => setImmediate(resolve)); } throw Error('Condition did not become true'); }
const mutationCalls = service => service.calls.filter(x => ['POST', 'PUT', 'DELETE'].includes(x.method));

test('SHA-256 matches published vectors, boundaries and multi-slice binary data', async () => {
  for (const n of [0, 1, 3, 55, 56, 63, 64, 65, 127, 128, 1048575, 1048576, 1048641]) { const bytes = Buffer.alloc(n); for (let i = 0; i < n; i++) bytes[i] = i % 251; assert.equal(await sha256Blob(new Blob([bytes])), digest(bytes), 'length ' + n); }
  assert.equal(await sha256Blob(new Blob(['abc'])), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
});
test('explicit backend opt-in is required before UI, hooks or network', async () => {
  for (const flag of ['', 'false', 'TRUE', '1']) { const h = harness({ flag }); assert.equal(install(h.w, h.d), null); assert.equal(h.service.calls.length, 0); assert.equal(h.native.connectCount, 0); assert.equal(byClass(h.d, 'bubble'), undefined); }
  const h = harness({ enabled: false }); const api = install(h.w, h.d); await api.ready; assert.equal(h.service.calls.length, 1); assert.equal(h.native.connectCount, 0); assert.equal(byClass(h.d, 'bubble'), undefined); h.emit({ type: 'saveDialog', requestId: 'off', filename: 'Doc.psd', intent: 'save' }); assert.equal(h.native.resolved.length, 0);
});
test('the panel follows the dragged bubble and the bubble stays above the panel', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); const bubble = byClass(h.d, 'bubble'), panel = byClass(h.d, 'panel'); const original = panel.style.left;
  bubble.dispatchEvent({ type: 'pointerdown', pointerId: 1, button: 0, clientX: 1150, clientY: 840 }); bubble.dispatchEvent({ type: 'pointermove', pointerId: 1, clientX: 80, clientY: 180 }); bubble.dispatchEvent({ type: 'pointerup', pointerId: 1 });
  assert.notEqual(panel.style.left, original); assert.equal(panel.hidden, false); assert.ok(parseFloat(panel.style.left) > parseFloat(bubble.style.left));
  bubble.click(); assert.equal(panel.hidden, false, 'drag does not accidentally close the panel');
  const css = all(h.d.body).find(x => x.tagName === 'STYLE').textContent; assert.match(css, /\.bubble\{[^}]*z-index:4;pointer-events:auto/); assert.match(css, /\.panel,\.dialog\{[^}]*z-index:2/);
  for (const width of [320, 768, 1200]) { const g = panelGeometry({ x: width / 2, y: -300 }, width, 700, 420, 460); assert.ok(g.left >= 12); assert.ok(g.left + g.width <= width - 12); assert.ok(g.y >= 12); }
});
test('every clean Save still asks for a destination and cancellation sends no bytes', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; h.native.noop = true; h.native.doc.dirty = false;
  h.emit({ type: 'saveDialog', requestId: 'clean-1', filename: 'Doc.psd', intent: 'save' }); assert.ok(byText(h.d, 'Save on server')); byText(h.d, 'Cancel').click(); await until(() => h.native.resolved.length === 1); assert.equal(h.native.resolved[0].destination, 'cancel');
  h.emit({ type: 'saveDialog', requestId: 'clean-2', filename: 'Doc.psd', intent: 'save' }); byText(h.d, 'Save on server').click(); await until(() => h.native.resolved.length === 2); assert.equal(h.native.resolved[1].destination, 'cloud'); assert.equal(mutationCalls(h.service).length, 0); assert.equal(h.native.results.length, 0, 'saveUnchanged grants no saved ACK authority');
});
test('device choice invokes only the explicit native download path', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; h.emit({ type: 'saveDialog', requestId: 'device-1', filename: 'Doc.psd', intent: 'save' }); byText(h.d, 'Download to device').click(); await until(() => h.d.downloads.length === 1);
  assert.equal(h.d.downloads[0].name, 'Doc.psd'); assert.equal(h.native.results.at(-1).status, 'download'); assert.equal(mutationCalls(h.service).length, 0); assert.equal(h.native.doc.dirty, true);
});
test('durable ACK belongs to the captured revision; edits during upload remain dirty', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; let release; const held = new Promise(resolve => { release = resolve; }); h.service.releasePart = held;
  const bytes = new Uint8Array([97, 98, 99]); h.native.doc.pendingRequestId = 'save-1'; h.native.captures.set('save-1', '1'); h.emit({ type: 'save', requestId: 'save-1', documentId: '1', revision: '1', filename: 'Doc.psd', intent: 'save', destination: 'cloud', bytes }); bytes[0] = 0;
  await until(() => h.service.calls.some(x => x.path.includes('/parts/0'))); assert.equal(h.native.results.length, 0); h.native.doc.revision = '2'; release(); await until(() => h.native.results.some(x => x.status === 'saved') && api.status().pending === 0);
  assert.equal(h.service.binary.toString(), 'abc', 'the event bytes were copied before asynchronous work'); assert.equal(h.native.doc.savedRevision, '1'); assert.equal(h.native.doc.dirty, true); assert.equal(api.status().pending, 0); assert.equal(h.w.indexedDB.records.size, 0);
  assert.ok(mutationCalls(h.service).every(x => x.headers['X-Craft-Cloud-Namespace'] === NS('a') && x.headers['X-Craft-CSRF']));
});
test('an invalid durable acknowledgement keeps the copy pending and never clears dirty', async () => {
  const h = harness(); h.service.wrongAck = true; const api = install(h.w, h.d); await api.ready; h.native.doc.pendingRequestId = 'bad-ack'; h.native.captures.set('bad-ack', '1'); h.emit({ type: 'save', requestId: 'bad-ack', documentId: '1', revision: '1', filename: 'Doc.psd', intent: 'save', destination: 'cloud', bytes: new Uint8Array([97, 98, 99]) });
  await until(() => h.native.results.some(x => x.status === 'failed')); assert.equal(h.native.results.some(x => x.status === 'saved'), false); assert.equal(h.native.doc.dirty, true); assert.equal(api.status().pending, 1); assert.equal(h.w.indexedDB.records.size, 1); assert.ok(byText(h.d, 'Download this copy'));
});
test('account change stops an admitted upload and never resubmits its bytes as the next user', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; let release; h.service.releasePart = new Promise(resolve => { release = resolve; }); h.native.doc.pendingRequestId = 'owner-a'; h.native.captures.set('owner-a', '1');
  h.emit({ type: 'save', requestId: 'owner-a', documentId: '1', revision: '1', filename: 'Private.psd', intent: 'save', destination: 'cloud', bytes: new Uint8Array([97, 98, 99]) }); await until(() => h.service.calls.some(x => x.path.includes('/parts/0'))); h.service.owner = 'b'; await api.refresh(); release(); await until(() => h.native.results.some(x => x.status === 'cancel')); await new Promise(resolve => setImmediate(resolve));
  assert.equal(api.status().identityChanged, true); assert.equal(h.native.results.some(x => x.status === 'saved'), false); assert.equal(h.native.cloudModes.at(-1), false); assert.deepEqual(h.native.recentLists.at(-1), []); assert.equal(mutationCalls(h.service).filter(x => x.headers['X-Craft-Cloud-Namespace'] === NS('b')).length, 0);
  assert.equal(all(h.d.body).some(n => n.tagName === 'STRONG' && n.textContent === 'Private.psd'), false, 'previous-user pending names are hidden');
  const next = harness({ owner: 'b', records: h.w.indexedDB.records }); const nextAPI = install(next.w, next.d); await nextAPI.ready; assert.equal(nextAPI.status().pending, 0); assert.equal(mutationCalls(next.service).length, 0); assert.equal(next.w.indexedDB.records.size, 1, 'old recovery bytes are retained under their original namespace');
});
test('401 suspends access and clears remote and pending lists without automatic retry', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); h.service.auth = false; await api.refresh(); assert.equal(api.status().locked, true); assert.deepEqual(h.native.recentLists.at(-1), []); assert.ok(all(h.d.body).find(n => n.tagName === 'A' && n.textContent === 'Sign in')); const count = mutationCalls(h.service).length; for (const callback of h.timers.values()) callback(); assert.equal(mutationCalls(h.service).length, count);
});
test('opening the global library cannot overwrite the current native application Recents', async () => {
  const photo = project(); const grid = project('gridcraft', 'Budget.xlsx', IDS.other); const h = harness({ projects: [grid, photo] }); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); assert.deepEqual(h.native.recentLists.at(-1).map(x => x.id), [IDS.project]); assert.ok(h.service.calls.some(x => String(x.url).includes('projects?app=photocraft'))); assert.ok(byText(h.d, 'Budget.xlsx'));
});
test('cross-app project links preserve intent through a delayed native bootstrap and path prefix', async () => {
  const h = harness({ app: '', prefix: '/suite/' }); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); byText(h.d, 'Example.psd').click(); await until(() => h.w.opened.length === 1); const target = h.w.opened[0]; assert.equal(target.target, '_blank'); assert.equal(target.features, 'noopener'); assert.equal(target.url, 'https://apps.example.test/suite/pictures/?cloudProject=' + IDS.project);
  const editor = harness({ prefix: '/suite/', url: target.url, snapshot: false }); const editorAPI = install(editor.w, editor.d); await editorAPI.ready; assert.equal(editor.native.opens.length, 0); assert.ok(editor.w.location.href.includes('cloudProject')); editor.native.availableSnapshot = true; editor.emit({ type: 'ready' }); await until(() => editor.native.opens.length === 1); assert.equal(editor.native.connectCount, 1); assert.equal(editor.native.opens[0].id, IDS.project); assert.ok(editor.w.location.href.includes('cloudProject'));
  editor.emit({ type: 'opened', projectId: IDS.project, documentId: '1', filename: 'Example.psd' }); assert.equal(new URL(editor.w.location.href).searchParams.has('cloudProject'), false); assert.ok(editor.service.calls.filter(x => x.path.startsWith('/projects')).every(x => String(x.url).startsWith('/suite/__cloud/api/')));
});
test('disabled cloud does not claim an unadapted editor or patch downloads', async () => {
  const h = harness({ native: false }); const originalCreate = h.w.URL.createObjectURL, originalClick = Element.prototype.click; const api = install(h.w, h.d); await api.ready; assert.equal(api.status().connected, false); assert.equal(h.w.URL.createObjectURL, originalCreate); assert.equal(Element.prototype.click, originalClick); assert.match(byClass(h.d, 'panel').textContent, /no native server-storage protocol/);
});

test('cloud text fields keep clipboard defaults and do not also trigger editor shortcuts', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); const input = byClass(h.d, 'search'), host = h.d.getElementById('craft-server-cloud'); let editorPaste = 0, editorSave = 0;
  h.d.addEventListener('paste', () => { editorPaste++; }); h.d.addEventListener('keydown', e => { if (e.ctrlKey && e.key === 's') editorSave++; });
  const paste = { type: 'paste', composedPath: () => [input, host.shadowRoot, host, h.d.body, h.d], stopImmediatePropagation() { this.stopped = true; } }; h.d.dispatchEvent(paste); assert.equal(editorPaste, 0); assert.equal(paste.defaultPrevented, undefined, 'native paste into the text field is preserved');
  const save = { type: 'keydown', key: 's', ctrlKey: true, composedPath: () => [input, host.shadowRoot, host, h.d.body, h.d], stopImmediatePropagation() { this.stopped = true; } }; h.d.dispatchEvent(save); assert.equal(editorSave, 0); assert.equal(save.defaultPrevented, true);
});
test('Save as never sends the previous project base version', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); byText(h.d, 'Example.psd').click(); await until(() => h.native.opens.length === 1); h.emit({ type: 'opened', projectId: IDS.project, documentId: '1' });
  h.native.doc.pendingRequestId = 'save-as'; h.native.captures.set('save-as', '1'); h.emit({ type: 'save', requestId: 'save-as', documentId: '1', revision: '1', filename: 'New.psd', intent: 'saveAs', destination: 'cloud', bytes: new Uint8Array([97, 98, 99]) }); await until(() => h.service.metadata !== null);
  assert.equal(h.service.metadata.intent, 'save_as'); assert.equal(h.service.metadata.project_id, null); assert.equal(h.service.metadata.base_id, null); await until(() => api.status().pending === 0);
});
test('turning the backend feature off removes the entire widget and restores the native local path', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; assert.ok(byClass(h.d, 'bubble')); h.service.enabled = false; await api.refresh(); assert.equal(api.status().enabled, false); assert.equal(byClass(h.d, 'bubble'), undefined); assert.equal(h.native.cloudModes.at(-1), false); assert.deepEqual(h.native.recentLists.at(-1), []); assert.equal(mutationCalls(h.service).length, 0);
});
test('hashing a large blob never requests its complete ArrayBuffer', async () => {
  const bytes = Buffer.alloc(2 * 1024 * 1024 + 73, 181), blob = new Blob([bytes]); blob.arrayBuffer = () => { throw Error('whole-blob allocation'); }; assert.equal(await sha256Blob(blob), digest(bytes));
});

test('returning to the same account re-enables native storage but retries no retained bytes automatically', async () => {
  const h = harness(); h.service.wrongAck = true; const api = install(h.w, h.d); await api.ready;
  h.native.doc.pendingRequestId = 'reauth'; h.native.captures.set('reauth', '1'); h.emit({ type: 'save', requestId: 'reauth', documentId: '1', revision: '1', filename: 'Unconfirmed.psd', intent: 'save', destination: 'cloud', bytes: new Uint8Array([97, 98, 99]) }); await until(() => h.native.results.some(x => x.status === 'failed'));
  h.service.auth = false; await api.refresh(); const count = mutationCalls(h.service).length; assert.equal(api.status().connected, false);
  h.service.auth = true; await api.refresh(); assert.equal(api.status().connected, true); assert.equal(h.native.cloudModes.at(-1), true); assert.equal(mutationCalls(h.service).length, count); assert.ok(byText(h.d, 'Retry server save'));
});

test('filenames use the server UTF-8 byte limit and reject blank or dot-only input', async () => {
  const h = harness(); const api = install(h.w, h.d); await api.ready; h.emit({ type: 'saveDialog', requestId: 'names', filename: 'Doc.psd', intent: 'save' }); const field = byClass(h.d, 'filename');
  for (const name of [' ', '.', '..', 'é'.repeat(119) + '.psd']) { field.value = name; byText(h.d, 'Save on server').click(); await new Promise(resolve => setImmediate(resolve)); assert.equal(h.native.resolved.length, 0, name); }
  field.value = 'é'.repeat(118) + '.psd'; byText(h.d, 'Download to device').click(); await until(() => h.native.resolved.length === 1); assert.equal(Buffer.byteLength(h.native.resolved[0].filename), 240);
});
test('a first save emitted before the native snapshot is retained until the handshake is ready', async () => {
  const h = harness({ snapshot: false }); const api = install(h.w, h.d); await api.ready; assert.equal(api.status().connected, false);
  h.native.doc.pendingRequestId = 'first-frame'; h.native.captures.set('first-frame', '1'); h.emit({ type: 'save', requestId: 'first-frame', documentId: '1', revision: '1', filename: 'First.psd', intent: 'save', destination: 'cloud', bytes: new Uint8Array([97, 98, 99]) });
  await until(() => h.w.indexedDB.records.size === 1); assert.equal(mutationCalls(h.service).length, 0); assert.equal(h.native.results.length, 0);
  h.native.doc.revision = '2'; h.native.availableSnapshot = true; h.emit({ type: 'ready' }); await until(() => h.native.results.some(x => x.status === 'saved') && api.status().pending === 0); assert.equal(h.native.connectCount, 1); assert.equal(h.native.doc.savedRevision, '1'); assert.equal(h.native.doc.dirty, true);
});

test('focus during a throttled handshake coalesces one retry and opens the deep link without another event', async () => {
  const h = harness({ snapshot: false, url: 'https://apps.example.test/pictures/?cloudProject=' + IDS.project });
  h.d.visibilityState = 'hidden'; const frames = []; h.w.requestAnimationFrame = callback => frames.push(callback);
  const originalNow = Date.now; let now = 0; Date.now = () => now;
  try {
    const api = install(h.w, h.d); await until(() => frames.length === 1); assert.equal(h.native.connectCount, 1);
    h.emit({ type: 'save', requestId: 'bad', documentId: 'bad-id', revision: '1', filename: 'Doc.psd', intent: 'save', destination: 'cloud', bytes: new Uint8Array([97]) });
    h.d.visibilityState = 'visible'; h.w.dispatchEvent({ type: 'focus' }); await until(() => h.service.calls.filter(x => x.path === '/config').length === 2); await new Promise(resolve => setImmediate(resolve));
    now = 4000; frames.shift()(); await until(() => frames.length === 1); assert.match(byClass(h.d, 'status').textContent, /invalid save request/, 'a late startup notice must not replace an unrelated error');
    h.native.availableSnapshot = true; frames.shift()(); await api.ready; await until(() => h.native.opens.length === 1);
    assert.equal(h.native.connectCount, 1); assert.equal(h.native.opens[0].id, IDS.project); assert.equal(api.status().connected, true); assert.equal(frames.length, 0, 'no new polling loop remains');
  } finally { Date.now = originalNow; }
});

test('asynchronously populated files remain inside the viewport without moving the bubble', async () => {
  const projects = Array.from({ length: 40 }, (_, i) => project('photocraft', 'Document ' + i + '.psd', randomUUID()));
  const h = harness({ projects }); const api = install(h.w, h.d); await api.ready;
  const panel = byClass(h.d, 'panel'), bubble = byClass(h.d, 'bubble'); let height = 180;
  panel.getBoundingClientRect = () => ({ width: 420, height, left: parseFloat(panel.style.left) || 0, top: parseFloat(panel.style.top) || 0 });
  const baseFetch = h.w.fetch; let release, requested = false; const held = new Promise(resolve => { release = resolve; });
  h.w.fetch = async (url, options) => { const u = new URL(url, h.w.location.href); if (u.pathname.endsWith('/projects') && !u.searchParams.has('app')) { requested = true; await held; } return baseFetch(url, options); };
  const opening = api.openLibrary(); await until(() => requested); const initialTop = parseFloat(panel.style.top), bubbleTop = bubble.style.top;
  height = h.w.innerHeight - 24; release(); await opening;
  assert.ok(parseFloat(panel.style.top) < initialTop, 'the grown panel moves upward');
  assert.ok(parseFloat(panel.style.top) + height <= h.w.innerHeight - 12, 'the last row stays in the available viewport');
  assert.equal(bubble.style.top, bubbleTop, 'populating a list never moves the draggable button');
});

test('provider-managed accounts expose no working launcher sign-out action', async () => {
  const h = harness({ canLogout: false }); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); byText(h.d, 'Account').click(); await until(() => all(h.d.body).some(n => n.textContent.includes('Your identity is managed by the sign-in provider or browser. Sign out there')));
  const logout = byText(h.d, 'Sign out'); assert.equal(logout.hidden, true); logout.click(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.service.calls.filter(x => new URL(x.url, h.w.location.href).pathname.endsWith('/auth/logout')).length, 0); assert.equal(api.status().locked, false); assert.equal(h.w.location.pathname, '/pictures/');
});
test('session sign-out rechecks its capability and navigates to the public login page instead of auto-login root', async () => {
  const h = harness({ prefix: '/suite/' }); const api = install(h.w, h.d); await api.ready; await api.openLibrary(); byText(h.d, 'Account').click(); await until(() => byText(h.d, 'Sign out').hidden === false);
  const meBefore = h.service.calls.filter(x => new URL(x.url, h.w.location.href).pathname.endsWith('/auth/me')).length;
  byText(h.d, 'Sign out').click(); await until(() => h.w.location.pathname === '/suite/auth/login');
  assert.equal(h.service.calls.filter(x => new URL(x.url, h.w.location.href).pathname.endsWith('/auth/me')).length, meBefore + 1);
  const logout = h.service.calls.filter(x => new URL(x.url, h.w.location.href).pathname.endsWith('/auth/logout')); assert.equal(logout.length, 1); assert.equal(logout[0].method, 'POST'); assert.equal(logout[0].headers['X-Craft-Cloud-Namespace'], NS('a')); assert.equal(api.status().locked, true); assert.deepEqual(h.native.recentLists.at(-1), []);
});

test('plain-text proxy 403 responses clear stale server files and pending rows without resending copies', async () => {
  for (const deniedPath of ['config', 'account']) {
    const h = harness(); h.service.wrongAck = true; const api = install(h.w, h.d); await api.ready;
    h.native.doc.pendingRequestId = 'proxy-denied'; h.native.captures.set('proxy-denied', '1'); h.emit({ type: 'save', requestId: 'proxy-denied', documentId: '1', revision: '1', filename: 'Private.psd', intent: 'save', destination: 'cloud', bytes: new Uint8Array([97, 98, 99]) });
    await until(() => h.native.results.some(x => x.status === 'failed')); await api.openLibrary(); assert.ok(byText(h.d, 'Private.psd')); assert.ok(byText(h.d, 'Download this copy'));
    const originalFetch = h.w.fetch; h.w.fetch = async (url, options) => { const path = new URL(url, h.w.location.href).pathname; if (path.endsWith(deniedPath === 'config' ? '/__cloud/api/config' : '/auth/me')) return { ok: false, status: 403, redirected: false, url: '', text: async () => 'forbidden: no identity from a trusted proxy\n' }; return originalFetch(url, options); };
    const before = mutationCalls(h.service).length;
    if (deniedPath === 'config') await api.refresh(); else { byText(h.d, 'Account').click(); await until(() => api.status().locked); }
    assert.equal(api.status().locked, true, deniedPath); assert.equal(api.status().connected, false); assert.deepEqual(h.native.recentLists.at(-1), []); assert.equal(h.native.cloudModes.at(-1), false);
    assert.equal(byText(h.d, 'Private.psd'), undefined); assert.equal(byText(h.d, 'Download this copy'), undefined); assert.equal(mutationCalls(h.service).length, before); assert.equal(h.w.indexedDB.records.size, 1, 'the original namespace still retains its recovery bytes'); assert.equal(h.native.results.some(x => x.status === 'saved'), false);
  }
});
