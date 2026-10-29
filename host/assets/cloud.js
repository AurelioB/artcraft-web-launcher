/* ArtCraft Web Launcher: optional, authenticated server document storage.
 * Native editor protocol v1 is required for Save and native server Recents.
 * No download interception, document-title changes, or browser locale overrides.
 */
(function (root, factory) {
  'use strict';
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else factory().install(root, root.document);
})(typeof window === 'undefined' ? null : window, function () {
  'use strict';
  const API_PATH = '/__cloud/api';
  const SAFE_ID = /^[A-Za-z0-9_-]{1,128}$/;
  const LABELS = { photocraft: 'PhotoCraft', vectorcraft: 'VectorCraft', filmcraft: 'FilmCraft', lightcraft: 'LightCraft', pdfcraft: 'PdfCraft', effectcraft: 'EffectCraft', designcraft: 'DesignCraft', wordcraft: 'WordCraft', deckcraft: 'DeckCraft', gridcraft: 'GridCraft', cadcraft: 'CadCraft', soundcraft: 'SoundCraft', figcraft: 'FigCraft' };
  const VALID_NAME = name => typeof name === 'string' && !!name.trim() && name!=='.' && name!=='..' && !/[\\/:\u0000-\u001f\u007f]/.test(name) && new TextEncoder().encode(name).length <= 240;
  const isId = x => typeof x === 'string' && SAFE_ID.test(x);
  const clamp = (x, lo, hi) => Math.max(lo, Math.min(x, Math.max(lo, hi)));
  function panelGeometry(position, width, height, panelWidth, panelHeight) {
    const margin = 12, size = 48, gap = 10;
    const x = clamp(position.x, margin, width - size - margin), y = clamp(position.y, margin, height - size - margin);
    const pw = Math.min(panelWidth, Math.max(0, width - margin * 2)), ph = Math.min(panelHeight, Math.max(0, height - margin * 2));
    const right = x + size + gap;
    const left = right + pw <= width - margin ? right : x - pw - gap;
    return { x, y, left: clamp(left, margin, width - pw - margin), top: clamp(y, margin, height - ph - margin), width: pw, height: ph };
  }

  // Incremental SHA-256: only a bounded slice of a large ZIP is held as ArrayBuffer.
  // Blob.arrayBuffer() for the entire document would exhaust mobile browser heaps.
  async function sha256Blob(blob) {
    const K = [0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
    const H = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    const words = new Uint32Array(64), block = new Uint8Array(64); let used = 0;
    const rotate = (x, n) => (x >>> n) | (x << (32 - n));
    function compress(bytes, offset) {
      for (let i = 0; i < 16; i++) { const j = offset + i * 4; words[i] = (bytes[j] << 24) | (bytes[j + 1] << 16) | (bytes[j + 2] << 8) | bytes[j + 3]; }
      for (let i = 16; i < 64; i++) { const a = words[i - 15], b = words[i - 2]; words[i] = (words[i - 16] + (rotate(a,7) ^ rotate(a,18) ^ (a >>> 3)) + words[i - 7] + (rotate(b,17) ^ rotate(b,19) ^ (b >>> 10))) >>> 0; }
      let [a,b,c,d,e,f,g,h] = H;
      for (let i = 0; i < 64; i++) { const t = (h + (rotate(e,6) ^ rotate(e,11) ^ rotate(e,25)) + ((e & f) ^ (~e & g)) + K[i] + words[i]) >>> 0; const u = ((rotate(a,2) ^ rotate(a,13) ^ rotate(a,22)) + ((a & b) ^ (a & c) ^ (b & c))) >>> 0; h=g;g=f;f=e;e=(d+t)>>>0;d=c;c=b;b=a;a=(t+u)>>>0; }
      [a,b,c,d,e,f,g,h].forEach((v,i) => { H[i] = (H[i] + v) >>> 0; });
    }
    for (let offset = 0; offset < blob.size; offset += 1024 * 1024) {
      const bytes = new Uint8Array(await blob.slice(offset, offset + 1024 * 1024).arrayBuffer()); let i = 0;
      if (used) { const n = Math.min(64 - used, bytes.length); block.set(bytes.subarray(0,n), used); used += n; i += n; if (used === 64) { compress(block,0); used=0; } }
      for (; i + 64 <= bytes.length; i += 64) compress(bytes, i);
      if (i < bytes.length) { block.set(bytes.subarray(i),0); used=bytes.length-i; }
    }
    block[used++] = 0x80;
    if (used > 56) { block.fill(0,used); compress(block,0); used=0; }
    block.fill(0,used,56);
    const bits = BigInt(blob.size) * 8n; for(let i=0;i<8;i++) block[63-i]=Number((bits >> BigInt(i*8)) & 255n);
    compress(block,0); return H.map(x => x.toString(16).padStart(8,'0')).join('');
  }

  function install(w, d) {
    const script = d.currentScript;
    // Presence of the script, a browser preference, or a malformed value is never opt-in.
    if (!script || script.dataset.cloudEnabled !== 'true' || w.ArtCraftServerCloud) return null;
    const scriptURL = new URL(script.src || '/__cloud/client.js', w.location.href);
    if (scriptURL.origin !== w.location.origin || !scriptURL.pathname.endsWith('/__cloud/client.js')) return null;
    const rootURL = new URL('../', scriptURL);
    const apiURL = new URL('api', scriptURL);
    const PREFIX = apiURL.pathname.replace(/\/$/, '');
    const app = script.dataset.cloudApp || '';
    if (app && !/^[a-z][a-z0-9-]{0,63}$/.test(app)) return null;
    const s = { enabled: false, namespace: null, csrf: '', config: null, locked: false, changed: false, notice: '', api: null, claimed: false, connected: false, connecting: false, resumePending: false, startupNotice: '', epoch: 0, listEpoch: 0, recentsEpoch: 0, projects: [], pending: new Map(), bindings: new Map(), openings: new Map(), dialogs: [], seen: new Set(), objects: new Set(), panelOpen: false, accountOpen: false, filter: '', ui: null, logoutConfirm: false, accountData: null, signingOut: false };
    const pageId = w.crypto.randomUUID();
    let dbPromise, refreshInFlight = null, watchTimer = null, intentBusy = false;
    let intention = null;
    try { const values = new URL(w.location.href).searchParams.getAll('cloudProject'); if (values.length === 1 && isId(values[0]) && app) intention = values[0]; else if (values.length) s.notice = 'This server-project link is invalid.'; } catch (_) { /* Plain page has no open intent. */ }
    const date = value => { const n = new Date(typeof value==='number'&&value<1e12?value*1000:value); return Number.isNaN(n.valueOf()) ? '' : n.toLocaleString(); };
    const recordKey = record => record.namespace + ':' + record.operation_id;
    const nativeObject = () => LABELS[app] ? w['artcraft' + LABELS[app]] : null;
    const current = (namespace, epoch) => s.enabled && !s.locked && !s.changed && namespace === s.namespace && epoch === s.epoch;
    function error(message, code) { const e = new Error(message); e.code = code; return e; }
    function suspend(changed, message) {
      s.locked = true; s.connected = false; s.changed = s.changed || changed; ++s.epoch; ++s.listEpoch; ++s.recentsEpoch;
      s.projects = []; s.accountData=null;s.logoutConfirm=false; if(s.ui)s.ui.signOutButton.hidden=true; s.bindings.clear(); s.openings.clear(); s.notice = message;
      for (const url of s.objects) w.URL.revokeObjectURL(url); s.objects.clear();
      if (s.api) { try { s.api.setProjects([]); s.api.setCloudEnabled(false); } catch (_) { /* Keep the original documents intact. */ } }
      if(changed)for(const r of s.pending.values()){if(canReply(r,true)){try{reply(r,'cancel');}catch(_){}}r.detached=true;}
      stopWatch(); closeDialog(true); renderProjects(); renderPending(); if(s.ui)s.ui.accountName.textContent='Sign in again'; renderStatus();
    }
    async function request(path, options = {}, namespace = s.namespace, json = true) {
      if (!path.startsWith('/') || path.includes('://')) throw error('Invalid server request.', 'invalid_request');
      if (s.changed || (namespace && namespace !== s.namespace)) throw error('The account changed. Reload before using server storage.', 'identity_changed');
      const headers = Object.assign({}, options.headers);
      if (namespace) headers['X-Craft-Cloud-Namespace'] = namespace;
      if (options.method && !['GET','HEAD'].includes(options.method)) headers['X-Craft-CSRF'] = s.csrf;
      const response = await w.fetch(PREFIX + path, Object.assign({}, options, { headers, credentials: 'same-origin', cache: 'no-store', redirect: 'error' }));
      if (response.redirected || (response.url && new URL(response.url, w.location.href).origin !== w.location.origin)) throw error('Unexpected sign-in redirect. Reload after signing in.', 'auth_required');
      // The upstream apps gate may reject before the cloud JSON handler: local
      // session expiry is plain-text 401, and a missing proxy identity is 403.
      // Clear stale server data from these authentication responses before parsing.
      if (response.status === 401 || (path === '/config' && response.status === 403)) {
        suspend(false, 'Sign in again to use your server files. Pending copies have not been resent.');
        throw error(response.status === 403 ? 'This account cannot access server storage. Sign in with the configured provider.' : 'Sign in again.', 'auth_required');
      }
      let value;
      if (json || !response.ok) {
        const text = await response.text();
        if (text.length > 1024 * 1024) throw error('The server response is too large.', 'invalid_response');
        try { value = JSON.parse(text); } catch (_) { throw error('The server returned an invalid response.', 'invalid_response'); }
      }
      if (!response.ok) {
        const code = value && (value.error || value.code);
        if (code === 'identity_changed') suspend(true, 'The signed-in account changed. Reload before using server storage.');
        const messages = { 401: 'Sign in again.', 403: 'This server request was not authorized.', 404: 'The file or server feature is unavailable.', 409: 'This project changed in another session. Keep this copy and use Save as.', 413: 'This copy exceeds the server limit.', 507: 'The server has no space for this copy.' };
        throw error(messages[response.status] || 'The server could not complete the request.', code || 'request_failed');
      }
      if (namespace && (s.changed || namespace !== s.namespace)) throw error('The account changed during this request.', 'identity_changed');
      return json ? value : response;
    }
    function validCapture(f) { return !!f && isId(f.id) && VALID_NAME(f.filename) && typeof f.app === 'string' && Number.isSafeInteger(f.size) && f.size >= 0 && /^[a-f0-9]{64}$/i.test(f.sha256 || '') && (f.url === API_PATH + '/files/' + f.id || f.url === PREFIX + '/files/' + f.id); }
    function validProject(p) { return !!p && isId(p.id) && VALID_NAME(p.name) && validCapture(p.head) && p.head.app === p.app; }
    function database() {
      if (!dbPromise) dbPromise = new Promise((resolve, reject) => {
        if (!w.indexedDB) { reject(error('This browser cannot retain a recovery copy. Keep the page open until saving completes.', 'no_recovery')); return; }
        const req = w.indexedDB.open('craft-server-cloud-v1', 1);
        req.onupgradeneeded = () => req.result.createObjectStore('pending', { keyPath: 'key' });
        req.onsuccess = () => resolve(req.result); req.onerror = () => reject(error('Recovery storage is unavailable. Keep the document open.', 'no_recovery'));
      });
      return dbPromise;
    }
    async function disk(mode, action) { const db = await database(); return new Promise((resolve,reject) => { const tx=db.transaction('pending',mode), store=tx.objectStore('pending'); let result; try { result=action(store); } catch(e) { reject(e);return; } tx.oncomplete=()=>resolve(result && result.result);tx.onabort=tx.onerror=()=>reject(error('The recovery copy could not be retained. Keep the page open.','no_recovery')); }); }
    function diskRecord(r) { return { key: recordKey(r), namespace:r.namespace, app:r.app, filename:r.filename, operation_id:r.operation_id, project_id:r.project_id, base_id:r.base_id, intent:r.intent, blob:r.blob, sha256:r.sha256 || null, upload_id:r.upload_id || null, ack:r.ack || null, captured_at:r.captured_at }; }
    async function persist(r) { await disk('readwrite', store => store.put(diskRecord(r))); }
    async function restore() {
      try { const all=await disk('readonly',store=>store.getAll()); for(const r of all || []) if(r.namespace===s.namespace && r.app && VALID_NAME(r.filename) && r.blob instanceof w.Blob && isId(r.operation_id)) s.pending.set(r.operation_id,Object.assign(r,{ detached:true, state:'recovered', error:'A previous page retained this copy. Download it or explicitly retry it.' })); }
      catch(e) { s.notice=e.message; }
    }
    function nativeSnapshot() {
      try { const v=s.api && s.api.snapshot(); return v && v.protocolVersion===1 && (v.app===undefined || v.app===app) && Array.isArray(v.documents) && v.documents.every(x=>x && typeof x.documentId==='string' && /^\d+$/.test(x.documentId) && Object.prototype.hasOwnProperty.call(x,'pendingRequestId')) ? v : null; } catch(_) { return null; }
    }
    function canReply(r, local = false) { return !r.detached && r.pageId===pageId && s.api===r.api && nativeObject()===r.api && (local || (s.connected && s.enabled && !s.locked && !s.changed && r.namespace===s.namespace)); }
    function reply(r,status,project) { if (!canReply(r,status==='cancel'||status==='download')) return false; s.api.saveResult(r.requestId,status,project || ''); return true; }
    async function config() {
      if (refreshInFlight) return refreshInFlight;
      refreshInFlight=(async()=>{
        const value=await request('/config',{},null);
        if (!value || typeof value.enabled!=='boolean') throw error('Invalid server configuration.','invalid_response');
        if (!value.enabled) {
          s.enabled=false; s.connected=false; ++s.epoch; stopWatch(); closeDialog(true);
          if(s.api){for(const r of s.pending.values()) if(canReply(r,true)){try{reply(r,'cancel');r.detached=true;}catch(_){}}try{s.api.setProjects([]);s.api.setCloudEnabled(false);}catch(_){}}
          if(s.ui){s.ui.host.remove();s.ui=null;} return false;
        }
        if(typeof value.namespace!=='string' || !/^[A-Za-z0-9_-]{16,128}$/.test(value.namespace) || typeof value.csrfToken!=='string' || !/^[A-Za-z0-9_-]{16,128}$/.test(value.csrfToken) || !Number.isSafeInteger(value.maxFileBytes) || value.maxFileBytes<1 || !Number.isSafeInteger(value.chunkBytes) || value.chunkBytes<1 || value.chunkBytes>8*1024*1024 || !value.apps || typeof value.apps!=='object') throw error('Invalid server configuration.','invalid_response');
        if(s.namespace && s.namespace!==value.namespace){suspend(true,'The signed-in account changed. Reload before using server storage.');return false;}
        if(s.changed) return false;
        if(s.locked && /^Sign in again to use your server files/.test(s.notice))s.notice='';s.namespace=value.namespace;s.csrf=value.csrfToken;s.config=value;s.enabled=true;s.locked=false; initUI(); return true;
      })().finally(()=>{refreshInFlight=null;}); return refreshInFlight;
    }
    async function ensureIdentity(namespace) { if(!await config() || s.locked || s.changed || (namespace && namespace!==s.namespace)) throw error('Sign in with the original account before continuing.','identity_changed'); }
    async function refreshRecents() {
      if(!s.connected || s.locked || !s.enabled) return;
      const token=++s.recentsEpoch, namespace=s.namespace, api=s.api, epoch=s.epoch;
      try { const value=await request('/projects?app='+encodeURIComponent(app),{},namespace); if(!value || !Array.isArray(value.projects) || value.projects.some(p=>!validProject(p)||p.app!==app)) throw error('Invalid server Recents.','invalid_response'); if(current(namespace,epoch) && token===s.recentsEpoch && s.api===api) api.setProjects(value.projects.slice(0,100).map(p=>({id:p.id,name:p.name,updatedAt:date(p.updated_at)}))); }
      catch(e){if(current(namespace,epoch)){s.notice='Server Recents could not be loaded. '+e.message;renderStatus();}}
    }
    async function refreshLibrary() {
      await ensureIdentity(); const token=++s.listEpoch, namespace=s.namespace, epoch=s.epoch;
      const value=await request('/projects',{},namespace);
      if(!value || !Array.isArray(value.projects) || value.projects.some(p=>!validProject(p))) throw error('Invalid server project list.','invalid_response');
      if(current(namespace,epoch) && token===s.listEpoch){s.projects=value.projects;renderProjects();}
    }
    async function openProject(id) {
      if (!s.connected || !isId(id) || s.openings.size) throw error('Wait for the editor to finish opening its current project.', 'open_busy');
      s.openings.set(id, null);
      try {
        await ensureIdentity();
        const namespace = s.namespace, epoch = s.epoch, api = s.api;
        const p = await request('/projects/' + id, {}, namespace);
        if (!validProject(p) || p.id !== id || p.app !== app || p.head.size > s.config.maxFileBytes) throw error('This project cannot be opened by this editor.', 'invalid_project');
        const response = await request('/files/' + p.head.id, {}, namespace, false), blob = await response.blob();
        if (blob.size !== p.head.size || await sha256Blob(blob) !== p.head.sha256.toLowerCase()) throw error('The downloaded project failed verification.', 'invalid_file');
        if (!current(namespace, epoch) || api !== s.api) throw error('The account changed while opening this project.', 'identity_changed');
        s.openings.set(id, { projectId: id, baseVersion: p.head.id, filename: p.head.filename, name: p.name, namespace });
        if (api.blobProjects === true && typeof api.openProjectBlob === 'function' && /\.zip$/i.test(p.head.filename)) api.openProjectBlob(id, p.head.filename, blob);
        else api.openProject(id, p.head.filename, new Uint8Array(await blob.arrayBuffer()));
        s.notice = 'The project was sent to the editor. Complete any import or unsaved-work prompts.';
        renderStatus();
      } catch (e) {
        s.openings.delete(id);
        throw e;
      }
    }
    async function attemptIntent() { if(!intention || intentBusy || !s.connected || !nativeSnapshot() || s.locked) return;intentBusy=true; try{await openProject(intention);}catch(e){s.notice='The linked project could not be opened. '+e.message;renderStatus();}finally{intentBusy=false;} }
    function stopWatch(){if(watchTimer!==null)w.clearTimeout(watchTimer);watchTimer=null;}
    function watch(){stopWatch();if(!s.connected||s.locked||!s.enabled||d.visibilityState==='hidden')return;renderStatus();watchTimer=w.setTimeout(watch,500);}
    async function tryConnect() {
      if(!app || s.changed || s.locked || !s.enabled) return;
      if(s.connecting){s.resumePending=true;return;}
      const api=nativeObject();
      if(!api || api.protocolVersion!==1 || (api.app!==app && !(app==='photocraft' && api.app===undefined)) || ['connect','setCloudEnabled','setProjects','openProject','resolveSaveDialog','saveResult','snapshot'].some(k=>typeof api[k]!=='function')) {renderStatus();return;}
      if(s.claimed && api!==s.api){s.notice='The editor was replaced. Reload before reconnecting server storage.';renderStatus();return;}
      s.connecting=true;
      try {
        s.api=api;
        if(!s.claimed){if(api.connect()!==true){s.api=null;return;}s.claimed=true;if(api.setCloudEnabled(true)!==true)throw error('The editor did not enable server storage.','unsupported_editor');}
        const deadline=Date.now()+3000;
        for(let i=0;!nativeSnapshot()&&i<60&&Date.now()<deadline;i++){if(!w.requestAnimationFrame)break;await new Promise(resolve=>w.requestAnimationFrame(resolve));}
        if(!nativeSnapshot()){const notice='The editor is still loading. Server storage will reconnect when this tab becomes active.';if(!s.notice||s.notice===s.startupNotice)s.notice=notice;s.startupNotice=notice;renderStatus();return;}
        if(!s.enabled||s.locked)return;if(s.notice===s.startupNotice)s.notice='';s.startupNotice=''; if(nativeSnapshot().cloudEnabled!==true && api.setCloudEnabled(true)!==true)throw error('The editor did not re-enable server storage.','unsupported_editor');s.connected=true; for(const record of s.pending.values())if(record.state==='waiting_native'&&!record.detached&&!record.busy&&record.namespace===s.namespace&&record.api===s.api)save(record); watch(); await refreshRecents();await attemptIntent();
      } catch(e){s.notice=e.message;renderStatus();} finally{
        const resumePending=s.resumePending;s.resumePending=false;s.connecting=false;
        // Focus/visibility can arrive while a hidden tab is still waiting for its
        // throttled animation frame. Consume that event once after the attempt,
        // retaining the claimed API and never starting another polling loop.
        if(resumePending&&!s.connected&&s.claimed&&s.enabled&&!s.locked&&!s.changed&&d.visibilityState!=='hidden'&&nativeObject()===s.api)tryConnect();
      }
    }
    function makeBlob(event) { if(event.blob instanceof w.Blob && s.api.blobProjects===true && event.bytes===undefined) return new w.Blob([event.blob]); if(ArrayBuffer.isView(event.bytes)&&event.bytes.BYTES_PER_ELEMENT===1) return new w.Blob([event.bytes]); throw error('The editor supplied an invalid document.','invalid_document'); }
    async function save(record) {
      if(record.busy || (record.namespace!==s.namespace) || s.locked || s.changed) return;
      record.busy=true;record.error='';record.state='Saving';renderStatus();renderPending();
      try {
        await ensureIdentity(record.namespace);
        if(!record.blob || record.blob.size>s.config.maxFileBytes)throw error('This copy exceeds the server limit. Download it to this device.','file_too_large');
        if(!record.detached && !s.connected){record.state='waiting_native';await persist(record);return;}
        if(record.project_id&&!record.base_id)throw error('The base version is unknown. Download this copy and use Save as.','missing_base');
        if(!record.sha256)record.sha256=await sha256Blob(record.blob);
        await persist(record);
        let upload=await request('/uploads',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({operation_id:record.operation_id,app:record.app,filename:record.filename,size:record.blob.size,sha256:record.sha256,intent:record.intent,project_id:record.project_id,base_id:record.base_id})},record.namespace);
        if(!upload || !isId(upload.id) || upload.chunk_bytes!==s.config.chunkBytes || !Array.isArray(upload.parts) || !['pending','complete'].includes(upload.status)) throw error('The server did not identify this upload.','invalid_response');
        record.upload_id=upload.id;await persist(record);
        if(upload.status!=='complete') {
          const count=Math.ceil(record.blob.size/upload.chunk_bytes);
          if(upload.parts.some(i=>!Number.isSafeInteger(i)||i<0||i>=count)||new Set(upload.parts).size!==upload.parts.length)throw error('The upload manifest is invalid.','invalid_response');
          for(let index=0;index<count;index++) if(!upload.parts.includes(index)) {
            if(s.locked||s.changed||record.namespace!==s.namespace)throw error('The account changed. The copy has not been resent.','identity_changed');
            const body=record.blob.slice(index*upload.chunk_bytes,(index+1)*upload.chunk_bytes);
            await request('/uploads/'+upload.id+'/parts/'+index,{method:'PUT',headers:{'Content-Type':'application/octet-stream'},body},record.namespace);
          }
          upload=await request('/uploads/'+upload.id+'/complete',{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'},record.namespace);
        }
        const project=upload.project,file=upload.file;
        if(!validProject(project)||!validCapture(file)||project.head.id!==file.id||project.app!==record.app||file.app!==record.app||file.filename!==record.filename||file.size!==record.blob.size||file.sha256.toLowerCase()!==record.sha256)throw error('The server acknowledgement does not match this copy.','invalid_ack');
        if(record.namespace!==s.namespace||s.changed||s.locked)throw error('The account changed before acknowledgement.','identity_changed');
        record.ack={project,file};await persist(record);
        // Only the native request owns its captured revision. A newer revision remains dirty.
        if(!record.detached && canReply(record)) {
          if(!reply(record,record.copy?'download':'saved',project.id))throw error('The editor did not accept the saved acknowledgement.','native_ack');
          if(!record.copy)s.bindings.set(record.documentId,{projectId:project.id,baseVersion:file.id,filename:file.filename,name:project.name,namespace:record.namespace});
        }
        await disk('readwrite',store=>store.delete(recordKey(record)));s.pending.delete(record.operation_id);s.notice='Saved on the server.'; await refreshRecents();if(s.panelOpen)await refreshLibrary();
      } catch(e) {record.error=e.message;record.state='Pending';if(canReply(record)){try{reply(record,'failed');}catch(_){}}try{await persist(record);}catch(_){}s.notice=e.message;} finally {record.busy=false;renderPending();renderStatus();if(record.state==='waiting_native'&&!record.detached&&s.connected&&!s.locked&&!s.changed)Promise.resolve().then(()=>save(record));}
    }
    function download(blob, name) {const url=w.URL.createObjectURL(blob),a=d.createElement('a');s.objects.add(url);a.href=url;a.download=name;d.body.appendChild(a);a.click();a.remove();w.setTimeout(()=>{w.URL.revokeObjectURL(url);s.objects.delete(url);},60000);}
    function receive(event) {
      if(!event || typeof event!=='object' || (event.app!==undefined&&event.app!==app)) return;
      if(event.type==='ready'){tryConnect();return;}
      if(!s.claimed || !s.api)return;
      try {
        if(event.type==='save') {
          if(s.seen.has(event.requestId))return;
          if(typeof event.requestId!=='string'||!event.requestId||!/^\d+$/.test(event.documentId||'')||!/^\d+$/.test(event.revision||'')||!VALID_NAME(event.filename)||!['save','saveAs','copy'].includes(event.intent)||!['cloud','device'].includes(event.destination))throw error('The editor supplied an invalid save request.','invalid_document');
          const blob=makeBlob(event),binding=s.bindings.get(event.documentId);
          const record={namespace:s.namespace,app,operation_id:w.crypto.randomUUID(),filename:event.filename,blob,intent:event.intent==='save'&&!event.copy?'save':'save_as',project_id:event.intent==='save'&&!event.copy?(binding?binding.projectId:(event.projectId||null)):null,base_id:event.intent==='save'&&!event.copy&&binding?binding.baseVersion:null,requestId:event.requestId,documentId:event.documentId,revision:event.revision,copy:event.copy===true||event.intent==='copy',api:s.api,pageId,captured_at:new Date().toISOString()};
          s.seen.add(event.requestId);
          if(event.destination==='device'){download(blob,event.filename);reply(record,'download');s.notice='Download requested. The browser does not confirm that a copy was retained.';renderStatus();return;}
          s.pending.set(record.operation_id,record);save(record);
        } else if(event.type==='saveDialog') {
          if(!s.enabled||s.locked||s.changed)return;
          if(typeof event.requestId!=='string'||!event.requestId||!VALID_NAME(event.filename)||!['save','saveAs','copy'].includes(event.intent))throw error('The editor supplied an invalid save dialog.','invalid_document');
          if(!s.dialogs.some(x=>x.requestId===event.requestId)){s.dialogs.push(Object.freeze(Object.assign({},event)));showDialog();}
        } else if(event.type==='saveUnchanged') {s.notice='No changes to save on the server.';renderStatus();}
        else if(event.type==='openRequest'){openProject(event.projectId).catch(showError);}
        else if(event.type==='opened') {
          const binding=s.openings.get(event.projectId);
          if(binding && binding.namespace===s.namespace && /^\d+$/.test(event.documentId||'')) {
            if(event.filename!==undefined){if(!VALID_NAME(event.filename))throw error('Invalid opened filename.','invalid_document');binding.filename=event.filename;}
            s.bindings.set(event.documentId,binding);s.openings.delete(event.projectId);s.notice='Project opened from the server.';
            if(intention===event.projectId){const url=new URL(w.location.href);if(url.searchParams.getAll('cloudProject').length===1&&url.searchParams.get('cloudProject')===intention){url.searchParams.delete('cloudProject');w.history.replaceState(null,'',url.href);}intention=null;}
            renderStatus();
          }
        } else if(event.type==='openFailed'||event.type==='openCancelled'){s.openings.delete(event.projectId);s.notice=event.type==='openCancelled'?'Opening was cancelled.':'The editor could not open this project.';renderStatus();}
      } catch(e){showError(e);}
    }
    function showError(e){s.notice=e.message||'The server action failed.';renderStatus();}
    function element(tag,text,parent){const n=d.createElement(tag);if(text!==null&&text!==undefined)n.textContent=text;if(parent)parent.appendChild(n);return n;}
    function button(text,callback,parent){const n=element('button',text,parent);n.type='button';n.addEventListener('click',callback);return n;}
    function clear(node){while(node.firstChild)node.removeChild(node.firstChild);}
    function position(){let p;try{p=JSON.parse(w.localStorage.getItem('craft-cloud-position-v1'));}catch(_){}return p&&Number.isFinite(p.x)&&Number.isFinite(p.y)?{x:clamp(p.x,0,1)*(w.innerWidth-48),y:clamp(p.y,0,1)*(w.innerHeight-48)}:{x:w.innerWidth-64,y:w.innerHeight-64};}
    function place(){if(!s.ui)return;const u=s.ui, box=u.dialog||u.panel, rect=box.getBoundingClientRect(),g=panelGeometry(u.position,w.innerWidth,w.innerHeight,rect.width||420,Math.min(rect.height||520,w.innerHeight-24));u.position={x:g.x,y:g.y};u.bubble.style.left=g.x+'px';u.bubble.style.top=g.y+'px';box.style.left=g.left+'px';box.style.top=g.top+'px';box.style.maxWidth=g.width+'px';box.style.maxHeight=(w.innerHeight-24)+'px';}
    function savePosition(){try{w.localStorage.setItem('craft-cloud-position-v1',JSON.stringify({x:s.ui.position.x/Math.max(1,w.innerWidth-48),y:s.ui.position.y/Math.max(1,w.innerHeight-48)}));}catch(_){}}
    function initUI(){
      if(s.ui||!d.body)return;
      const host=element('div',null,d.body);host.id='craft-server-cloud';host.style.cssText='position:fixed;inset:0;z-index:2147483500;pointer-events:none';
      const root=host.attachShadow({mode:'open'}),style=element('style',null,root);
      style.textContent=':host{font:14px/1.45 Archivo,system-ui,sans-serif;color-scheme:light dark;--bg:#f2f1ee;--ink:#101014;--muted:#656562;--line:#cfcec9;--accent:#428df5;--sunken:#e9e8e4}*{box-sizing:border-box}[hidden]{display:none!important}button,input{font:inherit;color:inherit}button{cursor:pointer;border:1px solid var(--line);background:var(--bg);padding:8px 11px;border-radius:6px}button:hover{border-color:var(--accent)}button:focus-visible,input:focus-visible,a:focus-visible{outline:2px solid var(--accent);outline-offset:2px}a{color:var(--accent)}.bubble{position:fixed;z-index:4;pointer-events:auto;touch-action:none;user-select:none;width:48px;height:48px;border-radius:50%;background:var(--bg);color:var(--ink);box-shadow:0 3px 14px #0005;display:grid;place-items:center;padding:0}.bubble svg{width:24px;height:24px;pointer-events:none}.bubble[data-pending=true]{border:2px solid var(--accent)}.panel,.dialog{position:fixed;z-index:2;pointer-events:auto;width:420px;max-width:calc(100vw - 24px);max-height:calc(100vh - 24px);overflow:auto;border:1px solid var(--line);border-radius:10px;background:var(--bg);color:var(--ink);box-shadow:0 8px 32px #0006;padding:16px}.dialog{z-index:3}.head{display:flex;align-items:center;gap:8px}.head h2{flex:1;font-size:17px;margin:0}.icon{padding:3px 8px}.status,.muted{color:var(--muted);font-size:12px}.status{margin:10px 0;overflow-wrap:anywhere}.search,.filename{display:block;width:100%;border:1px solid var(--line);background:var(--sunken);border-radius:5px;padding:9px;margin:12px 0}.files{list-style:none;padding:0;margin:0}.file,.pending{padding:12px 0;border-top:1px solid var(--line)}.name{display:block;width:100%;text-align:left;background:none;border:0;padding:0;overflow-wrap:anywhere}.meta{display:flex;gap:8px;align-items:center;margin-top:5px}.meta span{flex:1;font-size:12px;color:var(--muted)}.actions{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}.primary{background:var(--accent);color:white;border-color:var(--accent)}.error{color:#a62e32;overflow-wrap:anywhere;font-size:12px}.section{margin:14px 0 6px;color:var(--muted);text-transform:uppercase;font-size:11px;letter-spacing:.08em}@media(prefers-color-scheme:dark){:host{--bg:#111214;--ink:#f4f3f0;--muted:#a4a5a3;--line:#343538;--sunken:#08090a}.error{color:#ff9196}}';
      const bubble=button('',()=>{if(u.dragged){u.dragged=false;return;}s.panelOpen=!s.panelOpen;u.panel.hidden=!s.panelOpen;bubble.setAttribute('aria-expanded',String(s.panelOpen));place();if(s.panelOpen)refreshLibrary().catch(showError);},root);bubble.className='bubble';bubble.setAttribute('aria-label','Server files. Drag to move; use arrow keys to reposition.');bubble.setAttribute('aria-expanded','false');
      const svg=d.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('aria-hidden','true');const path=d.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('fill','currentColor');path.setAttribute('d','M7 19a5 5 0 0 1-.8-9.94A6 6 0 0 1 18 8a5.5 5.5 0 0 1 .5 11Z');svg.appendChild(path);bubble.appendChild(svg);
      const panel=element('section',null,root);panel.className='panel';panel.hidden=true;panel.setAttribute('aria-label','Server files');
      const head=element('div',null,panel);head.className='head';element('h2','Server files',head);
      button('Account',()=>{s.accountOpen=!s.accountOpen;u.account.hidden=!s.accountOpen;if(s.accountOpen)loadAccount();place();},head).className='icon';button('×',()=>{s.panelOpen=false;panel.hidden=true;bubble.setAttribute('aria-expanded','false');bubble.focus();},head).setAttribute('aria-label','Close server files');
      const status=element('p','',panel);status.className='status';status.setAttribute('role','status');status.setAttribute('aria-live','polite');
      const account=element('section',null,panel);account.hidden=true;const accountName=element('p','Signed in',account),accountHint=element('p','',account);accountHint.className='muted';const signOutButton=button('Sign out',signOut,account);signOutButton.hidden=true;
      const search=element('input',null,panel);search.type='search';search.placeholder='Search server files';search.className='search';search.setAttribute('aria-label','Search server files');search.addEventListener('input',()=>{s.filter=search.value;renderProjects();});
      const pending=element('section',null,panel);const heading=element('p','Your projects',panel);heading.className='section';const list=element('ul',null,panel);list.className='files';
      const info=element('p','',panel);info.className='muted';
      const u={host,root,bubble,panel,status,account,accountName,accountHint,signOutButton,search,pending,list,info,position:position(),dragged:false,dialog:null};s.ui=u;
      let drag=null;
      bubble.addEventListener('pointerdown',e=>{if(e.button!==undefined&&e.button!==0)return;drag={id:e.pointerId,x:e.clientX,y:e.clientY,at:{...u.position},moved:false};bubble.setPointerCapture(e.pointerId);});
      bubble.addEventListener('pointermove',e=>{if(!drag||drag.id!==e.pointerId)return;const dx=e.clientX-drag.x,dy=e.clientY-drag.y;if(Math.abs(dx)+Math.abs(dy)>5)drag.moved=true;if(drag.moved){u.position={x:drag.at.x+dx,y:drag.at.y+dy};place();}});
      bubble.addEventListener('pointerup',e=>{if(!drag||drag.id!==e.pointerId)return;u.dragged=drag.moved;drag=null;savePosition();});
      bubble.addEventListener('pointercancel',()=>{drag=null;u.dragged=true;});
      bubble.addEventListener('keydown',e=>{const moves={ArrowLeft:[-10,0],ArrowRight:[10,0],ArrowUp:[0,-10],ArrowDown:[0,10]};if(moves[e.key]){e.preventDefault();u.position.x+=moves[e.key][0];u.position.y+=moves[e.key][1];place();savePosition();}});
      panel.addEventListener('keydown',e=>{if(e.key==='Escape'){panel.hidden=true;s.panelOpen=false;bubble.setAttribute('aria-expanded','false');bubble.focus();}});
      renderStatus();renderProjects();renderPending();place();
    }
    async function loadAccount() {
      const namespace = s.namespace;
      s.accountData = null;
      if (s.ui) s.ui.signOutButton.hidden = true;
      try {
        await ensureIdentity(namespace);
        const epoch = s.epoch;
        const response = await w.fetch(new URL('auth/me', rootURL).href, { credentials: 'same-origin', cache: 'no-store', redirect: 'error' });
        if (response.status === 401 || response.status === 403) suspend(false, 'Sign in again to use your server files. Pending copies have not been resent.');
        if (!response.ok || response.redirected) throw error('Sign in again to check your account.', 'auth_required');
        const value = await response.json();
        if (!value || typeof value.can_logout !== 'boolean' || typeof value.user !== 'string') throw error('The server returned invalid account information.', 'invalid_response');
        await ensureIdentity(namespace);
        if (!current(namespace, epoch)) return null;
        s.accountData = { namespace, can_logout: value.can_logout, user: value.user };
        if (s.ui) {
          s.ui.accountName.textContent = value.user;
          s.ui.signOutButton.hidden = !value.can_logout;
          s.ui.accountHint.textContent = value.can_logout ? 'Sign out ends this launcher session. Your identity provider may remain signed in.' : 'Your identity is managed by the sign-in provider or browser. Sign out there to end access.';
        }
        return s.accountData;
      } catch (e) {
        if (s.ui) { s.ui.accountName.textContent = 'Sign in again'; s.ui.accountHint.textContent = e.message; }
        return null;
      } finally { place(); }
    }
    async function signOut() {
      if (s.signingOut) return;
      s.signingOut = true;
      const namespace = s.namespace;
      try {
        // Capability and identity are checked again after any pending-copy warning;
        // Basic/proxy authentication cannot be revoked by deleting a session cookie.
        const account = await loadAccount();
        if (!account || account.namespace !== namespace || !account.can_logout) {
          s.notice = account && !account.can_logout ? 'This identity is managed by the sign-in provider or browser. No launcher session was signed out.' : 'The account could not be verified. Sign out has not been requested.';
          renderStatus(); return;
        }
        if (s.pending.size && s.logoutConfirm !== namespace) {
          s.logoutConfirm = namespace;
          s.notice = 'Pending copies remain in this browser. Save or download them first, or choose Sign out again to continue.';
          renderStatus(); return;
        }
        if (s.changed || s.locked || namespace !== s.namespace) throw error('The account changed before sign out.', 'identity_changed');
        const response = await w.fetch(new URL('auth/logout', rootURL).href, { method: 'POST', headers: { 'X-Craft-Cloud-Namespace': namespace }, credentials: 'same-origin', redirect: 'error' });
        if (response.status === 409) { suspend(true, 'The signed-in account changed. Reload before signing out.'); return; }
        if (response.status === 401) { suspend(false, 'Sign in again before signing out.'); return; }
        if (!response.ok || response.redirected) throw error('Sign out failed. Keep this page open and try again.', 'logout_failed');
        suspend(false, 'Signed out.');
        // A public login page does not immediately start another OIDC sign-in,
        // whereas the protected launcher root may do so with an existing IdP cookie.
        w.location.assign(new URL('auth/login', rootURL).href);
      } catch (e) { s.notice = e.message || 'Sign out failed. Keep this page open and try again.'; renderStatus(); }
      finally { s.signingOut = false; }
    }
    function renderStatus() {
      if (!s.ui) return;
      const snap = nativeSnapshot(), dirty = snap && snap.documents.some(x => x.dirty || x.pendingRequestId);
      const text = s.notice || (s.locked ? 'Sign in to use server files.' : s.pending.size ? 'A server copy is pending.' : dirty ? 'Changes have not been saved to the server.' : 'Server files are ready.');
      const info = app && !s.claimed ? 'This editor release has no native server-storage protocol. Its normal device save remains available.' : '';
      const changed = s.ui.status.textContent !== text || s.ui.info.textContent !== info;
      s.ui.status.textContent = text; s.ui.info.textContent = info;
      s.ui.bubble.title = text;
      s.ui.bubble.setAttribute('data-pending', String(!!(s.pending.size || dirty || s.locked)));
      if (changed) place();
    }
    function renderProjects(){if(!s.ui)return;clear(s.ui.list);if(s.locked||s.changed){const li=element('li','Sign in with the same account to see its server files.',s.ui.list);const a=element('a','Sign in',li);const next=w.location.pathname.startsWith(rootURL.pathname)?w.location.pathname.slice(rootURL.pathname.length):'';a.href=new URL('auth/login?next='+encodeURIComponent(next),rootURL).href;place();return;}const items=s.projects.filter(p=>p.name.toLocaleLowerCase().includes(s.filter.toLocaleLowerCase()));for(const p of items){const li=element('li',null,s.ui.list);li.className='file';const name=button(p.name,()=>activateProject(p).catch(showError),li);name.className='name';const meta=element('div',null,li);meta.className='meta';element('span',(LABELS[p.app]||p.app)+' · '+date(p.updated_at),meta);button('Download',()=>downloadProject(p).catch(showError),meta);}if(!items.length)element('li',s.projects.length?'No matching projects.':'No server projects yet.',s.ui.list);place();}
    async function activateProject(p){await ensureIdentity();if(p.app===app&&s.connected)return openProject(p.id);const target=s.config.apps[p.app],entry=target&&target.entry;if(typeof entry!=='string'||!/^[a-z][a-z0-9-]{0,63}$/.test(entry))throw error('The matching editor is not configured on this server.','unsupported_editor');let base=rootURL;if(target.origin){const origin=new URL(target.origin);if(!['http:','https:'].includes(origin.protocol)||origin.username||origin.password)throw error('The editor origin is invalid.','unsupported_editor');base=new URL(origin.origin+'/');}const url=new URL(entry+'/',base);url.searchParams.set('cloudProject',p.id);w.open(url.href,'_blank','noopener');}
    async function downloadProject(p){await ensureIdentity();const namespace=s.namespace,response=await request('/files/'+p.head.id,{},namespace,false),blob=await response.blob();if(blob.size!==p.head.size||await sha256Blob(blob)!==p.head.sha256.toLowerCase())throw error('The download failed verification.','invalid_file');if(namespace===s.namespace&&!s.locked&&!s.changed)download(blob,p.head.filename);}
    function renderPending(){if(!s.ui)return;clear(s.ui.pending);if(s.locked||s.changed){place();return;}for(const r of s.pending.values()){if(r.namespace!==s.namespace)continue;const row=element('div',null,s.ui.pending);row.className='pending';element('strong',r.filename,row);const text=element('p',r.error||(r.state==='waiting_native'?'Waiting for the editor to finish loading.':'Saving a server copy…'),row);text.className=r.error?'error':'muted';const actions=element('div',null,row);actions.className='actions';if(!r.busy)button('Retry server save',()=>save(r),actions);button('Download this copy',()=>download(r.blob,r.filename),actions);if(!r.detached&&!r.busy)button('Return to editor',()=>{try{reply(r,'cancel');r.detached=true;r.error='The copy is retained here. Use Save as in the editor to create another project.';renderPending();}catch(e){showError(e);}},actions);}place();}
    function closeDialog(cancel){if(cancel&&s.api){for(const q of s.dialogs){try{s.api.resolveSaveDialog(q.requestId,'cancel','');}catch(_){}}s.dialogs=[];}if(s.ui&&s.ui.dialog){s.ui.dialog.remove();s.ui.dialog=null;s.ui.dialogCancel=null;}}
    function showDialog(){
      if(!s.ui)initUI();if(!s.ui||s.ui.dialog||!s.dialogs.length)return;const q=s.dialogs[0],u=s.ui;u.panel.hidden=true;s.panelOpen=false;u.bubble.setAttribute('aria-expanded','false');
      const dialog=element('section',null,u.root);dialog.className='dialog';dialog.setAttribute('role','dialog');dialog.setAttribute('aria-modal','false');dialog.setAttribute('aria-label','Save project');u.dialog=dialog;element('h2',q.intent==='saveAs'?'Save as':'Save project',dialog);const label=element('label','File name',dialog),input=element('input',null,label);input.type='text';input.value=q.filename;input.className='filename';const warning=element('p','',dialog);warning.className='error';const actions=element('div',null,dialog);actions.className='actions';
      async function choose(destination){try{if(destination==='cloud'){await ensureIdentity();if(!s.enabled||s.locked)throw error('Server storage is unavailable. Choose device or cancel.');}let filename=input.value;if(destination!=='cancel'&&!VALID_NAME(filename))throw error('Choose a file name without folders or control characters.');if(!/\.[^.]+$/.test(filename)){const extension=/\.([^.]+)$/.exec(q.filename);if(extension)filename+='.'+extension[1];}if(destination!=='cancel'&&!VALID_NAME(filename))throw error('Choose a file name without folders or control characters.');s.api.resolveSaveDialog(q.requestId,destination,destination==='cancel'?'':filename);s.dialogs.shift();dialog.remove();u.dialog=null;u.dialogCancel=null;showDialog();renderStatus();const canvas=d.getElementById(app+'_canvas')||d.querySelector('canvas');if(canvas&&typeof canvas.focus==='function')canvas.focus({preventScroll:true});}catch(e){warning.textContent=e.message;place();}}
      u.dialogCancel=()=>choose('cancel');
      button('Save on server',()=>choose('cloud'),actions).className='primary';button('Download to device',()=>choose('device'),actions);button('Cancel',()=>choose('cancel'),actions);dialog.addEventListener('keydown',e=>{if(e.key==='Escape')choose('cancel');});place();input.focus();
    }
    async function resume(){try{if(await config()){await tryConnect();renderPending();renderStatus();if(s.panelOpen)await refreshLibrary();}}catch(e){showError(e);}}
    const controller=Object.freeze({ ready:null, refresh:resume, openLibrary:async()=>{await ensureIdentity();initUI();s.panelOpen=true;s.ui.panel.hidden=false;s.ui.bubble.setAttribute('aria-expanded','true');place();await refreshLibrary();}, status:()=>({enabled:s.enabled,connected:s.connected,locked:s.locked,identityChanged:s.changed,pending:Array.from(s.pending.values()).filter(r=>r.namespace===s.namespace).length}), disconnect:()=>suspend(false,'Server storage disconnected.') });
    // The ready promise is installed through a separate object so the public API stays immutable.
    const ready=(async()=>{try{if(!await config())return;await restore();if(app)await tryConnect();renderPending();renderStatus();}catch(e){if(!s.ui)initUI();showError(e);}})();
    const publicAPI=Object.freeze(Object.assign({},controller,{ready}));w.ArtCraftServerCloud=publicAPI;
    // egui/winit shortcuts listen outside the Shadow DOM. A cloud text field must
    // keep its native text/clipboard defaults without also editing the canvas.
    function ownTextInput(event) {
      if (!s.ui || typeof event.composedPath !== 'function') return false;
      const path = event.composedPath();
      return path.includes(s.ui.host) && path.some(node => node && (node.tagName === 'INPUT' || node.tagName === 'TEXTAREA' || node.isContentEditable));
    }
    for (const name of ['keydown', 'keyup', 'keypress', 'copy', 'cut', 'paste']) d.addEventListener(name, event => {
      if (!ownTextInput(event)) return;
      if (event.type === 'keydown' && event.key === 'Escape') {
        event.preventDefault();
        if (s.ui.dialogCancel) s.ui.dialogCancel();
        else { s.panelOpen = false; s.ui.panel.hidden = true; s.ui.bubble.setAttribute('aria-expanded', 'false'); s.ui.bubble.focus(); }
      }
      if (event.type === 'keydown' && (event.ctrlKey || event.metaKey) && String(event.key).toLowerCase() === 's') event.preventDefault();
      event.stopImmediatePropagation();
    }, true);
    if(app)w.addEventListener('artcraft-'+app,e=>receive(e.detail));
    w.addEventListener('focus',resume);w.addEventListener('resize',()=>{if(s.ui){s.ui.position=position();place();}});d.addEventListener('visibilitychange',()=>{if(d.visibilityState==='hidden')stopWatch();else resume();});
    w.addEventListener('beforeunload',e=>{if(Array.from(s.pending.values()).some(r=>r.namespace===s.namespace)){e.preventDefault();e.returnValue='';}});
    return publicAPI;
  }
  return { install, panelGeometry, sha256Blob };
});
