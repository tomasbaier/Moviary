// Screenshots for the README: a fictional demo library (made-up titles, people and posters,
// see demo-data.js and posters.js) shown in a TEST build of the app.
//
//   npm run screenshots            builds the test exe, then takes all screenshots
//   npm run screenshots -- --no-build
//   npm run screenshots -- --no-build show     only one screenshot (data and posters kept)
//
// The test build has its own identifier (cz.filmoteka.e2etest), so it keeps its data in
// %APPDATA%\cz.filmoteka.e2etest and never touches the real library. The demo video files are
// sparse (they take no disk space) in MOVIARY_DEMO_DIR, by default \Moviary-demo on the drive
// of the repository. Output: docs/screenshots/*.png.
const fs = require('fs');
const path = require('path');
const { spawn, execFileSync, execSync } = require('child_process');
const { G, movies, shows, collection, people, cast } = require('./demo-data');

const HERE = __dirname;
const REPO = path.resolve(HERE, '..', '..');
const TARGET = path.join(REPO, 'src-tauri', 'target-screenshots');
const EXE = path.join(TARGET, 'release', 'moviary.exe');
const DATA_DIR = path.join(process.env.APPDATA, 'cz.filmoteka.e2etest');
const ROOT = process.env.MOVIARY_DEMO_DIR || path.join(path.parse(REPO).root, 'Moviary-demo');
const MOV = path.join(ROOT, 'Filmy');
const SER = path.join(ROOT, 'Seriály');
const OUT = path.join(REPO, 'docs', 'screenshots');
const args = process.argv.slice(2);
const only = args.find((a) => !a.startsWith('--'));

if (!args.includes('--no-build')) {
  execSync(`npx tauri build --no-bundle --config "${path.join(HERE, 'test-config.json')}"`,
    { cwd: REPO, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: TARGET } });
}
const PORT = 9336;
const DAY = 86400;
const now = Math.floor(Date.now() / 1000);
const GB = 1024 ** 3;

if (!DATA_DIR.includes('e2etest')) throw new Error('guard');
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ymd = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
const inDays = (n) => { const d = new Date(); d.setDate(d.getDate() + n); return ymd(d); };
const pad = (n) => String(n).padStart(2, '0');

// Pseudo-random but stable.
let seed = 42;
const rnd = () => { seed = (seed * 16807) % 2147483647; return seed / 2147483647; };

function sparse(file, size) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, Buffer.from(Array.from({ length: 4096 }, () => Math.floor(rnd() * 256))));
  execFileSync('fsutil', ['sparse', 'setflag', file], { stdio: 'ignore' });
  execFileSync('fsutil', ['file', 'seteof', file, String(size)], { stdio: 'ignore' });
}

const movieSize = (m, i) => Math.round((m.uhd ? 14 : 3) * GB + ((i * 7919) % 50) / 10 * GB);
movies.forEach((m, i) => { m.uhd = [1, 2, 3, 7].includes(i); });

function buildFiles() {
  if (fs.existsSync(ROOT)) return;
  movies.forEach((m, i) => sparse(path.join(MOV, m.file), movieSize(m, i)));
  for (const s of shows) for (const [season, n] of Object.entries(s.have)) for (let e = 1; e <= n; e++) {
    sparse(path.join(SER, s.name, `Season ${season}`, `${s.name} S${pad(season)}E${pad(e)}.mkv`), Math.round(1.1 * GB + rnd() * 0.6 * GB));
  }
}

function launch() {
  return spawn(EXE, [], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` }, stdio: 'ignore' });
}

async function connect() {
  let target;
  for (let i = 0; i < 60 && !target; i++) {
    await sleep(500);
    try { target = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === 'page'); } catch {}
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let seq = 0;
  const pending = new Map();
  ws.onmessage = (e) => { const m = JSON.parse(e.data); pending.get(m.id)?.(m); pending.delete(m.id); };
  const send = (method, params = {}) => new Promise((r) => { const id = ++seq; pending.set(id, r); ws.send(JSON.stringify({ id, method, params })); });
  const ev = async (expr) => {
    const m = await send('Runtime.evaluate', { expression: `(async () => { ${expr} })()`, awaitPromise: true, returnByValue: true });
    if (m.result?.exceptionDetails) throw new Error(JSON.stringify(m.result.exceptionDetails).slice(0, 800));
    return m.result?.result?.value;
  };
  for (let i = 0; i < 40; i++) { if (await ev(`return !!window.__TAURI__ && document.readyState === 'complete'`)) break; await sleep(250); }
  await sleep(1500);
  const settings = await ev(`return await window.__TAURI__.core.invoke('get_settings')`);
  if (!settings.dataPath.includes('e2etest')) throw new Error('GUARD: wrong data path ' + settings.dataPath);
  return { ws, send, ev };
}

const meta = (x, tv) => ({
  tmdbId: x.id, title: x.cs, titleEn: x.en, originalTitle: x.cs, year: x.year, overview: x.ov, overviewEn: x.ovEn,
  genres: x.g.map((k) => G[k][0]), genresEn: x.g.map((k) => G[k][1]), runtime: x.rt || 0, rating: x.r,
  director: people(x.dir).map((p) => p.name).join(', '), directors: people(x.dir), cast: cast(x.cast, x.roles),
  poster: `${tv ? 'tmdb-tv' : 'tmdb'}-${x.id}.jpg`, bilingual: true, version: 2, fetched: now,
  collection: x.collection ? { id: collection.id, name: collection.name, nameEn: collection.nameEn } : null, similar: x.similar || [],
});

const audio = (langs, ch) => langs.map((l, i) => ({ codec: i ? 'AAC' : ch > 2 ? 'E-AC-3' : 'AAC', language: l, channels: i ? 2 : ch, name: '', default: i === 0 }));
const media = (uhd, dur) => ({
  container: 'mkv', duration: dur,
  video: uhd ? { codec: 'HEVC', width: 3840, height: 2160, hdr: 'HDR10', bitDepth: 10 } : { codec: 'H.264', width: 1920, height: 1080, hdr: null, bitDepth: 8 },
  audio: audio(['cs', 'en'], 6), subtitles: [{ codec: 'SRT', language: 'cs', name: '', default: false }, { codec: 'SRT', language: 'en', name: '', default: false }],
});

// Personal data and state of the demo movies (by Czech title).
const STATE = {
  'Poslední maják': { cats: ['Drama', 'Oblíbené'], watched: 40, rating: 5, note: 'Konec mě dostal. Pustit Petrovi.' },
  'Hvězdný přístav': { cats: ['Sci-fi', 'Oblíbené'], watched: 120, rating: 5 },
  'Hvězdný přístav: Bouře': { cats: ['Sci-fi', 'Akční'], pos: 0.36, played: 1, wish: true, note: 'Dokoukat s Ondrou.' },
  'Neonové město': { cats: ['Sci-fi'], added: 2, wish: true },
  'Sklepní orchestr': { cats: ['Komedie', 'Na víkend'], watched: 70, rating: 4 },
  'Stíny nad Vltavou': { cats: ['Krimi'], pos: 0.62, played: 3 },
  'Tajemství kartografa': { cats: ['Dobrodružné', 'Na víkend'], wish: true },
  'Hora mlčení': { cats: ['Drama', 'Dobrodružné', 'Oblíbené'], watched: 160, rating: 5 },
  'Kočka v okně': { cats: ['S dětmi', 'Komedie'], watched: 30, rating: 4 },
  Ozvěny: { cats: [] },
  'Jarní sonáta': { cats: ['Drama'] },
  'Pouštní vítr': { cats: ['Dobrodružné'], wish: true },
  'Noční vlak do Terstu': { cats: ['Krimi', 'Na víkend'], added: 5 },
  'Dědečkův deník': { cats: ['S dětmi', 'Komedie'], watched: 200, rating: 4 },
  'Pod hladinou': { cats: ['Dokumenty'], added: 9, wish: true },
  'Velká loupež na Smíchově': { cats: ['Komedie', 'Krimi'], watched: 95, rating: 3 },
  'Zimní zahrada': { cats: ['Drama'] },
};
const SHOW_STATE = {
  'Stanice Kepler': { cats: ['Sci-fi', 'Oblíbené'], rating: 5, played: 0.5 },
  'Malé město': { cats: ['Komedie'], rating: 4 },
  'Případy inspektorky Novotné': { cats: ['Krimi'], wish: true },
};
movies.forEach((m) => { m.similar = movies.filter((o) => o !== m && o.g.some((g) => m.g.includes(g))).slice(0, 6).map((o) => o.id); });
shows.forEach((s) => { s.similar = shows.filter((o) => o !== s).map((o) => o.id); });

function enrich(db) {
  const base = now - 240 * DAY;
  const history = [];
  const play = (id, at, seconds, finished) => history.push({ id: +id, at: Math.round(at), seconds, finished });
  for (const [id, r] of Object.entries(db.records)) {
    const file = r.relPath.split('/').pop();
    const m = movies.find((x) => x.file === file);
    r.metaChecked = true; r.thumbChecked = true; r.mediaProbe = 1; r.fingerprint = r.fingerprint || '';
    if (m) {
      const st = STATE[m.cs];
      const dur = m.rt * 60;
      r.meta = meta(m, false);
      r.media = media(m.uhd, dur);
      r.duration = dur;
      r.categories = st.cats;
      r.added = st.added ? now - st.added * DAY : base + Math.floor(rnd() * 3 * DAY);
      if (st.watched) { r.watched = true; r.lastPlayed = now - st.watched * DAY; play(id, r.lastPlayed, dur, true); }
      if (st.pos) { r.position = Math.round(dur * st.pos); r.lastPlayed = now - st.played * DAY; play(id, r.lastPlayed, r.position, false); }
      r.personal = { wishlist: !!st.wish, rating: st.rating || 0, note: st.note || '' };
      continue;
    }
    const s = shows.find((x) => r.relPath.startsWith(x.name + '/'));
    const [, se, ep] = file.match(/S(\d+)E(\d+)/).map(Number);
    const dur = 47 * 60 + Math.floor(rnd() * 600);
    r.duration = dur; r.media = media(false, dur); r.added = base + 5 * DAY + se * 20 * DAY;
    if (s.name === 'Stanice Kepler' && se === 2) r.added = now - 30 * DAY + ep * 2 * DAY;
    if ((s.watched[se] || 0) >= ep) {
      r.watched = true;
      const order = Object.entries(s.watched).reduce((n, [ss, c]) => n + (+ss < se ? c : 0), 0) + ep;
      const total = Object.values(s.watched).reduce((a, b) => a + b, 0);
      r.lastPlayed = now - Math.round((total - order) * (s.name === 'Malé město' ? 6 : 2.5) * DAY + (s.name === 'Malé město' ? 30 * DAY : DAY));
      play(id, r.lastPlayed, dur, true);
    }
  }
  for (const [, sh] of Object.entries(db.shows)) {
    const s = shows.find((x) => x.name === sh.name);
    if (!s) throw new Error('unknown show ' + sh.name);
    const st = SHOW_STATE[s.cs];
    sh.meta = meta(s, true); sh.metaChecked = true; sh.thumbChecked = true; sh.categories = st.cats; sh.year = s.year;
    sh.seasons = s.seasons.map(([n, c]) => ({ number: n, name: `Řada ${n}`, episodeCount: c }));
    let k = 0;
    sh.episodes = s.seasons.flatMap(([n, c]) => Array.from({ length: c }, (_, i) => ({ season: n, episode: i + 1, title: s.eps[k], titleEn: s.epsEn[k++], overview: '', overviewEn: '', runtime: 48 })));
    sh.status = s.status;
    sh.nextEpisode = s.next ? { season: s.next.season, episode: s.next.episode, name: s.next.name, nameEn: s.next.nameEn, airDate: inDays(s.next.in) } : null;
    sh.personal = { wishlist: !!st.wish, rating: st.rating || 0, note: '' };
    sh.added = Math.min(...Object.values(db.records).filter((r) => r.relPath.startsWith(s.name + '/')).map((r) => r.added));
  }
  // Extra history for the statistics: rewatches spread over the months.
  const ids = Object.keys(db.records).filter((id) => db.records[id].meta);
  for (let i = 0; i < 46; i++) {
    const id = ids[Math.floor(rnd() * ids.length)];
    play(id, now - (10 + rnd() * 220) * DAY, db.records[id].duration * (0.3 + rnd() * 0.7), rnd() > 0.4);
  }
  history.sort((a, b) => a.at - b.at);
  db.history = history;
  db.categories = ['Oblíbené', 'Na víkend', 'S dětmi', 'Akční', 'Dobrodružné', 'Dokumenty', 'Drama', 'Komedie', 'Krimi', 'Sci-fi'];
  db.collections = { [collection.id]: { name: collection.name, nameEn: collection.nameEn, parts: collection.parts, fetched: now } };
  const f = (view, status, cats) => ({ view, search: '', status, libraryFilter: null, selectedCats: cats, catMode: 'any', uncategorized: false, tech: {}, focus: null, sort: 'title' });
  db.savedFilters = [
    { name: 'S dětmi', filter: f('movies', 'all', ['S dětmi']) },
    { name: 'Krimi na večer', filter: f('movies', 'unwatched', ['Krimi']) },
    { name: 'Chci vidět', filter: f('movies', 'wishlist', []) },
  ];
  db.settings = { ...db.settings, tmdbKey: 'demo', autoMetadata: false, videoThumbnails: false, watchFolders: false, checkUpdates: false, language: 'cs' };
  return db;
}

(async () => {
  buildFiles();
  const dbFile = path.join(DATA_DIR, 'library.json');
  let app, conn;
  try {
    // Phase 1: let the app scan the demo folders, so records and shows have its own keys.
    if (!only) {
      fs.rmSync(DATA_DIR, { recursive: true, force: true });
      fs.mkdirSync(DATA_DIR, { recursive: true });
      fs.writeFileSync(dbFile, JSON.stringify({ version: 2, nextId: 1, libraries: [{ id: 1, path: MOV }, { id: 2, path: SER }],
        settings: { tmdbKey: 'demo', autoMetadata: false, genresAsCategories: false, videoThumbnails: false, language: 'cs', watchFolders: false, checkUpdates: false },
        categories: [], records: {}, shows: {}, history: [], collections: {} }));
      app = launch();
      conn = await connect();
      await conn.ev(`return await window.__TAURI__.core.invoke('scan')`);
      await sleep(4000);
      conn.ws.close(); app.kill(); await sleep(1500);
      const db = JSON.parse(fs.readFileSync(dbFile, 'utf8'));
      console.log('scanned', Object.keys(db.records).length, 'records,', Object.keys(db.shows).length, 'shows');
      fs.writeFileSync(dbFile, JSON.stringify(enrich(db)));
    }

    // Phase 2: posters, then the screenshots.
    app = launch();
    conn = await connect();
    const { send, ev } = conn;
    await ev(`localStorage.clear(); return 1`);
    await send('Page.reload', { ignoreCache: true });
    await sleep(3000);
    const postersDir = path.join(DATA_DIR, 'posters');
    if (!fs.existsSync(path.join(postersDir, 'tmdb-7001.jpg')) ) {
      await ev(fs.readFileSync(path.join(HERE, 'posters.js'), 'utf8') + '; return 1');
      const list = [...movies.map((m) => ({ file: `tmdb-${m.id}.jpg`, m, tv: false })), ...shows.map((s) => ({ file: `tmdb-tv-${s.id}.jpg`, m: s, tv: true }))]
        .map(({ file, m, tv }, i) => ({ file, title: m.cs, ...m.poster, seed: 1000 + i * 37, credits: `${tv ? 'Seriál' : 'Film'} · ${people(m.dir)[0].name} · ${m.year}` }));
      const imgs = await ev(`return await window.__makePosters(${JSON.stringify(list)})`);
      fs.mkdirSync(postersDir, { recursive: true });
      for (const [f, b64] of Object.entries(imgs)) fs.writeFileSync(path.join(postersDir, f), Buffer.from(b64, 'base64'));
      await send('Page.reload', { ignoreCache: true });
      await sleep(3000);
    }

    const W = 1440, H = 900;
    const size = (w, h, scale = 1) => send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: scale, mobile: false });
    const shot = async (name) => {
      if (only && only !== name) return;
      await sleep(900);
      const m = await send('Page.captureScreenshot', { format: 'png' });
      fs.writeFileSync(path.join(OUT, name + '.png'), Buffer.from(m.result.data, 'base64'));
      console.log('shot', name);
    };
    const reset = `clearFilters?.(); state.detailId = null; state.showPage = null; document.querySelectorAll('.modal-backdrop, .modal').forEach(m => m.remove()); `;
    await size(W, H);
    await ev(`applyData(await window.__TAURI__.core.invoke('get_data')); switchView('movies'); render(); return 1`);

    await shot('home');

    await ev(`openDetail(movieItems.find(m => m.title === 'Hvězdný přístav: Bouře').id); return 1`);
    await shot('detail');
    await ev(`const d = document.querySelector('#drawer'); const sc = [d, ...d.querySelectorAll('*')].find(e => e.scrollHeight > e.clientHeight + 50 && getComputedStyle(e).overflowY !== 'visible'); const h = [...d.querySelectorAll('h3')].find(h => /Herci|Obsazení|Série|Kolekce/i.test(h.textContent)); if (sc && h) sc.scrollTop = h.offsetTop - 30; return [...d.querySelectorAll('h3')].map(h => h.textContent).join('|')`).then((x) => console.log('drawer sections', x));
    await shot('detail-more');

    await ev(`state.detailId = null; render(); switchView('shows'); openShowPage(showItems.find(s => s.title === 'Stanice Kepler').id); return 1`);
    await shot('show');
    await ev(`const p = document.querySelector('#show-page'); const t = p.querySelector('.season-tabs'); p.scrollTop = t.getBoundingClientRect().top - p.getBoundingClientRect().top + p.scrollTop - 24; return 1`);
    await shot('show-episodes');

    await ev(`state.showId = null; switchView('movies'); state.selectedCats = new Set(['Drama', 'Sci-fi']); render(); return 1`);
    await shot('filters');

    await ev(`clearFilters(); render(); await openStats(); return 1`);
    await shot('stats');

    await ev(`document.querySelectorAll('.modal-backdrop').forEach(m => m.remove()); return 1`);
    await size(390, 844, 2);
    await ev(`switchView('movies'); clearFilters(); render(); window.scrollTo(0, 0); return 1`);
    await shot('mobile');
    await ev(`openDetail(movieItems.find(m => m.title === 'Poslední maják').id); return 1`);
    await shot('mobile-detail');
  } catch (err) {
    console.log('ERROR', err.stack || err);
    process.exitCode = 1;
  } finally {
    try { conn?.ws.close(); } catch {}
    app?.kill();
    process.exit(process.exitCode || 0);
  }
})();
