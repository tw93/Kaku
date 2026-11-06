// Run with node --test; Playwright is a test-only dependency supplied by CI.
const { test, before, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const root = path.resolve(__dirname, '../../..');
let browser;
const launchOptions = (args = []) => ({ args,
  ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
// Headless Chromium on a Mac gives WebGL2 only on Metal and with its GPU
// blocklist off; without these the WebGL tests skip.
const GPU_ARGS = ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'];
// Most tests read the DOM renderer's rows, so this browser has no WebGL and
// also covers the page's fallback; the WebGL test launches its own.
before(async () => {
  browser = await chromium.launch(launchOptions(['--disable-webgl', '--disable-3d-apis']));
});
after(async () => { await browser?.close(); });

async function page(t, hash = '#t=fixture', options = {}, context = browser) {
  const { platform, noConfig, ...pageOptions } = options;
  const p = await context.newPage({ viewport: { width: 1000, height: 600 }, ...pageOptions });
  if (platform) await p.addInitScript(value => Object.defineProperty(navigator, 'platform', { value }), platform);
  t.after(() => p.close());
  const errors = [];
  p.on('pageerror', e => errors.push(e.message));
  // A test page that navigates on its own loses its probe ("Execution
  // context was destroyed", twice in the WebGL merge test, cause unknown):
  // name every load after the first so the next one says when. Loads, not
  // navigations: the page drops its own fragment, which navigates in place.
  const loads = [];
  p.on('load', () => loads.push(`${p.url()} at ${Date.now()}`));
  t.after(() => { if (loads.length > 1) console.log(`${t.name}: the page loaded ${loads.length} times: ${loads.join(' , ')}`); });
  await p.addInitScript(() => {
    window.messages = [];
    window.connections = [];
    class Socket extends EventTarget {
      static OPEN = 1; static CLOSED = 3; static CLOSING = 2;
      constructor() { super(); this.readyState = 1; connections.push(this); setTimeout(() => this.onopen?.(), 0); }
      send(data) { if (typeof data === 'string') messages.push(JSON.parse(data)); else (this.binary ||= []).push(new Uint8Array(data)); }
      close(code = 1006) {
        this.readyState = 3;
        const event = new CloseEvent('close', { code });
        this.onclose?.(event); this.dispatchEvent(event);
      }
    }
    window.WebSocket = Socket;
  });
  const html = fs.readFileSync(process.env.KAKU_WEB_SOURCE || path.join(root, 'crates/kaku-remote/assets/web/index.html'), 'utf8')
    .replace('term.registerLinkProvider({', 'term.registerLinkProvider(window.testLinkProvider = {')
    .replace(/\}\)\(\);\s*<\/script>/, 'window.probe = {term, handle, attach, openPast, onHistory, pastScroll, sendFile, sendFiles, sendOnce, snapshots, download, get desktop() { return desktop; }, get webgl() { return webgl; }};\n})();\n</script>');
  await p.route('http://localhost:9988/**', route => {
    const name = new URL(route.request().url()).pathname.split('/').pop();
    if (!name) return route.fulfill({ contentType: 'text/html', body: html });
    const file = path.join(root, 'crates/kaku-remote/assets/web', name);
    return fs.existsSync(file) ? route.fulfill({ path: file }) : route.fulfill({ status: 404, body: '' });
  });
  // Kaku sends its config first on every session; the page claims no size
  // before it and the bundled font are in. It comes through the socket, as
  // from Kaku, so the page counts itself connected: handled directly, the
  // "Connecting to Kaku" notice still showed 400 ms in on CI's slower runner
  // and kept the notice a test checks from showing. A relay page only takes
  // sealed messages, so it is handed the config directly.
  const open = async () => {
    await p.goto('http://localhost:9988/' + hash);
    await p.waitForTimeout(100);
    if (!noConfig) await p.evaluate(direct => {
      if (!window.probe) return;
      const socket = connections.at(-1);
      if (direct && socket) socket.onmessage({ data: '{"type":"config"}' });
      else probe.handle({ type: 'config' });
      return document.fonts.load('16px "Kaku Mono"');
    }, hash.startsWith('#t='));
  };
  // CI's Chromium now and then navigates a just-opened page by itself (the
  // page has no code that leaves the document; Helium reloads a fresh
  // profile's first page the same way), which destroyed the context under
  // this evaluate in two pairing tests. Open it once more from the link,
  // which still carries its secret, and print the main-frame navigations so
  // the next occurrence says what moved.
  const navigations = [];
  p.on('framenavigated', frame => {
    if (frame === p.mainFrame()) navigations.push(`${frame.url()} at ${Date.now()}`);
  });
  try {
    await open();
  } catch (e) {
    if (!/Execution context was destroyed/.test(e.message)) throw e;
    console.log(`${t.name}: the page navigated by itself while opening (${navigations.join(' , ')}); opening it again`);
    await open();
  }
  return { p, errors };
}
// The terminal fades in (#term's opacity transition) once its own grid
// arrives, or after a moment; a screenshot taken during the fade catches it
// half drawn, which on CI's slower runner failed the icon redraw check.
async function fadedIn(p) {
  await p.waitForFunction(() => !document.body.classList.contains('sizing')
    && getComputedStyle(document.getElementById('term')).opacity === '1');
}
async function screen(p, text = 'Password: ', pane = 1) {
  await p.evaluate(({ text, pane }) => {
    probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }, { tab_id: 2, pane_id: 2, panes: [], title: 'two' }] });
    probe.handle({ type: 'frame', seq: 1, pane_id: pane, cols: 80, rows: 24, local_scroll: true,
      data: '\x1b[2J\x1b[1;1H' + text + '\x1b[1;' + (text.length + 1) + 'H\x1b[1 q\x1b[?25h' });
  }, { text, pane });
  await p.waitForFunction(expected =>
    [...document.querySelectorAll('.xterm-rows')].some(rows => rows.textContent.includes(expected)), text);
  await p.evaluate(() => probe.term.focus());
}
const text = p => p.locator('.xterm-rows').innerText();

test('only authoritative output is visible, including secret and offline input', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  assert.match(await text(p), /Password:/);
  await p.keyboard.type('demo-secret');
  await p.waitForTimeout(80);
  assert.equal((await p.evaluate(() => messages.filter(m => m.type === 'text'))).length, 11);
  assert.doesNotMatch(await text(p), /demo-secret/);
  await screen(p, 'confirmed-visible');
  assert.match(await text(p), /confirmed-visible/);
  await p.evaluate(() => { connections.at(-1).readyState = 3; messages.length = 0; });
  await p.keyboard.type('LOST'); await p.waitForTimeout(80);
  assert.doesNotMatch(await text(p), /LOST/);
  assert.equal(await p.evaluate(() => messages.length), 0);
  assert.deepEqual(errors, []);
});

// A Mac for relay tests: its signing key, made here as Kaku makes its own.
const { webcrypto } = require('node:crypto');
async function newMac() {
  const keys = await webcrypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify']);
  return {
    pkcs8: Buffer.from(await webcrypto.subtle.exportKey('pkcs8', keys.privateKey)).toString('base64'),
    pub: Buffer.from(await webcrypto.subtle.exportKey('raw', keys.publicKey)),
  };
}
// The form Kaku prints: only the one-time secret. Kaku sends its key in the
// handshake.
const pairingLink = (mac, secret) => `#${secret.toString('base64url')}`;
// Earlier Kaku put its key in the link too.
const legacyLink = (mac, secret) => `#p=${secret.toString('base64url')}&m=${mac.pub.toString('base64url')}`;

// Act as Kaku on the page's newest relay connection: exchange keys, sign the
// exchange as `signer`, send its key with a tag under `tag` (Kaku holds the
// link's secret while the link is fresh; defaults to `secret`), read the
// page's proof, and (if `admit`) answer it as Kaku does once a device is in.
// Returns the proof and whether its signature, and its pairing proof for
// `secret`, check out.
async function handshake(p, signer, { admit = true, secret = null, tag = secret } = {}) {
  return p.evaluate(async ({ pkcs8, pub, secret, tag, admit }) => {
    const sub = crypto.subtle, enc = new TextEncoder(), dec = new TextDecoder();
    const sleep = ms => new Promise(r => setTimeout(r, ms));
    const b64 = b => btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    const unb64 = s => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
    // The page connects once it has read its device store and keys.
    while (!connections.length) await sleep(5);
    const socket = connections.at(-1);
    const sent = () => socket.binary?.length || 0;
    const eph = await sub.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits']);
    const macEph = new Uint8Array(await sub.exportKey('raw', eph.publicKey));
    const hello = new Uint8Array(69); hello.set(enc.encode('KCH3')); hello.set(macEph, 4);
    socket.onmessage({ data: hello.buffer });
    while (sent() < 1) await sleep(5);
    const answer = socket.binary[0];
    if (answer.length !== 69 || dec.decode(answer.slice(0, 4)) !== 'KCB3') throw new Error('no page hello');
    const host = location.pathname.split('/').find(x => /^[0-9a-f]{32}$/.test(x)) || '';
    const label = enc.encode('kaku-remote v3'), id = enc.encode(host);
    const all = new Uint8Array(label.length + id.length + 130);
    all.set(label); all.set(id, label.length); all.set(macEph, label.length + id.length); all.set(answer.slice(4), label.length + id.length + 65);
    const transcript = new Uint8Array(await sub.digest('SHA-256', all));
    const peer = await sub.importKey('raw', answer.slice(4), { name: 'ECDH', namedCurve: 'P-256' }, false, []);
    const shared = await sub.deriveBits({ name: 'ECDH', public: peer }, eph.privateKey, 256);
    const hkdf = await sub.importKey('raw', shared, 'HKDF', false, ['deriveKey']);
    const derive = info => sub.deriveKey({ name: 'HKDF', hash: 'SHA-256', salt: transcript, info: enc.encode(info) },
      hkdf, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
    const toHost = await derive('kaku c2h'), fromHost = await derive('kaku h2c');
    const aad = (dir, seq) => { const l = enc.encode(dir); const out = new Uint8Array(l.length + 8); out.set(l); new DataView(out.buffer).setBigUint64(l.length, BigInt(seq)); return out; };
    const deliver = async (msg, seq) => {
      const iv = crypto.getRandomValues(new Uint8Array(12));
      const data = new Uint8Array(await sub.encrypt({ name: 'AES-GCM', iv, additionalData: aad('kaku-web h2c', seq) }, fromHost, enc.encode(JSON.stringify(msg))));
      const frame = new Uint8Array(12 + data.length); frame.set(iv); frame.set(data, 12);
      socket.onmessage({ data: frame.buffer });
    };
    const signing = await sub.importKey('pkcs8', unb64(pkcs8.replace(/\+/g, '-').replace(/\//g, '_')), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign']);
    const macAuth = { type: 'mac_auth', sig: b64(await sub.sign({ name: 'ECDSA', hash: 'SHA-256' }, signing, transcript)), key: pub };
    if (tag) {
      const label = enc.encode('kaku-remote mac key'), key = unb64(pub);
      const tagged = new Uint8Array(label.length + transcript.length + key.length);
      tagged.set(label); tagged.set(transcript, label.length); tagged.set(key, label.length + transcript.length);
      const hmac = await sub.importKey('raw', unb64(tag), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
      macAuth.tag = b64(await sub.sign('HMAC', hmac, tagged));
    }
    await deliver(macAuth, 0);
    for (let i = 0; i < 100 && sent() < 2; i++) await sleep(5);
    if (sent() < 2) return { proof: null };
    const frame = socket.binary[1];
    const plain = await sub.decrypt({ name: 'AES-GCM', iv: frame.slice(0, 12), additionalData: aad('kaku-web c2h', 0) }, toHost, frame.slice(12));
    const proof = JSON.parse(dec.decode(plain));
    const device = await sub.importKey('raw', unb64(proof.device), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
    const signed = await sub.verify({ name: 'ECDSA', hash: 'SHA-256' }, device, unb64(proof.sig), transcript);
    let paired = null;
    if (secret) {
      const key = await sub.importKey('raw', unb64(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']);
      paired = await sub.verify('HMAC', key, unb64(proof.proof || ''), transcript);
    }
    if (admit) await deliver({ type: 'pong' }, 1);
    // Later messages from this Mac, raw bytes or JSON, in order.
    let next = admit ? 2 : 1;
    window.fromMac = async (msg) => {
      const plain = msg instanceof Uint8Array ? msg : enc.encode(JSON.stringify(msg));
      const iv = crypto.getRandomValues(new Uint8Array(12));
      const data = new Uint8Array(await sub.encrypt({ name: 'AES-GCM', iv, additionalData: aad('kaku-web h2c', next++) }, fromHost, plain));
      const frame = new Uint8Array(12 + data.length); frame.set(iv); frame.set(data, 12);
      socket.onmessage({ data: frame.buffer });
    };
    window.readPage = async (frame, seq) => JSON.parse(dec.decode(await sub.decrypt(
      { name: 'AES-GCM', iv: frame.slice(0, 12), additionalData: aad('kaku-web c2h', seq) }, toHost, frame.slice(12))));
    await sleep(50);
    return { proof, signed, paired };
  }, { pkcs8: signer.pkcs8, pub: signer.pub.toString('base64url'), secret: secret && secret.toString('base64url'), tag: tag && tag.toString('base64url'), admit });
}
const storedDevice = p => p.evaluate(() => new Promise(resolve => {
  const open = indexedDB.open('kaku', 1);
  open.onupgradeneeded = () => open.result.createObjectStore('devices');
  open.onsuccess = () => {
    const req = open.result.transaction('devices').objectStore('devices').get(location.pathname);
    req.onsuccess = () => { open.result.close(); resolve(req.result ? Array.from(req.result.publicKey) : null); };
  };
}));

test('incomplete, old or missing pairing fails visibly without throwing', async t => {
  for (const [hash, message] of [['#p=bad&m=YQ', /incomplete/], ['#k=' + Buffer.alloc(32, 1).toString('base64url'), /earlier Kaku/], ['', /Pair this browser first/]]) {
    const { p, errors } = await page(t, hash);
    // Shown once the device store has been read, which a slow runner took
    // past the fixture's first 100 ms.
    await p.locator('#stage', { hasText: message }).waitFor();
    assert.match(await p.locator('#stage').innerText(), message);
    assert.equal(await p.evaluate(() => location.hash), '');
    assert.equal(await p.evaluate(() => connections.length), 0);
    assert.deepEqual(errors, []);
  }
});

test('a pairing link proves itself once and the device signs in afterwards', async t => {
  const mac = await newMac();
  const secret = Buffer.alloc(16, 3);
  const context = await browser.newContext();
  t.after(() => context.close());
  const { p, errors } = await page(t, pairingLink(mac, secret), {}, context);
  assert.equal(await p.evaluate(() => location.hash), '');
  // Nothing is kept before Kaku admits the browser.
  assert.equal(await storedDevice(p), null);
  const first = await handshake(p, mac, { secret });
  assert.equal(first.proof.type, 'pair');
  assert.equal(first.signed, true);
  assert.equal(first.paired, true);
  assert.match(first.proof.name, /on /);
  // Said once the device is saved, after the Mac's admission.
  await p.locator('#toast', { hasText: /Paired/ }).waitFor({ state: 'attached' });
  assert.match(await p.locator('#toast').innerText(), /Paired/);
  const kept = await storedDevice(p);
  assert.ok(kept);
  // Back later with no link: the same device signs in, with no pairing proof.
  const next = await page(t, '', {}, context);
  await next.p.waitForFunction(() => connections.length > 0);
  assert.equal(await next.p.evaluate(() => connections.length), 1);
  const again = await handshake(next.p, mac);
  assert.equal(again.proof.type, 'auth');
  assert.equal(again.signed, true);
  assert.equal(again.proof.proof, undefined);
  assert.deepEqual(await storedDevice(next.p), kept);
  assert.deepEqual([...errors, ...next.errors], []);
});

// Kaku's `share::deflate`: one raw deflate stream, each message ended by a
// newline and flushed, behind a 1 byte.
function deflater() {
  const zlib = require('node:zlib');
  const stream = zlib.createDeflateRaw();
  const out = [];
  stream.on('data', chunk => out.push(chunk));
  return msg => new Promise(resolve => {
    stream.write(JSON.stringify(msg) + '\n');
    stream.flush(zlib.constants.Z_SYNC_FLUSH, () => {
      const bytes = Buffer.concat([Buffer.from([1]), ...out.splice(0)]);
      resolve([...bytes]);
    });
  });
}

test('through the relay, a page that inflates reads deflated messages among plain ones', async t => {
  const mac = await newMac();
  const secret = Buffer.alloc(16, 4);
  const { p, errors } = await page(t, pairingLink(mac, secret));
  const { proof } = await handshake(p, mac, { secret });
  assert.equal(proof.deflate, true, 'the page says it inflates');
  const deflate = deflater();
  const msgs = [
    await deflate({ type: 'config' }),
    await deflate({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] }),
    await deflate({ type: 'frame', seq: 1, pane_id: 1, cols: 80, rows: 24, title: 'claude', data: '\x1b[2J\x1b[1;1H✳ Working' }),
    { type: 'pong' },
    await deflate({ type: 'frame', seq: 2, data: '\x1b[1;1H✶' }),
  ];
  await p.evaluate(async (msgs) => {
    for (const msg of msgs) await fromMac(Array.isArray(msg) ? new Uint8Array(msg) : msg);
  }, msgs);
  await p.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('✶ Working'));
  assert.equal(await p.evaluate(() => document.title), 'claude - Kaku');
  assert.deepEqual(errors, []);
});

test('a link from earlier Kaku with the key in it still pairs', async t => {
  const mac = await newMac();
  const secret = Buffer.alloc(16, 5);
  const context = await browser.newContext();
  t.after(() => context.close());
  const { p, errors } = await page(t, legacyLink(mac, secret), {}, context);
  assert.equal(await p.evaluate(() => location.hash), '');
  const first = await handshake(p, mac, { secret, tag: null });
  assert.equal(first.signed, true);
  assert.equal(first.paired, true);
  const next = await page(t, '', {}, context);
  const again = await handshake(next.p, mac);
  assert.equal(again.proof.type, 'auth');
  assert.equal(again.signed, true);
  assert.deepEqual([...errors, ...next.errors], []);
});

test('a key the link secret did not vouch for is refused', async t => {
  const mac = await newMac();
  const impostor = await newMac();
  // The relay can stand in as a Mac but never sees the secret, so its tag is
  // under some other secret, or missing, which reads as a used link.
  for (const [tag, notice] of [[Buffer.alloc(16, 9), /Could not verify the Mac/], [null, /expired/]]) {
    const { p, errors } = await page(t, pairingLink(mac, Buffer.alloc(16, 4)));
    const result = await handshake(p, impostor, { tag });
    assert.equal(result.proof, null);
    assert.equal(await p.evaluate(() => connections.at(-1).readyState), 3);
    assert.match(await p.locator('#notice').innerText(), notice);
    await p.waitForTimeout(700);
    assert.equal(await p.evaluate(() => connections.length), 1);
    assert.equal(await storedDevice(p), null);
    assert.deepEqual(errors, []);
  }
});

test('a link from another Mac pairs this browser with a new key', async t => {
  const mac = await newMac();
  const other = await newMac();
  const context = await browser.newContext();
  t.after(() => context.close());
  const { p } = await page(t, pairingLink(mac, Buffer.alloc(16, 10)), {}, context);
  await handshake(p, mac, { secret: Buffer.alloc(16, 10) });
  const kept = await storedDevice(p);
  const same = await page(t, pairingLink(mac, Buffer.alloc(16, 11)), {}, context);
  assert.equal((await handshake(same.p, mac, { secret: Buffer.alloc(16, 11) })).paired, true);
  assert.deepEqual(await storedDevice(same.p), kept);
  const next = await page(t, pairingLink(other, Buffer.alloc(16, 12)), {}, context);
  const paired = await handshake(next.p, other, { secret: Buffer.alloc(16, 12) });
  assert.equal(paired.signed, true);
  assert.equal(paired.paired, true);
  assert.notDeepEqual(await storedDevice(next.p), kept);
});

test('a page proves nothing to a Mac that is not the one it paired with', async t => {
  const mac = await newMac();
  const impostor = await newMac();
  const { p, errors } = await page(t, legacyLink(mac, Buffer.alloc(16, 4)));
  const result = await handshake(p, impostor);
  // The page sent its hello and nothing else: no proof, no input.
  assert.equal(result.proof, null);
  assert.equal(await p.evaluate(() => connections.at(-1).readyState), 3);
  assert.match(await p.locator('#notice').innerText(), /Could not verify the Mac/);
  await p.waitForTimeout(700);
  assert.equal(await p.evaluate(() => connections.length), 1);
  assert.equal(await storedDevice(p), null);
  assert.deepEqual(errors, []);
});

test('a message sealed for another connection closes the connection', async t => {
  const mac = await newMac();
  const { p } = await page(t, pairingLink(mac, Buffer.alloc(16, 5)));
  const closed = await p.evaluate(async () => {
    const socket = connections.at(-1);
    let closed = false; socket.close = () => { closed = true; };
    const eph = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits']);
    const hello = new Uint8Array(69); hello.set(new TextEncoder().encode('KCH3'));
    hello.set(new Uint8Array(await crypto.subtle.exportKey('raw', eph.publicKey)), 4);
    socket.onmessage({ data: hello.buffer });
    await new Promise(r => setTimeout(r, 50));
    // A recorded frame from another connection: other keys, so it cannot open.
    const frame = crypto.getRandomValues(new Uint8Array(60));
    socket.onmessage({ data: frame.buffer });
    await new Promise(r => setTimeout(r, 100));
    return closed;
  });
  assert.equal(closed, true);
});

test('a removed device forgets its pairing and an expired link keeps nothing', async t => {
  const mac = await newMac();
  const secret = Buffer.alloc(16, 6);
  const context = await browser.newContext();
  t.after(() => context.close());
  const { p } = await page(t, pairingLink(mac, secret), {}, context);
  await handshake(p, mac, { secret });
  // A bare 4403 can come from anyone on the way: the key stays and the
  // page connects again.
  const blip = await page(t, '', {}, context);
  await handshake(blip.p, mac);
  await blip.p.evaluate(() => connections.at(-1).close(4403));
  await blip.p.waitForFunction(() => connections.length > 1);
  assert.notEqual(await storedDevice(blip.p), null);
  assert.doesNotMatch(await blip.p.locator('#notice').innerText(), /not paired/);
  // The Mac's sealed note is what removes it.
  const next = await page(t, '', {}, context);
  await handshake(next.p, mac, { admit: false });
  await next.p.evaluate(() => fromMac({ type: 'revoked' }));
  await next.p.waitForTimeout(50);
  await next.p.evaluate(() => connections.at(-1).close(4403));
  assert.match(await next.p.locator('#notice').innerText(), /not paired/);
  await next.p.waitForTimeout(100);
  assert.equal(await storedDevice(next.p), null);
  // An expired or used link: its own message, and nothing saved.
  const late = await page(t, pairingLink(mac, Buffer.alloc(16, 7)));
  await handshake(late.p, mac, { admit: false });
  await late.p.evaluate(() => connections.at(-1).close(4401));
  assert.match(await late.p.locator('#notice').innerText(), /expired/);
  assert.equal(await storedDevice(late.p), null);
  await late.p.waitForTimeout(700);
  assert.equal(await late.p.evaluate(() => connections.length), 1);
});

test('a new browser shows the code kaku remote shows and pairs once it is allowed', async t => {
  const mac = await newMac();
  const secret = Buffer.alloc(16, 13);
  const { p, errors } = await page(t, pairingLink(mac, secret));
  const { proof } = await handshake(p, mac, { secret, admit: false });
  await p.evaluate(() => fromMac({ type: 'pair_pending' }));
  const digits = require('node:crypto').createHmac('sha256', secret)
    .update(Buffer.concat([Buffer.from('kaku-remote pairing code'), Buffer.from(proof.device, 'base64url')]))
    .digest().readUInt32BE(0) % 1000000;
  const code = String(digits).padStart(6, '0');
  await p.waitForFunction(() => /Confirm/.test(document.querySelector('#notice').innerText));
  assert.ok((await p.locator('#notice').innerText()).includes(code.slice(0, 3) + ' ' + code.slice(3)));
  // Nothing is saved while it waits, and the heartbeat stays quiet.
  assert.equal(await storedDevice(p), null);
  const before = await p.evaluate(() => connections.at(-1).binary.length);
  await p.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await p.waitForTimeout(100);
  assert.equal(await p.evaluate(() => connections.at(-1).binary.length), before);
  // Enter on the Mac: the session starts and the browser is kept.
  await p.evaluate(() => fromMac({ type: 'pong' }));
  await p.waitForFunction(() => !/Confirm/.test(document.querySelector('#notice').innerText));
  await p.waitForTimeout(100);
  assert.notEqual(await storedDevice(p), null);
  assert.deepEqual(errors, []);
});

test('a pairing refused on the Mac or taken by another browser says so and stops', async t => {
  const mac = await newMac();
  for (const [code, said] of [[4409, /Not confirmed/], [4429, /Another browser/]]) {
    const secret = Buffer.alloc(16, code % 256);
    const { p } = await page(t, pairingLink(mac, secret));
    await handshake(p, mac, { secret, admit: false });
    await p.evaluate(code => connections.at(-1).close(code), code);
    assert.match(await p.locator('#notice').innerText(), said);
    await p.waitForTimeout(700);
    assert.equal(await p.evaluate(() => connections.length), 1);
    assert.equal(await storedDevice(p), null);
  }
});

test('a paired device survives closing and relaunching the browser process', async t => {
  const profile = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'kaku-browser-'));
  const options = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE
    ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {};
  let context;
  t.after(async () => { await context?.close(); fs.rmSync(profile, { recursive: true, force: true }); });
  context = await chromium.launchPersistentContext(profile, options);
  const mac = await newMac();
  const secret = Buffer.alloc(16, 8);
  const first = await page(t, pairingLink(mac, secret), {}, context);
  await handshake(first.p, mac, { secret });
  // Closed before the device is saved, the browser would come back unpaired.
  await first.p.locator('#toast', { hasText: /Paired/ }).waitFor({ state: 'attached' });
  await context.close();
  context = await chromium.launchPersistentContext(profile, options);
  const next = await page(t, '', {}, context);
  await next.p.waitForFunction(() => connections.length > 0);
  assert.equal(await next.p.evaluate(() => connections.length), 1);
  assert.equal((await handshake(next.p, mac)).proof.type, 'auth');
  assert.deepEqual(next.errors, []);
});

test('blocked site storage still pairs this tab and says it will not last', async t => {
  const context = await browser.newContext();
  t.after(() => context.close());
  await context.addInitScript(() => {
    const failing = { open() { const req = {}; setTimeout(() => { req.error = new DOMException('blocked', 'SecurityError'); req.onerror?.(); }); return req; } };
    Object.defineProperty(window, 'indexedDB', { get() { return failing; } });
  });
  const mac = await newMac();
  const secret = Buffer.alloc(16, 9);
  const { p, errors } = await page(t, pairingLink(mac, secret), {}, context);
  const result = await handshake(p, mac, { secret });
  assert.equal(result.paired, true);
  assert.match(await p.locator('#toast').innerText(), /this tab only/);
  assert.deepEqual(errors, []);
});

test('connection help and first-use notice fit narrow and short viewports', async t => {
  const mac = await newMac();
  const secret = Buffer.alloc(16, 10);
  const { p } = await page(t, pairingLink(mac, secret));
  await handshake(p, mac, { secret });
  for (const width of [375, 1280]) for (const height of [240, 900]) {
    await p.setViewportSize({ width, height });
    await p.locator('.xterm-helper-textarea').focus();
    await p.keyboard.press('Meta+Slash');
    const box = await p.locator('#help .card').boundingBox();
    assert.ok(box.x >= 15 && box.x + box.width <= width - 15);
    assert.ok(box.y >= 15 && box.y + box.height <= height - 15);
    assert.ok(await p.locator('.connection-note').count());
    await p.keyboard.press('Escape');
    assert.equal(await p.locator('#toast').evaluate(el => el.scrollWidth <= el.clientWidth), true);
  }
});

test('Cmd+clicking a Mac path downloads the file in acknowledged chunks', async t => {
  const { p, errors } = await page(t);
  await screen(p, 'Saved to ~/Desktop/shot.png.');
  // Hovering the path with Cmd shows it as a link; the click asks the Mac.
  const box = await p.locator('.xterm-screen').boundingBox();
  const cell = await p.evaluate(() => probe.term._core._renderService.dimensions.css.cell);
  await p.keyboard.down('Meta');
  await p.mouse.move(box.x + cell.width * 14.5, box.y + cell.height / 2);
  await p.mouse.click(box.x + cell.width * 14.5, box.y + cell.height / 2);
  await p.keyboard.up('Meta');
  const request = await p.waitForFunction(() => messages.find(m => m.type === 'download'));
  const { id, path: asked } = await request.jsonValue();
  assert.equal(asked, '~/Desktop/shot.png');
  const bytes = Buffer.from(Array.from({ length: 300000 }, (_, i) => i % 251));
  const downloaded = p.waitForEvent('download');
  await p.evaluate(({ id, chunks }) => {
    probe.handle({ type: 'file_start', id, name: 'shot.png', size: chunks.reduce((n, c) => n + atob(c).length, 0) });
    for (const data of chunks) probe.handle({ type: 'file_chunk', id, data });
    probe.handle({ type: 'file_end', id });
  }, { id, chunks: [bytes.subarray(0, 200000).toString('base64'), bytes.subarray(200000).toString('base64')] });
  const file = await downloaded;
  assert.equal(file.suggestedFilename(), 'shot.png');
  assert.deepEqual(fs.readFileSync(await file.path()), bytes);
  // Each chunk was acknowledged with the bytes held so far.
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'download_ack').map(m => m.received)), [200000, 300000]);
  assert.match(await p.locator('#toast').innerText(), /Downloaded shot.png/);
  // A refused path says why, and a dropped link does not leave it hanging.
  await p.evaluate(() => probe.handle({ type: 'file_error', id: 99, detail: 'x' }));
  await p.evaluate(id => probe.handle({ type: 'file_error', id, detail: '~/Desktop is a folder' }), id);
  assert.deepEqual(errors, []);
});

test('local token stays tab-only', async t => {
  const { p } = await page(t);
  assert.equal(await p.evaluate(() => localStorage.getItem('kaku.link:/')), null);
  assert.ok(await p.evaluate(() => sessionStorage.getItem('kaku.link:/')));
});

test('image paths preview after 80ms, scaled by the Mac, again from memory, and cancel on leave', async t => {
  const { p, errors } = await page(t);
  await screen(p, '~/Desktop/shot.png');
  await p.evaluate(() => new Promise(resolve => testLinkProvider.provideLinks(1, links => {
    window.previewLink = links[0];
    previewLink.hover({ clientX: 980, clientY: 580, buttons: 0 });
    resolve();
  })));
  await p.waitForTimeout(40);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'download').length), 0);
  await p.waitForFunction(() => messages.some(m => m.type === 'download'));
  const request = await p.evaluate(() => messages.find(m => m.type === 'download'));
  assert.equal(request.path, '~/Desktop/shot.png');
  assert.equal(request.pane_id, 1);
  // The preview's longest side in device pixels, for the Mac to scale to.
  assert.equal(request.preview, await p.evaluate(() => Math.round(640 * devicePixelRatio)));
  let saves = 0;
  p.on('download', () => saves++);
  await p.evaluate(id => {
    probe.handle({ type: 'file_start', id, name: 'shot.png', size: 68 });
    probe.handle({ type: 'file_chunk', id, data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aCFsAAAAASUVORK5CYII=' });
    probe.handle({ type: 'file_end', id });
  }, request.id);
  await p.waitForFunction(() => !document.querySelector('#image-preview').hidden);
  const box = await p.locator('#image-preview').boundingBox();
  assert.ok(box.x >= 12 && box.y >= 12 && box.x + box.width <= 988 && box.y + box.height <= 588);
  assert.equal(saves, 0);
  await p.evaluate(() => previewLink.leave());
  assert.equal(await p.locator('#image-preview').isVisible(), false);
  assert.equal(await p.locator('#image-preview').getAttribute('src'), null);
  await p.evaluate(() => {
    messages.length = 0;
    previewLink.hover({ clientX: 100, clientY: 100, buttons: 0 });
    previewLink.leave();
  });
  await p.waitForTimeout(350);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'download').length), 0);
  await p.evaluate(() => previewLink.hover({ clientX: 100, clientY: 100, buttons: 1 }));
  await p.waitForTimeout(350);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'download').length), 0);
  // Pointing at it again shows the picture already fetched, with no transfer.
  await p.evaluate(() => previewLink.hover({ clientX: 100, clientY: 100, buttons: 0 }));
  await p.waitForFunction(() => !document.querySelector('#image-preview').hidden);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'download').length), 0);
  await p.evaluate(() => previewLink.leave());
  assert.deepEqual(errors, []);
});

test('a selection left on screen does not hold an image preview back', async t => {
  const { p, errors } = await page(t);
  await screen(p, 'see ~/Desktop/shot.png now');
  await p.evaluate(() => probe.term.selectAll());
  assert.equal(await p.evaluate(() => probe.term.hasSelection()), true);
  await p.evaluate(() => new Promise(resolve => testLinkProvider.provideLinks(1, links => {
    links[0].hover({ clientX: 100, clientY: 100, buttons: 0 });
    resolve();
  })));
  await p.waitForFunction(() => messages.some(m => m.type === 'download' && m.preview));
  assert.deepEqual(errors, []);
});

test('Option+clicking an image path shows it over the page to zoom, sending the Mac no click', async t => {
  const { p, errors } = await page(t);
  await p.setViewportSize({ width: 1000, height: 600 });
  await screen(p, 'see ~/Desktop/shot.png now');
  const box = await p.locator('.xterm-screen').boundingBox();
  const cell = await p.evaluate(() => probe.term._core._renderService.dimensions.css.cell);
  const at = [box.x + cell.width * 8.5, box.y + cell.height / 2];
  await p.mouse.move(at[0] - cell.width, at[1]);
  await p.mouse.move(at[0], at[1]);
  await p.waitForTimeout(30);
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.down('Alt');
  await p.mouse.click(at[0], at[1]);
  await p.keyboard.up('Alt');
  await p.waitForFunction(() => document.querySelector('#viewer.show'));
  const request = await p.evaluate(() => messages.find(m => m.type === 'download'));
  assert.equal(request.path, '~/Desktop/shot.png');
  assert.equal(request.preview, await p.evaluate(() => Math.min(2048, Math.round(Math.max(screen.width, screen.height) * devicePixelRatio))));
  // xterm's Alt click would have moved the shell's cursor with arrow keys.
  assert.deepEqual(await p.evaluate(() => messages.filter(m => ['click', 'text', 'key'].includes(m.type))), []);
  // A 400 by 200 picture fits inside the page, centred.
  const png = await p.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 400; c.height = 200;
    const g = c.getContext('2d'); g.fillStyle = '#4a8'; g.fillRect(0, 0, 400, 200);
    return c.toDataURL('image/png').split(',')[1];
  });
  // A slow answer says it is loading, and the note goes once it lands.
  await p.waitForFunction(() => getComputedStyle(document.querySelector('#viewer .wait')).display !== 'none', null, { timeout: 2000 });
  await p.evaluate(({ id, png }) => {
    probe.handle({ type: 'file_start', id, name: 'preview', size: atob(png).length });
    probe.handle({ type: 'file_chunk', id, data: png });
    probe.handle({ type: 'file_end', id });
  }, { id: request.id, png });
  await p.waitForFunction(() => document.querySelector('#viewer.ready img')?.getBoundingClientRect().width > 2);
  assert.equal(await p.evaluate(() => getComputedStyle(document.querySelector('#viewer .wait')).display), 'none');
  const fit = await p.locator('#viewer img').boundingBox();
  assert.ok(Math.abs(fit.width - 400) < 1 && Math.abs(fit.x + fit.width / 2 - 500) < 1 && Math.abs(fit.y + fit.height / 2 - 300) < 1, JSON.stringify(fit));
  // The wheel zooms; while the picture is smaller than the page it stays
  // centred, and once larger the pixel under the pointer stays put.
  await p.mouse.move(400, 280);
  await p.mouse.wheel(0, -300);
  const larger = await p.locator('#viewer img').boundingBox();
  assert.ok(larger.width > fit.width * 1.5 && Math.abs(larger.x + larger.width / 2 - 500) < 1, JSON.stringify(larger));
  await p.mouse.wheel(0, -300);
  const zoomed = await p.locator('#viewer img').boundingBox();
  assert.ok(zoomed.width > 1000 && zoomed.height > 600, JSON.stringify(zoomed));
  const u = (400 - larger.x) / larger.width, v = (280 - larger.y) / larger.height;
  assert.ok(Math.abs(zoomed.x + u * zoomed.width - 400) < 2 && Math.abs(zoomed.y + v * zoomed.height - 280) < 2, JSON.stringify(zoomed));
  // A drag pans it.
  await p.mouse.move(500, 300);
  await p.mouse.down();
  await p.mouse.move(540, 290, { steps: 4 });
  await p.mouse.up();
  const panned = await p.locator('#viewer img').boundingBox();
  assert.ok(Math.abs(panned.x - zoomed.x - 40) < 2 && Math.abs(panned.y - zoomed.y + 10) < 2, JSON.stringify(panned));
  assert.equal(await p.locator('#viewer').isVisible(), true, 'a drag is not a click beside it');
  // Escape closes it and reaches nothing else.
  await p.keyboard.press('Escape');
  assert.equal(await p.locator('#viewer').isVisible(), false);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => ['click', 'text', 'key'].includes(m.type))), []);
  // Opening it again needs no transfer, the copy is kept.
  await p.evaluate(() => { messages.length = 0; });
  await p.mouse.move(at[0] - cell.width, at[1]);
  await p.mouse.move(at[0], at[1]);
  await p.waitForTimeout(30);
  await p.keyboard.down('Alt');
  await p.mouse.click(at[0], at[1]);
  await p.keyboard.up('Alt');
  await p.waitForFunction(() => document.querySelector('#viewer.ready img')?.getBoundingClientRect().width > 2);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'download').length), 0);
  // A click beside the picture closes it.
  await p.mouse.click(20, 20);
  assert.equal(await p.locator('#viewer').isVisible(), false);
  assert.deepEqual(errors, []);
});

test('revoking a connection clears an already displayed image preview', async t => {
  const { p, errors } = await page(t);
  await screen(p, '/tmp/preview.png');
  await p.evaluate(() => new Promise(resolve => testLinkProvider.provideLinks(1, links => {
    links[0].hover({ clientX: 100, clientY: 100, buttons: 0 });
    resolve();
  })));
  await p.waitForFunction(() => messages.some(m => m.type === 'download'));
  await p.evaluate(() => {
    const id = messages.find(m => m.type === 'download').id;
    probe.handle({ type: 'file_chunk', id, data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aCFsAAAAASUVORK5CYII=' });
    probe.handle({ type: 'file_end', id });
  });
  await p.waitForFunction(() => !document.querySelector('#image-preview').hidden);
  await p.evaluate(() => connections.at(-1).close(1008));
  assert.equal(await p.locator('#image-preview').isVisible(), false);
  assert.equal(await p.locator('#image-preview').getAttribute('src'), null);
  assert.deepEqual(errors, []);
});

test('real mouse hover previews an image and leaving cancels the transfer', async t => {
  const { p, errors } = await page(t);
  await screen(p, '/tmp/preview.png');
  const box = await p.locator('.xterm-screen').boundingBox();
  const cell = await p.evaluate(() => probe.term._core._renderService.dimensions.css.cell);
  await p.mouse.move(box.x + cell.width * 5, box.y + cell.height / 2);
  await p.waitForFunction(() => messages.some(m => m.type === 'download'));
  const id = await p.evaluate(() => messages.find(m => m.type === 'download').id);
  await p.mouse.move(box.x + cell.width * 30, box.y + cell.height * 3);
  await p.waitForFunction(id => messages.some(m => m.type === 'download_cancel' && m.id === id), id);
  assert.equal(await p.locator('#image-preview').isVisible(), false);
  assert.deepEqual(errors, []);
});

test('image preview rejects large files and ignores late replies after cancellation', async t => {
  const { p, errors } = await page(t);
  await screen(p, '/tmp/photo.webp');
  await p.evaluate(() => new Promise(resolve => testLinkProvider.provideLinks(1, links => {
    window.previewLink = links[0];
    previewLink.hover({ clientX: 40, clientY: 40, buttons: 0 });
    resolve();
  })));
  await p.waitForFunction(() => messages.some(m => m.type === 'download'));
  const id = await p.evaluate(() => messages.find(m => m.type === 'download').id);
  await p.evaluate(id => {
    probe.handle({ type: 'file_start', id, name: 'photo.webp', size: 9 * 1024 * 1024 });
    probe.handle({ type: 'file_chunk', id, data: 'YWJj' });
    probe.handle({ type: 'file_end', id });
  }, id);
  assert.ok(await p.evaluate(id => messages.some(m => m.type === 'download_cancel' && m.id === id), id));
  assert.equal(await p.locator('#image-preview').isVisible(), false);
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'download_ack')), false);
  assert.deepEqual(errors, []);
});

test('authentication rejection stops reconnecting', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => connections.at(-1).close(4403));
  await p.waitForTimeout(900);
  assert.match(await p.locator('#notice').innerText(), /link/i);
  assert.equal(await p.evaluate(() => connections.length), 1);
});

test('returning to a cached tab attaches before claiming its new size', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.attach({ pane_id: 2, tab_id: 2 }));
  await screen(p, 'second', 2);
  await p.evaluate(() => { messages.length = 0; probe.attach({ pane_id: 1, tab_id: 1 }); });
  const commands = await p.evaluate(() => messages.filter(m => ['attach', 'resize'].includes(m.type)));
  assert.equal(commands[0].type, 'attach');
  assert.equal(commands[0].pane_id, 1);
  assert.ok(commands.some(m => m.type === 'resize'));
});

test('a page opened on a Mac-sized tab shows the terminal once its own grid arrives', async t => {
  const { p } = await page(t, '#t=fixture', { viewport: { width: 900, height: 560 } });
  const shown = () => p.evaluate(() => document.body.classList.contains('drawn') && !document.body.classList.contains('sizing'));
  const frame = (seq, cols, rows) => p.evaluate(([seq, cols, rows]) => probe.handle({ type: 'frame', seq, pane_id: 1, cols, rows, data: '\x1b[1;1Hterminal' }), [seq, cols, rows]);
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] }));
  // The Mac window's grid comes first: parsed, not shown.
  await frame(1, 200, 50);
  await p.waitForFunction(() => messages.some(m => m.type === 'resize'));
  assert.equal(await shown(), false);
  const claim = await p.evaluate(() => messages.filter(m => m.type === 'resize').at(-1));
  await frame(2, claim.cols, claim.rows);
  assert.equal(await shown(), true);
  assert.ok(await p.evaluate(() => document.querySelector('.xterm-rows').textContent.includes('terminal')));
});

test('a page whose grid never arrives shows the terminal after a moment', async t => {
  const { p } = await page(t, '#t=fixture', { viewport: { width: 900, height: 560 } });
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] }));
  await p.evaluate(() => probe.handle({ type: 'frame', seq: 1, pane_id: 1, cols: 200, rows: 50, data: '\x1b[1;1Hterminal' }));
  assert.equal(await p.evaluate(() => document.body.classList.contains('drawn') && !document.body.classList.contains('sizing')), false);
  await p.waitForFunction(() => document.body.classList.contains('drawn') && !document.body.classList.contains('sizing'), null, { timeout: 1500 });
});

test('typing or leaving the desktop view claims back a tab the Mac took for its own window', async t => {
  const { p } = await page(t, '#t=fixture', { viewport: { width: 900, height: 560 } });
  await screen(p, 'terminal');
  const claims = () => p.evaluate(() => messages.filter(m => m.type === 'resize').map(m => m.cols + 'x' + m.rows));
  let seq = 2;
  const frame = (cols, rows) => p.evaluate(([seq, cols, rows]) => probe.handle({ type: 'frame', seq, pane_id: 1, cols, rows, data: '\x1b[1;1Hterminal' }), [seq++, cols, rows]);
  // Kaku applies the page's grid.
  const [cols, rows] = (await claims()).at(-1).split('x').map(Number);
  await frame(cols, rows);
  await p.waitForTimeout(150);
  // Someone uses the Kaku window on the Mac: the tab goes back to its size,
  // and the page waits, as it does while the Mac is in use.
  await p.evaluate(() => { messages.length = 0; });
  await frame(60, 18);
  await p.waitForTimeout(150);
  assert.deepEqual(await claims(), []);
  // The first key typed here claims the page's grid again, once.
  await p.keyboard.type('ab');
  assert.deepEqual(await claims(), [cols + 'x' + rows]);
  await frame(cols, rows);
  await p.keyboard.type('c');
  assert.deepEqual(await claims(), [cols + 'x' + rows]);

  // Back from the Mac's screen, where clicking into Kaku took the tab.
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').waitFor();
  await p.locator('#bar .tool.desktop').click();
  await frame(60, 18);
  await p.evaluate(() => { messages.length = 0; });
  await p.locator('#bar .tool.desktop').click();
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  assert.deepEqual(await claims(), [cols + 'x' + rows]);
});

test('clicking back to a cached tab shows it in the next frame, never blank', async t => {
  const { p } = await page(t);
  await screen(p, 'first');
  await p.evaluate(() => probe.attach({ pane_id: 2, tab_id: 2 }));
  await screen(p, 'second', 2);
  await p.evaluate(() => {
    const rows = () => document.querySelector('.xterm-rows').textContent;
    window.__states = [];
    new MutationObserver(() => window.__states.push(rows().trim()))
      .observe(document.querySelector('.xterm-rows'), { subtree: true, childList: true, characterData: true });
    // After the tab's own click handler: what the first frame after it shows.
    window.addEventListener('click', () => requestAnimationFrame(() => { window.__firstFrame = rows(); }), { once: true });
  });
  await p.locator('#bar .tab').first().click();
  await p.waitForFunction(() => window.__firstFrame !== undefined);
  // xterm parses a write on a timer, which Chrome runs only after the frame
  // following a click; the snapshot then showed a frame late, and a
  // term.reset() before it blanked the screen in between.
  assert.ok(await p.evaluate(() => window.__firstFrame.includes('first')));
  assert.ok(!(await p.evaluate(() => window.__states.some(s => s === ''))));
});

test('a closed tab leaves the bar on the click and stays gone until Kaku agrees', async t => {
  const { p } = await page(t);
  await screen(p, 'first');
  await p.evaluate(() => probe.attach({ pane_id: 2, tab_id: 2 }));
  await screen(p, 'second', 2);
  const titles = () => p.evaluate(() => [...document.querySelectorAll('#bar .tab')].map(b => b.textContent.trim()));
  await p.evaluate(() => { messages.length = 0; });
  await p.locator('#bar .tab').nth(1).hover();
  await p.locator('#bar .tab').nth(1).locator('.close').click();
  // Gone at once, and the neighbour shown from its snapshot.
  assert.deepEqual(await titles(), ['one']);
  await p.waitForFunction(() => document.querySelector('.xterm-rows').textContent.includes('first'), null, { timeout: 1000 });
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'close_tab').map(m => m.tab_id)), [2]);
  // Drop the request in flight and reconnect before the grace expires.
  await p.evaluate(() => connections.at(-1).close(1006));
  await p.waitForFunction(() => connections.length === 2);
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }, { tab_id: 2, pane_id: 2, panes: [], title: 'two' }] }));
  assert.deepEqual(await titles(), ['one']);
  // A quick reconnect returns this same list before the grace expires.
  // The host deduplicates it afterwards, so recovery must use a real timer.
  await p.waitForTimeout(3300);
  assert.deepEqual(await titles(), ['one', 'two']);
  // Hiding a pending close must not discard its cached screen.
  assert.equal(await p.evaluate(() => probe.snapshots.has(2)), true);
});

test('closing while disconnected leaves the live tab visible', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => { connections.at(-1).readyState = 3; messages.length = 0; });
  await p.locator('#bar .tab').nth(1).hover();
  await p.locator('#bar .tab').nth(1).locator('.close').click();
  assert.deepEqual(await p.evaluate(() => [...document.querySelectorAll('#bar .tab')].map(b => b.textContent.trim())), ['one', 'two']);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'close_tab').length), 0);
});

test('an acknowledged close stays gone after the recovery timer', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.locator('#bar .tab').nth(1).hover();
  await p.locator('#bar .tab').nth(1).locator('.close').click();
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] }));
  await p.waitForTimeout(3300);
  assert.deepEqual(await p.evaluate(() => [...document.querySelectorAll('#bar .tab')].map(b => b.textContent.trim())), ['one']);
});

test('the terminal names no emoji font, so status symbols keep their color', async t => {
  const { p } = await page(t);
  await screen(p);
  // Named, an emoji font drew Claude Code's ⏺ and ✳ (green, red by status)
  // as fixed-color emoji whenever the fonts before it lacked them.
  const family = await p.evaluate(() => probe.term.options.fontFamily);
  assert.doesNotMatch(family, /Emoji/i);
  assert.match(family, /Kaku Mono/);
});

test('renaming from a menu opened before the tab bar was rebuilt still edits the tab', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.locator('#bar .tab').first().click({ button: 'right' });
  // A title update rebuilds every tab button while the menu is open.
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one*', active: true }, { tab_id: 2, pane_id: 2, panes: [], title: 'two' }] }));
  await p.locator('#menu button', { hasText: 'Rename Tab' }).click();
  await p.keyboard.press('Meta+a');
  await p.keyboard.type('renamed');
  await p.keyboard.press('Enter');
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'rename_tab').map(m => [m.tab_id, m.title])), [[1, 'renamed']]);
  // The bar is not stuck: the next update shows.
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'renamed', active: true }, { tab_id: 2, pane_id: 2, panes: [], title: 'three' }] }));
  assert.match(await p.locator('#bar').innerText(), /three/);
});

test('presses on controls over the terminal are not clicks for the program', async t => {
  // Narrow enough that the grid runs under the find bar.
  const { p } = await page(t, '#t=fixture', { viewport: { width: 700, height: 600 } });
  await screen(p);
  await p.keyboard.press(process.platform === 'darwin' ? 'Meta+f' : 'Control+Shift+F');
  await p.locator('#find button[data-dir="older"]').click();
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'click').length), 0);
});

test('a late history reply from before the view reopened is dropped', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    probe.openPast();
    const stale = messages.filter(m => m.type === 'history').at(-1).request;
    // The view closes and reopens before that reply arrives.
    probe.term.focus();
    probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 81, rows: 24, local_scroll: true, data: '' });
    probe.openPast();
    probe.onHistory({ request: stale, pane_id: 1, first: 0, top: 0, rows: ['\x1b[0mstale-row'] });
  });
  assert.equal(await p.evaluate(() => document.getElementById('past').classList.contains('show')), false);
});

async function focusHistory(p, select = false) {
  await p.evaluate(select => {
    probe.openPast();
    probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1).request,
      pane_id: 1, first: 0, top: 0, rows: ['\x1b[0mhistory text'] });
    document.body.tabIndex = -1;
    document.body.focus();
    if (select) {
      const range = document.createRange();
      range.selectNodeContents(document.querySelector('#past .row'));
      getSelection().removeAllRanges(); getSelection().addRange(range);
    }
    messages.length = 0;
  }, select);
}

for (const [name, key, expected] of [
  ['newline', 'Shift+Enter', { type: 'text', data: '\n' }],
  ['function key', 'F2', { type: 'key', key: 'F2', ctrl: false, alt: false, shift: false }],
  ['word motion', process.platform === 'darwin' ? 'Alt+ArrowLeft' : 'Control+ArrowLeft', { type: 'key', key: 'b', ctrl: false, alt: true, shift: false }],
  ['plain character', 'x', { type: 'text', data: 'x' }],
]) test('history first key preserves ' + name, async t => {
  const { p, errors } = await page(t); await screen(p); await captureClipboard(p);
  await focusHistory(p);
  await p.keyboard.press(key);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'text' || m.type === 'key')), [expected]);
  assert.deepEqual(errors, []);
});

test('history paste keeps the browser default and focuses the terminal', async t => {
  const { p } = await page(t, '#t=fixture', { platform: 'Linux x86_64' });
  await screen(p); await captureClipboard(p); await focusHistory(p);
  const result = await p.evaluate(() => {
    const ev = new KeyboardEvent('keydown', { key: 'v', code: 'KeyV', ctrlKey: true, bubbles: true, cancelable: true });
    document.body.dispatchEvent(ev);
    const focused = document.activeElement.classList.contains('xterm-helper-textarea');
    const data = new DataTransfer(); data.setData('text/plain', 'paste fixture');
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    return { prevented: ev.defaultPrevented, focused, inputs: messages.filter(m => ['text', 'key', 'paste'].includes(m.type)) };
  });
  assert.deepEqual(result, { prevented: false, focused: true, inputs: [{ type: 'paste', data: 'paste fixture' }] });
});

test('history first IME key focuses input without cancelling composition', async t => {
  const { p, errors } = await page(t); await screen(p); await captureClipboard(p); await focusHistory(p);
  const result = await p.evaluate(() => {
    const ev = new KeyboardEvent('keydown', { key: 'Unidentified', keyCode: 229, bubbles: true, cancelable: true });
    document.body.dispatchEvent(ev);
    return { prevented: ev.defaultPrevented, focused: document.activeElement.classList.contains('xterm-helper-textarea') };
  });
  assert.deepEqual(result, { prevented: false, focused: true });
  const cdp = await p.context().newCDPSession(p);
  await cdp.send('Input.imeSetComposition', { text: 'nihao', selectionStart: 5, selectionEnd: 5 });
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'text')), []);
  await cdp.send('Input.insertText', { text: '你好' });
  await p.waitForTimeout(30);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'text')), [{ type: 'text', data: '你好' }]);
  assert.deepEqual(errors, []);
});

test('queued wheel scrolling cannot follow attachment to another pane', async t => {
  const { p } = await page(t); await screen(p);
  await p.evaluate(() => {
    probe.handle({ type: 'frame', pane_id: 1, cols: 80, rows: 24, local_scroll: false, data: '' });
    messages.length = 0;
    document.querySelector('#stage').dispatchEvent(new WheelEvent('wheel', { deltaY: 100, bubbles: true, cancelable: true }));
    probe.attach({ pane_id: 2, tab_id: 2 }, true);
  });
  await p.waitForTimeout(50);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'scroll')), []);
});

test('touch fling stops when attachment changes', async t => {
  const { p } = await page(t, '#t=fixture', { hasTouch: true }); await screen(p);
  await p.evaluate(() => {
    probe.handle({ type: 'frame', pane_id: 1, cols: 80, rows: 24, local_scroll: false, data: '' });
    const stage = document.querySelector('#stage');
    const touch = y => new Touch({ identifier: 1, target: stage, clientX: 50, clientY: y });
    stage.dispatchEvent(new TouchEvent('touchstart', { touches: [touch(250)], bubbles: true }));
    stage.dispatchEvent(new TouchEvent('touchmove', { touches: [touch(50)], bubbles: true, cancelable: true }));
    stage.dispatchEvent(new TouchEvent('touchend', { touches: [], bubbles: true, cancelable: true }));
    probe.attach({ pane_id: 2, tab_id: 2 }, true);
    messages.length = 0;
  });
  await p.waitForTimeout(100);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'scroll' || m.type === 'history')), []);
});

test('changing a custom font back to the default restores the bundled stack', async t => {
  const { p } = await page(t); await screen(p);
  const fonts = await p.evaluate(() => {
    const initial = probe.term.options.fontFamily;
    probe.handle({ type: 'config', font_family: 'Menlo' });
    const custom = probe.term.options.fontFamily;
    probe.handle({ type: 'config', font_family: 'JetBrains Mono' });
    return { initial, custom, restored: probe.term.options.fontFamily };
  });
  assert.match(fonts.custom, /^"Menlo",/);
  assert.equal(fonts.restored, fonts.initial);
});

test('history shortcuts do not duplicate tab actions or leak find keys', async t => {
  const { p, errors } = await page(t, '#t=fixture', { platform: 'Linux' });
  await screen(p);
  await focusHistory(p);
  await p.keyboard.press('Control+Shift+f');
  assert.equal(await p.locator('#find').evaluate(el => el.classList.contains('show')), true);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => ['key', 'text'].includes(m.type))), []);
  assert.deepEqual(errors, []);
  const mac = (await page(t, '#t=fixture', { platform: 'MacIntel' })).p;
  await screen(mac);
  await focusHistory(mac);
  await mac.keyboard.press('Meta+2');
  assert.deepEqual(await mac.evaluate(() => messages.filter(m => m.type === 'attach').map(m => m.pane_id)), [2]);
});

test('history copy keeps native selection and does not send terminal input', async t => {
  for (const platform of ['MacIntel', 'Linux']) {
    const { p } = await page(t, '#t=fixture', { platform });
    await screen(p);
    await captureClipboard(p);
    await focusHistory(p, true);
    const result = await p.evaluate(mac => {
      const before = String(getSelection());
      const ev = new KeyboardEvent('keydown', { key: 'c', code: 'KeyC', metaKey: mac, ctrlKey: !mac, bubbles: true, cancelable: true });
      document.body.dispatchEvent(ev);
      // A late font load can re-measure and send a resize; only input counts.
      const input = messages.filter(m => ['key', 'text'].includes(m.type));
      return { before, after: String(getSelection()), prevented: ev.defaultPrevented, messages: input };
    }, platform === 'MacIntel');
    assert.ok(result.before);
    assert.equal(result.after, result.before);
    assert.equal(result.prevented, false);
    assert.deepEqual(result.messages, []);
  }
});

test('oversized UTF-8 paste is rejected visibly and a normal paste is preserved', async t => {
  const { p } = await page(t);
  await screen(p);
  const paste = s => p.evaluate(s => {
    const data = new DataTransfer(); data.setData('text/plain', s);
    document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true }));
  }, s);
  await paste('中文');
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'paste').at(-1).data), '中文');
  await paste('中'.repeat(23000));
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'paste').length), 1);
  assert.match(await p.locator('#toast').innerText(), /large|64 KB/i);
});

test('history retains rows while limiting rendered DOM', async t => {
  const { p } = await page(t);
  await screen(p);
  const result = await p.evaluate(() => {
    probe.openPast();
    const times = [];
    for (let n = 0; n < 25; n++) {
      const start = performance.now();
      probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1)?.request, pane_id: 1, first: 0, top: 10000 - (n + 1) * 400,
        rows: Array.from({ length: 400 }, (_, i) => '\x1b[0mhistory-' + (10000 - (n + 1) * 400 + i)) });
      probe.pastScroll(1e7);
      times.push(performance.now() - start);
    }
    return { nodes: document.querySelectorAll('#past .row').length, text: document.querySelector('#past').textContent,
      maxBatchMs: Math.max(...times) };
  });
  t.diagnostic(JSON.stringify({ nodes: result.nodes, maxBatchMs: result.maxBatchMs }));
  assert.ok(result.nodes > 0);
  assert.ok(result.nodes < 200, 'render visible rows and a small overscan, not every loaded row');
  assert.match(result.text, /history-0\D/);
});

test('history selection survives scrolling and older-page insertion', async t => {
  const { p } = await page(t);
  await screen(p);
  const result = await p.evaluate(() => {
    probe.openPast();
    probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1)?.request, pane_id: 1, first: 0, top: 400, rows: Array.from({ length: 400 }, (_, i) => '\x1b[0mrow-' + (400 + i)) });
    probe.pastScroll(1e7);
    const node = document.querySelector('#past .row span span').firstChild;
    const range = document.createRange(); range.selectNodeContents(node);
    const selection = getSelection(); selection.removeAllRanges(); selection.addRange(range);
    const before = selection.toString();
    probe.pastScroll(-20);
    const afterScroll = selection.toString();
    probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1)?.request, pane_id: 1, first: 0, top: 0, rows: Array.from({ length: 400 }, (_, i) => '\x1b[0mrow-' + i) });
    return { before, afterScroll, afterPrepend: selection.toString() };
  });
  assert.ok(result.before.length > 0);
  assert.equal(result.afterScroll, result.before);
  assert.equal(result.afterPrepend, result.before);
});

test('an upload waiting for relay authentication settles when its socket closes', async t => {
  const mac = await newMac(), secret = Buffer.alloc(16, 31);
  const { p } = await page(t, pairingLink(mac, secret));
  await p.evaluate(() => {
    const socket = connections.at(-1);
    window.uploadSettled = false;
    probe.sendOnce(new File(['waiting'], 'waiting.txt'), 'waiting.txt', socket, 'fixture', 1, false)
      .then(value => { uploadSettled = value === null; });
    setTimeout(() => socket.close(), 40);
  });
  await p.waitForFunction(() => uploadSettled, null, { timeout: 1500 });
});

for (const eager of [false, true]) test((eager ? 'an eager socket exposes downstream buffering' : 'a slow encrypted upload leaves room for keyboard input') + ' without breaking message order', async t => {
  const mac = await newMac(), secret = Buffer.alloc(16, 23);
  const { p, errors } = await page(t, pairingLink(mac, secret));
  await handshake(p, mac, { secret });
  await screen(p);
  await p.waitForTimeout(50);
  await p.evaluate(eager => {
    const socket = connections.at(-1), original = socket.send.bind(socket);
    const bytesPerMs = 250; // A deterministic 2 Mbps writable socket, not a network benchmark.
    let writtenUntil = performance.now(), chain = Promise.resolve(), received = 0, typedAt = null;
    window.uploadProbe = { messages: [], delays: [], failures: [] };
    Object.defineProperty(socket, 'bufferedAmount', { get: () => eager ? 0 : Math.max(0, writtenUntil - performance.now()) * bytesPerMs });
    socket.send = data => {
      const seq = socket.binary.length - 1;
      original(data);
      writtenUntil = Math.max(writtenUntil, performance.now()) + data.byteLength / bytesPerMs;
      const wireAt = writtenUntil;
      chain = chain.then(async () => {
        const m = await readPage(new Uint8Array(data), seq);
        uploadProbe.messages.push(m);
        if (m.type === 'upload_chunk') {
          received += atob(m.data).length;
          const total = received;
          setTimeout(() => probe.handle({ type: 'upload_ack', id: m.id, received: total }), Math.max(0, wireAt - performance.now()));
          if (typedAt === null && (!eager || received === 256 * 1024)) {
            typedAt = performance.now();
            probe.term.textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', code: 'KeyC', ctrlKey: true, bubbles: true, cancelable: true }));
          }
        }
        if (m.type === 'key' && m.key === 'c') uploadProbe.delays.push(wireAt - typedAt);
        if (m.type === 'upload_end') setTimeout(() => probe.handle({ type: 'uploaded', id: m.id, ok: true, detail: 'fixture' }), Math.max(0, wireAt - performance.now()));
      }).catch(e => uploadProbe.failures.push(String(e)));
    };
    window.uploadDone = probe.sendFiles([new File([new Uint8Array(256 * 1024)], 'slow.bin')], false);
  }, eager);
  await p.waitForFunction(() => uploadProbe.delays.length || uploadProbe.failures.length);
  await p.evaluate(() => uploadDone);
  const result = await p.evaluate(() => uploadProbe);
  assert.deepEqual(result.failures, [], 'every ciphertext verifies under its ordered sequence');
  t.diagnostic('modeled key wait: ' + result.delays[0].toFixed(1) + ' ms');
  if (eager) assert.ok(result.delays[0] > 1000, 'bufferedAmount cannot see data already accepted downstream');
  else assert.ok(result.delays[0] < 200, 'key waits behind ' + result.delays[0].toFixed(1) + ' ms of modeled wire bytes');
  const uploads = result.messages.filter(m => m.type.startsWith('upload_'));
  assert.equal(uploads[0].type, 'upload_start');
  assert.equal(uploads.at(-1).type, 'upload_end');
  assert.equal(uploads.filter(m => m.type === 'upload_chunk').reduce((n, m) => n + Buffer.from(m.data, 'base64').length, 0), 256 * 1024);
  assert.deepEqual(errors, []);
});

test('a multi-file drop stays on its original pane and retains the upload window', async t => {
  const { p } = await page(t);
  await screen(p);
  const result = await p.evaluate(async () => {
    const socket = connections.at(-1);
    const original = socket.send.bind(socket);
    const totals = new Map();
    let changed = false, outstanding = 0, peak = 0;
    socket.send = data => {
      original(data);
      const m = JSON.parse(data);
      if (m.type === 'upload_start') {
        totals.set(m.id, 0);
        if (!changed) { changed = true; probe.attach({ tab_id: 2, pane_id: 2 }); }
      }
      if (m.type === 'upload_chunk') {
        const bytes = atob(m.data).length;
        totals.set(m.id, totals.get(m.id) + bytes);
        outstanding += bytes; peak = Math.max(peak, outstanding);
        const received = totals.get(m.id);
        if (received % (512 * 1024) === 0 || bytes === 3) setTimeout(() => { outstanding = 0; probe.handle({ type: 'upload_ack', id: m.id, received }); }, 50);
      }
      if (m.type === 'upload_end') setTimeout(() => probe.handle({ type: 'uploaded', id: m.id, ok: true, detail: 'fixture' }), 10);
    };
    await probe.sendFiles([new File([new Uint8Array(1024 * 1024)], 'one.bin'), new File(['two'], 'two.txt')], false);
    return { starts: messages.filter(m => m.type === 'upload_start'), peak,
      chunks: messages.filter(m => m.type === 'upload_chunk').map(m => atob(m.data).length) };
  });
  assert.equal(result.starts.length, 2);
  assert.deepEqual(result.starts.map(m => m.pane_id), [1, 1]);
  assert.equal(new Set(result.starts.map(m => m.transfer)).size, 2);
  assert.equal(result.peak, 512 * 1024, 'the complete byte window is sent before its first acknowledgment');
  assert.ok(result.chunks.every(n => n <= 16 * 1024));
});

test('frame acknowledgments include discarded panes and follow xterm parsing', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.waitForFunction(() => messages.some(m => m.type === 'frame_ack' && m.seq === 1), null, { timeout: 1000 });
  await p.evaluate(() => {
    probe.handle({ type: 'frame', seq: 2, pane_id: 99, cols: 80, rows: 24, data: 'MUST NOT DRAW' });
  });
  await p.waitForFunction(() => messages.some(m => m.type === 'frame_ack' && m.seq === 2), null, { timeout: 1000 });
  assert.doesNotMatch(await text(p), /MUST NOT DRAW/);
});

test('acks go one for every four frames, and the last within 300 ms', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.waitForTimeout(400);
  const acks = await p.evaluate(async () => {
    messages.length = 0;
    // Delta frames from the same pane leave out its id and grid.
    probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 80, rows: 24, data: '\x1b[2;1Hstart' });
    for (let seq = 3; seq <= 10; seq++) probe.handle({ type: 'frame', seq, data: '\x1b[3;1Htick ' + seq });
    await new Promise(resolve => probe.term.write('', resolve));
    const soon = messages.filter(m => m.type === 'frame_ack').map(m => m.seq);
    await new Promise(resolve => setTimeout(resolve, 400));
    return { soon, later: messages.filter(m => m.type === 'frame_ack').map(m => m.seq) };
  });
  assert.deepEqual(acks.soon, [5, 9]);
  assert.deepEqual(acks.later, [5, 9, 10]);
  assert.match(await text(p), /tick 10/, 'frames without pane_id draw on the pane of the last one');
});

test('phone, tablet and desktop grids stay within their viewports, the desktop bar one row tall', async t => {
  for (const [width, height, touch] of [[375, 667, true], [768, 1024, true], [844, 499, true], [844, 501, true], [1280, 720, false]]) {
    const { p, errors } = await page(t, '#t=fixture', { viewport: { width, height }, hasTouch: touch, isMobile: touch });
    await screen(p, 'terminal');
    // Apply the grid requested from the Mac, as a real size acknowledgment.
    // The bar settling to one row grows the stage once, so acknowledge the
    // follow-up request too.
    for (const seq of [2, 3]) {
      await p.evaluate(seq => {
        const size = messages.filter(m => m.type === 'resize').at(-1);
        if (!size) throw new Error('no requested grid');
        probe.handle({ type: 'frame', seq, pane_id: 1, ...size, type: 'frame', data: '\x1b[1;1Hterminal' });
      }, seq);
      await p.waitForTimeout(150);
    }
    const geometry = await p.evaluate(() => {
      const screen = document.querySelector('.xterm-screen').getBoundingClientRect();
      const stage = document.querySelector('#stage').getBoundingClientRect();
      const rows = messages.filter(m => m.type === 'resize').at(-1).rows;
      return { overflow: document.documentElement.scrollWidth > innerWidth, right: screen.right - stage.right, bottom: screen.bottom - stage.bottom,
        bar: document.querySelector('#bar').getBoundingClientRect().height, row: screen.height / rows };
    });
    assert.equal(geometry.overflow, false, JSON.stringify({ width, height, geometry }));
    // Like Kaku's own tab bar the desktop bar is one terminal row; touch keeps its finger-sized bar.
    if (!touch) assert.ok(Math.abs(geometry.bar - geometry.row) <= 1, JSON.stringify({ width, height, geometry }));
    assert.ok(geometry.right <= 1 && geometry.bottom <= 1, JSON.stringify({ width, height, geometry }));
    assert.deepEqual(errors, []);
  }
});

async function captureClipboard(p, deny = false) {
  await p.evaluate(deny => {
    window.copied = [];
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText: async text => { copied.push(text); if (deny) throw new DOMException('Denied', 'NotAllowedError'); }
    } });
  }, deny);
}

test('mouse selection copies on release and respects the Mac setting', async t => {
  const { p } = await page(t);
  await screen(p, 'hello world');
  await captureClipboard(p);
  await p.evaluate(() => document.fonts.ready);
  const initial = await p.locator('.xterm-screen').boundingBox();
  await p.mouse.click(initial.x + initial.width - 4, initial.y + initial.height - 4);
  const rect = await p.locator('.xterm-screen').boundingBox();
  const cols = await p.evaluate(() => probe.term.cols);
  const select = async () => {
    await p.mouse.move(rect.x + rect.width / cols * 0.1, rect.y + 8);
    await p.mouse.down();
    // pointerdown re-fits the font. Measure that grid, then end just beyond
    // the fifth boundary rather than rounding into the following space.
    const current = await p.locator('.xterm-screen').boundingBox();
    const cell = current.width / await p.evaluate(() => probe.term.cols);
    await p.mouse.move(current.x + cell * 5.1, current.y + 8, { steps: 4 });
    assert.equal(await p.evaluate(() => copied.length), 0, 'do not copy intermediate drag selections');
    await p.mouse.up();
  };
  await select();
  assert.deepEqual(await p.evaluate(() => copied), ['hello']);
  await p.evaluate(() => { copied.length = 0; probe.handle({ type: 'config', copy_on_select: false }); });
  await select();
  assert.deepEqual(await p.evaluate(() => copied), []);
});

test('history auto-copy and manual copy preserve Unicode, newlines and virtual gaps', async t => {
  const { p } = await page(t);
  await screen(p);
  await captureClipboard(p);
  const result = await p.evaluate(() => {
    probe.openPast();
    probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1)?.request, pane_id: 1, first: 0, top: 0,
      rows: Array.from({ length: 400 }, (_, i) => '\x1b[0;38;2;10;20;30m中文-' + i) });
    const last = document.querySelector('#past .row:last-child span span').firstChild;
    // Keep an endpoint at the end while materializing the top; the middle
    // remains virtual and must still be included in the copied text.
    const selection = getSelection(), range = document.createRange();
    range.selectNodeContents(last); selection.removeAllRanges(); selection.addRange(range);
    probe.pastScroll(1e7);
    const first = document.querySelector('#past .row span span').firstChild;
    range.setStart(first, 0); range.setEnd(last, last.length);
    const expected = Array.from({ length: 400 }, (_, i) => '中文-' + i).join('\n');
    document.querySelector('#past').dispatchEvent(new PointerEvent('pointerdown', { button: 0, bubbles: true }));
    window.dispatchEvent(new MouseEvent('mouseup', { button: 0 }));
    const data = new DataTransfer();
    document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, cancelable: true }));
    return { automatic: copied.at(-1), manual: data.getData('text/plain'), expected };
  });
  assert.equal(result.automatic, result.expected);
  assert.equal(result.manual, result.expected);
});

test('live copy joins soft wraps and keeps hard breaks across all copy paths and snapshots', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await captureClipboard(p);
  const result = await p.evaluate(async () => {
    const drain = () => new Promise(resolve => probe.term.write('', resolve));
    probe.handle({ type: 'frame', pane_id: 1, cols: 10, rows: 3, local_scroll: true, wrapped: [true, false, false],
      data: '\x1b[2J\x1b[1;1HABCDEFGHIJ\x1b[2;1HK\x1b[3;1H你好' });
    await drain();
    document.querySelector('#stage').dispatchEvent(new PointerEvent('pointerdown', { button: 0, bubbles: true }));
    probe.term.select(0, 0, 24);
    window.dispatchEvent(new MouseEvent('mouseup', { button: 0 }));
    const copy = () => {
      const data = new DataTransfer();
      document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return data.getData('text/plain');
    };
    const automatic = copied.at(-1), manual = copy();
    document.querySelector('#term').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 200, clientY: 100 }));
    [...document.querySelectorAll('#menu button')].find(b => b.textContent === 'Copy').click();
    const menu = copied.at(-1);
    probe.attach({ tab_id: 2, pane_id: 2 });
    probe.handle({ type: 'frame', pane_id: 2, cols: 10, rows: 3, data: '\x1b[2J\x1b[1;1Hother' });
    await drain();
    probe.attach({ tab_id: 1, pane_id: 1 });
    await drain();
    probe.term.select(0, 0, 24);
    return { automatic, manual, menu, cached: copy() };
  });
  assert.deepEqual(result, Object.fromEntries(['automatic', 'manual', 'menu', 'cached'].map(k => [k, 'ABCDEFGHIJK\n你好'])));
  assert.deepEqual(errors, []);
});

test('copy does not guess soft wraps from a full row when metadata is absent or false', async t => {
  const { p } = await page(t);
  await screen(p);
  await captureClipboard(p);
  for (const wrapped of [undefined, [false, false, false]]) {
    const selected = await p.evaluate(async wrapped => {
      probe.handle({ type: 'frame', pane_id: 1, cols: 10, rows: 3, wrapped,
        data: '\x1b[2J\x1b[1;1HABCDEFGHIJ\x1b[2;1HK' });
      await new Promise(resolve => probe.term.write('', resolve));
      probe.term.select(0, 0, 11);
      const data = new DataTransfer();
      document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return data.getData('text/plain');
    }, wrapped);
    assert.equal(selected, 'ABCDEFGHIJ\nK');
  }
});

test('history copy preserves soft wraps when older pages are prepended', async t => {
  const { p } = await page(t);
  await screen(p);
  await captureClipboard(p);
  const selected = await p.evaluate(() => {
    const request = () => messages.filter(m => m.type === 'history').at(-1).request;
    probe.openPast();
    probe.onHistory({ request: request(), pane_id: 1, first: 0, top: 2, rows: ['tail', 'hard'], wrapped: [false, false] });
    probe.pastScroll(1e7);
    probe.onHistory({ request: request(), pane_id: 1, first: 0, top: 0, rows: ['ABCDEFGHIJ', 'K'], wrapped: [true, true] });
    const rows = document.querySelectorAll('#past .row');
    const range = document.createRange();
    range.setStart(rows[0].querySelector('span span').firstChild, 2);
    range.setEnd(rows[3].querySelector('span span').firstChild, 4);
    getSelection().removeAllRanges(); getSelection().addRange(range);
    const data = new DataTransfer();
    document.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, cancelable: true }));
    return data.getData('text/plain');
  });
  assert.equal(selected, 'CDEFGHIJKtail\nhard');
});

test('soft-wrap copy keeps spaces and partial endpoints and updates on a metadata-only frame', async t => {
  const { p } = await page(t);
  await screen(p);
  const result = await p.evaluate(async () => {
    const frame = wrapped => probe.handle({ type: 'frame', pane_id: 1, cols: 10, rows: 3, wrapped,
      data: '\x1b[2J\x1b[1;1HAB  CDEF  \x1b[2;1H  K' });
    const copy = async () => {
      await new Promise(resolve => probe.term.write('', resolve));
      probe.term.select(2, 0, 11);
      const data = new DataTransfer();
      document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
      return data.getData('text/plain');
    };
    frame([true, false, false]);
    const soft = await copy();
    probe.handle({ type: 'frame', pane_id: 1, cols: 10, rows: 3, wrapped: [false, false, false], data: '' });
    return { soft, hard: await copy() };
  });
  assert.deepEqual(result, { soft: '  CDEF    K', hard: '  CDEF  \n  K' });
});

test('soft-wrap metadata does not join a rectangular selection', async t => {
  const { p } = await page(t);
  await screen(p);
  await captureClipboard(p);
  await p.evaluate(async () => {
    probe.handle({ type: 'frame', pane_id: 1, cols: 10, rows: 3, wrapped: [true, false, false],
      data: '\x1b[2J\x1b[1;1HABCDEFGHIJ\x1b[2;1HTUVWXYZ' });
    await new Promise(resolve => probe.term.write('', resolve));
  });
  const rect = await p.locator('.xterm-screen').boundingBox();
  await p.keyboard.down('Alt');
  await p.mouse.move(rect.x + rect.width / 10 * 1.1, rect.y + rect.height / 6);
  await p.mouse.down();
  const current = await p.locator('.xterm-screen').boundingBox();
  await p.mouse.move(current.x + current.width / 10 * 4.1, current.y + current.height / 2, { steps: 3 });
  await p.mouse.up();
  await p.keyboard.up('Alt');
  const result = await p.evaluate(() => {
    const data = new DataTransfer();
    document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
    return { selected: probe.term.getSelection(), manual: data.getData('text/plain'), automatic: copied.at(-1) };
  });
  assert.equal(result.selected, 'BCD\nUVW');
  assert.equal(result.manual, result.selected);
  assert.equal(result.automatic, result.selected);
  // Shift extends the existing rectangular selection after Alt is released.
  await p.keyboard.down('Shift');
  await p.mouse.click(current.x + current.width / 10 * 5.1, current.y + current.height * 5 / 6);
  await p.keyboard.up('Shift');
  const extended = await p.evaluate(() => {
    const data = new DataTransfer();
    document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
    return { selected: probe.term.getSelection(), manual: data.getData('text/plain') };
  });
  assert.match(extended.selected, /\n/);
  assert.equal(extended.manual, extended.selected);
});

test('denied auto-copy never queues stale text for a later unrelated click', async t => {
  const { p, errors } = await page(t);
  await screen(p, 'hello world');
  await captureClipboard(p, true);
  await p.evaluate(() => {
    probe.term.select(0, 0, 5);
    document.querySelector('#term').dispatchEvent(new PointerEvent('pointerdown', { button: 0, bubbles: true }));
    window.dispatchEvent(new MouseEvent('mouseup', { button: 0 }));
  });
  await p.waitForTimeout(20);
  await p.evaluate(() => { window.dispatchEvent(new Event('focus')); document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); });
  assert.deepEqual(await p.evaluate(() => copied), ['hello']);
  assert.deepEqual(errors, []);
});

test('touch history selection changes copy locally without copying page labels', async t => {
  const { p } = await page(t, '#t=fixture', { hasTouch: true, viewport: { width: 375, height: 667 } });
  await screen(p);
  await captureClipboard(p);
  await p.evaluate(() => {
    probe.openPast();
    probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1)?.request, pane_id: 1, first: 0, top: 0, rows: ['\x1b[0m中文 history'] });
    const range = document.createRange();
    range.selectNodeContents(document.querySelector('#past .row span span'));
    getSelection().removeAllRanges(); getSelection().addRange(range);
  });
  await p.waitForTimeout(40);
  assert.deepEqual(await p.evaluate(() => copied), ['中文 history']);
  await p.evaluate(() => {
    const range = document.createRange(); range.selectNodeContents(document.querySelector('#bar'));
    getSelection().removeAllRanges(); getSelection().addRange(range);
  });
  await p.waitForTimeout(40);
  assert.deepEqual(await p.evaluate(() => copied), ['中文 history']);
});

test('links exclude sentence punctuation and retain balanced paths and Unicode cell ranges', async t => {
  const { p } = await page(t);
  const issue = 'https://github.com/tw93/Mole/issues/1734#issuecomment-6140809506';
  const cases = [
    [`#1734 (${issue})，并以`, issue],
    ['见 https://example.com/a，后续说明', 'https://example.com/a'],
    ['(https://example.com/Function_(math)).', 'https://example.com/Function_(math)'],
    ['[https://example.com/a?x=1&y=2#section]。', 'https://example.com/a?x=1&y=2#section'],
    ['😀 https://example.com/中文', 'https://example.com/中文'],
    ['https://example.com/a%29?q=%E4%B8%AD', 'https://example.com/a%29?q=%E4%B8%AD'],
  ];
  for (const [source, expected] of cases) {
    const found = await p.evaluate(async source => {
      probe.term.resize(180, 24);
      await new Promise(resolve => probe.term.write('\x1b[?7l\x1b[2J\x1b[H' + source, resolve));
      const links = await new Promise(resolve => testLinkProvider.provideLinks(1, resolve));
      const opened = []; window.open = url => opened.push(url);
      for (const link of links || []) link.activate({ ctrlKey: true }, link.text);
      return { links: (links || []).map(l => ({ text: l.text, range: l.range })), opened };
    }, source);
    assert.deepEqual(found.opened, [expected]);
    if (source.startsWith('😀')) {
      // Unicode 11 widths give this emoji two cells, as Kaku does.
      assert.equal(found.links[0].range.start.x, 4);
      assert.equal(found.links[0].range.end.x, 27);
    }
  }
});

test('emoji take the two cells Kaku gives them, so their rows match the Mac', async t => {
  const { p } = await page(t);
  const row = await p.evaluate(async () => {
    probe.term.resize(80, 24);
    // Kaku pins each run to its column, as the server's frames do.
    await new Promise(resolve => probe.term.write('\x1b[2J\x1b[H\u2705\x1b[3G done \u26a1\x1b[11G x', resolve));
    const line = probe.term.buffer.active.getLine(0);
    return { text: line.translateToString(true), widths: [0, 8].map(x => line.getCell(x).getWidth()) };
  });
  assert.equal(row.text, '\u2705 done \u26a1 x');
  assert.deepEqual(row.widths, [2, 2]);
});

test('Mac paths stop at Chinese sentence punctuation but keep Chinese file names', async t => {
  const { p } = await page(t);
  const cases = [
    ['对照图在 /private/tmp/icons/all-ship.png，按字母顺序排：chatgpt', '/private/tmp/icons/all-ship.png'],
    ['存到了 ~/Desktop/shot.png。', '~/Desktop/shot.png'],
    ['见（~/Desktop/a.png）和「./b.txt」', '~/Desktop/a.png ./b.txt'],
    ['截图在 ~/桌面/截图.png', '~/桌面/截图.png'],
  ];
  for (const [source, expected] of cases) {
    const found = await p.evaluate(async source => {
      probe.term.resize(180, 24);
      await new Promise(resolve => probe.term.write('\x1b[?7l\x1b[2J\x1b[H' + source, resolve));
      const links = await new Promise(resolve => testLinkProvider.provideLinks(1, resolve));
      return (links || []).map(l => l.text).join(' ');
    }, source);
    assert.equal(found, expected);
  }
});

// Claude Code breaks a long path with a newline and an indent; the joined
// path is a link on both rows once the Mac says it exists (#547), and the
// fragments stay as they were when it does not or Kaku never answers.
test('a path a TUI wrapped onto the next row links whole once the Mac finds it', async t => {
  const { p } = await page(t); await screen(p);
  const links = row => p.evaluate(async row => {
    const found = await new Promise(resolve => testLinkProvider.provideLinks(row, resolve));
    return (found || []).map(l => ({ text: l.text, from: [l.range.start.x, l.range.start.y], to: [l.range.end.x, l.range.end.y] }));
  }, row);
  const show = text => p.evaluate(text => new Promise(resolve => {
    probe.term.resize(60, 24);
    probe.term.write('\x1b[2J\x1b[H' + text, resolve);
  }), text);
  const answer = exists => p.waitForFunction(() => messages.some(m => m.type === 'paths_exist')).then(() => p.evaluate(exists => {
    const ask = messages.filter(m => m.type === 'paths_exist').at(-1);
    messages.length = 0;
    probe.handle({ type: 'paths_exist', id: ask.id, exists: ask.paths.map(() => exists) });
    return ask.paths;
  }, exists));
  await p.evaluate(() => { messages.length = 0; });
  await show('对比图在 /private/tmp/a/47ca47e\r\n  d-ea27/shot.png：左边改前\r\n');
  const first = links(1);
  assert.deepEqual(await answer(true), ['/private/tmp/a/47ca47ed-ea27/shot.png']);
  const whole = { text: '/private/tmp/a/47ca47ed-ea27/shot.png', from: [10, 1], to: [17, 2] };
  assert.deepEqual(await first, [whole]);
  assert.deepEqual(await links(2), [whole]);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'paths_exist').length), 0, 'asked once');

  await show('see /tmp/b/one\r\n  two/x.png here\r\n');
  const missing = links(1);
  await answer(false);
  assert.deepEqual((await missing).map(l => l.text), ['/tmp/b/one']);
  assert.deepEqual(await links(2), []);

  // A Kaku that does not know the question: the fragment, after a moment.
  await show('see /tmp/c/one\r\n  two/x.png here\r\n');
  assert.deepEqual((await links(1)).map(l => l.text), ['/tmp/c/one']);
});

test('middle click closes the clicked tab through the same confirmation path', async t => {
  const { p } = await page(t); await screen(p);
  await p.evaluate(() => { messages.length = 0; probe.handle({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, title: 'one', active: true },
    { tab_id: 2, pane_id: 2, title: 'two', confirm_close: true },
  ] }); });
  await p.locator('#bar .tab').nth(1).click({ button: 'middle' });
  assert.equal(await p.locator('#dialog.show').count(), 1);
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'close_tab')), false);
  await p.locator('#dialog [data-act="cancel"]').click();
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'close_tab')), false);
  await p.locator('#bar .tab').nth(1).click({ button: 'middle' });
  await p.locator('#dialog [data-act="ok"]').click();
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'close_tab')), [{ type: 'close_tab', tab_id: 2 }]);
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'attach' && m.pane_id === 2)), false);
});

test('notifications from the Mac reach a background page, and a click returns to their pane', async t => {
  const { p, errors } = await page(t); await screen(p);
  await p.evaluate(() => {
    window.shown = [];
    window.permissionAsked = 0;
    window.Notification = class {
      constructor(title, options) { this.title = title; this.body = options.body; shown.push(this); }
      close() {}
    };
    Notification.permission = 'default';
    Notification.requestPermission = async () => { permissionAsked++; Notification.permission = 'granted'; return 'granted'; };
    window.pageHidden = false;
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => pageHidden });
    document.hasFocus = () => !pageHidden;
  });
  // In front: another tab's notice is a toast, the attached pane's is already on screen.
  await p.evaluate(() => probe.handle({ type: 'notify', pane_id: 1, title: 'Claude Code', body: 'Done' }));
  assert.equal(await p.locator('#toast.show').count(), 0);
  await p.evaluate(() => probe.handle({ type: 'notify', pane_id: 2, title: 'Claude Code', body: 'Done' }));
  assert.match(await p.locator('#toast').innerText(), /Claude Code: Done/);
  // In the background, without permission yet: offer it once, ask only on a click.
  await p.evaluate(() => { pageHidden = true; probe.handle({ type: 'notify', pane_id: 2, title: 'Build', body: 'ok' }); });
  assert.match(await p.locator('#toast').innerText(), /notifications/);
  assert.equal(await p.evaluate(() => permissionAsked), 0);
  await p.evaluate(() => { pageHidden = false; });
  await p.locator('#toast').click();
  assert.equal(await p.evaluate(() => permissionAsked), 1);
  // Granted: a background notice is the browser's own, and its click goes to that pane.
  await p.evaluate(() => { pageHidden = true; messages.length = 0; probe.handle({ type: 'notify', pane_id: 2, title: 'Build', body: 'ok' }); });
  assert.deepEqual(await p.evaluate(() => shown.map(n => [n.title, n.body])), [['Build', 'ok']]);
  await p.evaluate(() => shown[0].onclick());
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'attach' && m.pane_id === 2)), true);
  // An unrelated toast later is not a permission prompt.
  await p.evaluate(() => { pageHidden = false; probe.handle({ type: 'error', detail: 'something else' }); });
  await p.locator('#toast').click();
  assert.equal(await p.evaluate(() => permissionAsked), 1);
  assert.deepEqual(errors, []);
});

test('inline images from the Mac sit over their cells and leave with the pane', async t => {
  const { p, errors } = await page(t); await screen(p);
  const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  await p.evaluate(png => {
    probe.handle({ type: 'image', id: 'ab12', mime: 'image/png', data: png });
    probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 80, rows: 24, local_scroll: true, data: '',
      images: [{ id: 'ab12', col: 3, row: 2, cols: 4, rows: 2 }] });
  }, png);
  const box = await p.evaluate(() => {
    const img = document.querySelector('#images img');
    const cell = document.querySelector('.xterm-screen').getBoundingClientRect();
    return img && { left: parseFloat(img.style.left), top: parseFloat(img.style.top), width: parseFloat(img.style.width),
      cellW: cell.width / 80, cellH: cell.height / 24, height: parseFloat(img.style.height) };
  });
  assert.ok(box, 'image placed');
  assert.ok(Math.abs(box.left - 3 * box.cellW) < 1 && Math.abs(box.top - 2 * box.cellH) < 1, JSON.stringify(box));
  assert.ok(Math.abs(box.width - 4 * box.cellW) < 1 && Math.abs(box.height - 2 * box.cellH) < 1, JSON.stringify(box));
  // A frame without an images field keeps them; switching panes clears them.
  await p.evaluate(() => probe.handle({ type: 'frame', seq: 3, pane_id: 1, cols: 80, rows: 24, local_scroll: true, data: '\x1b[1;1Hx' }));
  assert.equal(await p.locator('#images img').count(), 1);
  await p.evaluate(() => probe.attach({ tab_id: 2, pane_id: 2 }, true));
  assert.equal(await p.locator('#images img').count(), 0);
  assert.deepEqual(errors, []);
});

// Selection and find (which selects its match) use Kaku's selection colors,
// text included; xterm.js's default kept white text on a light highlight.
test('selection uses Kaku\'s selection colors so highlighted text stays readable', async t => {
  const { p } = await page(t); await screen(p);
  const theme = await p.evaluate(() => {
    probe.handle({ type: 'config', background: '#16161a', foreground: '#d4d4d8', selection_bg: 'rgba(142,106,217,0.55)', selection_fg: '#ffffff' });
    const t = probe.term.options.theme;
    return [t.selectionBackground, t.selectionInactiveBackground, t.selectionForeground];
  });
  assert.deepEqual(theme, ['rgba(142,106,217,0.55)', 'rgba(142,106,217,0.55)', '#ffffff']);
});

test('the close confirmation answers to N, Y and clicks outside it', async t => {
  const { p } = await page(t); await screen(p);
  await p.evaluate(() => { messages.length = 0; probe.handle({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, title: 'one', active: true },
    { tab_id: 2, pane_id: 2, title: 'two', confirm_close: true },
  ] }); });
  const closes = () => p.evaluate(() => messages.filter(m => m.type === 'close_tab').length);
  const ask = () => p.locator('#bar .tab').nth(1).click({ button: 'middle' });
  await ask();
  assert.equal(await p.locator('#dialog .title').innerText(), 'Close this tab?');
  assert.equal(await p.locator('#dialog [data-act="ok"]').innerText(), 'Close');
  await p.keyboard.press('n');
  assert.equal(await p.locator('#dialog.show').count(), 0);
  await ask();
  await p.mouse.click(5, 300);
  assert.equal(await p.locator('#dialog.show').count(), 0);
  assert.equal(await closes(), 0);
  await ask();
  await p.keyboard.press('y');
  assert.equal(await closes(), 1);
});

// The confirmation sits by what it asks about with its tip on it: 16px under
// the close mark that was clicked, above the tab when the bar is at the
// bottom, by the spot the pane menu opened at; scrolling outside cancels.
test('the close confirmation points at the tab or spot it asks about', async t => {
  const { p } = await page(t); await screen(p);
  const tabs = bottom => p.evaluate(bottom => {
    connections.at(-1).onmessage({ data: JSON.stringify({ type: 'config', tab_bar_at_bottom: bottom }) });
    probe.handle({ type: 'tabs', tabs: [
      { tab_id: 1, pane_id: 1, title: 'one', active: true },
      { tab_id: 2, pane_id: 2, title: 'two', confirm_close: true, panes: [
        { pane_id: 2, left: 0, top: 0, width: 40, height: 24, confirm_close: true },
        { pane_id: 4, left: 40, top: 0, width: 40, height: 24 },
      ] },
    ] });
  }, bottom);
  const geometry = () => p.evaluate(() => {
    const card = document.querySelector('#dialog .card').getBoundingClientRect();
    const tip = document.querySelector('#dialog .tip').getBoundingClientRect();
    const mark = document.querySelectorAll('#bar .tab')[1].querySelector('.close').getBoundingClientRect();
    const tab = document.querySelectorAll('#bar .tab')[1].getBoundingClientRect();
    return { card, tip: { x: tip.left + tip.width / 2, top: tip.top, bottom: tip.bottom }, mark, tab };
  });
  await tabs(false);
  await p.locator('#bar .tab').nth(1).hover();
  await p.locator('#bar .tab').nth(1).locator('.close').click();
  let g = await geometry();
  assert.ok(Math.abs(g.card.top - g.mark.bottom - 16) <= 1, `card ${g.card.top} under the mark at ${g.mark.bottom}`);
  assert.ok(Math.abs(g.tip.x - (g.mark.left + g.mark.right) / 2) <= 1, `tip ${g.tip.x} on the mark`);
  assert.ok(g.tip.top < g.card.top && g.tip.top > g.mark.bottom, 'the tip rises from the card toward the mark');
  await p.mouse.move(500, 400);
  await p.mouse.wheel(0, 100);
  await p.waitForFunction(() => !document.querySelector('#dialog.show'), null, { timeout: 2000 });
  await tabs(true);
  await p.locator('#bar .tab').nth(1).click({ button: 'middle' });
  g = await geometry();
  assert.ok(Math.abs(g.tab.top - g.card.bottom - 16) <= 1, `card ${g.card.bottom} above the bottom tab at ${g.tab.top}`);
  assert.ok(Math.abs(g.tip.x - (g.tab.left + g.tab.right) / 2) <= 1, 'tip on the tab');
  assert.ok(g.tip.bottom > g.card.bottom, 'the tip points down at the tab');
  await p.keyboard.press('Escape');
  await tabs(false);
  await p.evaluate(() => probe.attach({ tab_id: 2, pane_id: 2 }, true));
  await p.locator('.xterm-screen').click({ button: 'right', position: { x: 300, y: 200 } });
  const spot = await p.locator('.xterm-screen').boundingBox();
  await p.locator('#menu button').getByText('Close Pane', { exact: true }).click();
  assert.equal(await p.locator('#dialog .title').innerText(), 'Close this pane?');
  g = await geometry();
  assert.ok(Math.abs(g.card.top - (spot.y + 200) - 16) <= 1, `card ${g.card.top} under the menu spot at ${spot.y + 200}`);
  assert.ok(Math.abs(g.tip.x - (spot.x + 300)) <= 1, 'tip on the menu spot');
});

test('terminal menu copies its selection and shares paste limits and pane guards', async t => {
  const { p } = await page(t); await screen(p, 'hello world'); await captureClipboard(p);
  await p.evaluate(() => { probe.term.select(0, 0, 5); navigator.clipboard.readText = async () => 'pasted'; });
  const menu = async () => p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
  await menu(); await p.locator('#menu button').getByText('Copy', { exact: true }).click();
  assert.deepEqual(await p.evaluate(() => copied), ['hello']);
  await menu(); await p.locator('#menu button').getByText('Paste', { exact: true }).click();
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'paste').at(-1).data), 'pasted');
  await p.evaluate(() => { navigator.clipboard.readText = async () => '中'.repeat(23000); });
  await menu(); await p.locator('#menu button').getByText('Paste', { exact: true }).click();
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'paste').length), 1);
  await p.evaluate(() => { navigator.clipboard.readText = () => new Promise(resolve => { window.finishRead = resolve; }); });
  await menu(); await p.locator('#menu button').getByText('Paste', { exact: true }).click();
  await p.evaluate(() => { probe.attach({ tab_id: 2, pane_id: 2 }, true); finishRead('wrong pane'); });
  await p.waitForTimeout(20);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'paste').length), 1);
});

test('pane actions retain the menu target and close confirmation names a pane', async t => {
  const { p } = await page(t); await screen(p);
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, title: 'split', panes: [
      { pane_id: 1, left: 0, top: 0, width: 40, height: 24, confirm_close: true },
      { pane_id: 3, left: 40, top: 0, width: 40, height: 24, confirm_close: false },
    ] }, { tab_id: 2, pane_id: 2, title: 'two' },
  ] }));
  await p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
  await p.locator('#menu button').getByText('Split Right', { exact: true }).click();
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'split_pane')), [{ type: 'split_pane', pane_id: 1, direction: 'Right' }]);
  await p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
  await p.locator('#menu button').getByText('Close Pane', { exact: true }).click();
  assert.equal(await p.locator('#dialog .title').innerText(), 'Close this pane?');
  await p.evaluate(() => probe.attach({ tab_id: 2, pane_id: 2 }, true));
  await p.locator('#dialog [data-act="ok"]').click();
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'close_pane')), [{ type: 'close_pane', pane_id: 1 }]);
});

test('a split pane zooms from the menu or Cmd+Shift+Enter, and the menu names the state', async t => {
  const { p } = await page(t); await screen(p);
  const split = zoomed => p.evaluate(zoomed => probe.handle({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, title: 'split', zoomed, panes: [
      { pane_id: 1, left: 0, top: 0, width: 40, height: 24 },
      { pane_id: 3, left: 40, top: 0, width: 40, height: 24 },
    ] }, { tab_id: 2, pane_id: 2, title: 'two' },
  ] }), zoomed);
  await split(false);
  await p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
  await p.locator('#menu button').getByText('Zoom Pane', { exact: true }).click();
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'zoom_pane')), [{ type: 'zoom_pane', pane_id: 1 }]);
  await p.evaluate(() => { messages.length = 0; });
  await p.locator('.xterm-helper-textarea').focus();
  await p.keyboard.press('Meta+Shift+Enter');
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'zoom_pane' || m.type === 'text')), [{ type: 'zoom_pane', pane_id: 1 }]);
  await split(true);
  await p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
  assert.equal(await p.locator('#menu button').getByText('Unzoom Pane', { exact: true }).count(), 1);
  await p.keyboard.press('Escape');
  // A tab without splits has nothing to zoom.
  await p.evaluate(() => probe.attach({ tab_id: 2, pane_id: 2 }, true));
  await p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
  assert.equal(await p.locator('#menu button').getByText(/Zoom Pane/).count(), 0);
});

for (const entry of ['keyboard', 'menu']) test('browser zoom reclaims its grid after the authoritative frame via ' + entry, async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(200);
  const initial = await p.evaluate(async () => {
    const grid = messages.filter(m => m.type === 'resize').at(-1);
    probe.handle({ type: 'frame', pane_id: 1, cols: grid.cols, rows: grid.rows, data: '\x1b[2J\x1b[1;1Hzoom' });
    await new Promise(resolve => probe.term.write('', resolve));
    probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, title: 'split', zoomed: false,
      panes: [{ pane_id: 1 }, { pane_id: 3 }] }] });
    messages.length = 0;
    return { grid, font: probe.term.options.fontSize };
  });
  const frame = cols => p.evaluate(async ({ cols, rows }) => {
    probe.handle({ type: 'frame', pane_id: 1, cols, rows, data: '' });
    await new Promise(resolve => probe.term.write('', resolve));
    return messages.filter(m => m.type === 'resize');
  }, { cols, rows: initial.grid.rows });
  // A resize from native Kaku or another browser does not give this page
  // permission to take the tab back without its own interaction.
  assert.deepEqual(await frame(initial.grid.cols * 2), []);
  await frame(initial.grid.cols);
  for (const zoomed of [false, true]) {
    await p.evaluate(zoomed => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, title: 'split', zoomed,
      panes: [{ pane_id: 1 }, { pane_id: 3 }] }] }), zoomed);
    await p.evaluate(() => { messages.length = 0; });
    if (entry === 'keyboard') {
      await p.locator('.xterm-helper-textarea').focus();
      await p.keyboard.press('Meta+Shift+Enter');
    } else {
      await p.locator('.xterm-screen').click({ button: 'right', position: { x: 20, y: 10 } });
      await p.locator('#menu button').getByText(zoomed ? 'Unzoom Pane' : 'Zoom Pane', { exact: true }).click();
    }
    assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'zoom_pane').length), 1);
    await p.evaluate(() => { messages.length = 0; });
    // Kaku reports the zoom, then the pane's frame. Zoomed, the pane is the
    // whole tab and the page claims it; unzoomed, the tab is split again and
    // keeps the Mac's layout. Half as wide and as tall as before, the pane
    // already fills the page's height, so the text keeps its size.
    await p.evaluate(zoomed => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, title: 'split', zoomed: !zoomed,
      panes: [{ pane_id: 1 }, { pane_id: 3 }] }] }), zoomed);
    if (zoomed) {
      assert.deepEqual(await frame(Math.floor(initial.grid.cols / 2)), []);
      assert.equal(await p.evaluate(() => probe.term.options.fontSize), initial.font);
    } else {
      assert.deepEqual(await frame(initial.grid.cols * 2), [initial.grid]);
    }
    await frame(initial.grid.cols);
    assert.equal(await p.evaluate(() => probe.term.options.fontSize), initial.font);
  }
  assert.deepEqual(errors, []);
});

test('terminal menu fits narrow and short viewport corners', async t => {
  const { p } = await page(t); await screen(p);
  for (const [width, height] of [[375, 240], [375, 900], [1280, 900], [1280, 240]]) {
    await p.setViewportSize({ width, height });
    await p.evaluate(() => document.querySelector('#term').dispatchEvent(new MouseEvent('contextmenu', {
      clientX: innerWidth - 2, clientY: innerHeight - 2, bubbles: true, cancelable: true,
    })));
    const bounds = await p.locator('#menu').boundingBox();
    assert.ok(bounds.x >= 0 && bounds.y >= 0);
    assert.ok(bounds.x + bounds.width <= width && bounds.y + bounds.height <= height);
    if (process.env.KAKU_TEST_SCREENSHOTS) {
      fs.mkdirSync(process.env.KAKU_TEST_SCREENSHOTS, { recursive: true });
      await p.screenshot({ path: path.join(process.env.KAKU_TEST_SCREENSHOTS, `menu-${width}-${height}.png`) });
    }
    await p.keyboard.press('Escape');
    assert.equal(await p.locator('#menu.show').count(), 0);
  }
});

// A background tab can lose its GPU context. The DOM renderer must take
// over at once (the addon itself waits about 3 s, showing a blank or broken
// grid), and WebGL must come back once the page is in front again.
test('a lost WebGL context hands the grid to the DOM at once and comes back', async t => {
  const gpu = await chromium.launch(launchOptions(GPU_ARGS));
  t.after(() => gpu.close());
  const { p, errors } = await page(t, '#t=fixture', {}, gpu);
  const hasWebgl = await p.evaluate(() => !!document.createElement('canvas').getContext('webgl2'));
  if (!hasWebgl) return t.skip('this Chromium has no WebGL2');
  await p.evaluate(() => {
    probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] });
    probe.handle({ type: 'frame', seq: 1, pane_id: 1, cols: 80, rows: 24, data: '\x1b[2J\x1b[1;1HLOST-CONTEXT' });
  });
  await p.waitForFunction(() => probe.term.buffer.active.getLine(0).translateToString(true) === 'LOST-CONTEXT');
  const webglCanvas = () => p.evaluate(() => [...document.querySelectorAll('.xterm-screen canvas')].some(c => c.getContext('webgl2')));
  assert.equal(await webglCanvas(), true);
  await p.evaluate(() => {
    const c = [...document.querySelectorAll('.xterm-screen canvas')].find(c => c.getContext('webgl2'));
    c.getContext('webgl2').getExtension('WEBGL_lose_context').loseContext();
  });
  await p.waitForTimeout(300);
  // Read now, without waiting: the addon alone hands over only after ~3 s.
  assert.match(await p.evaluate(() => document.querySelector('.xterm-rows')?.textContent || ''), /LOST-CONTEXT/);
  await p.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await p.waitForTimeout(300);
  assert.equal(await webglCanvas(), true);
  assert.equal(await p.locator('.xterm-rows').count(), 0);
  assert.deepEqual(errors, []);
});

// Once its glyph pages are full, the WebGL addon merges four of them in the
// middle of a frame, and that frame drew wrong letters until the next redraw
// (maintainer, 2026-10-21, switching tabs). Six tabs of colored CJK fill the
// pages; every frame in which a merge happened must equal the same screen
// drawn again a frame later.
test('switching tabs never shows a frame of wrong letters while WebGL merges glyph pages', async t => {
  const gpu = await chromium.launch(launchOptions(GPU_ARGS));
  t.after(() => gpu.close());
  // Room for 80x24 at that size: a grid larger than the window shrinks the
  // font on every switch, and each new size starts the atlas over.
  const { p, errors } = await page(t, '#t=fixture', { viewport: { width: 2000, height: 1300 } }, gpu);
  const hasWebgl = await p.evaluate(() => !!document.createElement('canvas').getContext('webgl2'));
  if (!hasWebgl) return t.skip('this Chromium has no WebGL2');
  const result = await p.evaluate(async () => {
    const tabs = [1, 2, 3, 4, 5, 6].map(i => ({ tab_id: i, pane_id: i, panes: [], title: 'tab' + i, active: i === 1 }));
    // The maintainer's 16px at 2.2x is about 35 device pixels a glyph.
    probe.handle({ type: 'config', font_size: 36 });
    probe.handle({ type: 'tabs', tabs });
    const HAN = '的一是在不了有和人这中大为上个国我以要他时来用们生到作地于出就分对成会可主发年动同工也能下过子说产种面而方后多定行学法所民得经十三之进着等部度家电力里如水化高自二理起小物现实加量都两体制机当使点从业本去把性好应开它合还因由其些然前外天政四日那社义事平形相全表间样与关各重新线内数正心反你明看原又么利比或但质气第向道命此变条只没结解问意建月公无系军很情者最立代想已通并提直题党程展五果料象员革位入常文总次品式活设及管特件长求老头基资边流路级少图山统接知较将组见计别她手角期根论运农指几九区强放决西被干做必战先回则任取据处府';
    const screenOf = k => {
      let seed = k * 7919 + 1;
      const rnd = n => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) % n; };
      let s = '\x1b[2J';
      for (let r = 1; r <= 24; r++) {
        s += `\x1b[${r};1H`;
        for (let c = 0; c < 60; c++) s += `\x1b[38;5;${rnd(256)}m` + (rnd(3) ? HAN[rnd(HAN.length)] : String.fromCharCode(33 + rnd(90)));
      }
      return s;
    };
    const canvas = () => [...document.querySelectorAll('.xterm-screen canvas')].find(c => c.getContext('webgl2'));
    const frame = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
    let merges = 0, checked = 0, wrong = 0, moved = 0, merged = false, pending = null, pendingRows = null, checking = false;
    // A screen written in slices draws between them: a merge on such a frame
    // is compared only if the redraw shows the same rows.
    const rowsNow = () => { const b = probe.term.buffer.active; let s = ''; for (let y = 0; y < probe.term.rows; y++) s += b.getLine(b.viewportY + y)?.translateToString(true) + '\n'; return s; };
    probe.webgl.onRemoveTextureAtlasCanvas(() => { merges++; merged = true; });
    // A texture slot left holding another page's glyphs draws them wrong in
    // every frame alike, which comparing two frames cannot see: read each
    // slot the addon counts as uploaded back and compare it with its page.
    const renderer = () => probe.term._core._renderService._renderer.value;
    const staleSlots = () => {
      const glyphs = renderer()._glyphRenderer.value, gl = glyphs._gl, atlas = renderer()._charAtlas;
      const before = gl.getParameter(gl.FRAMEBUFFER_BINDING), fb = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      const stale = [];
      let compared = 0;
      atlas.pages.forEach((page, i) => {
        const slot = glyphs._atlasTextures[i];
        if (!slot || slot.version !== page.version) return;
        compared++;
        const { width: w, height: h } = page.canvas;
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, slot.texture, 0);
        const read = new Uint8Array(w * h * 4);
        gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, read);
        const drawn = page.ctx.getImageData(0, 0, w, h).data;
        // Alpha alone: it is the same either side of premultiplying.
        let a = 0, b = 0;
        for (let k = 3; k < read.length; k += 4) { a += read[k]; b += drawn[k]; }
        if (a !== b) stale.push(i);
      });
      gl.bindFramebuffer(gl.FRAMEBUFFER, before);
      gl.deleteFramebuffer(fb);
      return { stale, compared };
    };
    // Registered after the page's own listener, so it sees what the frame shows.
    probe.term.onRender(() => {
      if (pending !== null) {
        if (rowsNow() !== pendingRows) moved++;
        else if (canvas().toDataURL() !== pending) wrong++;
        pending = null; checking = false;
        return;
      }
      if (!merged) return;
      merged = false; checked++;
      pending = canvas().toDataURL();
      pendingRows = rowsNow();
      checking = true;
      requestAnimationFrame(() => probe.term.refresh(0, probe.term.rows - 1));
    });
    let seq = 1;
    for (let i = 0; i < 36; i++) {
      const k = i % 6 + 1;
      probe.attach(tabs[k - 1]);
      // No cursor: its blink would differ between the two pictures.
      probe.handle({ type: 'frame', seq: seq++, pane_id: k, cols: 80, rows: 24, data: screenOf(k) + '\x1b[?25l' });
      await frame(); await frame();
      // The next tab's screen waits for a pending check: drawn into the
      // redraw, it read as wrong letters in a merge frame that had none.
      while (checking) await frame();
    }
    // Once, at the end: reading the pages back during the switches made
    // Helium draw a wrong frame now and then by itself.
    const slots = staleSlots();
    return { merges, checked, wrong, moved, ...slots };
  });
  assert.ok(result.merges > 0, `the tabs merged no glyph pages: ${JSON.stringify(result)}`);
  assert.ok(result.checked > result.moved, `no merge frame could be compared: ${JSON.stringify(result)}`);
  assert.equal(result.wrong, 0, `frames with wrong letters: ${JSON.stringify(result)}`);
  assert.ok(result.compared > 0, `no texture slot could be read back: ${JSON.stringify(result)}`);
  assert.deepEqual(result.stale, [], `texture slots holding another page's glyphs: ${JSON.stringify(result)}`);
  assert.deepEqual(errors, []);
});

// The state the maintainer's page reported after leaving and re-entering
// full screen: the view one row past the buffer's last line, and rows
// 18-29 one line object with row 17, so every write to any of them showed
// in all and Kaku's full redraws left the status line stacked down the page.
test('a buffer whose rows share one line object is repaired and drawn again', async t => {
  const { p, errors } = await page(t);
  const result = await p.evaluate(async () => {
    probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] });
    const screenOf = tag => { let d = ''; for (let r = 1; r <= 34; r++) d += `\x1b[${r};1H${tag} row ${r}\x1b[K`; return d; };
    const applied = seq => new Promise(r => { const t = setInterval(() => { if (messages.some(m => m.type === 'frame_ack' && m.seq === seq)) { clearInterval(t); r(); } }, 5); });
    probe.handle({ type: 'frame', seq: 1, pane_id: 1, cols: 100, rows: 34, data: '\x1b[2J' + screenOf('first') });
    await applied(1);
    const buf = probe.term._core.buffer;
    const shared = buf.lines.get(16);
    for (let i = 17; i <= 28; i++) buf.lines.set(i, shared);
    buf.ybase = 1; buf.ydisp = 1;
    messages.length = 0;
    probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 100, rows: 34, data: '\x1b[34;1Hstatus line\x1b[K' });
    await applied(2);
    const asked = messages.some(m => m.type === 'redraw');
    // Kaku answers a redraw with every row.
    probe.handle({ type: 'frame', seq: 3, pane_id: 1, cols: 100, rows: 34, data: screenOf('again') });
    await applied(3);
    const b = probe.term.buffer.active;
    const rows = [];
    for (let y = 0; y < 34; y++) rows.push(b.getLine(b.viewportY + y).translateToString(true));
    const lines = probe.term._core.buffer.lines;
    const objects = new Set();
    for (let i = 0; i < lines.length; i++) objects.add(lines.get(i));
    return { asked, rows, distinct: objects.size, length: lines.length, base: b.baseY };
  });
  assert.equal(result.asked, true, 'the page asks Kaku for every row again');
  assert.equal(result.distinct, result.length, 'every row is its own line object');
  assert.ok(result.base + 34 <= result.length, `view inside the buffer: base ${result.base} of ${result.length}`);
  assert.deepEqual(result.rows, Array.from({ length: 34 }, (_, i) => `again row ${i + 1}`));
  assert.deepEqual(errors, []);
});

test('the grid draws with WebGL where the browser has it, and with the DOM otherwise', async t => {
  const { p: dom, errors: domErrors } = await page(t);
  await screen(dom, 'DOM-GRID');
  assert.equal(await dom.locator('.xterm-rows').count(), 1);
  assert.deepEqual(domErrors, []);

  const gpu = await chromium.launch(launchOptions(GPU_ARGS));
  t.after(() => gpu.close());
  const { p, errors } = await page(t, '#t=fixture', {}, gpu);
  const hasWebgl = await p.evaluate(() => !!document.createElement('canvas').getContext('webgl2'));
  if (!hasWebgl) return t.skip('this Chromium has no WebGL2');
  await p.evaluate(() => {
    probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] });
    probe.handle({ type: 'frame', seq: 1, pane_id: 1, cols: 80, rows: 24, data: '\x1b[2J\x1b[1;1HGPU-GRID' });
  });
  await p.waitForFunction(() => probe.term.buffer.active.getLine(0).translateToString(true) === 'GPU-GRID');
  // The WebGL renderer replaces the DOM rows.
  assert.equal(await p.locator('.xterm-rows').count(), 0);
  // And it really draws the text: a canvas with only the background passes
  // every buffer check above.
  await fadedIn(p);
  const shot = (await p.locator('.xterm-screen').screenshot()).toString('base64');
  const lit = await p.evaluate(async png => {
    const img = await createImageBitmap(await (await fetch('data:image/png;base64,' + png)).blob());
    const c = new OffscreenCanvas(img.width, img.height).getContext('2d');
    c.drawImage(img, 0, 0);
    const d = c.getImageData(0, 0, img.width, img.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (d[i] + d[i + 1] + d[i + 2] > 450) n++;
    return n;
  }, shot);
  assert.ok(lit > 50, `WebGL drew ${lit} text pixels`);
  // History still opens over the grid in the terminal's font.
  const past = await p.evaluate(() => {
    probe.openPast();
    probe.onHistory({ request: messages.filter(m => m.type === 'history').at(-1)?.request, pane_id: 1, first: 0, top: 0,
      rows: ['\x1b[0mhistory-row'] });
    const el = document.querySelector('#past');
    return { shown: el.classList.contains('show'), text: el.textContent, font: el.style.fontFamily, size: el.style.fontSize };
  });
  assert.ok(past.shown);
  assert.match(past.text, /history-row/);
  assert.match(past.font, /Kaku Mono/);
  assert.equal(past.size, (await p.evaluate(() => probe.term.options.fontSize)) + 'px');
  assert.deepEqual(errors, []);
});

test('the cursor stays steady while frames arrive and blinks once the screen is still', async t => {
  const { p } = await page(t);
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] }));
  const frame = (seq, data) => p.evaluate(({ seq, data }) =>
    probe.handle({ type: 'frame', seq, pane_id: 1, cols: 80, rows: 24, data }), { seq, data });
  const blink = () => p.evaluate(() => probe.term.options.cursorBlink);
  const style = () => p.evaluate(() => probe.term.options.cursorStyle);
  await frame(1, '\x1b[1;1Hone\x1b[1;4H\x1b[5 q');
  await p.waitForTimeout(50);
  assert.equal(await blink(), false);
  assert.equal(await style(), 'bar');
  // Frames without a cursor escape keep it steady too.
  for (let seq = 2; seq < 8; seq++) {
    await frame(seq, '\x1b[1;1Hbusy ' + seq);
    await p.waitForTimeout(150);
    assert.equal(await blink(), false);
  }
  await p.waitForFunction(() => probe.term.options.cursorBlink === true, null, { timeout: 2000 });
  assert.equal(await style(), 'bar');
  await frame(9, '\x1b[1;1Hagain');
  await p.waitForTimeout(50);
  assert.equal(await blink(), false);
  // A steady cursor the app asked for never starts blinking.
  await frame(10, '\x1b[2 q');
  await p.waitForTimeout(1000);
  assert.equal(await blink(), false);
  assert.equal(await style(), 'block');
});

test('offline stops retrying and coming back online reconnects at once', async t => {
  const { p } = await page(t);
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] }));
  await p.context().setOffline(true);
  const before = await p.evaluate(() => { connections.at(-1).close(1006); return connections.length; });
  await p.waitForTimeout(2500);
  assert.equal(await p.evaluate(() => connections.length), before);
  assert.match(await p.locator('#notice').innerText(), /No connection/);
  await p.context().setOffline(false);
  await p.waitForFunction(n => connections.length === n + 1, before, { timeout: 500 });
});

test('the agent working in a tab shows its mark and still topic', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  const agent = p.locator('#bar .agent');
  assert.equal(await agent.isVisible(), false, 'no agent, no title');
  await p.evaluate(() => probe.handle({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true, agent: { program: 'claude', topic: 'Fix the padding bug' } },
    { tab_id: 2, pane_id: 2, panes: [], title: 'two', agent: { program: 'codex', topic: 'Other tab' } },
  ] }));
  assert.equal(await agent.isVisible(), true);
  assert.equal(await agent.innerText(), 'Fix the padding bug');
  assert.ok((await agent.locator('path').getAttribute('d')).startsWith('M'), 'the Claude mark is drawn');
  // The spinner in the pane title never reaches the browser tab.
  await p.evaluate(() => probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 80, rows: 24, title: '✳ Fix the padding bug', data: '' }));
  assert.equal(await p.title(), 'Fix the padding bug - Kaku');
  await p.setViewportSize({ width: 375, height: 600 });
  assert.equal(await agent.isVisible(), false, 'phones keep the bar for tabs');
  assert.deepEqual(errors, []);
});

test('the Mac covering a pane with AI chat dims it and says so', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  const tabs = chat => ({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true, ...(chat ? { ai_chat: [1] } : {}) },
    { tab_id: 2, pane_id: 2, panes: [], title: 'two' },
  ] });
  await p.evaluate(m => probe.handle(m), tabs(true));
  assert.match(await p.locator('#notice').innerText(), /AI chat is open on the Mac/);
  assert.equal(await p.evaluate(() => document.body.classList.contains('stale')), true);
  await p.evaluate(m => probe.handle(m), tabs(false));
  assert.equal(await p.locator('#notice').isVisible(), false, 'closing the chat clears the notice');
  assert.equal(await p.evaluate(() => document.body.classList.contains('stale')), false);
  assert.deepEqual(errors, []);
});

test('a page open across an update reloads into the new one on reconnect, once', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  const reconnect = async () => {
    const before = await p.evaluate(() => connections.length);
    await p.evaluate(() => connections.at(-1).close());
    await p.waitForFunction(n => connections.length > n, before);
    await p.evaluate(() => connections.at(-1).onmessage({ data: '{"type":"pong"}' }));
    await p.waitForTimeout(300);
  };
  let loads = 0;
  p.on('load', () => loads++);
  // The first message only counts as a reconnect after an earlier session.
  await p.evaluate(() => connections.at(-1).onmessage({ data: '{"type":"pong"}' }));
  await reconnect();
  assert.equal(loads, 0, 'the same script never reloads');

  // The address now serves a new script, but a stale cache keeps handing
  // the reload the old page: it reloads once and then stays put.
  const served = fs.readFileSync(path.join(root, 'crates/kaku-remote/assets/web/index.html'), 'utf8')
    .replace('<script>', '<script>window.newPage = true;');
  await p.route('http://localhost:9988/', route => route.request().resourceType() === 'fetch'
    ? route.fulfill({ contentType: 'text/html', body: served })
    : route.fallback());
  await reconnect();
  await p.waitForFunction(() => window.probe);
  assert.equal(loads, 1, 'a changed script reloads the page');
  await p.evaluate(() => connections.at(-1).onmessage({ data: '{"type":"pong"}' }));
  await reconnect();
  assert.equal(loads, 1, 'the same new version never reloads twice');
  assert.deepEqual(errors, []);
});

test('no size is claimed before Kaku\'s config and font are in, then one', async t => {
  const { p } = await page(t, '#t=fixture', { noConfig: true });
  await screen(p);
  await p.setViewportSize({ width: 900, height: 560 });
  await p.waitForTimeout(250);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'resize')), []);
  await p.evaluate(() => { probe.handle({ type: 'config', font_size: 14 }); return document.fonts.load('16px "Kaku Mono"'); });
  await p.waitForTimeout(250);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'resize').length), 1);
});

for (const [lineHeight, expected] of [[0.95, 1], [1, 1], [1.28, 1.28]]) {
  test(`remote line height ${lineHeight} keeps initial sizing and resize working`, async t => {
    const { p, errors } = await page(t, '#t=fixture', { noConfig: true });
    await screen(p);
    assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'resize')), []);
    // Start with the first config: a prior config would already have made
    // fontReady true and hidden the failure reported in #612.
    await p.evaluate(line_height => probe.handle({ type: 'config', line_height, font_size: 14 }), lineHeight);
    await p.waitForFunction(() => messages.some(m => m.type === 'resize'));
    assert.equal(await p.evaluate(() => probe.term.options.lineHeight), expected);
    const initial = await p.evaluate(() => messages.filter(m => m.type === 'resize').at(-1));
    // Apply the claimed grid, as Kaku would, then change the browser size.
    await p.evaluate(grid => {
      probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: grid.cols, rows: grid.rows, data: '' });
      messages.length = 0;
    }, initial);
    await p.setViewportSize({ width: 600, height: 700 });
    await p.waitForFunction(() => messages.some(m => m.type === 'resize'));
    const resized = await p.evaluate(() => messages.filter(m => m.type === 'resize').at(-1));
    assert.ok(resized.cols < initial.cols, 'the narrower window claims fewer columns');
    assert.notEqual(resized.rows, initial.rows, 'the taller window claims a new row count');
    assert.deepEqual(errors, []);
  });
}

test('invalid remote line heights keep the last valid setting', async t => {
  const { p, errors } = await page(t, '#t=fixture', { noConfig: true });
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', line_height: 1.28 }));
  await p.waitForFunction(() => messages.some(m => m.type === 'resize'));
  for (const value of [null, 0, -0.5, '0.95', NaN, Infinity]) {
    await p.evaluate(line_height => probe.handle({ type: 'config', line_height }), value);
    assert.equal(await p.evaluate(() => probe.term.options.lineHeight), 1.28);
  }
  await p.evaluate(() => probe.handle({ type: 'config' }));
  assert.equal(await p.evaluate(() => probe.term.options.lineHeight), 1.28);
  assert.deepEqual(errors, []);
});

test('the drift check names the last frame drawn, so it runs while output streams', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    for (let seq = 2; seq <= 3; seq++) probe.handle({ type: 'frame', seq, pane_id: 1, cols: 80, rows: 24, data: '\x1b[2;1Hline ' + seq });
    probe.handle({ type: 'frame', seq: 4, pane_id: 1, cols: 90, rows: 24, data: '\x1b[2J\x1b[1;1Hresized' });
  });
  await p.waitForTimeout(1400);
  const checks = await p.evaluate(() => messages.filter(m => m.type === 'verify'));
  assert.ok(checks.length >= 1, 'a resize is checked within a second or so');
  const last = checks.at(-1);
  assert.equal(last.seq, 4);
  assert.deepEqual([last.view.cols, last.view.rows], [90, 24]);
  assert.equal(last.rows[0].trimEnd(), 'resized');
  // How the page draws goes along for the Mac's log.
  assert.ok(last.view.font > 0 && last.view.cell_w > 0 && last.view.cell_h > 0 && last.view.icon > 0, JSON.stringify(last.view));
  assert.equal(last.view.past, false);
  // The buffer's own lines, and rows sharing one line object, which no redraw fixes.
  assert.match(last.view.trace, /^repairs 0 lines 24 shared \[\] cursor /);
  const resets = last.view.atlas;
  await p.evaluate(() => window.dispatchEvent(new Event('focus')));
  await p.evaluate(() => probe.handle({ type: 'frame', seq: 5, pane_id: 1, cols: 90, rows: 24, data: '\x1b[2;1Hmore' }));
  await p.waitForTimeout(1400);
  const after = (await p.evaluate(() => messages.filter(m => m.type === 'verify'))).at(-1);
  // Focus no longer rebuilds the atlas: a rebuild only made glyph page merges more frequent.
  assert.equal(after.view.atlas, resets);
});

test('a digest check sends a hash, and rows only when Kaku asks', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    probe.handle({ type: 'config', verify_digest: true });
    probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 80, rows: 24, data: '\x1b[2J\x1b[1;1H✳ Hi 中文\x1b[3;1H  a b' });
  });
  await p.waitForTimeout(1400);
  await p.evaluate(() => { messages.length = 0; probe.handle({ type: 'frame', seq: 3, pane_id: 1, cols: 80, rows: 24, data: '\x1b[5;1H' }); });
  await p.waitForFunction(() => messages.some(m => m.type === 'verify'), null, { timeout: 4000 });
  const check = await p.evaluate(() => messages.find(m => m.type === 'verify'));
  // The same rows hash the same in web.rs (`rows_digest_matches_the_page`).
  assert.equal(check.digest, 0x936654e8);
  assert.equal(check.rows, undefined);
  assert.equal(check.view?.trace, undefined, 'no trace without a mismatch');
  await p.evaluate(() => { messages.length = 0; probe.handle({ type: 'verify_rows' }); });
  await p.waitForFunction(() => messages.some(m => m.type === 'verify'));
  const full = await p.evaluate(() => messages.find(m => m.type === 'verify'));
  assert.equal(full.rows[0].trimEnd(), '✳ Hi 中文');
  assert.match(full.view.trace, /^repairs /);
});

test('a frame without title or wrap flags keeps the ones the page has', async t => {
  const { p } = await page(t);
  await screen(p);
  const hello = await p.evaluate(() => messages.find(m => m.type === 'hello'));
  assert.equal(hello.delta, true);
  const titles = await p.evaluate(async () => {
    probe.handle({ type: 'frame', seq: 2, pane_id: 1, cols: 10, rows: 3, title: 'claude', wrapped: [true, false, false], data: '\x1b[2J\x1b[1;1HAB  CDEF  \x1b[2;1H  K' });
    const first = document.title;
    probe.handle({ type: 'frame', seq: 3, pane_id: 1, cols: 10, rows: 3, data: '\x1b[3;1Hx' });
    await new Promise(resolve => probe.term.write('', resolve));
    probe.term.select(2, 0, 11);
    const data = new DataTransfer();
    document.querySelector('.xterm-helper-textarea').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
    return { first, after: document.title, copied: data.getData('text/plain') };
  });
  assert.equal(titles.first, 'claude - Kaku');
  assert.equal(titles.after, 'claude - Kaku');
  assert.equal(titles.copied, '  CDEF    K', 'wrap flags kept');
});

test('the page tells Kaku when it goes to the background and back', async t => {
  const { p } = await page(t);
  await screen(p);
  const sent = await p.evaluate(() => {
    messages.length = 0;
    for (const hidden of [true, false]) {
      Object.defineProperty(document, 'hidden', { value: hidden, configurable: true });
      Object.defineProperty(document, 'visibilityState', { value: hidden ? 'hidden' : 'visible', configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    }
    return messages.filter(m => m.type === 'visibility');
  });
  assert.deepEqual(sent, [{ type: 'visibility', hidden: true }, { type: 'visibility', hidden: false }]);
});

test('the page draws with Kaku\'s font families in their order', async t => {
  const { p } = await page(t);
  assert.equal(await p.evaluate(() => probe.term.options.fontFamily),
    '"Kaku Mono", "Kaku Symbols", "PingFang SC", "Apple Symbols", "Microsoft YaHei", "Segoe UI Symbol", monospace');
  await p.evaluate(() => probe.handle({ type: 'config', font_families: ['Maple Mono NF', 'JetBrains Mono', 'PingFang SC', 'Apple Color Emoji'] }));
  assert.equal(await p.evaluate(() => probe.term.options.fontFamily),
    '"Maple Mono NF", "Kaku Mono", "Kaku Symbols", "PingFang SC", "Apple Symbols", "Microsoft YaHei", "Segoe UI Symbol", monospace');
  // A Mac on an older Kaku names only its first family.
  await p.evaluate(() => probe.handle({ type: 'config', font_family: 'Iosevka' }));
  assert.ok((await p.evaluate(() => probe.term.options.fontFamily)).startsWith('"Iosevka", "Kaku Mono", '));
});

test('Kaku\'s bundled icons draw on the page', async t => {
  const { p } = await page(t);
  const drawn = await p.evaluate(async () => {
    const faces = await document.fonts.load('16px "Kaku Symbols"', '');
    const ctx = document.createElement('canvas').getContext('2d');
    const width = (font) => { ctx.font = font; return ctx.measureText('').width; };
    return { faces: faces.length, stack: width('16px ' + probe.term.options.fontFamily), icons: width('16px "Kaku Symbols"') };
  });
  assert.equal(drawn.faces, 1);
  assert.equal(drawn.stack, drawn.icons, 'the stack draws icons from the bundled symbol font');
});

test('a font only the Mac has is fetched once, then kept by this browser', async t => {
  const context = await browser.newContext();
  t.after(() => context.close());
  const font = fs.readFileSync(path.join(root, 'crates/kaku-remote/assets/web/font-regular.woff2'));
  const file = { family: 'Kaku Test Mono', path: '/Users/me/Library/Fonts/Test Mono.woff2', size: font.length, modified: 7 };
  const serve = async (p) => {
    const asked = await p.evaluate(() => messages.filter(m => m.type === 'download'));
    for (const { id } of asked) {
      await p.evaluate(({ id, size, data }) => {
        probe.handle({ type: 'file_start', id, name: 'Test Mono.woff2', size });
        for (let i = 0; i < data.length; i += 65536) probe.handle({ type: 'file_chunk', id, data: data.slice(i, i + 65536) });
        probe.handle({ type: 'file_end', id });
      }, { id, size: font.length, data: font.toString('base64') });
    }
    return asked;
  };
  const loaded = (p) => p.evaluate(() => [...document.fonts].some(f => f.family.replace(/"/g, '') === 'Kaku Test Mono' && f.status === 'loaded'));
  const first = await page(t, '#t=fixture', {}, context);
  await screen(first.p);
  await first.p.evaluate(file => probe.handle({ type: 'config', font_families: [file.family], font_files: [file] }), file);
  await first.p.waitForTimeout(200);
  const asked = await serve(first.p);
  assert.deepEqual(asked.map(m => m.path), [file.path]);
  await first.p.waitForTimeout(300);
  assert.ok(await loaded(first.p), 'the fetched file draws the terminal');
  assert.equal(await first.p.evaluate(() => document.querySelector('#toast').classList.contains('show')), false, 'no download toast for a font');
  assert.deepEqual(first.errors, []);

  const again = await page(t, '#t=fixture', {}, context);
  await screen(again.p);
  await again.p.evaluate(file => probe.handle({ type: 'config', font_families: [file.family], font_files: [file] }), file);
  await again.p.waitForTimeout(400);
  assert.deepEqual(await again.p.evaluate(() => messages.filter(m => m.type === 'download')), [], 'kept from the first visit');
  assert.ok(await loaded(again.p));
});

test('a font this browser already has is not fetched', async t => {
  const { p } = await page(t);
  await p.evaluate(() => probe.handle({ type: 'config', font_families: ['Menlo'], font_files: [{ family: 'Menlo', path: '/Library/Fonts/Menlo.ttf', size: 100, modified: 1 }] }));
  await p.waitForTimeout(300);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'download')), []);
});

// The icon font loads only once an icon is drawn, after WebGL already
// rasterized a box for it; the grid must redraw with the icon by itself.
test('an icon drawn before its font loaded is redrawn once the font is in', async t => {
  const gpu = await chromium.launch(launchOptions(GPU_ARGS));
  t.after(() => gpu.close());
  const { p, errors } = await page(t, '#t=fixture', {}, gpu);
  if (!await p.evaluate(() => !!document.createElement('canvas').getContext('webgl2'))) return t.skip('this Chromium has no WebGL2');
  await p.evaluate(() => {
    probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, panes: [], title: 'one', active: true }] });
    probe.handle({ type: 'frame', seq: 1, pane_id: 1, cols: 80, rows: 24, data: '\x1b[2J\x1b[1;1H dir' });
  });
  // Drawing into a canvas does not always start the load of a font limited
  // by unicode-range, so start it here; what is checked is the redraw after.
  await p.evaluate(() => document.fonts.load('16px "Kaku Symbols"', '\uf115'));
  await p.waitForFunction(() => [...document.fonts].some(f => f.family.includes('Kaku Symbols') && f.status === 'loaded'));
  await p.waitForTimeout(300);
  await fadedIn(p);
  const box = await p.locator('.xterm-screen').boundingBox();
  const clip = { x: box.x, y: box.y, width: 80, height: 24 };
  const drawn = await p.screenshot({ clip });
  // What the cell looks like once the atlas is rebuilt with the font in.
  await p.evaluate(() => { probe.term.clearTextureAtlas(); probe.term.refresh(0, probe.term.rows - 1); });
  await p.waitForTimeout(300);
  assert.ok(drawn.equals(await p.screenshot({ clip })), 'the icon, not the box drawn before its font arrived');
  assert.deepEqual(errors, []);
});

test('a tab split on the Mac keeps the Mac\'s sizes, its split shown as large as fits', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.waitForTimeout(200);
  const grid = await p.evaluate(() => messages.filter(m => m.type === 'resize').at(-1));
  const font = await p.evaluate(() => probe.term.options.fontSize);
  const tabs = (panes) => p.evaluate(panes => probe.handle({ type: 'tabs', tabs: [{ tab_id: 1, pane_id: 1, title: 'one', active: true,
    panes: panes > 1 ? Array.from({ length: panes }, (_, i) => ({ pane_id: i + 1 })) : undefined }] }), panes);
  const frame = (cols, rows) => p.evaluate(async ({ cols, rows }) => {
    probe.handle({ type: 'frame', pane_id: 1, cols, rows, data: '\x1b[2J\x1b[1;1Hpane' });
    await new Promise(resolve => probe.term.write('', resolve));
    const stage = document.querySelector('#stage') || document.querySelector('.xterm').parentElement;
    const screen = document.querySelector('.xterm-screen').getBoundingClientRect();
    const room = stage.getBoundingClientRect();
    return { font: probe.term.options.fontSize, fits: screen.width <= room.width + 1 && screen.height <= room.height + 1,
      claims: messages.filter(m => m.type === 'resize') };
  }, { cols, rows });
  await p.evaluate(() => { messages.length = 0; });
  await tabs(2);
  const half = await frame(Math.floor(grid.cols / 2), Math.floor(grid.rows / 2));
  assert.deepEqual(half.claims, [], 'the Mac keeps its layout');
  assert.ok(half.font > font * 1.5 && half.fits, 'a half-size split fills the page');
  await tabs(4);
  const quarter = await frame(Math.floor(grid.cols / 4), Math.floor(grid.rows / 4));
  assert.deepEqual(quarter.claims, []);
  assert.ok(quarter.font <= font * 2 && quarter.fits, 'at most twice Kaku\'s size');
  await tabs(1);
  const back = await frame(grid.cols, grid.rows);
  assert.deepEqual(back.claims.map(({ cols, rows }) => ({ cols, rows })), [{ cols: grid.cols, rows: grid.rows }], 'one pane again: the page sizes the tab');
  assert.equal(back.font, font);
  assert.deepEqual(errors, []);
});

test('the desktop view is offered only when Kaku has its helper', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  assert.equal(await p.locator('#bar .tool.desktop').count(), 0);
  assert.equal(await p.evaluate(() => !!window.KakuDesktop), false, 'its code is not even loaded');
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').waitFor();
  assert.deepEqual(errors, []);
});

// The desktop view opens in full screen; leave it, as holding Esc does,
// where a test wants the plain window.
const windowed = p => p.evaluate(() => document.fullscreenElement && document.exitFullscreen());

test('the desktop view takes the keyboard and mouse, and gives them back', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');
  // The display button between them shows only with two displays.
  assert.equal(await tool.evaluate(b => b.nextElementSibling?.classList.contains('screens') && b.nextElementSibling.nextElementSibling?.classList.contains('fs')), true, 'beside the full screen button');
  await tool.click();
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), true);
  await p.waitForFunction(() => !!document.fullscreenElement, null, { timeout: 3000 });
  assert.equal(await p.evaluate(() => !!document.fullscreenElement), true, 'the screen opens in full screen');
  const cover = await p.evaluate(() => { const r = document.getElementById('desktop').getBoundingClientRect(); const b = document.getElementById('bar').getBoundingClientRect(); return r.bottom <= b.top || r.top >= b.bottom; });
  assert.equal(cover, true, 'beside the bar, which keeps the way back');
  // The picture is asked for once full screen settles, at the view's size,
  // which the Mac's screen takes on.
  await p.waitForFunction(() => messages.some(m => m.type === 'desktop_start'), null, { timeout: 3000 });
  const start = await p.evaluate(() => messages.filter(m => m.type === 'desktop_start'));
  assert.equal(start.length, 1);
  assert.ok(start[0].max_width >= 640 && start[0].max_height >= 400, JSON.stringify(start[0]));
  const shape = await p.evaluate(() => ({ w: document.getElementById('desktop').clientWidth, h: document.getElementById('desktop').clientHeight }));
  assert.deepEqual([start[0].view_width, start[0].view_height, start[0].fps], [shape.w, shape.h, 60], JSON.stringify(start[0]));
  assert.equal(typeof start[0].dpr, 'number');
  assert.match(await p.locator('#desktop .status .title').innerText(), /Connecting/);

  // Until a picture is drawn, keys are held back from both the terminal
  // and the Mac.
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.press('a');
  assert.deepEqual(await p.evaluate(() => messages.filter(m => ['text', 'key', 'desktop_key'].includes(m.type))), []);

  // A helper error reads as one.
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'error', detail: 'Kaku has no Screen Recording permission on this Mac' }));
  assert.match(await p.locator('#desktop .status').innerText(), /Screen Recording/);

  // A reconnect asks again.
  await p.evaluate(() => { messages.length = 0; probe.handle({ type: 'config', desktop: true }); });
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'desktop_start').length), 1);

  // Back to the terminal: the helper stops and typing goes to the pane.
  await p.evaluate(() => { messages.length = 0; });
  await windowed(p);
  await tool.click();
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'desktop_stop').length), 1);
  await p.keyboard.type('b');
  assert.ok(await p.evaluate(() => messages.some(m => m.type === 'text' && m.data === 'b')));
  assert.deepEqual(errors, []);
});

// Watch what reaches the desktop view, whichever way it came.
const spyDesktop = p => p.evaluate(() => {
  window.desktopSeen = [];
  Object.defineProperty(window, 'KakuDesktop', { configurable: true, set(real) {
    Object.defineProperty(window, 'KakuDesktop', { configurable: true, writable: true, value: { init(api) {
      const view = real.init(api);
      const handle = view.handle;
      view.handle = (msg) => { desktopSeen.push(msg); handle(msg); };
      return view;
    } } });
  } });
});
// A picture from the Mac, decoded by a stand-in for WebCodecs that draws a
// plain 320x200 frame for each chunk.
const showPicture = (p, seq = 1, inputSeq, streamId) => p.evaluate(async ({ seq, inputSeq, streamId }) => {
  // Entering asks for full screen first and the picture after it settles.
  const t0 = performance.now();
  while (!document.fullscreenElement && performance.now() - t0 < 1200) await new Promise(r => setTimeout(r, 10));
  for (let i = 0; i < 3; i++) await new Promise(r => requestAnimationFrame(r));
  if (!window.fakeDecoder) {
    window.fakeDecoder = true;
    window.VideoDecoder = class {
      constructor(init) { this.init = init; this.state = 'unconfigured'; }
      configure() { this.state = 'configured'; }
      decode(chunk) {
        const frame = document.createElement('canvas');
        Object.assign(frame, { width: 320, height: 200, displayWidth: 320, displayHeight: 200, timestamp: chunk.timestamp, close() {} });
        setTimeout(() => this.init.output(frame), 0);
      }
      close() { this.state = 'closed'; }
    };
  }
  probe.handle({ type: 'desktop_frame', seq, input_seq: inputSeq, stream_id: streamId, key: true, codec: 'avc1.640028', desc: 'AAAA', data: 'AAAA' });
  // Drawn, and acked, once the decoder hands it back; a touch screen then
  // frames its first picture and sends the pointer there on the next
  // animation frame. On a slow runner both came after a test had cleared
  // its messages, or before it read the pointer. A frame the page drops is
  // never acked, so the wait is bounded.
  const t1 = performance.now();
  while (!messages.some(m => m.type === 'desktop_ack' && m.seq === seq) && performance.now() - t1 < 500) await new Promise(r => setTimeout(r, 10));
  for (let i = 0; i < 2; i++) await new Promise(r => requestAnimationFrame(r));
}, { seq, inputSeq, streamId });

test('desktop stream IDs reject old frames and recover a frame that overtakes status', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p, 10, undefined, 'first');
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 0, 'a new stream waits for its status');
  assert.deepEqual(await p.evaluate(() => messages.find(m => m.type === 'desktop_ack' && m.seq === 10)),
    { type: 'desktop_ack', seq: 10, stream_id: 'first' });
  await p.evaluate(() => { messages.length = 0; probe.handle({ type: 'desktop_status', state: 'streaming', stream_id: 'first' }); });
  assert.ok(await p.evaluate(() => messages.some(m => m.type === 'desktop_keyframe')), 'recover a discarded first keyframe');
  await showPicture(p, 11, undefined, 'first');
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 11);
  await windowed(p);
  await p.locator('#bar .tool.desktop').click();
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p, 12, undefined, 'first');
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', stream_id: 'first' }));
  await showPicture(p, 13, undefined, 'first');
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 11, 'a resumed view cannot reactivate the old stream');
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', stream_id: 'second' }));
  await showPicture(p, 1, undefined, 'second');
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 1, 'a restarted helper may begin at sequence one with a new ID');
  await showPicture(p, 999, undefined, 'first');
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 1);
  assert.deepEqual(await p.evaluate(() => messages.find(m => m.type === 'desktop_ack' && m.seq === 999)),
    { type: 'desktop_ack', seq: 999, stream_id: 'first' }, 'an old frame never receives the new stream identity');
  assert.deepEqual(errors, []);
});

test('an old desktop decoder cannot paint or reset a new stream', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  await p.evaluate(() => {
    window.outputs = [];
    window.decoderCallbacks = [];
    window.closedFrames = 0;
    window.VideoDecoder = class {
      constructor(init) { this.init = init; this.state = 'unconfigured'; decoderCallbacks.push(init); }
      configure() { this.state = 'configured'; }
      close() { this.state = 'closed'; }
      decode(chunk) {
        const frame = document.createElement('canvas');
        Object.assign(frame, { width: 320, height: 200, displayWidth: 320, displayHeight: 200, timestamp: chunk.timestamp, close() { closedFrames++; } });
        frame.getContext('2d').fillStyle = decoderCallbacks.length === 1 ? 'red' : 'blue';
        frame.getContext('2d').fillRect(0, 0, 320, 200);
        outputs.push(() => this.init.output(frame));
      }
    };
    for (const [id, seq] of [['first', 100], ['second', 1]]) {
      probe.handle({ type: 'desktop_status', state: 'streaming', stream_id: id });
      probe.handle({ type: 'desktop_frame', seq, stream_id: id, key: true, codec: 'avc1.640028', desc: 'AAAA', data: 'AAAA' });
    }
    outputs[1]();
    outputs[0]();
    decoderCallbacks[0].error(new Error('late old decoder error'));
  });
  // The new stream's blue, not the old one's red; a GPU canvas may round
  // 255 to 254 on the way.
  const [r, g, b] = await p.evaluate(() => Array.from(document.querySelector('#desktop canvas').getContext('2d').getImageData(0, 0, 1, 1).data));
  assert.ok(r < 8 && g < 8 && b > 247, `pixel ${r},${g},${b}`);
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 1);
  assert.equal(await p.evaluate(() => closedFrames), 2, 'both current and discarded frames are released');
  await showPicture(p, 2, undefined, 'second');
  await p.evaluate(() => outputs.at(-1)());
  assert.equal(await p.evaluate(() => probe.desktop.lastDrawn), 2, 'a stale error leaves the new decoder usable');
  assert.deepEqual(errors, []);
});

// A picture as Kaku sends it to a page that asked for binary messages.
const binaryFrame = (header, bytes) => {
  const head = new TextEncoder().encode(JSON.stringify(header));
  const out = new Uint8Array(5 + head.length + bytes.length);
  new DataView(out.buffer).setUint32(1, head.length);
  out.set(head, 5);
  out.set(bytes, 5 + head.length);
  return out;
};

test('desktop pictures arrive as binary messages, locally and through the relay', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await spyDesktop(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await p.waitForFunction(() => messages.some(m => m.type === 'desktop_start'), null, { timeout: 3000 });
  const start = await p.evaluate(() => messages.find(m => m.type === 'desktop_start'));
  assert.equal(start.binary, true, 'the page asks for them');
  const frame = binaryFrame({ type: 'desktop_frame', seq: 5, key: true }, [0, 0, 0, 1, 9]);
  await p.evaluate((bytes) => connections.at(-1).onmessage({ data: new Uint8Array(bytes).buffer }), [...frame]);
  const seen = await p.evaluate(() => desktopSeen.map(m => ({ type: m.type, seq: m.seq, bytes: m.bytes && [...m.bytes] })));
  assert.deepEqual(seen, [{ type: 'desktop_frame', seq: 5, bytes: [0, 0, 0, 1, 9] }]);
  // A key frame without its parameter sets cannot be shown; it is still
  // acknowledged, so the Mac keeps sending.
  assert.ok(await p.evaluate(() => messages.some(m => m.type === 'desktop_ack' && m.seq === 5)));
  assert.deepEqual(errors, []);

  const mac = await newMac();
  const secret = Buffer.alloc(16, 9);
  const relayed = await page(t, pairingLink(mac, secret));
  await spyDesktop(relayed.p);
  await handshake(relayed.p, mac, { secret });
  await relayed.p.evaluate(async (bytes) => {
    await fromMac({ type: 'config', desktop: true });
    await new Promise(r => setTimeout(r, 300));
    document.querySelector('#bar .tool.desktop').click();
    await fromMac(new Uint8Array(bytes));
    // The binary message used its place in the sequence: what follows opens.
    await fromMac({ type: 'desktop_status', state: 'streaming' });
    await new Promise(r => setTimeout(r, 100));
  }, [...frame]);
  const relaySeen = await relayed.p.evaluate(() => desktopSeen.map(m => m.type));
  assert.deepEqual(relaySeen, ['desktop_frame', 'desktop_status']);
  assert.notEqual(await relayed.p.evaluate(() => connections.at(-1).readyState), 3);
  assert.deepEqual(relayed.errors, []);
});

test('a locked Mac says so over its picture until it is unlocked', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  const notice = p.locator('#desktop .locked');
  const visible = () => notice.evaluate(e => getComputedStyle(e).opacity === '1');
  assert.equal(await visible(), false);
  await p.evaluate(() => probe.handle({ type: 'desktop_locked', locked: true }));
  await p.waitForFunction(() => getComputedStyle(document.querySelector('#desktop .locked')).opacity === '1');
  assert.match(await notice.textContent(), /locked/);
  await p.evaluate(() => probe.handle({ type: 'desktop_locked', locked: false }));
  await p.waitForFunction(() => getComputedStyle(document.querySelector('#desktop .locked')).opacity === '0');
  // Leaving forgets it; the helper says again when the view opens.
  await p.evaluate(() => probe.handle({ type: 'desktop_locked', locked: true }));
  await p.locator('#bar .tool.desktop').click();
  assert.equal(await p.evaluate(() => document.getElementById('desktop').classList.contains('locked')), false);
  assert.deepEqual(errors, []);
});

test('the desktop view takes the Mac\'s pointer shape, its displays and dropped files', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);

  // The pointer over the screen takes the Mac's shape, at twice its size.
  const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
  await p.evaluate((png) => probe.handle({ type: 'desktop_cursor', png, scale: 2, hot_x: 4, hot_y: 9 }), png);
  const cursor = await p.locator('#desktop canvas').evaluate(c => c.style.cursor);
  assert.match(cursor, /image-set\(url\("data:image\/png;base64,iVBOR.*2x\) 4 9, default/);
  // A standard pointer is the browser's own, at this computer's size.
  await p.evaluate((png) => probe.handle({ type: 'desktop_cursor', name: 'text', png, scale: 2, hot_x: 4, hot_y: 9 }), png);
  assert.equal(await p.locator('#desktop canvas').evaluate(c => c.style.cursor), 'text');
  // Any other grows with the screen: drawn at twice its points, the
  // picture shows at twice its size, hot spot moved with it.
  const drawn = await p.locator('#desktop canvas').evaluate(c => c.getBoundingClientRect().width);
  await p.evaluate((w) => probe.handle({ type: 'desktop_status', state: 'streaming', screen_width: w }), Math.round(drawn / 2));
  await p.evaluate((png) => probe.handle({ type: 'desktop_cursor', name: 'url(x)', png, scale: 2, hot_x: 4, hot_y: 9 }), png);
  assert.match(await p.locator('#desktop canvas').evaluate(c => c.style.cursor), /image-set\(url\("data:image\/png;base64,iVBOR.*1x\) 8 18, default/);

  // One display: nothing to switch. Two: a button beside the screen's.
  const screens = p.locator('#bar .tool.screens');
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', display: 1, displays: [{ id: 1, name: 'Built-in', main: true }] }));
  await windowed(p);
  assert.equal(await screens.isVisible(), false);
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', display: 1, displays: [{ id: 1, name: 'Built-in', main: true }, { id: 7, name: 'Studio Display', main: false }] }));
  assert.equal(await screens.isVisible(), true);
  assert.equal(await screens.evaluate(b => b.previousElementSibling.classList.contains('desktop') && b.nextElementSibling.classList.contains('fs')), true);
  assert.match(await screens.getAttribute('title'), /Showing Built-in\. Switch to Studio Display/);
  await p.evaluate(() => { messages.length = 0; });
  await screens.click();
  const asked = await p.evaluate(() => messages.filter(m => m.type === 'desktop_start'));
  assert.equal(asked.length, 1);
  assert.equal(asked[0].display, 7);
  assert.equal(await p.evaluate(() => localStorage.getItem('kaku.desktop.display')), '7', 'remembered for next time');

  // A file dropped on the screen pastes no path into the pane; once it is
  // on the Mac, Finder shows it.
  await showPicture(p);
  const sent = await p.evaluate(async () => {
    const socket = connections.at(-1);
    const original = socket.send.bind(socket);
    socket.send = data => {
      original(data);
      const m = JSON.parse(data);
      if (m.type === 'upload_chunk') setTimeout(() => probe.handle({ type: 'upload_ack', id: m.id, received: atob(m.data).length }), 5);
      if (m.type === 'upload_end') setTimeout(() => probe.handle({ type: 'uploaded', id: m.id, ok: true, detail: '/Users/me/Downloads/Kaku/a.txt' }), 5);
    };
    messages.length = 0;
    await probe.sendFiles([new File(['hello'], 'a.txt')], false, true);
    return messages.filter(m => m.type === 'upload_start' || m.type === 'desktop_reveal');
  });
  assert.equal(sent[0].type, 'upload_start');
  assert.equal(sent[0].paste, false);
  assert.deepEqual(sent[1], { type: 'desktop_reveal', paths: ['/Users/me/Downloads/Kaku/a.txt'] });

  // Back to the terminal: the browser's own pointer, and no display button.
  await p.locator('#bar .tool.desktop').click();
  assert.equal(await p.locator('#desktop canvas').evaluate(c => c.style.cursor), '');
  assert.equal(await screens.isVisible(), false);
  assert.deepEqual(errors, []);
});

test('in full screen the bar stays, and its button switches both ways keeping the full screen it found', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');

  // From a windowed terminal the view opens its own full screen, and going
  // back closes it again.
  await tool.click();
  await p.waitForFunction(() => !!document.fullscreenElement, null, { timeout: 3000 });
  const bar = await p.locator('#bar').boundingBox();
  const view = await p.locator('#desktop').boundingBox();
  assert.ok(bar.height > 0 && (view.y >= bar.y + bar.height || view.y + view.height <= bar.y), 'the screen sits beside the bar, not over it');
  assert.equal(await tool.isVisible(), true);
  assert.equal(await tool.getAttribute('title'), 'Back to the terminal (Ctrl+Option+Cmd+D)');
  await tool.click();
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  await p.waitForFunction(() => !document.fullscreenElement, null, { timeout: 3000 });

  // From a full-screen terminal, back and forth from the same button stays
  // in full screen, and the terminal gets its own locked keys back.
  await p.locator('#bar .tool.fs').click();
  await p.waitForFunction(() => document.body.classList.contains('fullscreen'), null, { timeout: 3000 });
  await p.evaluate(() => { if (navigator.keyboard) navigator.keyboard.lock = (keys) => { window.lockedKeys = keys || 'all'; return Promise.resolve(); }; });
  await tool.click();
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), true);
  assert.equal(await p.evaluate(() => window.lockedKeys), 'all', 'every key goes to the Mac');
  assert.equal(await p.locator('#bar .clock').isVisible(), true, 'the clock stays in view');
  await p.evaluate(() => { messages.length = 0; });
  await tool.click();
  await p.waitForTimeout(200);
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  assert.equal(await p.evaluate(() => !!document.fullscreenElement), true, 'still in full screen');
  assert.ok(Array.isArray(await p.evaluate(() => window.lockedKeys)), 'the terminal\'s keys are locked again');
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'desktop_stop').length), 1);
  await tool.click();
  assert.equal(await p.evaluate(() => !!document.fullscreenElement), true);
  assert.deepEqual(errors, []);
});

test('coming back to the Mac\'s screen keeps its last picture until the new one, and input waits for it', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');
  await tool.click();
  await windowed(p);
  await showPicture(p);
  const canvas = p.locator('#desktop canvas');
  const moves = () => p.evaluate(() => messages.filter(m => m.type === 'desktop_pointer').length);
  const box = await canvas.boundingBox();
  await p.mouse.move(box.x + 50, box.y + 50);
  await p.mouse.move(box.x + 60, box.y + 60);
  await p.waitForTimeout(50);
  assert.ok(await moves() > 0, 'a live picture takes the mouse');

  // Away and back: the old picture shows at once, no black and no spinner.
  await tool.click();
  await tool.click();
  await windowed(p);
  assert.equal(await p.evaluate(() => document.getElementById('desktop').classList.contains('live')), true);
  assert.equal(await p.evaluate(() => getComputedStyle(document.querySelector('#desktop canvas')).opacity), '1');
  // Until the new stream draws, the mouse stays on the page.
  await p.evaluate(() => { messages.length = 0; });
  await p.mouse.move(box.x + 70, box.y + 70);
  await p.mouse.move(box.x + 80, box.y + 80);
  await p.waitForTimeout(50);
  assert.equal(await moves(), 0);
  await showPicture(p);
  await p.mouse.move(box.x + 90, box.y + 90);
  await p.waitForTimeout(50);
  assert.ok(await moves() > 0);

  // Another display: its picture is not this one, so it waits visibly.
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', display: 1, displays: [{ id: 1, name: 'Built-in', main: true }, { id: 7, name: 'Studio Display', main: false }] }));
  await p.locator('#bar .tool.screens').click();
  assert.equal(await p.evaluate(() => document.getElementById('desktop').classList.contains('live')), false);
  assert.match(await p.locator('#desktop .status .title').innerText(), /Connecting/);
  assert.deepEqual(errors, []);
});

test('the desktop view reports its timings, and a filled view asks again only when its size really changes', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');
  await tool.click();
  await showPicture(p);
  const box = await p.locator('#desktop canvas').boundingBox();
  await p.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await showPicture(p, 2);

  // A few pixels are not worth a new screen on the Mac; a real change is.
  const starts = () => p.evaluate(() => messages.filter(m => m.type === 'desktop_start'));
  const before = (await starts()).length;
  const size = p.viewportSize();
  await p.setViewportSize({ width: size.width - 8, height: size.height });
  await p.waitForTimeout(600);
  assert.equal((await starts()).length, before, 'eight pixels narrower');
  await p.setViewportSize({ width: size.width - 200, height: size.height });
  await p.waitForFunction((n) => messages.filter(m => m.type === 'desktop_start').length > n, before, { timeout: 3000 });
  const again = (await starts()).at(-1);
  assert.equal(again.view_width, await p.evaluate(() => document.getElementById('desktop').clientWidth));

  // Leaving sends what the period measured.
  await tool.click();
  const report = await p.evaluate(() => messages.find(m => m.type === 'desktop_report'));
  assert.ok(report, 'a report on the way out');
  assert.equal(report.drawn, 2, JSON.stringify(report));
  assert.equal(report.inputs, undefined, 'an old helper cannot confirm which input preceded capture');
  assert.equal(report.clicks, undefined, 'do not report next-frame timing as click response');
  assert.ok(report.decode_max >= report.decode_p50, JSON.stringify(report));
  assert.match(report.view, /^\d+x\d+@\d/);
  assert.match(report.view, / pointer \S+.* shown \S+$/);
  assert.deepEqual(errors, []);
});

test('desktop post timings are reported even when input changes no pixels', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    const interval = window.setInterval;
    window.setInterval = (fn, ms, ...args) => {
      if (ms === 10000) window.flushDesktopReport = fn;
      return interval(fn, ms, ...args);
    };
    probe.handle({ type: 'config', desktop: true });
  });
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  await p.evaluate(() => { flushDesktopReport(); messages.length = 0; });
  await p.keyboard.press('a');
  const report = await p.evaluate(() => {
    const input = messages.find(m => m.type === 'desktop_key' && m.down);
    probe.handle({ type: 'desktop_input_ack', input_seq: input.input_seq, queue_ms: 0 });
    flushDesktopReport();
    return messages.find(m => m.type === 'desktop_report');
  });
  assert.ok(report);
  assert.equal(report.drawn, 0);
  assert.equal(report.posted, 1);
  assert.equal(report.inputs, undefined, 'posting does not imply a new picture');
});

test('desktop reports incomplete supported inputs without inventing legacy timeouts', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    const interval = window.setInterval;
    window.setInterval = (fn, ms, ...args) => {
      if (ms === 10000) window.flushDesktopReport = fn;
      return interval(fn, ms, ...args);
    };
    const now = performance.now.bind(performance);
    window.metricClock = 0;
    performance.now = () => now() + metricClock;
    probe.handle({ type: 'config', desktop: true });
  });
  const tool = p.locator('#bar .tool.desktop');
  await tool.click();
  await p.waitForFunction(() => messages.some(m => m.type === 'desktop_start'));
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', stream_id: 'modern' }));
  await showPicture(p, 1, undefined, 'modern');
  await p.evaluate(() => { flushDesktopReport(); messages.length = 0; });
  await p.keyboard.press('a');
  await p.keyboard.press('b');
  const timedOut = await p.evaluate(() => {
    const inputs = messages.filter(m => m.type === 'desktop_key' && m.down);
    probe.handle({ type: 'desktop_input_ack', input_seq: inputs[1].input_seq, queue_ms: 0 });
    metricClock += 30001;
    flushDesktopReport();
    return messages.find(m => m.type === 'desktop_report');
  });
  assert.equal(timedOut.input_unposted, 1);
  assert.equal(timedOut.input_unframed, 1);
  assert.equal(timedOut.input_cancelled, undefined);
  assert.equal(timedOut.inputs, undefined);
  await p.evaluate(() => { messages.length = 0; flushDesktopReport(); });
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'desktop_report')), [], 'incomplete counts drain once');
  await p.keyboard.press('c');
  await windowed(p);
  await tool.click();
  assert.equal(await p.evaluate(() => messages.find(m => m.type === 'desktop_report').input_cancelled), 1);

  await tool.click();
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming' }));
  await showPicture(p);
  await p.keyboard.press('d');
  await p.evaluate(() => { messages.length = 0; metricClock += 30001; flushDesktopReport(); });
  const legacy = await p.evaluate(() => messages.find(m => m.type === 'desktop_report'));
  assert.equal(legacy.input_unposted, undefined, 'a legacy helper never promised an input ack');
  assert.equal(legacy.input_unframed, undefined);
  assert.equal(legacy.input_cancelled, undefined);
  assert.deepEqual(errors, []);
});

test('desktop input history accounts for capacity eviction and lifecycle cancellation', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    const interval = window.setInterval;
    window.setInterval = (fn, ms, ...args) => {
      if (ms === 10000) window.flushDesktopReport = fn;
      return interval(fn, ms, ...args);
    };
    probe.handle({ type: 'config', desktop: true });
  });
  await p.locator('#bar .tool.desktop').click();
  await p.waitForFunction(() => messages.some(m => m.type === 'desktop_start'));
  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', stream_id: 'modern' }));
  await showPicture(p, 1, undefined, 'modern');
  const overflow = await p.evaluate(() => {
    flushDesktopReport(); messages.length = 0;
    for (let i = 0; i < 1030; i++) document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', code: 'KeyA', repeat: i > 0, bubbles: true, cancelable: true }));
    flushDesktopReport();
    return messages.find(m => m.type === 'desktop_report');
  });
  assert.equal(overflow.input_unposted, 6);
  assert.equal(overflow.input_unframed, undefined);
  await windowed(p);
  await p.evaluate(() => { messages.length = 0; });
  await p.locator('#bar .tool.desktop').click();
  assert.equal(await p.evaluate(() => messages.find(m => m.type === 'desktop_report').input_cancelled), 1024);
});

test('desktop timings retain the first post-capture frame when its input ack arrives later', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');
  await tool.click();
  await showPicture(p);
  const before = await p.evaluate(() => performance.now());
  await p.keyboard.press('a');
  const seq = await p.evaluate(() => messages.find(m => m.type === 'desktop_key' && m.down).input_seq);
  await showPicture(p, 2, seq);
  const firstFrameBy = await p.evaluate(() => performance.now());
  await p.waitForTimeout(180);
  await showPicture(p, 3, seq);
  await p.evaluate(seq => probe.handle({ type: 'desktop_input_ack', input_seq: seq, queue_ms: 4 }), seq);
  await tool.click();
  const report = await p.evaluate(() => messages.find(m => m.type === 'desktop_report'));
  assert.equal(report.posted, 1, JSON.stringify(report));
  assert.equal(report.inputs, 1);
  assert.ok(report.input_frame_max <= firstFrameBy - before + 1, JSON.stringify(report));
  assert.ok(report.post_max > report.input_frame_max + 150, JSON.stringify(report));
});

test('desktop input timings wait for post acknowledgments and post-capture watermarks, including slow input', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');
  await tool.click();
  await showPicture(p);
  await p.keyboard.press('a');
  await p.evaluate(() => document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Unidentified', code: 'Unidentified', bubbles: true, cancelable: true })));
  await p.keyboard.press('b');
  const inputs = await p.evaluate(() => messages.filter(m => m.type === 'desktop_key' && m.down));
  assert.equal(inputs.length, 3);
  assert.ok(Number.isSafeInteger(inputs[0].input_seq) && inputs[0].input_seq > 0);
  assert.ok(inputs[1].input_seq > inputs[0].input_seq);
  await showPicture(p, 2); // A frame already in flight proves nothing about these keys.
  await p.evaluate(seq => {
    probe.handle({ type: 'desktop_input_ack', input_seq: seq, queue_ms: 12 });
    probe.handle({ type: 'desktop_input_ack', input_seq: seq, queue_ms: 12 });
  }, inputs[0].input_seq);
  await p.evaluate(seq => probe.handle({ type: 'desktop_input_ack', input_seq: seq, queue_ms: 12 }), inputs[2].input_seq);
  // The helper rejected the middle input, so a later watermark must not count it.
  await p.waitForTimeout(1100);
  await showPicture(p, 3, inputs[2].input_seq);
  await showPicture(p, 4, inputs[2].input_seq); // The same watermark must not count twice.
  await tool.click();
  const report = await p.evaluate(() => messages.find(m => m.type === 'desktop_report'));
  assert.equal(report.posted, 2, JSON.stringify(report));
  assert.equal(report.queue_p50, 12);
  assert.equal(report.inputs, 2, JSON.stringify(report));
  assert.ok(report.input_frame_p50 >= 1100, JSON.stringify(report));
  assert.equal(report.clicks, undefined);
  // A new stream must not attribute the previous session's delayed input.
  await tool.click();
  await showPicture(p, 5, inputs[2].input_seq);
  await tool.click();
  const next = await p.evaluate(() => messages.filter(m => m.type === 'desktop_report').at(-1));
  assert.equal(next.inputs, undefined);
});

test('picking a tab leaves the desktop view for the terminal', async t => {
  const { p } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  const looks = await p.evaluate(() => [...document.querySelectorAll('#tabs .tab')].map(b => getComputedStyle(b).backgroundColor));
  assert.equal(new Set(looks).size, 1, 'no tab reads as current over the screen: ' + looks);
  assert.notEqual(await p.locator('#bar .tool.desktop').evaluate(b => getComputedStyle(b).backgroundColor), looks[0]);
  await windowed(p);
  await p.locator('#tabs .tab').first().click();
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  assert.ok(await p.evaluate(() => messages.some(m => m.type === 'desktop_stop')));
});

test('real desktop wheel events never scroll the hidden terminal', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  const box = await p.locator('#desktop canvas').boundingBox();
  await p.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await p.evaluate(() => { messages.length = 0; });
  await p.mouse.wheel(0, 120);
  await p.waitForTimeout(80);
  const sent = await p.evaluate(() => messages);
  assert.ok(sent.some(m => m.type === 'desktop_wheel' && m.dy === 120), JSON.stringify(sent));
  assert.equal(sent.some(m => m.type === 'scroll'), false);
  assert.deepEqual(errors, []);
});

for (const cancel of [false, true]) test('real desktop touch ' + (cancel ? 'cancel releases a drag' : 'pinch leaves terminal scale alone'), async t => {
  const { p, errors } = await page(t, '#t=fixture', { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  const box = await p.locator('#desktop canvas').boundingBox(), x = box.x + box.width / 2, y = box.y + box.height / 2;
  const cdp = await p.context().newCDPSession(p);
  const touch = (type, points) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points.map(([id, x, y]) => ({ id, x, y })) });
  const scale = await p.evaluate(() => localStorage.getItem('kaku.scale'));
  await p.evaluate(() => { messages.length = 0; });
  if (cancel) {
    // A finger held still presses the button; moving it then drags.
    await touch('touchStart', [[1, x, y]]);
    await p.waitForTimeout(400);
    await touch('touchMove', [[1, x + 30, y + 30]]);
    await touch('touchCancel', []);
    await p.waitForTimeout(50);
    const sent = await p.evaluate(() => messages.filter(m => m.type === 'desktop_pointer'));
    assert.equal(sent.filter(m => m.kind === 'down').length, 1, JSON.stringify(sent));
    assert.equal(sent.filter(m => m.kind === 'up').length, 1, JSON.stringify(sent));
    assert.equal(sent.at(-1).kind, 'up', 'no pending move after cancellation');
  } else {
    await touch('touchStart', [[1, x - 20, y], [2, x + 20, y]]);
    await touch('touchMove', [[1, x - 50, y], [2, x + 50, y]]);
    await touch('touchEnd', []);
    await p.waitForTimeout(80);
    assert.equal(await p.evaluate(() => localStorage.getItem('kaku.scale')), scale);
    assert.deepEqual(await p.evaluate(() => messages.filter(m => ['scroll', 'resize'].includes(m.type))), []);
    assert.ok((await p.locator('#desktop canvas').boundingBox()).width > box.width * 2);
  }
  assert.deepEqual(errors, []);
});

test('on a touch screen switching tabs leaves the keyboard down, a tap on the terminal raises it', async t => {
  const { p } = await page(t, '#t=fixture', { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  if (!(await p.evaluate(() => document.body.classList.contains('touch')))) {
    await p.addInitScript(() => {
      const real = window.matchMedia.bind(window);
      window.matchMedia = q => q === '(pointer: coarse)' ? { matches: true, media: q, addEventListener() {}, removeEventListener() {} } : real(q);
    });
    await p.reload();
    await p.waitForTimeout(100);
    await p.evaluate(() => { probe.handle({ type: 'config' }); return document.fonts.load('16px "Kaku Mono"'); });
  }
  await screen(p, 'first');
  await p.evaluate(() => probe.attach({ pane_id: 2, tab_id: 2 }));
  await screen(p, 'second', 2);
  const typing = () => p.evaluate(() => document.activeElement?.classList.contains('xterm-helper-textarea') ?? false);
  await p.evaluate(() => document.activeElement?.blur());
  await p.locator('#mobile-switch').tap();
  await p.locator('.mobile-select').first().tap();
  await p.waitForTimeout(50);
  assert.equal(await typing(), false, 'a tab switch raised the keyboard');
  const box = await p.locator('#stage .xterm-screen').boundingBox();
  await p.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
  await p.waitForTimeout(50);
  assert.equal(await typing(), true);
});

test('losing focus cancels a desktop long press before it clicks', async t => {
  const { p } = await page(t, '#t=fixture', { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  const box = await p.locator('#desktop canvas').boundingBox();
  const cdp = await p.context().newCDPSession(p);
  await p.evaluate(() => { messages.length = 0; });
  // Focus goes away right after the finger lands, well inside the hold: a
  // separate evaluate after the touch could arrive past the hold on a slow
  // runner and see the press it meant to prevent.
  await p.evaluate(() => document.querySelector('#desktop canvas').addEventListener('pointerdown',
    () => setTimeout(() => window.dispatchEvent(new Event('blur'))), { once: true }));
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ id: 1, x: box.x + box.width / 2, y: box.y + box.height / 2 }] });
  await p.waitForTimeout(500);
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'desktop_pointer')), []);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'desktop_pointer')), []);
});

test('on a phone the desktop view opens zoomed in and works like a trackpad', async t => {
  const { p, errors } = await page(t, '#t=fixture', { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  // CI's Chromium has not always reported a coarse pointer for an emulated
  // phone; the page's touch layout is what this test is about.
  if (!(await p.evaluate(() => document.body.classList.contains('touch')))) {
    await p.addInitScript(() => {
      const real = window.matchMedia.bind(window);
      window.matchMedia = q => q === '(pointer: coarse)' ? { matches: true, media: q, addEventListener() {}, removeEventListener() {} } : real(q);
    });
    await p.reload();
    await p.waitForTimeout(100);
    await p.evaluate(() => { probe.handle({ type: 'config' }); return document.fonts.load('16px "Kaku Mono"'); });
  }
  assert.equal(await p.evaluate(() => document.body.classList.contains('touch')), true);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  const start = await p.evaluate(() => messages.find(m => m.type === 'desktop_start'));
  assert.ok(start.max_width >= 1920, 'enough detail to zoom into: ' + JSON.stringify(start));
  assert.equal(await p.locator('#keys').isVisible(), false, 'the terminal keys would type into the terminal');

  await p.evaluate(() => probe.handle({ type: 'desktop_status', state: 'streaming', screen_width: 1512 }));
  await showPicture(p);
  const rect = () => p.evaluate(() => {
    const r = document.querySelector('#desktop canvas').getBoundingClientRect();
    const v = document.getElementById('desktop').getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height, vx: v.left, vy: v.top, vw: v.width, vh: v.height };
  });
  const pointers = () => p.evaluate(() => messages.filter(m => m.type === 'desktop_pointer'));
  // The Mac's pointer on screen, from the last position sent.
  const onScreen = async () => {
    const r = await rect();
    const last = (await pointers()).at(-1);
    return { x: r.x + last.x * r.w, y: r.y + last.y * r.h, r };
  };

  // It opens at about 0.6 CSS pixels per Mac point, the pointer in the middle.
  const opened = await rect();
  assert.ok(Math.abs(opened.w - 1512 * 0.6) < 2, 'readable size: ' + opened.w);
  assert.ok(opened.h > opened.vh * 0.6, 'the picture uses the phone\'s height too: ' + JSON.stringify(opened));
  const first = (await pointers()).at(-1);
  assert.deepEqual([first.kind, first.x, first.y], ['move', 0.5, 0.5]);
  const middle = await onScreen();
  assert.ok(Math.abs(middle.x - (opened.vx + opened.vw / 2)) < 2 && Math.abs(middle.y - (opened.vy + opened.vh / 2)) < 2, JSON.stringify(middle));

  const touch = (type, id, x, y) => p.evaluate(([type, id, x, y]) => {
    document.querySelector('#desktop canvas').dispatchEvent(new PointerEvent(type, { pointerId: id, pointerType: 'touch', clientX: x, clientY: y, bubbles: true, cancelable: true, isPrimary: id === 1 }));
  }, [type, id, x, y]);
  const cx = opened.vx + opened.vw / 2;
  const cy = opened.vy + opened.vh / 2;
  const flush = () => p.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));

  // A finger moves the pointer by its travel, not to where it touches, and
  // the view follows the pointer before it reaches the edge.
  await p.evaluate(() => { messages.length = 0; });
  // The finger leaves the tap range in the same task it lands in: one round
  // trip per step took a slow runner past the hold, which then pressed.
  await p.evaluate(([cy]) => {
    const canvas = document.querySelector('#desktop canvas');
    for (const [type, x] of [['pointerdown', 60], ['pointermove', 80]]) {
      canvas.dispatchEvent(new PointerEvent(type, { pointerId: 1, pointerType: 'touch', clientX: x, clientY: cy, bubbles: true, cancelable: true, isPrimary: true }));
    }
  }, [cy]);
  for (let x = 90; x <= 180; x += 10) { await touch('pointermove', 1, x, cy); await flush(); }
  await touch('pointerup', 1, 180, cy);
  await flush();
  let sent = await pointers();
  assert.equal(sent.some(m => m.kind === 'down'), false, 'moving is not a click: ' + JSON.stringify(sent));
  assert.ok(sent.at(-1).x > 0.6 && sent.at(-1).x < 0.9, 'the pointer went right: ' + sent.at(-1).x);
  let seen = await onScreen();
  assert.ok(seen.x <= opened.vx + opened.vw - 79 && seen.x >= opened.vx, 'still in sight: ' + JSON.stringify(seen));
  assert.ok(seen.r.x < opened.x - 10, 'the view moved with it');

  // A tap clicks where the pointer is, wherever the finger lands.
  const at = sent.at(-1);
  await p.evaluate(() => { messages.length = 0; });
  await touch('pointerdown', 1, 30, cy + 100);
  await touch('pointerup', 1, 30, cy + 100);
  sent = await pointers();
  assert.deepEqual(sent.filter(m => m.kind !== 'move').map(m => [m.kind, m.button, m.x, m.y]), [['down', 0, at.x, at.y], ['up', 0, at.x, at.y]]);

  // Two fingers tapped right-click there.
  await p.evaluate(() => { messages.length = 0; });
  await touch('pointerdown', 1, cx - 20, cy);
  await touch('pointerdown', 2, cx + 20, cy);
  await touch('pointerup', 1, cx - 20, cy);
  await touch('pointerup', 2, cx + 20, cy);
  sent = await pointers();
  assert.deepEqual(sent.filter(m => m.kind !== 'move').map(m => [m.kind, m.button]), [['down', 2], ['up', 2]]);

  // Two fingers moving together scroll the Mac and leave the view alone.
  const before = await rect();
  await p.evaluate(() => { messages.length = 0; });
  await touch('pointerdown', 1, cx - 20, cy);
  await touch('pointerdown', 2, cx + 20, cy);
  for (let d = 10; d <= 60; d += 10) {
    await touch('pointermove', 1, cx - 20, cy - d);
    await touch('pointermove', 2, cx + 20, cy - d);
  }
  await touch('pointerup', 1, cx - 20, cy - 60);
  await touch('pointerup', 2, cx + 20, cy - 60);
  const wheels = await p.evaluate(() => messages.filter(m => m.type === 'desktop_wheel'));
  assert.ok(wheels.length >= 6 && wheels.every(m => m.dy > 0), JSON.stringify(wheels));
  assert.equal((await pointers()).some(m => m.kind === 'down'), false);
  assert.deepEqual(await rect(), before);

  // A pinch zooms around the fingers' midpoint and sends nothing to the Mac.
  // The scroll above still glides until a finger lands (on a slow runner a
  // glide frame came after a fixed 300 ms wait), so count from that finger.
  await touch('pointerdown', 1, cx - 20, cy);
  await p.evaluate(() => { messages.length = 0; });
  await touch('pointerdown', 2, cx + 20, cy);
  for (let d = 22; d <= 30; d += 2) {
    await touch('pointermove', 1, cx - d, cy);
    await touch('pointermove', 2, cx + d, cy);
  }
  await touch('pointerup', 1, cx - 30, cy);
  await touch('pointerup', 2, cx + 30, cy);
  const zoomed = await rect();
  assert.ok(Math.abs(zoomed.w / before.w - 1.5) < 0.05, 'one and a half times the spread: ' + zoomed.w / before.w);
  const mid = (cx - before.x) / before.w;
  assert.ok(Math.abs(zoomed.x + mid * zoomed.w - cx) < 2, 'the midpoint stayed put');
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type.startsWith('desktop_'))), []);

  // A finger held still presses the button, with a ring, and moving drags.
  await touch('pointerdown', 1, cx, cy);
  await p.waitForTimeout(400);
  assert.equal(await p.evaluate(() => document.querySelector('#desktop .press').classList.contains('held')), true);
  await touch('pointermove', 1, cx + 40, cy);
  await touch('pointermove', 1, cx + 80, cy);
  await flush();
  await touch('pointerup', 1, cx + 80, cy);
  sent = await pointers();
  assert.deepEqual(sent.filter(m => m.kind !== 'move').map(m => [m.kind, m.button]), [['down', 0], ['up', 0]]);
  assert.ok(sent.at(-1).x > sent.find(m => m.kind === 'down').x, 'dragged to the right');
  assert.equal(await p.evaluate(() => document.querySelector('#desktop .press').classList.contains('on')), false);
  assert.deepEqual(errors, []);
});

test('the desktop view carries the clipboard both ways', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    window.wrote = { text: [], item: [] };
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText: async text => { wrote.text.push(text); },
      write: async items => { for (const item of items) wrote.item.push(await (await item.getType('text/plain')).text()); },
    } });
    probe.handle({ type: 'config', desktop: true });
  });
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  assert.equal(await p.evaluate(() => document.activeElement.className), 'sink', 'focus can take a paste');
  const key = (type, code, extra = {}) => p.evaluate(([type, code, extra]) => {
    const command = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? { metaKey: true } : { ctrlKey: true };
    document.activeElement.dispatchEvent(new KeyboardEvent(type, { code, key: code.slice(3).toLowerCase(), bubbles: true, cancelable: true, ...command, ...extra }));
  }, [type, code, extra]);
  const desktopMessages = () => p.evaluate(() => messages.filter(m => m.type.startsWith('desktop_') && m.type !== 'desktop_start'));

  // Paste: the browser's clipboard goes over first, then the keys.
  await p.evaluate(() => { messages.length = 0; });
  await key('keydown', 'KeyV');
  assert.deepEqual(await desktopMessages(), [], 'nothing pressed before the clipboard is in');
  await p.evaluate(() => {
    const data = new DataTransfer();
    data.setData('text/plain', 'from the browser 中文');
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  });
  await key('keyup', 'KeyV');
  const pasted = await desktopMessages();
  assert.deepEqual(pasted.map(m => [m.type, m.text ?? m.code, m.down]), [
    ['desktop_clipboard', 'from the browser 中文', undefined],
    ['desktop_key', 'KeyV', true],
    ['desktop_key', 'KeyV', false],
  ]);
  assert.ok(pasted[1].meta && !pasted[1].ctrl, 'Command on the Mac');
  assert.equal(await p.evaluate(() => document.activeElement.value), '', 'nothing lands in the hidden field');

  // A paste the browser never delivers still presses the keys, with
  // whatever the Mac holds.
  await p.evaluate(() => { messages.length = 0; });
  await key('keydown', 'KeyV');
  await p.waitForTimeout(450);
  assert.deepEqual((await desktopMessages()).map(m => [m.type, m.code, m.down]), [['desktop_key', 'KeyV', true], ['desktop_key', 'KeyV', false]]);

  // Copy: the browser's clipboard gets what the Mac copied, through an
  // entry written during the key press, as Safari requires.
  await p.evaluate(() => { messages.length = 0; });
  await key('keydown', 'KeyC');
  assert.ok((await desktopMessages()).some(m => m.type === 'desktop_key' && m.code === 'KeyC' && m.down));
  await p.evaluate(() => probe.handle({ type: 'desktop_clipboard', text: 'from the Mac' }));
  await p.waitForFunction(() => wrote.item.includes('from the Mac'));
  assert.deepEqual(await p.evaluate(() => wrote.text), []);

  // A copy made on the Mac some other way reaches the browser too.
  await p.evaluate(() => probe.handle({ type: 'desktop_clipboard', text: 'from a menu' }));
  await p.waitForFunction(() => wrote.text.includes('from a menu'));

  // Back on the terminal, the Mac's clipboard no longer comes through.
  await windowed(p);
  await p.locator('#bar .tool.desktop').click();
  await p.evaluate(() => probe.handle({ type: 'desktop_clipboard', text: 'too late' }));
  await p.waitForTimeout(50);
  assert.equal(await p.evaluate(() => wrote.text.includes('too late') || wrote.item.includes('too late')), false);
  assert.deepEqual(errors, []);
});

test('the desktop view copies and pastes images in parts', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => {
    window.wrote = [];
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText: async text => { wrote.push(['text', text]); },
      write: async items => { for (const item of items) for (const type of item.types) wrote.push([type, [...new Uint8Array(await (await item.getType(type)).arrayBuffer())]]); },
    } });
    probe.handle({ type: 'config', desktop: true });
  });
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);

  // An image copied on the Mac comes in parts and goes to the browser as PNG.
  await p.evaluate(() => {
    const b64 = bytes => btoa(String.fromCharCode(...bytes));
    probe.handle({ type: 'desktop_clipboard_part', id: 'mac-1', part: 0, parts: 2, data: b64([137, 80, 78, 71]) });
    probe.handle({ type: 'desktop_clipboard_part', id: 'mac-1', part: 1, parts: 2, data: b64([1, 2, 3]) });
    probe.handle({ type: 'desktop_clipboard', image: 'mac-1' });
  });
  await p.waitForFunction(() => wrote.length === 1);
  assert.deepEqual(await p.evaluate(() => wrote), [['image/png', [137, 80, 78, 71, 1, 2, 3]]]);
  // One the Mac kept, too large, says so.
  await p.evaluate(() => probe.handle({ type: 'desktop_clipboard', too_large: 'image' }));
  assert.match(await p.locator('#toast').innerText(), /too large/);

  // An image pasted here goes to the Mac in parts before the keys.
  const size = 200 * 1024;
  await p.evaluate(() => { messages.length = 0; });
  await p.evaluate(size => {
    const command = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? { metaKey: true } : { ctrlKey: true };
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyV', key: 'v', bubbles: true, cancelable: true, ...command }));
    const bytes = new Uint8Array(size).map((_, i) => i % 251);
    const data = new DataTransfer();
    data.items.add(new File([bytes], 'shot.png', { type: 'image/png' }));
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  }, size);
  await p.waitForFunction(() => messages.some(m => m.type === 'desktop_key' && m.down === false));
  const sent = await p.evaluate(() => messages.filter(m => m.type.startsWith('desktop_clipboard') || m.type === 'desktop_key'));
  assert.deepEqual(sent.map(m => m.type), ['desktop_clipboard_part', 'desktop_clipboard_part', 'desktop_clipboard', 'desktop_key', 'desktop_key']);
  assert.ok(sent.slice(0, 2).every(m => m.data.length <= 256 * 1024), 'each part fits a relay message');
  const joined = await p.evaluate(parts => parts.flatMap(m => [...atob(m.data)].map(c => c.charCodeAt(0))), sent.slice(0, 2));
  assert.equal(joined.length, size);
  assert.ok(joined.every((b, i) => b === i % 251));
  assert.equal(sent[2].image, sent[0].id);
  assert.deepEqual(errors, []);
});

test('the desktop view dims its last picture while the link is down, and shows timings only with ?debug', async t => {
  for (const debug of [false, true]) {
    const { p, errors } = await page(t, (debug ? '?debug' : '') + '#t=fixture');
    await screen(p);
    await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
    await p.locator('#bar .tool.desktop').click();
    await showPicture(p);
    await p.waitForTimeout(1100);
    if (!debug) {
      assert.equal(await p.locator('#desktop .hud').count(), 0, 'no timings for everyone');
    } else {
      const hud = await p.locator('#desktop .hud').innerText();
      assert.match(hud, /320x200/);
      assert.match(hud, /fps \d+ shown/);
      assert.match(hud, /queue/);
    }
    // The link drops and stays down (this device went offline): the picture
    // dims under the page's notice, and input waits.
    await p.evaluate(() => { Object.defineProperty(navigator, 'onLine', { configurable: true, value: false }); connections.at(-1).close(); });
    await p.waitForFunction(() => document.body.classList.contains('offline'), null, { timeout: 3000 });
    const look = await p.evaluate(() => {
      const notice = document.getElementById('notice');
      const r = notice.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return {
        filter: getComputedStyle(document.querySelector('#desktop canvas')).filter,
        status: getComputedStyle(document.querySelector('#desktop .status')).opacity,
        title: notice.querySelector('.title').textContent,
        above: Number(getComputedStyle(notice).zIndex) > Number(getComputedStyle(document.getElementById('desktop')).zIndex),
      };
    });
    assert.deepEqual(look, { filter: 'brightness(0.45)', status: '0', title: look.title, above: true });
    assert.ok(look.title.length > 0, 'the notice says what is happening');
    const box = await p.locator('#desktop > canvas').boundingBox();
    await p.evaluate(() => { messages.length = 0; });
    await p.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'desktop_pointer')), []);
    assert.deepEqual(errors, []);
  }
});

test('the desktop view lets go of held keys, sends modifiers with clicks, and asks before the page closes', async t => {
  const { p, errors } = await page(t);
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  const tool = p.locator('#bar .tool.desktop');
  await tool.click();
  await showPicture(p);
  const box = await p.locator('#desktop canvas').boundingBox();

  // Shift-click selects a range in Finder; the click carries the key.
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.down('Shift');
  await p.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await p.keyboard.up('Shift');
  const down = await p.evaluate(() => messages.find(m => m.type === 'desktop_pointer' && m.kind === 'down'));
  assert.equal(down.shift, true, JSON.stringify(down));
  assert.equal('clicks' in down, false, 'the Mac counts clicks itself');

  // Leaving with a key still down releases it on the Mac first.
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.down('Alt');
  const keys = await p.evaluate(() => messages.filter(m => m.type === 'desktop_key').map(m => [m.code, m.down]));
  assert.deepEqual(keys, [['AltLeft', true]]);

  // Closing the page while the Mac's screen shows asks first. Headless
  // Chromium shows no beforeunload dialog even for a bare page, so this
  // reads the event the browser would ask on.
  const asks = () => p.evaluate(() => { const e = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(e); return e.defaultPrevented; });
  assert.equal(await asks(), true);
  await p.evaluate(() => { messages.length = 0; });
  await windowed(p);
  await tool.click();
  await p.keyboard.up('Alt');
  const after = await p.evaluate(() => messages.filter(m => ['desktop_key', 'desktop_stop'].includes(m.type)).map(m => m.type === 'desktop_key' ? [m.code, m.down] : m.type));
  assert.deepEqual(after, [['AltLeft', false], 'desktop_stop']);

  // Back on the terminal, the page closes without asking.
  assert.equal(await asks(), false);
  assert.deepEqual(errors, []);
});

test('a direct path takes Kaku\'s pieces and the page\'s messages once it opens', async t => {
  const { p, errors } = await page(t, '?p2p#t=fixture', { noConfig: true });
  // A stand-in peer connection: what the page offers, answers and sends is
  // the page's own logic; Rust and a real browser meet in p2p_probe.cjs.
  await p.evaluate(() => {
    // Messages through the socket run the page's freshness check, which
    // reloads this test page (its script carries the probe) mid-test.
    window.fetch = () => Promise.reject(new Error('offline in this test'));
    window.peer = { sent: [] };
    window.RTCPeerConnection = class {
      constructor() { peer.pc = this; this.iceGatheringState = 'complete'; }
      createDataChannel() { peer.dc = { readyState: 'connecting', send: (d) => peer.sent.push(d), close() {} }; return peer.dc; }
      async createOffer() { return { type: 'offer', sdp: 'v=0 offer' }; }
      async setLocalDescription(d) { this.localDescription = d; }
      async setRemoteDescription(d) { peer.answer = d; }
      addEventListener() {}
      close() {}
    };
    // A Kaku that does not say it answers gets no offer.
    connections[0].onmessage({ data: JSON.stringify({ type: 'config' }) });
  });
  await new Promise(r => setTimeout(r, 200));
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'rtc_offer')), false);
  await p.evaluate(() => connections[0].onmessage({ data: JSON.stringify({ type: 'config', direct: true }) }));
  await p.waitForFunction(() => messages.some(m => m.type === 'rtc_offer'));
  assert.equal(await p.evaluate(() => messages.find(m => m.type === 'rtc_offer').sdp), 'v=0 offer');
  await p.evaluate(() => connections[0].onmessage({ data: JSON.stringify({ type: 'rtc_answer', sdp: 'v=0 answer', id: 3 }) }));
  await p.waitForFunction(() => peer.answer);
  assert.deepEqual(await p.evaluate(() => peer.answer), { type: 'answer', sdp: 'v=0 answer' });

  // Before the channel opens, the page keeps using the session.
  await p.evaluate(() => { messages.length = 0; probe.attach({ pane_id: 4, tab_id: 1 }); });
  assert.deepEqual(await p.evaluate(() => [messages.map(m => m.type), peer.sent.length]), [['attach'], 0]);

  // Open: the page's last word on the session says so, and what Kaku sends
  // on the path waits for Kaku's own word that its lane moved.
  const detail = 'direct '.repeat(4000);
  await p.evaluate((detail) => {
    messages.length = 0;
    peer.dc.readyState = 'open';
    peer.dc.onopen();
    window.deliver = (flags, bytes) => {
      const cut = 16 * 1024 - 1;
      for (let at = 0; at < bytes.length; at += cut) {
        const part = bytes.subarray(at, at + cut);
        const piece = new Uint8Array(part.length + 1);
        piece[0] = flags | (at + cut >= bytes.length ? 1 : 0);
        piece.set(part, 1);
        peer.dc.onmessage({ data: piece.buffer });
      }
    };
    deliver(2, new TextEncoder().encode(JSON.stringify({ type: 'error', detail })));
  }, detail);
  assert.deepEqual(await p.evaluate(() => messages), [{ type: 'rtc_ready', id: 3 }]);
  await p.evaluate(() => connections[0].onmessage({ data: JSON.stringify({ type: 'error', detail: 'by the relay' }) }));
  assert.equal(await p.evaluate(() => document.getElementById('toast').textContent), 'by the relay');
  // Another path's word moves nothing.
  await p.evaluate(() => connections[0].onmessage({ data: JSON.stringify({ type: 'rtc_ready', lane: 'text', id: 2 }) }));
  assert.equal(await p.evaluate(() => document.getElementById('toast').textContent), 'by the relay');
  await p.evaluate(() => connections[0].onmessage({ data: JSON.stringify({ type: 'rtc_ready', lane: 'text', id: 3 }) }));
  // A message larger than one piece arrives whole.
  await p.waitForFunction((n) => document.getElementById('toast').textContent.length === n, detail.length);

  // Small messages now go direct.
  await p.evaluate(() => { messages.length = 0; probe.attach({ pane_id: 5, tab_id: 1 }); });
  assert.deepEqual(await p.evaluate(() => peer.sent.map(d => JSON.parse(d).type)), ['attach']);
  assert.deepEqual(await p.evaluate(() => messages.map(m => m.type)), []);
  // The heartbeat stays on the session, which the relay times out on
  // silence; the answer may come either way.
  await p.evaluate(() => { messages.length = 0; peer.sent.length = 0; document.dispatchEvent(new Event('visibilitychange')); });
  assert.deepEqual(await p.evaluate(() => [messages.map(m => m.type), peer.sent.map(d => JSON.parse(d).type).includes('ping')]), [['ping'], false]);
  await p.evaluate(() => deliver(2, new TextEncoder().encode(JSON.stringify({ type: 'pong' }))));

  // A path that stops answering leaves the page's messages to the session
  // until it answers again.
  await p.evaluate(() => {
    peer.pc.iceConnectionState = 'disconnected';
    peer.pc.oniceconnectionstatechange();
    messages.length = 0;
    peer.sent.length = 0;
    probe.attach({ pane_id: 5, tab_id: 1 });
  });
  assert.deepEqual(await p.evaluate(() => [messages.map(m => m.type), peer.sent.length]), [['attach'], 0]);
  await p.evaluate(() => {
    peer.pc.iceConnectionState = 'connected';
    peer.pc.oniceconnectionstatechange();
    messages.length = 0;
    probe.attach({ pane_id: 5, tab_id: 1 });
  });
  assert.deepEqual(await p.evaluate(() => [messages.length, peer.sent.length]), [0, 1]);

  // Kaku ends it: back to the session, the screen starts over, and a new
  // offer follows once the wait is up.
  await p.evaluate(() => {
    messages.length = 0;
    connections[0].onmessage({ data: JSON.stringify({ type: 'rtc_closed', id: 3 }) });
  });
  assert.deepEqual(await p.evaluate(() => messages.map(m => m.type)), ['hello', 'attach']);
  await p.evaluate(() => { messages.length = 0; probe.attach({ pane_id: 4, tab_id: 1 }); });
  assert.deepEqual(await p.evaluate(() => messages.map(m => m.type)), ['attach']);
  await p.waitForFunction(() => messages.some(m => m.type === 'rtc_offer'), null, { timeout: 5000 });

  // The browser gives up on the new one: Kaku is told by the session.
  await p.evaluate(() => connections[0].onmessage({ data: JSON.stringify({ type: 'rtc_answer', sdp: 'v=0 answer', id: 4 }) }));
  await p.evaluate(() => {
    peer.dc.readyState = 'open';
    peer.dc.onopen();
    messages.length = 0;
    peer.pc.iceConnectionState = 'failed';
    peer.pc.oniceconnectionstatechange();
  });
  assert.deepEqual(await p.evaluate(() => messages), [
    { type: 'rtc_close', id: 4 },
    { type: 'hello', frame_ack: true, delta: true },
    { type: 'attach', pane_id: 4 },
  ]);
  assert.deepEqual(errors, []);
});

// Control-Up and F11 belong to the computer in front of the page, so the
// switch key's modifiers with Up and Down open Mission Control and the
// desktop on the Mac, letting go of the modifiers already sent there.
test('Ctrl+Option+Cmd+Up and Down open Mission Control and the desktop on the Mac', async t => {
  const { p, errors } = await page(t, '#t=fixture', { platform: 'MacIntel' });
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').click();
  await showPicture(p);
  const sent = () => p.evaluate(() => messages.filter(m => m.type.startsWith('desktop_key') || m.type === 'desktop_mission_control')
    .map(m => m.type === 'desktop_key' ? `${m.code} ${m.down ? 'down' : 'up'}` : `mission ${m.show || 'windows'}`));
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.press('Control+Alt+Meta+ArrowUp');
  assert.deepEqual(await sent(), ['ControlLeft down', 'AltLeft down', 'MetaLeft down', 'ControlLeft up', 'AltLeft up', 'MetaLeft up', 'mission windows']);
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.press('Control+Alt+Meta+ArrowDown');
  assert.equal((await sent()).at(-1), 'mission desktop');
  // Control-Up alone still goes to the Mac as keys.
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.press('Control+ArrowUp');
  assert.deepEqual(await sent(), ['ControlLeft down', 'ArrowUp down', 'ArrowUp up', 'ControlLeft up']);
  assert.deepEqual(errors, []);
});

test('one key switches between the terminal and the desktop view, and the system keys fill the screen', async t => {
  const { p, errors } = await page(t, '#t=fixture', { platform: 'MacIntel' });
  await screen(p);
  await p.evaluate(() => probe.handle({ type: 'config', desktop: true }));
  await p.locator('#bar .tool.desktop').waitFor();
  const typed = () => p.evaluate(() => messages.filter(m => ['text', 'key', 'desktop_key'].includes(m.type)));

  // Ctrl+Cmd+F fills the screen from the terminal, and leaves it again.
  await p.keyboard.press('Control+Meta+KeyF');
  await p.waitForFunction(() => !!document.fullscreenElement, null, { timeout: 3000 });
  await p.keyboard.press('Control+Meta+KeyF');
  await p.waitForFunction(() => !document.fullscreenElement, null, { timeout: 3000 });

  // Ctrl+Option+Cmd+D goes to the Mac's screen and back, typing nothing;
  // the other platforms' Ctrl+Alt+Shift+D does nothing on a Mac.
  await p.keyboard.press('Control+Alt+Shift+KeyD');
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  await p.evaluate(() => { messages.length = 0; });
  await p.keyboard.press('Control+Alt+Meta+KeyD');
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), true);
  await p.waitForFunction(() => !!document.fullscreenElement, null, { timeout: 3000 });
  // There, Ctrl+Cmd+F is the Mac's, for its own windows.
  await p.keyboard.press('Control+Meta+KeyF');
  assert.equal(await p.evaluate(() => !!document.fullscreenElement), true);
  await p.keyboard.press('Control+Alt+Meta+KeyD');
  assert.equal(await p.evaluate(() => document.body.classList.contains('desktop')), false);
  assert.equal(await p.evaluate(() => messages.filter(m => m.type === 'desktop_stop').length), 1);
  assert.deepEqual((await typed()).filter(m => m.type !== 'desktop_key' || /D$/.test(m.code)), []);
  assert.deepEqual(errors, []);
});

test('mobile tab list switches, creates, renames and closes with recovery', async t => {
  const { p, errors } = await page(t, '#t=fixture', { viewport: { width: 320, height: 700 }, hasTouch: true, isMobile: true });
  await screen(p);
  const long = 'A very long tab title '.repeat(8);
  await p.evaluate(title => { messages.length = 0; probe.handle({ type: 'tabs', tabs: [
    { tab_id: 1, pane_id: 1, title: 'one', active: true },
    { tab_id: 2, pane_id: 2, title: title + ' •', confirm_close: true },
    { tab_id: 3, pane_id: 3, title: 'three', confirm_close: false },
  ] }); }, long);
  assert.equal(await p.locator('#mobile-switch .count').count(), 0);
  await p.locator('#mobile-switch').click();
  assert.equal(await p.locator('#mobile-head .count').textContent(), '3');
  assert.equal(await p.locator('#mobile-tabs.show .mobile-row').count(), 3);
  assert.equal(await p.locator('.mobile-select[aria-current=true]').count(), 1);
  await p.locator('.mobile-more').nth(1).click();
  assert.equal(await p.locator('#mobile-sheet-title').textContent(), long.trimEnd());
  await p.keyboard.press('Shift+Tab');
  assert.equal(await p.evaluate(() => document.activeElement.dataset.action), 'cancel');
  await p.keyboard.press('Escape');
  assert.equal(await p.evaluate(() => document.activeElement.className), 'mobile-more');
  await p.locator('.mobile-more').nth(1).click();
  await p.locator('#mobile-sheet [data-action=rename]').click();
  await p.locator('#mobile-sheet input').fill('renamed');
  await p.keyboard.press('Enter');
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'rename_tab')), [{ type: 'rename_tab', tab_id: 2, title: 'renamed' }]);
  await p.locator('.mobile-close').nth(1).click();
  await p.locator('#dialog [data-act=cancel]').click();
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'close_tab')), false);
  await p.locator('.mobile-close').nth(1).click();
  await p.locator('#dialog [data-act=ok]').click();
  assert.equal(await p.locator('.mobile-row').count(), 2);
  await p.waitForTimeout(3200);
  assert.equal(await p.locator('.mobile-row').count(), 3, 'rejected close restores authoritative tabs');
  await p.locator('.mobile-close').nth(2).click();
  assert.deepEqual(await p.evaluate(() => messages.filter(m => m.type === 'close_tab').map(m => m.tab_id)), [2, 3]);
  await p.locator('.mobile-select').nth(1).click();
  assert.equal(await p.locator('#mobile-tabs.show').count(), 0);
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'attach' && m.pane_id === 2)), true);
  await p.locator('#mobile-switch').click();
  await p.locator('#mobile-new').click();
  assert.equal(await p.evaluate(() => messages.some(m => m.type === 'new_tab')), true);
  assert.deepEqual(errors, []);
});

test('mobile tabs fit narrow and landscape layouts with either bar position', async t => {
  const { p, errors } = await page(t, '#t=fixture', { viewport: { width: 320, height: 568 }, hasTouch: true, isMobile: true });
  await screen(p);
  for (const [width, height, bottom] of [[320, 568, false], [844, 390, true]]) {
    await p.setViewportSize({ width, height });
    await p.evaluate(bottom => {
      probe.handle({ type: 'config', tab_bar_at_bottom: bottom });
      probe.handle({ type: 'tabs', tabs: Array.from({ length: 20 }, (_, i) => ({ tab_id: i + 1, pane_id: i + 1, title: `Tab ${i}`, active: i === 0 })) });
    }, bottom);
    await p.locator('#mobile-switch').click();
    const dimensions = await p.evaluate(() => {
      const list = document.querySelector('#mobile-list'), foot = document.querySelector('#mobile-foot').getBoundingClientRect();
      return { scrollable: list.scrollHeight > list.clientHeight, footBottom: foot.bottom, width: document.documentElement.scrollWidth, height: innerHeight };
    });
    assert.equal(dimensions.scrollable, true);
    assert.ok(dimensions.footBottom <= dimensions.height + 1);
    assert.ok(dimensions.width <= width);
    await p.keyboard.press('Escape');
    assert.equal(await p.locator('#mobile-tabs.show').count(), 0);
  }
  assert.deepEqual(errors, []);
});
