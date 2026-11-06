'use strict';

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialog = window.__TAURI__.dialog;

const $ = (sel) => document.querySelector(sel);

const IS_ANDROID = /Android/i.test(navigator.userAgent);
// Phone-sized window: filters in a slide-in panel, detail over the whole screen.
const compactQuery = window.matchMedia('(max-width: 760px)');

const PREFS_KEY = 'filmoteka.prefs';

const state = {
  libraries: [],
  categories: [],
  // Movies and episodes (episodes have `showId`).
  movies: [],
  shows: [],
  // Movie series of the shown movies.
  collections: [],
  missing: 0,
  settings: null,
  // 'movies' or 'shows'
  view: 'movies',
  // Show whose page is open, and its selected season.
  showId: null,
  season: null,
  search: '',
  status: 'all',
  libraryFilter: null,
  selectedCats: new Set(),
  // Technical filters: group id -> selected values.
  tech: {},
  catMode: 'any',
  uncategorized: false,
  sort: 'title',
  selection: new Set(),
  detailId: null,
  job: null,
  // Filter panel on small screens.
  filtersOpen: false,
  // Collapsed sections of the filter panel: 'status', 'libraries', 'cats', 'tech'.
  collapsed: new Set(),
  // Movies and shows of a person or a movie series: { kind: 'person' | 'collection', id, name }.
  focus: null,
  // Rows above the grid (continue watching, new, unwatched favourites).
  homeRows: true,
  // Named filter combinations: [{ name, filter }] (see currentFilter).
  savedFilters: [],
};

// Movies (without episodes) and shows with their episodes; rebuilt on every render.
let movieItems = [];
let showItems = [];
// When the first video was added (the first scan); later ones can be "new".
let firstAdded = null;
// Items currently shown in the grid and categories in display order;
// DOM elements refer to them by index.
let visible = [];
let sortedCats = [];
let lastClickedIndex = null;
// Re-renders the settings dialog while it is open.
let refreshSettings = null;
// Closes the open modal (only one at a time).
let closeCurrentModal = null;

const VIEWS = {
  movies: { label: 'Filmy', all: 'Všechny filmy', noun: ['film', 'filmy', 'filmů'] },
  shows: { label: 'Seriály', all: 'Všechny seriály', noun: ['seriál', 'seriály', 'seriálů'] },
};

// Tests work for movies and for shows (see showItem).
const STATUS_FILTERS = [
  { id: 'all', label: () => VIEWS[state.view].all, color: 'var(--text-3)', test: () => true },
  { id: 'progress', label: 'Rozkoukané', color: 'var(--accent)', test: (m) => (m.isShow ? m.inProgress : m.position > 0) },
  {
    id: 'unwatched', label: 'Neviděné', color: 'var(--live)',
    test: (m) => (m.isShow ? !m.watchedCount && !m.inProgress : !m.watched && m.position <= 0),
  },
  { id: 'wishlist', label: 'Chci vidět', color: 'var(--wish)', test: (m) => m.wishlist && !m.watched },
  { id: 'watched', label: 'Zhlédnuté', color: 'var(--ok)', test: (m) => m.watched },
  { id: 'offline', label: 'Nedostupné', color: '#ef4444', test: (m) => !m.available, hideEmpty: true },
];

// Recreated when the language changes (see applyLanguage).
let collator = new Intl.Collator('cs', { sensitivity: 'base', numeric: true });
const byTitle = (a, b) => collator.compare(a.title, b.title);
const SORTS = {
  title: byTitle,
  year: (a, b) => (b.year || 0) - (a.year || 0) || byTitle(a, b),
  rating: (a, b) => (b.rating || 0) - (a.rating || 0) || byTitle(a, b),
  mine: (a, b) => (b.myRating || 0) - (a.myRating || 0) || (b.rating || 0) - (a.rating || 0) || byTitle(a, b),
  recent: (a, b) => (b.lastPlayed || 0) - (a.lastPlayed || 0) || byTitle(a, b),
  added: (a, b) => b.added - a.added || byTitle(a, b),
  size: (a, b) => b.size - a.size || byTitle(a, b),
  folder: (a, b) => collator.compare(a.folder, b.folder) || byTitle(a, b),
};

const ICONS = {
  play: '<svg viewBox="0 0 24 24"><path d="M7 4.5v15l13-7.5z"/></svg>',
  check: '<svg viewBox="0 0 24 24"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>',
  edit: '<svg viewBox="0 0 24 24"><path d="M4 20h4L19 9l-4-4L4 16z"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>',
  close: '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  folder: '<svg viewBox="0 0 24 24"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5h4.6l2 2h8.4A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z"/></svg>',
  film: '<svg viewBox="0 0 24 24"><rect x="3" y="3" width="18" height="18" rx="3"/><path d="M7 3v18M17 3v18M3 8h4M3 12h4M3 16h4M17 8h4M17 12h4M17 16h4"/></svg>',
  search: '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>',
  restart: '<svg viewBox="0 0 24 24"><path d="M4 12a8 8 0 1 0 2.3-5.7M4 5v5h5"/></svg>',
  plug: '<svg viewBox="0 0 24 24"><path d="M9 2v6M15 2v6M6 8h12v3a6 6 0 0 1-12 0zM12 17v5"/></svg>',
  back: '<svg viewBox="0 0 24 24"><path d="M15 5l-7 7 7 7"/></svg>',
  tv: '<svg viewBox="0 0 24 24"><rect x="3" y="6" width="18" height="13" rx="2"/><path d="m8 2 4 4 4-4"/></svg>',
  left: '<svg viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6"/></svg>',
};

/** Name of a language code in the app's language ("cs" -> "čeština"); unknown codes in capitals. */
function langName(code) {
  const known = state.settings?.languages?.find(([c]) => c === code);
  return known ? known[1] : String(code).toUpperCase();
}

const TECH_LABELS = { surround: 'Prostorový (5.1+)', any: 'S titulky', none: 'Bez titulků' };

// Groups of the technical filters; values come from the tags of a movie (see media.rs).
const TECH_GROUPS = [
  { id: 'res', label: 'Rozlišení', order: ['4K', '1080p', '720p', 'SD'], values: (t) => (t.resolution ? [t.resolution] : []) },
  { id: 'hdr', label: 'Obraz', values: (t) => (t.hdr ? [t.hdr] : []) },
  { id: 'codec', label: 'Kodek videa', values: (t) => (t.videoCodec ? [t.videoCodec] : []) },
  { id: 'audio', label: 'Jazyk zvuku', values: (t) => [...t.audioLanguages, ...(t.channels >= 6 ? ['surround'] : [])] },
  { id: 'subs', label: 'Titulky', values: (t) => (t.hasSubtitles ? [...t.subtitleLanguages, 'any'] : ['none']) },
];

const techLabel = (group, value) =>
  (TECH_LABELS[value] && t(TECH_LABELS[value])) || (['audio', 'subs'].includes(group) ? langName(value) : value);

/** Values of a technical group for a movie, or for all episodes of a show. */
function techValues(m, group) {
  const g = TECH_GROUPS.find((x) => x.id === group);
  if (!m.isShow) return g.values(m.tags);
  const all = new Set();
  for (const e of m.episodes) for (const v of g.values(e.tags)) all.add(v);
  return [...all];
}

/** "4K HDR" for a card. */
function techBadge(m) {
  const t = m.isShow ? null : m.tags;
  if (!t) return '';
  const hdr = t.hdr === 'Dolby Vision' ? 'DV' : t.hdr ? 'HDR' : '';
  return [t.resolution, hdr].filter(Boolean).join(' ');
}

// ---------- helpers ----------

function esc(value) {
  return String(value ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function normalize(text) {
  return text.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase();
}

function fmtTime(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

function fmtRuntime(minutes) {
  const h = Math.floor(minutes / 60);
  return h ? `${h} h ${minutes % 60} min` : `${minutes} min`;
}

function fmtSize(bytes) {
  if (bytes >= 1024 ** 3) return `${num(bytes / 1024 ** 3, 1)} GB`;
  return `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;
}

function fmtDate(unix) {
  return new Date(unix * 1000).toLocaleString(locale(), { day: 'numeric', month: 'numeric', year: 'numeric', hour: '2-digit', minute: '2-digit' });
}

/** Noun form for a count, given by its Czech forms (1, 2–4, 5+); English from the dictionary. */
function plural(n, one, few, many) {
  if (LANG === 'en') return pluralEn(n, one) ?? t(n === 1 ? one : many);
  if (n === 1) return one;
  return n >= 2 && n <= 4 ? few : many;
}

function hue(text) {
  let h = 0;
  for (const ch of text) h = (h * 31 + ch.codePointAt(0)) >>> 0;
  return h % 360;
}

function percent(m) {
  return m.duration > 0 && m.position > 0 ? Math.min(100, (m.position / m.duration) * 100) : 0;
}

const canResume = (m) => m.position >= 10;
const posterUrl = (name) => convertFileSrc(name, 'poster');
const movieById = (id) => state.movies.find((m) => m.id === id);
const showById = (id) => showItems.find((s) => s.id === id);
const viewItems = () => (state.view === 'shows' ? showItems : movieItems);
const nounFor = (n) => plural(n, ...VIEWS[state.view].noun);

const pad2 = (n) => String(n).padStart(2, '0');
const epCode = (e) => `S${pad2(e.season)}E${pad2(e.episode)}${e.lastEpisode ? `–E${pad2(e.lastEpisode)}` : ''}`;
const seasonName = (n) => (n === 0 ? t('Speciály') : t('{n}. sezóna', { n }));
// Specials after the regular seasons.
const episodeOrder = (a, b) =>
  (a.season === 0) - (b.season === 0) || a.season - b.season || a.episode - b.episode || collator.compare(a.fileName, b.fileName);

/** A show with its episodes grouped into seasons and the fields movies have (for filters and sorting). */
function showItem(show, episodes) {
  const seasons = [];
  for (const e of episodes) {
    let season = seasons.find((s) => s.number === e.season);
    if (!season) {
      const meta = show.seasons.find((s) => s.number === e.season);
      season = { number: e.season, total: meta?.episodeCount || 0, episodes: [] };
      seasons.push(season);
    }
    season.episodes.push(e);
  }
  const watchedCount = episodes.filter((e) => e.watched).length;
  return {
    ...show,
    isShow: true,
    episodes,
    seasons,
    episodeCount: episodes.length,
    watchedCount,
    watched: watchedCount === episodes.length,
    inProgress: episodes.some((e) => e.position > 0) || (watchedCount > 0 && watchedCount < episodes.length),
    available: episodes.some((e) => e.available),
    playing: episodes.some((e) => e.playing),
    size: episodes.reduce((sum, e) => sum + e.size, 0),
    added: Math.max(show.added, ...episodes.map((e) => e.added)),
    lastPlayed: Math.max(0, ...episodes.map((e) => e.lastPlayed || 0)) || null,
    folder: episodes[0].folder,
    libraryIds: new Set(episodes.map((e) => e.libraryId)),
    director: show.creators,
  };
}

function deriveItems() {
  firstAdded = state.movies.reduce((min, m) => (min === null || m.added < min ? m.added : min), null);
  const byShow = new Map();
  movieItems = [];
  for (const m of state.movies) {
    if (m.showId == null) movieItems.push(m);
    else if (byShow.has(m.showId)) byShow.get(m.showId).push(m);
    else byShow.set(m.showId, [m]);
  }
  showItems = state.shows
    .filter((s) => byShow.has(s.id))
    .map((s) => showItem(s, byShow.get(s.id).sort(episodeOrder)));
}

/**
 * Episode to continue with: the last played one if unfinished, otherwise the next unwatched
 * one after it. Specials only count when the show has nothing else. Null when all is watched.
 */
function nextEpisode(show) {
  const regular = show.episodes.filter((e) => e.season !== 0);
  const list = regular.length ? regular : show.episodes;
  const last = list.reduce((best, e) => ((e.lastPlayed || 0) > (best?.lastPlayed || 0) ? e : best), null);
  if (last && !last.watched) return last;
  const from = last ? list.indexOf(last) + 1 : 0;
  return list.slice(from).find((e) => !e.watched) || list.find((e) => !e.watched) || null;
}

/** Percent of watched episodes of a show, or of the playback of a movie. */
function itemPercent(m) {
  return m.isShow ? (m.watchedCount / m.episodeCount) * 100 : percent(m);
}

function toast(message, kind = 'info') {
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.textContent = message;
  $('#toasts').append(el);
  setTimeout(() => el.remove(), kind === 'error' ? 7000 : 3500);
}

/** Invokes a backend command; shows errors as a toast and returns undefined on failure. */
async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (err) {
    toast(String(err), 'error');
    return undefined;
  }
}

async function withLoading(text, fn) {
  $('#loading-text').textContent = text;
  $('#loading').classList.remove('hidden');
  try {
    return await fn();
  } finally {
    $('#loading').classList.add('hidden');
  }
}

function loadPrefs() {
  try {
    const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
    if (STATUS_FILTERS.some((f) => f.id === p.status)) state.status = p.status;
    if (SORTS[p.sort]) state.sort = p.sort;
    if (p.catMode === 'all' || p.catMode === 'any') state.catMode = p.catMode;
    if (Array.isArray(p.selectedCats)) state.selectedCats = new Set(p.selectedCats);
    if (typeof p.libraryFilter === 'number') state.libraryFilter = p.libraryFilter;
    if (VIEWS[p.view]) state.view = p.view;
    if (p.tech && typeof p.tech === 'object') {
      for (const g of TECH_GROUPS) if (Array.isArray(p.tech[g.id]) && p.tech[g.id].length) state.tech[g.id] = new Set(p.tech[g.id]);
    }
    state.uncategorized = !!p.uncategorized;
    if (Array.isArray(p.collapsed)) state.collapsed = new Set(p.collapsed);
    if (typeof p.homeRows === 'boolean') state.homeRows = p.homeRows;
    else if (p.techOpen === false) state.collapsed.add('tech');
  } catch {
    /* ignore broken prefs */
  }
}

function savePrefs() {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({
      status: state.status,
      sort: state.sort,
      catMode: state.catMode,
      selectedCats: [...state.selectedCats],
      uncategorized: state.uncategorized,
      libraryFilter: state.libraryFilter,
      view: state.view,
      tech: Object.fromEntries(Object.entries(state.tech).map(([k, v]) => [k, [...v]])),
      collapsed: [...state.collapsed],
      homeRows: state.homeRows,
    }));
  } catch {
    /* storage unavailable */
  }
}

// ---------- data ----------

function applyData(data) {
  state.libraries = data.libraries;
  state.categories = data.categories;
  state.movies = data.movies;
  state.shows = data.shows;
  state.missing = data.missing;
  state.collections = data.collections || [];
  state.savedFilters = data.savedFilters || [];
  const cats = new Set(data.categories);
  state.selectedCats = new Set([...state.selectedCats].filter((c) => cats.has(c)));
  const ids = new Set([...data.movies, ...data.shows].map((m) => m.id));
  state.selection = new Set([...state.selection].filter((id) => ids.has(id)));
  if (state.detailId !== null && !ids.has(state.detailId)) state.detailId = null;
  if (state.libraryFilter !== null && !data.libraries.some((l) => l.id === state.libraryFilter)) state.libraryFilter = null;
  render();
  refreshSettings?.();
}

function updateMovie(update) {
  const m = movieById(update.id);
  if (!m) return;
  Object.assign(m, update);
  render();
}

function updateShow(update) {
  const s = state.shows.find((x) => x.id === update.id);
  if (!s) return;
  Object.assign(s, update);
  render();
}

function findCategory(name) {
  const n = name.trim().toLowerCase();
  return state.categories.find((c) => c.toLowerCase() === n);
}

/** The current filters as a plain object – what a saved filter stores. */
function currentFilter() {
  const tech = Object.entries(state.tech)
    .filter(([, values]) => values.size)
    .map(([group, values]) => [group, [...values].sort()])
    .sort(([a], [b]) => a.localeCompare(b));
  return {
    view: state.view,
    search: state.search.trim(),
    status: state.status,
    libraryFilter: state.libraryFilter,
    selectedCats: [...state.selectedCats].sort(),
    catMode: state.catMode,
    uncategorized: state.uncategorized,
    tech: Object.fromEntries(tech),
    focus: state.focus,
    sort: state.sort,
  };
}

/** Same filters (the sort order does not matter, nor how categories combine without any). */
function sameFilter(a, b) {
  const key = (f) => JSON.stringify([f.view, f.search || '', f.status || 'all', f.libraryFilter ?? null, f.selectedCats || [],
    f.selectedCats?.length ? f.catMode : '', !!f.uncategorized, f.tech || {}, f.focus ? [f.focus.kind, f.focus.id] : null]);
  return key(a) === key(b);
}

/** Movies or shows (by `f.view`) passing the filter object `f`, sorted. */
function itemsFor(f) {
  const q = normalize((f.search || '').trim());
  const status = STATUS_FILTERS.find((s) => s.id === f.status) || STATUS_FILTERS[0];
  const cats = f.selectedCats || [];
  const tech = Object.entries(f.tech || {}).map(([group, values]) => [group, new Set(values)]);
  const lib = f.libraryFilter ?? null;
  return (f.view === 'shows' ? showItems : movieItems)
    .filter((m) => {
      if (!status.test(m)) return false;
      if (lib !== null && !(m.isShow ? m.libraryIds.has(lib) : m.libraryId === lib)) return false;
      if (f.uncategorized && m.categories.length) return false;
      if (cats.length) {
        const has = (c) => m.categories.includes(c);
        if (f.catMode === 'all' ? !cats.every(has) : !cats.some(has)) return false;
      }
      if (f.focus && !matchesFocus(m, f.focus)) return false;
      // Any selected value within a group, every group with a selection.
      for (const [group, selected] of tech) {
        if (selected.size && !techValues(m, group).some((v) => selected.has(v))) return false;
      }
      if (q) {
        const words = [m.title, m.originalTitle, m.year, m.director, m.fileName, m.folder, m.note, ...m.genres, ...(m.cast || []).map((p) => p.name)];
        if (m.isShow) words.push(...m.episodes.map((e) => e.title));
        if (!normalize(words.filter(Boolean).join(' ')).includes(q)) return false;
      }
      return true;
    })
    .sort(SORTS[f.sort] || SORTS.title);
}

function filtered() {
  return itemsFor(currentFilter());
}

const techCount = () => Object.values(state.tech).reduce((n, s) => n + s.size, 0);
const hasFilters = () =>
  state.search.trim() || state.status !== 'all' || state.selectedCats.size || state.uncategorized || state.libraryFilter !== null || techCount() || state.focus;

function clearFilters() {
  state.search = '';
  $('#search').value = '';
  state.status = 'all';
  state.selectedCats.clear();
  state.uncategorized = false;
  state.libraryFilter = null;
  state.tech = {};
  state.focus = null;
  savePrefs();
  render();
}

// ---------- rendering ----------

function render() {
  sortedCats = [...state.categories].sort(collator.compare);
  deriveItems();
  if (state.showId !== null && !showById(state.showId)) state.showId = null;
  $('#btn-rescan').disabled = !state.libraries.length;
  renderViewTabs();
  renderJob();
  renderSidebar();
  renderGrid();
  renderShowPage();
  renderDetail();
  renderBulkBar();
  syncHistory();
}

function renderViewTabs() {
  const counts = { movies: movieItems.length, shows: showItems.length };
  $('#mode-tabs').innerHTML = Object.entries(VIEWS).map(([id, v]) => `
    <button class="${state.view === id ? 'active' : ''}" data-view="${id}">
      ${id === 'shows' ? ICONS.tv : ICONS.film}<span>${t(v.label)}</span><span class="n">${counts[id]}</span>
    </button>`).join('');
}

/** The open show page, if any. */
const openShow = () => (state.view === 'shows' && state.showId !== null ? showById(state.showId) : null);

function renderJob() {
  const el = $('#job');
  const job = state.job;
  if (!job) {
    el.innerHTML = '';
    return;
  }
  const pct = job.total ? (job.done / job.total) * 100 : 0;
  el.innerHTML = `
    <div class="job-bar"><div style="width:${pct}%"></div></div>
    <span class="job-text">${t('Načítám informace {done} / {total}', { done: job.done + 1, total: job.total })}${job.title ? ` · ${esc(job.title)}` : ''}</span>
    <button class="btn small" data-act="cancel-job">${t('Zastavit')}</button>`;
}

function renderSidebar() {
  const items = viewItems();
  $('#status-filters').innerHTML = STATUS_FILTERS.map((f) => {
    const count = items.filter(f.test).length;
    if (f.hideEmpty && !count && state.status !== f.id) return '';
    return `
      <button class="filter ${state.status === f.id ? 'active' : ''}" data-status="${f.id}">
        <span class="dot" style="background:${f.color}"></span>
        <span class="name">${t(typeof f.label === 'function' ? f.label() : f.label)}</span>
        <span class="count">${count}</span>
      </button>`;
  }).join('');

  $('#library-list').innerHTML = state.libraries.length
    ? state.libraries.map((l) => `
      <button class="filter ${state.libraryFilter === l.id ? 'active' : ''} ${l.online ? '' : 'offline'}" data-lib="${l.id}" title="${esc(l.path)}${l.online ? '' : ` – ${t('nedostupná')}`}">
        <span class="dot ${l.online ? 'on' : 'off'}"></span>
        <span class="name">${esc(l.name)}</span>
        <span class="count">${l.online ? l.count : t('odpojeno')}</span>
        <span class="cat-actions">
          <span data-act="relocate" title="${t('Změnit umístění složky')}">${ICONS.folder}</span>
          <span data-act="remove" title="${t('Odebrat složku')}">${ICONS.trash}</span>
        </span>
      </button>`).join('')
    : `<div class="empty-cats">${t('Žádné složky.')}</div>`;

  const counts = new Map();
  for (const m of items) for (const c of m.categories) counts.set(c, (counts.get(c) || 0) + 1);
  const uncategorized = items.filter((m) => !m.categories.length).length;

  $('#cat-list').innerHTML = (sortedCats.length ? sortedCats.map((c, i) => `
    <button class="filter ${state.selectedCats.has(c) ? 'active' : ''}" data-cat-i="${i}">
      <span class="check"></span>
      <span class="name" title="${esc(c)}">${esc(c)}</span>
      <span class="count">${counts.get(c) || 0}</span>
      <span class="cat-actions">
        <span data-act="rename" title="${t('Přejmenovat')}">${ICONS.edit}</span>
        <span data-act="delete" title="${t('Smazat kategorii')}">${ICONS.trash}</span>
      </span>
    </button>`).join('') : `<div class="empty-cats">${t('Zatím žádné kategorie.')}</div>`) + `
    <button class="filter special ${state.uncategorized ? 'active' : ''}" data-uncategorized>
      <span class="check"></span>
      <span class="name">${t('Bez kategorie')}</span>
      <span class="count">${uncategorized}</span>
    </button>`;

  renderTechFilters(items);
  renderSavedFilters();
  renderSectionToggles();

  for (const b of document.querySelectorAll('#cat-mode button')) {
    b.classList.toggle('active', b.dataset.mode === state.catMode);
  }
  $('#btn-clear-filters').classList.toggle('hidden', !hasFilters());
  $('#sort').value = state.sort;
  const active = (state.status !== 'all') + state.selectedCats.size + state.uncategorized + (state.libraryFilter !== null) + techCount() + !!state.focus;
  $('#filters-count').textContent = active || '';
}

// ---------- saved filters ----------

function renderSavedFilters() {
  const current = currentFilter();
  $('#saved-list').innerHTML = state.savedFilters.length
    ? state.savedFilters.map((s, i) => `
      <button class="filter ${sameFilter(s.filter, current) ? 'active' : ''}" data-saved-i="${i}" title="${esc(savedFilterSummary(s.filter))}">
        <span class="dot" style="background:${s.filter.view === 'shows' ? 'var(--accent-2)' : 'var(--text-3)'}"></span>
        <span class="name">${esc(s.name)}</span>
        <span class="count">${itemsFor(s.filter).length}</span>
        <span class="cat-actions">
          <span data-act="rename" title="${t('Přejmenovat')}">${ICONS.edit}</span>
          <span data-act="delete" title="${t('Smazat filtr')}">${ICONS.trash}</span>
        </span>
      </button>`).join('')
    : `<div class="empty-cats">${t('Nastavte filtry a uložte je tlačítkem „+ Uložit“.')}</div>`;
}

/** Readable description of a filter ("Seriály · Neviděné · Komedie, Drama · 4K"). */
function savedFilterSummary(f) {
  const status = STATUS_FILTERS.find((s) => s.id === f.status);
  const parts = [t(VIEWS[f.view]?.label || 'Filmy')];
  if (status && status.id !== 'all') parts.push(t(typeof status.label === 'function' ? status.label() : status.label));
  if (f.selectedCats?.length) parts.push(f.selectedCats.join(f.catMode === 'all' ? ' + ' : ', '));
  if (f.uncategorized) parts.push(t('Bez kategorie'));
  for (const [group, values] of Object.entries(f.tech || {})) parts.push(values.map((v) => techLabel(group, v)).join(', '));
  if (f.libraryFilter != null) parts.push(state.libraries.find((l) => l.id === f.libraryFilter)?.name || t('složka'));
  if (f.focus) parts.push(f.focus.name);
  if (f.search) parts.push(`„${f.search}“`);
  return parts.join(' · ');
}

function applySavedFilter(f) {
  state.view = VIEWS[f.view] ? f.view : 'movies';
  state.showId = null;
  state.detailId = null;
  state.selection.clear();
  state.search = f.search || '';
  $('#search').value = state.search;
  state.status = STATUS_FILTERS.some((s) => s.id === f.status) ? f.status : 'all';
  state.libraryFilter = state.libraries.some((l) => l.id === f.libraryFilter) ? f.libraryFilter : null;
  state.selectedCats = new Set((f.selectedCats || []).filter((c) => state.categories.includes(c)));
  state.catMode = f.catMode === 'all' ? 'all' : 'any';
  state.uncategorized = !!f.uncategorized;
  state.tech = Object.fromEntries(Object.entries(f.tech || {}).map(([group, values]) => [group, new Set(values)]));
  state.focus = f.focus || null;
  if (SORTS[f.sort]) state.sort = f.sort;
  savePrefs();
  render();
}

async function saveCurrentFilter() {
  if (!hasFilters()) {
    toast(t('Nejdřív nastavte filtry, které chcete uložit.'));
    return;
  }
  const f = currentFilter();
  const name = await modal({
    title: t('Uložit filtry'),
    text: savedFilterSummary(f),
    input: savedFilterSummary(f).split(' · ').slice(1).join(' · ').slice(0, 60),
    okLabel: t('Uložit'),
  });
  if (!name?.trim()) return;
  const list = await call('save_filter', { name, filter: f });
  if (list) {
    state.savedFilters = list;
    render();
    toast(t('Filtr „{name}“ je uložený.', { name: name.trim() }), 'ok');
  }
}

async function onSavedFilterClick(e) {
  const row = e.target.closest('[data-saved-i]');
  if (!row) return;
  const saved = state.savedFilters[+row.dataset.savedI];
  const act = e.target.closest('[data-act]')?.dataset.act;
  if (act === 'rename') {
    const name = await modal({ title: t('Přejmenovat filtr'), input: saved.name, okLabel: t('Přejmenovat') });
    if (!name?.trim() || name.trim() === saved.name) return;
    const list = await call('rename_filter', { name: saved.name, newName: name });
    if (list) { state.savedFilters = list; render(); }
  } else if (act === 'delete') {
    const ok = await modal({ title: t('Smazat filtr „{name}“?', { name: saved.name }), text: t('Filmy ani kategorie se nezmění.'), okLabel: t('Smazat'), danger: true });
    if (!ok) return;
    const list = await call('delete_filter', { name: saved.name });
    if (list) { state.savedFilters = list; render(); }
  } else if (sameFilter(saved.filter, currentFilter())) {
    clearFilters();
  } else {
    applySavedFilter(saved.filter);
  }
  if (compactQuery.matches && !act) setFiltersOpen(false);
}

/** Tooltip of the play button: what will be played. */
function playLabel(m) {
  if (m.isShow) {
    const next = nextEpisode(m);
    if (!next) return t('Přehrát znovu od začátku');
    return `${canResume(next) ? t('Pokračovat') : t('Přehrát')} ${epCode(next)} · ${next.title}`;
  }
  return canResume(m) ? t('Pokračovat od {time}', { time: fmtTime(m.position) }) : t('Přehrát ve VLC');
}

/** `selectable`: with the selection box (cards of the grid). */
function posterHtml(m, withControls, selectable = withControls) {
  const pct = itemPercent(m);
  let badge = '';
  if (!m.available) badge = `<span class="badge offline">${t('Nedostupné')}</span>`;
  else if (m.playing) badge = `<span class="badge live">${t('Přehrává se')}</span>`;
  else if (m.watched) badge = `<span class="badge done">✓ ${t('Zhlédnuto')}</span>`;
  else if (isNew(m)) badge = `<span class="badge new">${m.isShow ? t('Nové díly') : t('Nový')}</span>`;
  else if (m.wishlist) badge = `<span class="badge wish">${t('Chci vidět')}</span>`;
  const kind = m.poster ? m.posterKind : '';
  return `
    <div class="poster ${kind}" style="--h:${hue(m.title)}">
      ${m.poster ? `<img class="poster-img" src="${posterUrl(m.poster)}" loading="lazy" alt="" draggable="false" />` : ''}
      ${selectable ? `<button class="sel" data-act="select" title="${t('Vybrat (Ctrl+klik)')}">${ICONS.check}</button>` : ''}
      ${badge}
      <div class="poster-title">${esc(m.title)}</div>
      ${m.year ? `<div class="poster-year">${m.year}</div>` : ''}
      ${withControls && m.available ? `<button class="play" data-act="play" title="${esc(playLabel(m))}">${ICONS.play}</button>` : ''}
      ${pct > 0 ? `<div class="progress"><div style="width:${pct}%"></div></div>` : ''}
    </div>`;
}

function emptyState(icon, title, text, button) {
  return `
    <div class="empty">
      ${icon.replace('<svg', '<svg class="big-icon"')}
      <h2>${title}</h2>
      <p>${text}</p>
      ${button}
    </div>`;
}

function renderGrid() {
  const grid = $('#grid');
  const page = !!openShow();
  grid.classList.toggle('hidden', page);
  $('.toolbar').classList.toggle('hidden', page);
  visible = filtered();
  grid.classList.toggle('selecting', state.selection.size > 0);

  const shows = state.view === 'shows';
  const total = viewItems().length;
  renderFocusChip();
  $('#result-count').innerHTML = !state.libraries.length
    ? ''
    : visible.length === total
      ? `<b>${total}</b> ${nounFor(total)}`
      : t('Zobrazeno <b>{n}</b> z {total}', { n: visible.length, total });

  if (!state.libraries.length) {
    grid.innerHTML = emptyState(ICONS.film, t('Vítejte v Moviary'),
      t('Přidejte složku, ve které máte uložené filmy nebo seriály. Aplikace v ní (včetně podsložek) najde všechny video soubory. Složek můžete přidat víc, třeba i z {where}.', {
        where: IS_ANDROID ? t('SD karty nebo USB disku') : t('externích disků'),
      }),
      `<button class="btn primary big" data-act="add-library">${ICONS.folder} ${t('Přidat složku')}</button>`);
    return;
  }
  if (!total && shows) {
    grid.innerHTML = emptyState(ICONS.tv, t('Žádné seriály'),
      t('Seriály se poznají podle označení epizody v názvu souboru (např. „Přátelé S01E02“ nebo „Přátelé 1x02“) nebo podle složky sezóny („Přátelé\\Sezóna 1\\02.mkv“, také „Season 1“, „Série 1“, „S01“).'),
      `<button class="btn" data-act="add-library">${ICONS.folder} ${t('Přidat složku')}</button>`);
    return;
  }
  if (!total) {
    grid.innerHTML = emptyState(ICONS.film, t('Žádné filmy'), state.movies.length
      ? t('Ve složkách jsou jen epizody seriálů – najdete je v záložce Seriály.')
      : t('Ve vybraných složkách nebyly nalezeny žádné video soubory.'),
      `<button class="btn" data-act="add-library">${ICONS.folder} ${t('Přidat další složku')}</button>`);
    return;
  }
  if (!visible.length) {
    grid.innerHTML = emptyState(ICONS.search, t('Nic nenalezeno'),
      shows ? t('Žádný seriál neodpovídá zvoleným filtrům.') : t('Žádný film neodpovídá zvoleným filtrům.'),
      `<button class="btn" data-act="clear-filters">${t('Zrušit filtry')}</button>`);
    return;
  }

  grid.innerHTML = homeRowsHtml() + visible.map((m, i) => {
    const pct = itemPercent(m);
    const rating = m.rating ? `★ ${num(m.rating, 1)}` : null;
    const mine = m.myRating ? `<span class="mine" title="${t('Moje hodnocení')}">♥ ${m.myRating}/5</span>` : null;
    const meta = (m.isShow
      ? [m.year, mine, rating, `${m.seasons.length} ${plural(m.seasons.length, 'sezóna', 'sezóny', 'sezón')}`, `${m.episodeCount} ${plural(m.episodeCount, 'epizoda', 'epizody', 'epizod')}`]
      : [m.year, mine, rating, techBadge(m) ? `<span class="tech">${techBadge(m)}</span>` : null, fmtSize(m.size)]).filter(Boolean).join(' · ');
    const progress = m.isShow
      ? m.inProgress ? ` · <span class="accent">${m.watchedCount}/${m.episodeCount}</span>` : ''
      : pct > 0 ? ` · <span class="accent">${Math.round(pct)} %</span>` : '';
    const cats = [...m.categories].sort(collator.compare);
    const classes = ['card', state.selection.has(m.id) ? 'selected' : '', m.available ? '' : 'offline'].join(' ');
    return `
      <div class="${classes}" data-i="${i}">
        ${posterHtml(m, true)}
        <div class="card-body">
          <div class="card-title" title="${esc(m.isShow ? m.title : m.fileName)}">${esc(m.title)}</div>
          <div class="card-meta">${meta}${progress}</div>
          ${cats.length ? `<div class="chips">${cats.map((c) => `<span class="chip">${esc(c)}</span>`).join('')}</div>` : ''}
        </div>
      </div>`;
  }).join('');
}

/** TMDB status and actions of a movie or show. */
function metaSectionHtml(m) {
  if (m.tmdbId) {
    return `
      <div class="muted">${t('Informace a plakát z TMDB (themoviedb.org).')}</div>
      <div class="meta-actions">
        <button class="btn small" data-act="match">${t('Změnit shodu…')}</button>
        <button class="btn small" data-act="refetch">${t('Znovu načíst')}</button>
        <button class="btn small" data-act="clear-meta">${t('Odebrat informace')}</button>
      </div>`;
  }
  if (state.settings?.tmdbKey) {
    const notFound = m.isShow ? t('Seriál se na TMDB nepodařilo automaticky najít.') : t('Film se na TMDB nepodařilo automaticky najít.');
    return `
      <div class="muted">${m.metaChecked ? notFound : t('Informace zatím nebyly načteny.')}</div>
      <div class="meta-actions">
        <button class="btn small" data-act="match">${t('Vyhledat na TMDB…')}</button>
        ${m.metaChecked ? '' : `<button class="btn small" data-act="refetch">${t('Načíst automaticky')}</button>`}
      </div>`;
  }
  return `<div class="hint">${t('Plakáty, popisy, hodnocení a žánry{episodes} se načtou po zadání bezplatného TMDB klíče v {settings}.', {
    episodes: m.isShow ? t(' i názvy epizod') : '',
    settings: `<button class="link-btn" data-act="settings">${t('Nastavení')}</button>`,
  })}</div>`;
}

/** Want to watch, own rating (1–5) and a private note of a movie or show. */
function personalHtml(m, noteId) {
  const stars = [1, 2, 3, 4, 5].map((n) => `
    <button class="star ${n <= (m.myRating || 0) ? 'on' : ''}" data-personal="star" data-n="${n}"
      title="${n === m.myRating ? t('Zrušit hodnocení') : t('{n} z 5', { n })}">♥</button>`).join('');
  return `
    <div class="personal">
      <button class="btn small wish ${m.wishlist ? 'on' : ''}" data-personal="wish">${m.wishlist ? `✓ ${t('Chci vidět')}` : `+ ${t('Chci vidět')}`}</button>
      <div class="stars" role="group" aria-label="${t('Moje hodnocení')}">${stars}</div>
    </div>
    <textarea id="${noteId}" class="note" data-note data-item="${m.isShow ? 's' : 'm'}${m.id}" rows="2" maxlength="4000" spellcheck="true" placeholder="${t('Poznámka – vidíte ji jen vy…')}">${esc(m.note || '')}</textarea>`;
}

async function savePersonal(m, change) {
  // The current state: `m` may be a copy from an earlier render.
  const cur = (m.isShow ? showById(m.id) : movieById(m.id)) || m;
  const value = { wishlist: !!cur.wishlist, rating: cur.myRating || 0, note: cur.note || '', ...change };
  const u = await call(m.isShow ? 'set_show_personal' : 'set_movie_personal', { id: m.id, ...value });
  if (u) (m.isShow ? updateShow : updateMovie)(u);
}

/** Handles a click on the wishlist button or a heart; true when it was one. */
function onPersonalClick(e, m) {
  const btn = e.target.closest('[data-personal]');
  if (!btn) return false;
  if (btn.dataset.personal === 'wish') savePersonal(m, { wishlist: !m.wishlist });
  else savePersonal(m, { rating: +btn.dataset.n === m.myRating ? 0 : +btn.dataset.n });
  return true;
}

let noteTimer = null;

/** Movie or show a note field belongs to (`data-item` "m12" / "s34"). */
function noteItem(el) {
  const ref = el.dataset.item || '';
  const id = +ref.slice(1);
  return ref[0] === 's' ? showById(id) : movieById(id);
}

/**
 * Saves a changed note shortly after typing stops, and when leaving the field – always to
 * the movie or show the field belongs to, even when the detail switched meanwhile.
 */
function onNoteEvent(e) {
  if (!e.target.matches?.('[data-note]')) return false;
  clearTimeout(noteTimer);
  const text = e.target.value;
  const field = e.target;
  const save = () => {
    const cur = noteItem(field);
    if (cur && text.trim() !== (cur.note || '')) savePersonal(cur, { note: text });
  };
  if (e.type === 'input') noteTimer = setTimeout(save, 800);
  else save();
  return true;
}

/** Category toggles and the form for a new one. */
function categoriesHtml(m, inputId) {
  return `
    ${sortedCats.length
      ? `<div class="cat-toggles">${sortedCats.map((c, i) => `<button class="chip toggle ${m.categories.includes(c) ? 'on' : ''}" data-cat-i="${i}">${esc(c)}</button>`).join('')}</div>`
      : `<div class="muted">${t('Zatím nemáte žádné kategorie – vytvořte první níže.')}</div>`}
    <form class="new-cat">
      <input id="${inputId}" placeholder="${t('Nová kategorie…')}" autocomplete="off" spellcheck="false" maxlength="60" />
      <button class="btn small" type="submit">${t('Přidat')}</button>
    </form>`;
}

/**
 * Re-renders `container` keeping its scroll position and half-typed texts: the inputs in
 * `inputIds` keep their text; a textarea (the note) only while it is being typed in and
 * only when it still belongs to the same movie or show (`data-item`).
 */
function keepingInput(container, inputIds, draw) {
  const kept = [].concat(inputIds).map((id) => {
    const el = document.getElementById(id);
    const focused = !!el && document.activeElement === el;
    return { id, focused, item: el?.dataset.item, keep: !!el && (focused || el.tagName !== 'TEXTAREA'), value: el?.value || '', caret: el?.selectionStart };
  });
  const scroll = container.scrollTop;
  draw();
  for (const k of kept) {
    const el = document.getElementById(k.id);
    if (!el || !k.keep || el.dataset.item !== k.item) continue;
    el.value = k.value;
    if (k.focused) {
      el.focus();
      if (k.caret != null) el.setSelectionRange(k.caret, k.caret);
    }
  }
  container.scrollTop = scroll;
}

function renderShowPage() {
  const page = $('#show-page');
  const s = openShow();
  if (!s) {
    page.classList.add('hidden');
    page.innerHTML = '';
    return;
  }
  const next = nextEpisode(s);
  const season = s.seasons.find((x) => x.number === state.season)
    || s.seasons.find((x) => x.number === next?.season)
    || s.seasons[0];
  state.season = season.number;

  const playIcon = ICONS.play.replace('<svg', '<svg style="fill:#fff;stroke:none"');
  const playing = s.episodes.find((e) => e.playing);
  let actions;
  if (playing) {
    actions = `<div class="playing-note">● ${t('Právě se přehrává')} ${epCode(playing)} · ${esc(playing.title)}</div>`;
  } else if (!s.available) {
    actions = `<div class="playing-note" style="color:#f87171">${ICONS.plug} ${t('Disk se seriálem není připojený')}</div>`;
  } else if (!next) {
    actions = `
      <div class="playing-note" style="color:var(--ok)">✓ ${t('Všechny epizody jsou zhlédnuté')}</div>
      <button class="btn" data-act="play-first">${ICONS.restart} ${t('Přehrát znovu od začátku')}</button>`;
  } else if (!next.available) {
    actions = `<div class="playing-note" style="color:#f87171">${ICONS.plug} ${t('Další epizoda ({code}) je na odpojeném disku', { code: epCode(next) })}</div>`;
  } else {
    const label = canResume(next)
      ? t('Pokračovat {code} od {time}', { code: epCode(next), time: fmtTime(next.position) })
      : t('Přehrát {code}', { code: epCode(next) });
    actions = `<button class="btn primary big" data-act="play-next" title="${esc(next.title)}">${playIcon} ${label}</button>`;
  }

  const facts = [
    s.year,
    `${s.seasons.length} ${plural(s.seasons.length, 'sezóna', 'sezóny', 'sezón')}`,
    `${s.episodeCount} ${plural(s.episodeCount, 'epizoda', 'epizody', 'epizod')}`,
    s.runtime ? `≈ ${fmtRuntime(s.runtime)}` : null,
    fmtSize(s.size),
  ].filter(Boolean).map((f) => `<span>${f}</span>`);
  if (s.rating) facts.unshift(`<span class="rating">★ ${num(s.rating, 1)}</span>`);

  const pct = itemPercent(s);
  const seasonWatched = season.episodes.every((e) => e.watched);
  // Episodes TMDB knows that are not in the library.
  const have = new Set(season.episodes.flatMap((e) => {
    const nums = [];
    for (let n = e.episode; n <= (e.lastEpisode || e.episode); n++) nums.push(n);
    return nums;
  }));
  const missing = [];
  for (let n = 1; n <= season.total; n++) if (!have.has(n)) missing.push(n);

  keepingInput(page, ['show-cat-input', 'show-note-input'], () => {
    page.classList.remove('hidden');
    page.innerHTML = `
      <button class="back-btn" data-act="back">${ICONS.back} ${t('Všechny seriály')}</button>
      <div class="show-head">
        ${posterHtml(s, false)}
        <div class="head-text">
          <h1>${esc(s.title)}</h1>
          ${s.originalTitle ? `<div class="original-title">${esc(s.originalTitle)}</div>` : ''}
          <div class="facts">${facts.join('')}</div>
          ${airingHtml(s)}
          ${s.creators ? `<div class="muted" style="margin-top:6px">${t('Tvůrci')}: ${s.creatorList?.length ? s.creatorList.map(personLink).join(', ') : esc(s.creators)}</div>` : ''}
          ${s.genres.length ? `<div class="genres">${s.genres.map((g) => `<span class="chip">${esc(g)}</span>`).join('')}</div>` : ''}
          ${s.overview ? `<div class="overview">${esc(s.overview)}</div>` : ''}
          <div class="show-actions">${actions}</div>
          <div class="show-progress">
            <div class="label"><span>${t('Zhlédnuto <b>{n}</b> z {total} {noun}', { n: s.watchedCount, total: s.episodeCount, noun: plural(s.episodeCount, 'epizody', 'epizod', 'epizod') })}</span><b>${Math.round(pct)} %</b></div>
            <div class="progress big"><div style="width:${pct}%"></div></div>
          </div>
        </div>
      </div>

      <div class="show-panels">
        <div class="panel">
          <h3>${t('Moje hodnocení')}</h3>
          ${personalHtml(s, 'show-note-input')}
        </div>
        <div class="panel">
          <h3>${t('Kategorie')}</h3>
          ${categoriesHtml(s, 'show-cat-input')}
        </div>
        <div class="panel">
          <h3>${t('Informace o seriálu')}</h3>
          ${metaSectionHtml(s)}
        </div>
        <div class="panel">
          <h3>${t('Intro a titulky')}</h3>
          ${skipPanelHtml(s)}
        </div>
        ${IS_ANDROID ? '' : `<div class="panel">
          <h3>${t('Zvuk a titulky pro všechny epizody')}</h3>
          ${showTracksHtml(s)}
        </div>`}
      </div>

      ${s.cast?.length || similarItems(s).length ? `
      <div class="show-extra">
        ${s.cast?.length ? `<div><h3>${t('Hrají')}</h3>${castHtml(s.cast)}</div>` : ''}
        ${similarItems(s).length ? `<div><h3>${t('Podobné ve vaší knihovně')}</h3>${similarHtml(s)}</div>` : ''}
      </div>` : ''}

      <div class="season-tabs">
        ${s.seasons.map((x) => {
          const watched = x.episodes.filter((e) => e.watched).length;
          return `<button class="season-tab ${x === season ? 'active' : ''} ${watched === x.episodes.length ? 'done' : ''}" data-season="${x.number}">
            ${seasonName(x.number)}<span class="n">${watched}/${x.episodes.length}</span></button>`;
        }).join('')}
      </div>
      <div class="season-head">
        <div class="muted">
          ${season.episodes.length} ${plural(season.episodes.length, 'epizoda', 'epizody', 'epizod')}${season.total > season.episodes.length ? ` ${t('z')} ${season.total}` : ''}
          · ${fmtSize(season.episodes.reduce((sum, e) => sum + e.size, 0))}
        </div>
        <button class="btn small" data-act="season-watched">${seasonWatched ? t('Označit sezónu jako neviděnou') : t('Označit sezónu jako zhlédnutou')}</button>
      </div>
      <div class="episodes">${season.episodes.map(episodeRow).join('')}</div>
      ${missing.length ? `<div class="muted season-missing">${t('V knihovně chybí {list} (podle TMDB).', {
        list: missing.length > 20 ? `${missing.length} ${plural(missing.length, 'epizoda', 'epizody', 'epizod')}` : missing.map((n) => `E${pad2(n)}`).join(', '),
      })}</div>` : ''}`;
  });
}

// ---------- intro and credits times (TheIntroDB) ----------

/** Show id -> "12/40" while its times are being downloaded. */
const segmentsProgress = new Map();

/** The show page panel: how many episodes have times, and the download button. */
function skipPanelHtml(s) {
  const asked = s.episodes.filter((e) => e.skipTimes);
  const found = asked.filter(hasSkipTimes).length;
  const running = segmentsProgress.get(s.id);
  const set = state.settings;
  const enabled = set && (set.skipIntro || (set.skipCredits && !IS_ANDROID));
  let status;
  if (!s.tmdbId) status = t('Časy se hledají podle TMDB – nejdřív seriálu přiřaďte informace z TMDB.');
  else if (!asked.length) status = t('Zatím nestaženo. Se zapnutým přeskakováním se časy stáhnou samy.');
  else status = t('Časy intra nebo titulků zná TheIntroDB u {n} z {total} {noun}.', {
    n: found, total: s.episodes.length, noun: plural(s.episodes.length, 'epizody', 'epizod', 'epizod'),
  });
  return `
    <div class="muted">${status}</div>
    ${enabled ? '' : `<div class="muted" style="margin-top:6px">${t('Přeskakování zapnete v Nastavení → Přehrávání seriálů.')}</div>`}
    <div class="row" style="margin-top:10px">
      <button class="btn small" data-act="fetch-segments" ${!s.tmdbId || running ? 'disabled' : ''}>${running
        ? t('Stahuji {progress}…', { progress: running })
        : t('Stáhnout z TheIntroDB')}</button>
      <button class="btn small" type="button" data-link="theintrodb">TheIntroDB</button>
    </div>`;
}

async function fetchSegments(s) {
  segmentsProgress.set(s.id, `0/${s.episodes.length}`);
  render();
  let result;
  try {
    result = await invoke('fetch_show_segments', { id: s.id });
  } catch (err) {
    toast(String(err), 'error');
  }
  segmentsProgress.delete(s.id);
  if (!result) return render();
  applyData(result.data);
  if (result.error) toast(result.error, 'error');
  toast(t('TheIntroDB má časy pro {n} z {total} {noun}.', {
    n: result.found, total: result.total, noun: plural(result.total, 'epizody', 'epizod', 'epizod'),
  }), result.found ? 'ok' : 'info');
}

// ---------- upcoming episodes (TMDB next_episode_to_air) ----------

/** Days from today to a "YYYY-MM-DD" date (negative = in the past). */
function daysUntil(date) {
  const [y, m, d] = date.split('-').map(Number);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return Math.round((new Date(y, m - 1, d) - today) / 86400000);
}

/** "dnes", "zítra", "čt 12. 11." – how far a release date is. */
function airDateLabel(date, withYear = false) {
  const days = daysUntil(date);
  if (days === 0) return t('dnes');
  if (days === 1) return t('zítra');
  if (days === -1) return t('včera');
  const [y, m, d] = date.split('-').map(Number);
  const options = { day: 'numeric', month: 'numeric', ...(withYear ? { year: 'numeric' } : {}), ...(days > 0 && days < 7 ? { weekday: 'short' } : {}) };
  return new Date(y, m - 1, d).toLocaleDateString(locale(), options);
}

const genericEpisodeName = (name) => !name || /^(epizoda|episode|díl)\s*\d+$/i.test(name.trim());

/** The line on the show page about the next episode, or that the series ended. */
function airingHtml(s) {
  const n = s.nextEpisode;
  if (n) {
    const code = epCode({ season: n.season, episode: n.episode });
    const name = genericEpisodeName(n.title) ? '' : ` „${esc(n.title)}“`;
    const days = daysUntil(n.airDate);
    const when = days < 0
      ? t('vyšel {date}', { date: airDateLabel(n.airDate, true) })
      : days === 0 ? t('vychází dnes') : t('vyjde {date}', { date: airDateLabel(n.airDate, true) });
    return `<div class="next-air ${days <= 0 ? 'out' : ''}">${t('Další díl {code}{name} {when}', { code, name, when })}</div>`;
  }
  if (s.status === 'Ended') return `<div class="next-air ended">${t('Seriál je ukončený.')}</div>`;
  if (s.status === 'Canceled') return `<div class="next-air ended">${t('Seriál byl zrušen.')}</div>`;
  return '';
}

/** Shows whose next episode aired in the last week or comes within a month, soonest first. */
function upcomingShows() {
  return showItems
    .filter((s) => s.nextEpisode && daysUntil(s.nextEpisode.airDate) >= -7 && daysUntil(s.nextEpisode.airDate) <= 30)
    .sort((a, b) => a.nextEpisode.airDate.localeCompare(b.nextEpisode.airDate) || byTitle(a, b));
}

const SKIP_ICON = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6v12l8.5-6zM13.5 6v12l8.5-6z"/></svg>';

/** Episode has intro, recap or credits times from TheIntroDB. */
const hasSkipTimes = (e) => !!(e.skipTimes && (e.skipTimes.intro || e.skipTimes.recap || e.skipTimes.credits));

/** Chips with the intro, recap and credits times of an episode (from TheIntroDB). */
function skipTimesHtml(e) {
  if (!hasSkipTimes(e)) return '';
  const { intro, recap, credits, afterCredits, introManual, creditsManual } = e.skipTimes;
  const set = state.settings || {};
  const range = ([from, to]) => `${fmtTime(from)}–${fmtTime(to)}`;
  const chip = (label, time, on, title, manual) => `
    <span class="skip-chip ${on ? '' : 'off'} ${manual ? 'manual' : ''}" title="${esc(`${title} ${manual ? t('Vlastní čas.') : t('Čas z TheIntroDB.')} ${t('Kliknutím upravíte.')}`)}">${SKIP_ICON}${label} <b>${time}</b>${manual ? `<i>${t('vlastní')}</i>` : ''}</span>`;
  const introTitle = set.skipIntro ? t('Přeskočí se.') : t('Přeskakování intra je vypnuté v Nastavení.');
  const creditsOn = set.skipCredits && !IS_ANDROID;
  const creditsTitle = !creditsOn
    ? t('Přeskakování titulků je vypnuté v Nastavení.')
    : afterCredits ? t('Přeskočí se, přehrávání pokračuje scénou po titulcích.') : t('Epizoda tu skončí.');
  return `<span class="skip-times" data-act="ep-times">${[
    recap ? chip(t('rekapitulace'), range(recap), set.skipIntro, introTitle, introManual) : '',
    intro ? chip(t('intro'), range(intro), set.skipIntro, introTitle, introManual) : '',
    credits ? chip(t('titulky'), afterCredits ? range([credits, afterCredits]) : t('od {time}', { time: fmtTime(credits) }), creditsOn, creditsTitle, creditsManual) : '',
  ].join('')}</span>`;
}

const TIMER_ICON = '<svg viewBox="0 0 24 24"><circle cx="12" cy="13.5" r="7.5"/><path d="M12 10v3.5l2.5 2M9.5 3h5"/></svg>';

/** "1:23", "1:02:03" or "83" (seconds) -> seconds; NaN when it is not a time, null when empty.
 *  A trailing colon ("24:", left by the automatic colon) means whole minutes. */
function parseTime(text) {
  let s = String(text ?? '').trim().replace(',', '.');
  if (!s) return null;
  if (s.endsWith(':')) s += '00';
  if (!/^\d+(:\d{1,2}){0,2}(\.\d+)?$/.test(s)) return NaN;
  const parts = s.split(':').map(Number);
  if (parts.slice(1).some((p) => p >= 60)) return NaN;
  return parts.reduce((total, p) => total * 60 + p, 0);
}

/** Own intro and credits times of an episode, over TheIntroDB's. */
function openSkipTimesEditor(e) {
  const db = e.introDbTimes;
  const own = e.manualTimes || {};
  const length = e.length || e.duration || 0;
  const range = ([from, to]) => `${fmtTime(from)}–${fmtTime(to)}`;
  const dbIntro = [db?.recap ? `${t('rekapitulace')} ${range(db.recap)}` : '', db?.intro ? `${t('intro')} ${range(db.intro)}` : ''].filter(Boolean).join(', ');
  const dbCredits = db?.credits ? t('od {time}', { time: fmtTime(db.credits) }) : '';
  const introMode = own.noIntro ? 'none' : own.intro ? 'own' : 'db';
  const creditsMode = own.noCredits ? 'none' : own.credits != null ? 'own' : 'db';
  // The intro always runs from the start: only its end is set (TheIntroDB's end as a hint).
  const introEnd = own.intro?.[1] ?? db?.intro?.[1] ?? db?.recap?.[1];
  const creditsValue = own.credits ?? db?.credits;
  // As the automatic colon writes them: "0:45", "3:48", "47:12", "1:02:03".
  const timeValue = (v) => (v == null ? '' : fmtTime(Math.round(v)));
  const modes = (part, mode, dbText, noneText) => [
    ['db', t('Podle TheIntroDB'), dbText || (db ? t('TheIntroDB pro tuto epizodu nic nemá.') : t('Zatím nestaženo.'))],
    ['own', t('Vlastní čas'), ''],
    ['none', t('Nepřeskakovat'), noneText],
  ].map(([value, label, hint]) => `
    <label class="radio-row"><input type="radio" name="${part}-mode" value="${value}" ${mode === value ? 'checked' : ''} />
      <span>${label}${hint ? `<small>${esc(hint)}</small>` : ''}</span></label>`).join('');
  const { el, close } = openModal(`
    <h2>${t('Intro a titulky')}</h2>
    <div class="muted">${epCode(e)} · ${esc(e.title)}${length ? ` · ${t('délka {time}', { time: fmtTime(length) })}` : ''}</div>
    <div class="setting">
      <div class="title">${t('Intro')}</div>
      ${modes('intro', introMode, dbIntro, t('Ani když ho TheIntroDB zná.'))}
      <div class="time-fields" data-for="intro" ${introMode === 'own' ? '' : 'hidden'}>
        <label>${t('Intro končí v')} <input class="time-input" name="intro-end" value="${timeValue(introEnd)}" placeholder="0:45" inputmode="numeric" maxlength="8" spellcheck="false" autocomplete="off" /></label>
        <span class="muted">${t('Počítá se vždy od začátku epizody, ta pak začne až v tomto čase.')}</span>
      </div>
    </div>
    <div class="setting">
      <div class="title">${t('Závěrečné titulky')}</div>
      ${modes('credits', creditsMode, dbCredits, t('Ani když je TheIntroDB zná.'))}
      <div class="time-fields" data-for="credits" ${creditsMode === 'own' ? '' : 'hidden'}>
        <label>${t('Titulky začínají v')} <input class="time-input" name="credits-start" value="${timeValue(creditsValue)}" placeholder="${length ? timeValue(Math.max(0, length - 90)) : '42:00'}" inputmode="numeric" maxlength="8" spellcheck="false" autocomplete="off" /></label>
        <span class="muted">${t('Epizoda v tento čas skončí.')}</span>
      </div>
      ${IS_ANDROID ? `<div class="muted" style="margin-top:8px">${t('Na Androidu se titulky nepřeskakují – VLC pro Android se během přehrávání nedá ovládat.')}</div>` : ''}
    </div>
    <div class="muted" style="margin-top:12px">${t('Pište jen číslice, dvojtečka se doplní sama: 2410 je 24:10 a 045 je 0:45. Vlastní časy mají vždy přednost a stažení z TheIntroDB je nikdy nepřepíše.')}</div>
    <div class="modal-actions">
      <button class="btn" data-act="cancel">${t('Zrušit')}</button>
      <button class="btn primary" data-act="save">${t('Uložit')}</button>
    </div>`, { extraClass: 'times-modal' });

  el.addEventListener('change', (ev) => {
    const m = ev.target.name?.match(/^(intro|credits)-mode$/);
    if (!m) return;
    const fields = el.querySelector(`.time-fields[data-for="${m[1]}"]`);
    fields.hidden = ev.target.value !== 'own';
    if (!fields.hidden) fields.querySelector('input').focus();
  });
  el.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter' && ev.target.matches('.time-input')) el.querySelector('[data-act="save"]').click();
  });
  // Typing "24" adds the colon: "24:", then "24:10" (and "01:02:03" with hours).
  // A leading 0 is a whole part on its own: "0" -> "0:", then "0:45".
  el.addEventListener('input', (ev) => {
    const input = ev.target;
    if (!input.matches('.time-input')) return;
    let v = input.value.replace(/[^\d:]/g, '').replace(/::+/g, ':');
    const typedAtEnd = ev.inputType?.startsWith('insert') && input.selectionStart === input.value.length;
    if (/^0\d{2}$/.test(v)) {
      // Pasted "045" -> "0:45".
      v = `0:${v.slice(1)}`;
    } else if (/^\d{3,6}$/.test(v)) {
      // Pasted digits: by two from the left ("2410" -> "24:10").
      v = v.match(/\d{1,2}/g).join(':');
    } else if (typedAtEnd && ev.data !== ':') {
      const colons = (v.match(/:/g) || []).length;
      // A third digit in a part starts the next one: "24:105" -> "24:10:5" (hours).
      v = colons < 2 ? v.replace(/(^|:)(\d{2})(\d)$/, '$1$2:$3') : v.replace(/(:\d{2})\d+$/, '$1');
      // The colon after the minutes: "24" -> "24:", and "0" -> "0:" right away.
      if (/^(\d{2}|0)$/.test(v)) v += ':';
    }
    if (v !== input.value) {
      input.value = v;
      input.setSelectionRange(v.length, v.length);
    }
  });
  el.addEventListener('click', async (ev) => {
    const act = ev.target.closest('[data-act]')?.dataset.act;
    if (act === 'cancel') return close();
    if (act !== 'save') return;
    const mode = (part) => el.querySelector(`[name="${part}-mode"]:checked`).value;
    const value = (name) => parseTime(el.querySelector(`[name="${name}"]`).value);
    const times = { intro: null, credits: null, noIntro: mode('intro') === 'none', noCredits: mode('credits') === 'none' };
    const bad = (message, name) => {
      toast(message, 'error');
      el.querySelector(`[name="${name}"]`)?.focus();
    };
    if (mode('intro') === 'own') {
      const end = value('intro-end');
      if (end == null || Number.isNaN(end) || end <= 0) return bad(t('Neplatný čas – zadejte ho jako 01:23.'), 'intro-end');
      if (length && end >= length) return bad(t('Čas je za koncem epizody ({time}).', { time: fmtTime(length) }), 'intro-end');
      times.intro = [0, end];
    }
    if (mode('credits') === 'own') {
      const start = value('credits-start');
      if (start == null || Number.isNaN(start) || start <= 0) return bad(t('Neplatný čas – zadejte ho jako 01:23.'), 'credits-start');
      if (length && start >= length) return bad(t('Čas je za koncem epizody ({time}).', { time: fmtTime(length) }), 'credits-start');
      times.credits = start;
    }
    const u = await call('set_manual_times', { id: e.id, times });
    if (!u) return;
    updateMovie(u);
    close();
    toast(t('Časy intra a titulků jsou uložené.'), 'ok');
  });
}

function episodeRow(e) {
  const pct = percent(e);
  const classes = ['ep', e.watched ? 'watched' : '', e.available ? '' : 'offline', e.playing ? 'playing' : ''].join(' ');
  const meta = [
    epCode(e),
    e.runtime ? `${e.runtime} min` : e.duration ? fmtTime(e.duration) : null,
    fmtSize(e.size),
    e.position > 0 ? `<span class="accent">${fmtTime(e.position)}${e.duration ? ` / ${fmtTime(e.duration)}` : ''}</span>` : null,
    e.watched ? `<span class="ok">✓ ${t('Zhlédnuto')}</span>` : null,
  ].filter(Boolean).join(' · ');
  const canPlay = e.available && !e.playing;
  return `
    <div class="${classes}" data-ep="${e.id}">
      <div class="ep-num">${e.lastEpisode ? `${e.episode}–${e.lastEpisode}` : e.episode}</div>
      <div class="ep-main">
        <div class="ep-title" title="${esc(e.fileName)}">${esc(e.title)}${e.playing ? `<span class="ep-live">${t('Přehrává se')}</span>` : ''}</div>
        <div class="ep-meta">${meta}${skipTimesHtml(e)}</div>
        ${e.overview ? `<div class="ep-overview">${esc(e.overview)}</div>` : ''}
        ${pct > 0 ? `<div class="progress big"><div style="width:${pct}%"></div></div>` : ''}
      </div>
      <div class="ep-actions">
        ${canPlay ? `<button class="ep-btn play" data-act="ep-play" title="${canResume(e) ? t('Pokračovat od {time}', { time: fmtTime(e.position) }) : t('Přehrát ve VLC')}">${ICONS.play}</button>` : ''}
        ${canPlay && canResume(e) ? `<button class="ep-btn" data-act="ep-restart" title="${t('Přehrát od začátku')}">${ICONS.restart}</button>` : ''}
        ${e.position > 0 ? `<button class="ep-btn" data-act="ep-reset" title="${t('Vynulovat pozici')}">${ICONS.close}</button>` : ''}
        <button class="ep-btn ${e.manualTimes ? 'on' : ''}" data-act="ep-times" title="${t('Časy intra a titulků')}">${TIMER_ICON}</button>
        <button class="ep-btn ${e.watched ? 'on' : ''}" data-act="ep-watched" title="${e.watched ? t('Označit jako neviděné') : t('Označit jako zhlédnuté')}">${ICONS.check}</button>
        ${IS_ANDROID ? '' : `<button class="ep-btn" data-act="ep-reveal" title="${e.available ? t('Zobrazit v Průzkumníkovi') : t('Disk není připojený')}" ${e.available ? '' : 'disabled'}>${ICONS.folder}</button>`}
      </div>
    </div>`;
}

function renderDetail() {
  const drawer = $('#drawer');
  const m = state.view === 'movies' ? movieById(state.detailId) : null;
  if (!m) {
    drawer.classList.add('hidden');
    if (state.view === 'movies') state.detailId = null;
    return;
  }

  const pct = percent(m);
  let progress;
  if (m.position > 0) {
    progress = `
      <div class="label"><span>${t('Dokoukáno <b>{time}</b>', { time: fmtTime(m.position) })}${m.duration ? ` ${t('z')} ${fmtTime(m.duration)}` : ''}</span><b>${Math.round(pct)} %</b></div>
      <div class="progress big"><div style="width:${pct}%"></div></div>`;
  } else if (m.watched) {
    progress = `<div class="label"><span style="color:var(--ok)"><b style="color:inherit">✓ ${t('Zhlédnuto')}</b></span></div>`;
  } else {
    progress = `<div class="label"><span>${t('Zatím nepřehráno')}</span></div>`;
  }
  if (m.lastPlayed) progress += `<div class="muted" style="margin-top:8px">${t('Naposledy přehráno {date}', { date: fmtDate(m.lastPlayed) })}</div>`;

  const playIcon = ICONS.play.replace('<svg', '<svg style="fill:#fff;stroke:none"');
  let actions;
  if (!m.available) {
    actions = `<div class="playing-note" style="color:#f87171">${ICONS.plug} ${t('Disk se složkou není připojený')}</div>`;
  } else if (m.playing) {
    actions = `<div class="playing-note">● ${t('Právě se přehrává ve VLC')}</div>`;
  } else if (canResume(m)) {
    actions = `
      <button class="btn primary big" data-act="resume">${playIcon} ${t('Pokračovat od {time}', { time: fmtTime(m.position) })}</button>
      <button class="btn" data-act="restart">${ICONS.restart} ${t('Přehrát od začátku')}</button>`;
  } else {
    actions = `<button class="btn primary big" data-act="restart">${playIcon} ${t('Přehrát ve VLC')}</button>`;
  }

  const facts = [
    m.year,
    m.runtime ? fmtRuntime(m.runtime) : m.duration ? fmtTime(m.duration) : null,
    fmtSize(m.size),
  ].filter(Boolean).map((f) => `<span>${f}</span>`);
  if (m.rating) facts.unshift(`<span class="rating">★ ${num(m.rating, 1)}</span>`);

  const html = `
    <div class="drawer-head">
      ${posterHtml(m, false)}
      <div class="head-text">
        <h2>${esc(m.title)}</h2>
        ${m.originalTitle ? `<div class="original-title">${esc(m.originalTitle)}</div>` : ''}
        <div class="facts">${facts.join('')}</div>
        ${m.director ? `<div class="muted" style="margin-top:6px">${t('Režie')}: ${m.directors?.length ? m.directors.map(personLink).join(', ') : esc(m.director)}</div>` : ''}
      </div>
      <button class="close-btn" data-act="close" title="${t('Zavřít (Esc)')}">${ICONS.close}</button>
    </div>
    ${m.genres.length ? `<div class="genres">${m.genres.map((g) => `<span class="chip">${esc(g)}</span>`).join('')}</div>` : ''}
    ${m.overview ? `<div class="overview">${esc(m.overview)}</div>` : ''}
    ${IS_ANDROID
      ? `<div class="path-btn">${ICONS.folder}<span>${esc(m.path)}</span></div>`
      : `<button class="path-btn" data-act="reveal" title="${m.available ? t('Zobrazit v Průzkumníkovi') : t('Disk není připojený')}" ${m.available ? '' : 'disabled'}>${ICONS.folder}<span>${esc(m.path)}</span></button>`}

    <div class="progress-block">${progress}</div>
    <div class="actions">
      ${actions}
      <div class="row">
        <button class="btn small" data-act="toggle-watched">${m.watched ? t('Označit jako neviděné') : t('Označit jako zhlédnuté')}</button>
        ${m.position > 0 ? `<button class="btn small" data-act="reset">${t('Vynulovat pozici')}</button>` : ''}
      </div>
    </div>

    <h3>${t('Moje hodnocení')}</h3>
    ${personalHtml(m, 'note-input')}

    ${IS_ANDROID ? '' : `<h3>${t('Zvuk a titulky')}</h3>${tracksHtml(m)}`}

    <h3>${t('Kategorie')}</h3>
    ${categoriesHtml(m, 'new-cat-input')}

    ${m.cast?.length ? `<h3>${t('Hrají')}</h3>${castHtml(m.cast)}` : ''}

    ${m.collection ? `<h3>${t('Filmová série')}</h3>${collectionHtml(m)}` : ''}

    ${similarItems(m).length ? `<h3>${t('Podobné ve vaší knihovně')}</h3>${similarHtml(m)}` : ''}

    <h3>${t('Technické údaje')}</h3>
    ${techInfoHtml(m)}

    <h3>${t('Informace o filmu')}</h3>
    ${metaSectionHtml(m)}`;

  // Keep a half-typed category name across re-renders (progress updates arrive while open).
  keepingInput(drawer, ['new-cat-input', 'note-input'], () => {
    drawer.classList.remove('hidden');
    $('#drawer-content').innerHTML = html;
  });
}

/** Every selected movie or show is on the "want to watch" list. */
function selectionWished() {
  const items = viewItems().filter((m) => state.selection.has(m.id));
  return items.length > 0 && items.every((m) => m.wishlist);
}

function renderBulkBar() {
  const bar = $('#bulkbar');
  const n = state.selection.size;
  if (!n || openShow()) {
    bar.classList.add('hidden');
    return;
  }
  const previous = $('#bulk-cat')?.value || '';
  bar.classList.remove('hidden');
  bar.innerHTML = `
    <span>${t('Vybráno <b>{n}</b>', { n })}</span>
    <span class="sep"></span>
    <select id="bulk-cat">
      <option value="">${t('Zvolte kategorii…')}</option>
      ${sortedCats.map((c, i) => `<option value="${i}">${esc(c)}</option>`).join('')}
      <option value="new">+ ${t('Nová kategorie…')}</option>
    </select>
    <button class="btn small primary" data-act="bulk-add">${t('Přidat')}</button>
    <button class="btn small" data-act="bulk-remove">${t('Odebrat')}</button>
    <span class="sep"></span>
    <button class="btn small" data-act="bulk-wish">${selectionWished() ? t('Odebrat z Chci vidět') : `+ ${t('Chci vidět')}`}</button>
    <button class="btn small" data-act="bulk-meta" title="${t('Znovu vyhledat informace a plakáty')}">${t('Načíst informace')}</button>
    <button class="btn small" data-act="select-all">${t('Vybrat vše')}</button>
    <button class="btn small" data-act="clear-selection">${t('Zrušit výběr')}</button>`;
  const select = $('#bulk-cat');
  if ([...select.options].some((o) => o.value === previous)) select.value = previous;
}

// ---------- modal dialogs ----------

/** Shows `html` in a modal; returns the root element and a close function. */
function openModal(html, { wide = false, extraClass = '', onClose } = {}) {
  closeCurrentModal?.();
  const root = $('#modal-root');
  root.innerHTML = `<div class="modal-backdrop"><div class="modal ${wide ? 'wide' : ''} ${extraClass}">${html}</div></div>`;
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    closeCurrentModal = null;
    root.innerHTML = '';
    document.removeEventListener('keydown', onKey, true);
    onClose?.();
    syncHistory();
  };
  closeCurrentModal = close;
  syncHistory();
  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
  };
  document.addEventListener('keydown', onKey, true);
  root.querySelector('.modal-backdrop').addEventListener('mousedown', (e) => {
    if (e.target === e.currentTarget) close();
  });
  return { el: root.querySelector('.modal'), close };
}

/** Simple confirm / prompt dialog. Resolves to the input text, true, or null when cancelled. */
function modal({ title, text = '', input = null, okLabel = 'OK', danger = false }) {
  return new Promise((resolve) => {
    let result = null;
    const { el, close } = openModal(`
      <form>
        <h2>${esc(title)}</h2>
        ${text ? `<p>${esc(text)}</p>` : ''}
        ${input !== null ? `<input id="modal-input" maxlength="60" autocomplete="off" spellcheck="false" value="${esc(input)}" />` : ''}
        <div class="modal-actions">
          <button type="button" class="btn" data-act="cancel">${t('Zrušit')}</button>
          <button type="submit" class="btn ${danger ? 'danger' : 'primary'}">${esc(okLabel)}</button>
        </div>
      </form>`, { onClose: () => resolve(result) });
    const field = el.querySelector('#modal-input');
    el.querySelector('form').addEventListener('submit', (e) => {
      e.preventDefault();
      result = field ? field.value : true;
      close();
    });
    el.querySelector('[data-act="cancel"]').addEventListener('click', close);
    if (field) {
      field.focus();
      field.select();
    } else {
      el.querySelector('[type="submit"]').focus();
    }
  });
}

function openMatchPicker(m) {
  const { el, close } = openModal(`
    <h2>${m.isShow ? t('Vyhledat seriál na TMDB') : t('Vyhledat film na TMDB')}</h2>
    <p class="muted" style="word-break:break-all">${esc(m.isShow ? m.folder : m.fileName)}</p>
    <form class="match-search">
      <input id="match-query" class="input" value="${esc(m.originalTitle || m.title)}" placeholder="${m.isShow ? t('Název seriálu') : t('Název filmu')}" autocomplete="off" spellcheck="false" />
      <input id="match-year" class="year" value="${m.year || ''}" placeholder="${t('Rok')}" inputmode="numeric" maxlength="4" />
      <button class="btn primary" type="submit">${t('Hledat')}</button>
    </form>
    <div id="match-list" class="match-list"></div>
    <div class="modal-actions"><button class="btn" data-act="close">${t('Zavřít')}</button></div>`, { wide: true });
  for (const input of el.querySelectorAll('input')) {
    Object.assign(input.style, { height: '38px', padding: '0 10px', borderRadius: '7px', border: '1px solid var(--line)', background: 'var(--bg)', outline: 'none' });
  }
  const list = el.querySelector('#match-list');
  let results = [];

  const search = async () => {
    const query = el.querySelector('#match-query').value.trim();
    const year = parseInt(el.querySelector('#match-year').value, 10);
    if (!query) return;
    list.innerHTML = `<div class="muted">${t('Hledám…')}</div>`;
    results = (await call('search_tmdb', { query, year: Number.isFinite(year) ? year : null, tv: !!m.isShow })) || [];
    list.innerHTML = results.length
      ? results.map((r, i) => `
        <button class="match-item" data-i="${i}">
          ${r.posterUrl ? `<img src="${esc(r.posterUrl)}" alt="" loading="lazy" />` : '<div class="no-img"></div>'}
          <div>
            <div class="t">${esc(r.title)}${r.year ? ` (${r.year})` : ''}</div>
            ${r.originalTitle && r.originalTitle !== r.title ? `<div class="o">${esc(r.originalTitle)}</div>` : ''}
            <div class="d">${esc(r.overview)}</div>
          </div>
        </button>`).join('')
      : `<div class="muted">${t('Nic nenalezeno – zkuste jiný název nebo bez roku.')}</div>`;
  };

  el.querySelector('form').addEventListener('submit', (e) => {
    e.preventDefault();
    search();
  });
  el.addEventListener('click', async (e) => {
    if (e.target.closest('[data-act="close"]')) return close();
    const item = e.target.closest('.match-item');
    if (!item) return;
    list.innerHTML = `<div class="muted">${t('Načítám informace…')}</div>`;
    const updated = await call(m.isShow ? 'apply_tmdb_show_match' : 'apply_tmdb_match', { id: m.id, tmdbId: results[+item.dataset.i].id });
    if (updated) {
      (m.isShow ? updateShow : updateMovie)(updated);
      close();
      toast(t('Přiřazeno: {title}', { title: updated.title }), 'ok');
    } else {
      search();
    }
  });
  search();
}

async function openSettings() {
  const { el, close } = openModal('', { wide: true, extraClass: 'settings', onClose: () => { refreshSettings = null; } });

  const draw = async () => {
    const s = await call('get_settings');
    if (!s) return;
    state.settings = s;
    const keyInput = el.querySelector('#tmdb-key');
    const keyDraft = keyInput && keyInput.value !== (s.tmdbKey || '') ? keyInput.value : null;
    const active = s.vlcPath || s.vlcDetected;
    const android = s.platform === 'android';
    const storage = android ? await call('storage_access', { request: false }) : true;
    el.innerHTML = `
      <h2>${t('Nastavení')}</h2>

      <div class="setting">
        <div class="title">${t('Jazyk aplikace')} / ${LANG === 'cs' ? 'Language' : 'Jazyk'}</div>
        <div class="row" style="gap:18px">
          ${[['cs', 'Čeština'], ['en', 'English']].map(([code, name]) => `
            <label class="radio-row" style="margin-top:0"><input type="radio" name="app-lang" value="${code}" ${LANG === code ? 'checked' : ''} />
              <span>${name}</span></label>`).join('')}
        </div>
      </div>

      <div class="setting">
        <div class="title">${t('Zobrazení')}</div>
        <label class="check-row" style="margin-top:0"><input type="checkbox" data-pref="homeRows" ${state.homeRows ? 'checked' : ''} />
          <span>${t('Řady nad seznamem')}<small>${t('Pokračovat ve sledování, nově přidané a neviděné z oblíbených kategorií – když není zapnutý žádný filtr.')}</small></span></label>
      </div>

      <div class="setting">
        <div class="title">${t('Složky s filmy a seriály')}</div>
        ${state.libraries.length ? state.libraries.map((l) => `
          <div class="lib-row">
            <div class="p">${esc(l.path)}
              <small class="${l.online ? '' : 'off'}">${l.online ? `${l.count} ${plural(l.count, 'video', 'videa', 'videí')}` : t('Nedostupná – disk není připojený nebo se složka přesunula')}</small>
            </div>
            <button class="btn small" data-act="relocate" data-id="${l.id}">${t('Změnit umístění…')}</button>
            <button class="btn small" data-act="remove" data-id="${l.id}">${t('Odebrat')}</button>
          </div>`).join('') : `<div class="value">${t('Zatím žádné složky.')}</div>`}
        <div class="row" style="margin-top:10px">
          <button class="btn small primary" data-act="add-library">+ ${t('Přidat složku')}</button>
        </div>
        <label class="check-row"><input type="checkbox" data-opt="watchFolders" ${s.watchFolders ? 'checked' : ''} />
          <span>${t('Hlídat složky')}<small>${t('Nové, přesunuté a smazané soubory se projeví samy, stejně jako připojení nebo odpojení disku.')}</small></span></label>
        <div class="muted" style="margin-top:10px">${android
          ? t('Filmy i epizody se poznají i po přejmenování, přesunu nebo výměně SD karty či USB disku. Stačí u složky zvolit „Změnit umístění“.')
          : t('Filmy i epizody se poznají i po přejmenování, přesunu nebo změně písmena disku. Stačí u složky zvolit „Změnit umístění“.')}</div>
        ${state.missing ? `
          <div class="row" style="margin-top:10px;align-items:center">
            <span class="muted" style="flex:1">${t('{n} o souborech, které už neexistují (uložené kategorie a pozice).', { n: `${state.missing} ${plural(state.missing, 'záznam', 'záznamy', 'záznamů')}` })}</span>
            <button class="btn small" data-act="cleanup">${t('Odstranit')}</button>
          </div>` : ''}
      </div>

      <div class="setting">
        <div class="title">${t('Informace o filmech a seriálech (TMDB)')}</div>
        <div class="value ${s.tmdbKey ? 'ok' : ''}">${s.tmdbKey ? `✓ ${t('Klíč je nastavený')}` : t('Pro plakáty, popisy, hodnocení a žánry zadejte bezplatný klíč z themoviedb.org (Nastavení účtu → API).')}</div>
        ${s.tmdbKey ? '' : `<div class="row" style="margin-bottom:10px"><button class="btn small" type="button" data-link="tmdb-api">${t('Získat klíč na themoviedb.org')}</button></div>`}
        <form class="input-row" id="tmdb-form">
          <input id="tmdb-key" placeholder="${t('API klíč nebo API Read Access Token')}" autocomplete="off" spellcheck="false" value="${esc(keyDraft ?? (s.tmdbKey || ''))}" />
          <button class="btn small primary" type="submit">${t('Uložit')}</button>
          ${s.tmdbKey ? `<button class="btn small" type="button" data-act="remove-key">${t('Odebrat')}</button>` : ''}
        </form>
        <label class="check-row"><input type="checkbox" data-opt="autoMetadata" ${s.autoMetadata ? 'checked' : ''} />
          <span>${t('Automaticky načítat informace o nových filmech a seriálech')}<small>${t('Po každém načtení složek, na pozadí.')}</small></span></label>
        <label class="check-row"><input type="checkbox" data-opt="genresAsCategories" ${s.genresAsCategories ? 'checked' : ''} />
          <span>${t('Přidávat žánry jako kategorie')}<small>${t('Např. Komedie, Drama, Sci-fi – kategorie se vytvoří automaticky.')}</small></span></label>
        <label class="check-row"><input type="checkbox" data-opt="videoThumbnails" ${s.videoThumbnails ? 'checked' : ''} />
          <span>${t('Náhled z videa u filmů a seriálů bez plakátu')}<small>${android
            ? t('Snímek z filmu vytvořený systémovým dekodérem Androidu; funguje i bez TMDB klíče.')
            : t('Snímek z filmu vytvořený pomocí VLC; funguje i bez TMDB klíče.')}</small></span></label>
        <div class="row" style="margin-top:12px">
          <button class="btn small" data-act="fetch-missing" ${state.job ? 'disabled' : ''}>${t('Načíst chybějící teď')}</button>
          <button class="btn small" data-act="fetch-all" ${state.job || !s.tmdbKey ? 'disabled' : ''}>${t('Znovu vyhledat vše')}</button>
        </div>
        ${tmdbNoticeHtml()}
      </div>

      ${android ? `
      <div class="setting">
        <div class="title">${t('Přehrávač VLC pro Android')}</div>
        <div class="value ${active ? 'ok' : 'bad'}">${active ? `✓ ${esc(active)}` : t('VLC pro Android není nainstalovaný.')}</div>
        <div class="row">
          ${active ? '' : `<button class="btn small primary" data-act="install-vlc">${t('Nainstalovat z Google Play')}</button>`}
          <button class="btn small" data-act="refresh">${t('Zkontrolovat znovu')}</button>
        </div>
        <div class="muted" style="margin-top:10px">${t('Po zavření VLC si Moviary uloží, kde jste skončili. VLC musí mít povolený přístup k souborům (zeptá se při prvním spuštění).')}</div>
      </div>

      <div class="setting">
        <div class="title">${t('Přístup k souborům')}</div>
        <div class="value ${storage ? 'ok' : 'bad'}">${storage ? `✓ ${t('Povoleno')}` : t('Není povoleno – Moviary nevidí video soubory.')}</div>
        ${storage ? '' : `<div class="row"><button class="btn small primary" data-act="grant-storage">${t('Povolit přístup ke všem souborům')}</button></div>`}
      </div>` : `
      <div class="setting">
        <div class="title">${t('Přehrávač VLC')}</div>
        <div class="value ${active ? 'ok' : 'bad'}">
          ${active ? esc(active) : t('VLC nebyl nalezen – nainstalujte jej z videolan.org nebo vyberte vlc.exe ručně.')}
          ${s.vlcPath ? ` (${t('nastaveno ručně')})` : active ? ` (${t('nalezeno automaticky')})` : ''}
        </div>
        <div class="row">
          <button class="btn small" data-act="pick-vlc">${t('Vybrat vlc.exe…')}</button>
          ${s.vlcPath ? `<button class="btn small" data-act="auto-vlc">${t('Použít automatickou detekci')}</button>` : ''}
        </div>
      </div>`}

      <div class="setting">
        <div class="title">${t('Přehrávání seriálů')}</div>
        <label class="check-row" style="margin-top:0"><input type="checkbox" data-opt="autoplayNext" ${s.autoplayNext ? 'checked' : ''} />
          <span>${t('Po skončení epizody pustit další')}<small>${android
            ? t('Další díl se spustí sám, když epizoda doběhne do konce.')
            : t('Další díl se spustí sám, když epizoda doběhne do konce, nebo když začnou titulky, pokud je přeskakujete.')}</small></span></label>
        <label class="check-row"><input type="checkbox" data-opt="skipIntro" ${s.skipIntro ? 'checked' : ''} />
          <span>${t('Přeskakovat intro a rekapitulaci')}<small>${android
            ? t('Jen když intro začíná hned na začátku epizody – VLC pro Android se během přehrávání nedá ovládat.')
            : t('VLC intro přeskočí samo. Když se do něj vrátíte, nechá ho běžet.')}</small></span></label>
        ${android ? '' : `<label class="check-row"><input type="checkbox" data-opt="skipCredits" ${s.skipCredits ? 'checked' : ''} />
          <span>${t('Přeskakovat závěrečné titulky')}<small>${t('Epizoda skončí, jakmile začnou titulky. Scéna po titulcích se nepřeskočí.')}</small></span></label>`}
        <label class="check-row"><input type="checkbox" data-opt="autoSegments" ${s.autoSegments ? 'checked' : ''} />
          <span>${t('Automaticky stahovat časy intra a titulků')}<small>${t('Po každém načtení složek, na pozadí, pro epizody, které je ještě nemají. Jen když je zapnuté přeskakování.')}</small></span></label>
        ${introDbNoticeHtml()}
      </div>

      ${android ? '' : trackRulesHtml(s.tracks)}

      <div class="setting">
        <div class="title">${t('Aktualizace')}</div>
        <div class="value">${t('Verze {version}', { version: esc(s.version || '') })}</div>
        <label class="check-row" style="margin-top:0"><input type="checkbox" data-opt="checkUpdates" ${s.checkUpdates ? 'checked' : ''} />
          <span>${t('Hlídat nové verze')}<small>${t('Jednou denně se podívá na GitHub, jestli nevyšla nová verze, a nabídne její instalaci.')}</small></span></label>
        <div class="row" style="margin-top:10px">
          <button class="btn small" data-act="check-update">${t('Zkontrolovat teď')}</button>
          <button class="btn small" type="button" data-link="releases">${t('Stránka vydání')}</button>
        </div>
      </div>

      ${await backupsHtml(android)}

      <div class="setting">
        <div class="title">${t('Datový soubor')}</div>
        <div class="value">${esc(s.dataPath)}</div>
        <div class="muted">${t('Kategorie, pozice přehrávání a informace o filmech jsou uloženy v tomto souboru, plakáty ve složce „posters“ vedle něj.')}</div>
      </div>
      <div class="modal-actions">
        <button class="btn" data-act="about" style="margin-right:auto">${t('O aplikaci a licence')}</button>
        <button class="btn primary" data-act="close">${t('Zavřít')}</button>
      </div>`;
  };

  refreshSettings = draw;
  await draw();

  el.addEventListener('submit', async (e) => {
    e.preventDefault();
    const key = el.querySelector('#tmdb-key').value.trim();
    if (!key) return;
    const s = await withLoading(t('Ověřuji klíč u TMDB…'), () => call('set_tmdb_key', { key }));
    if (!s) return;
    state.settings = s;
    toast(t('TMDB klíč je ověřený a uložený.'), 'ok');
    await draw();
    render();
    startMetadata(null, false);
  });

  el.addEventListener('change', async (e) => {
    if (e.target.name === 'app-lang') return switchLanguage(e.target.value);
    if (e.target.dataset.pref === 'homeRows') {
      state.homeRows = e.target.checked;
      savePrefs();
      render();
      return;
    }
    if (await onTrackRulesChange(e, draw)) return;
    if (!e.target.dataset.opt) return;
    const opts = {};
    for (const box of el.querySelectorAll('[data-opt]')) opts[box.dataset.opt] = box.checked;
    const s = await call('set_options', opts);
    if (s) state.settings = s;
  });

  el.addEventListener('click', async (e) => {
    const link = e.target.closest('[data-link]');
    if (link) return openLink(link.dataset.link);
    const btn = e.target.closest('[data-act]');
    if (!btn) return;
    const act = btn.dataset.act;
    const id = btn.dataset.id ? +btn.dataset.id : null;
    if (act === 'about') return openAbout();
    if (act === 'check-update') return checkForUpdate(true);
    if (await onTrackRulesClick(btn, draw)) return;
    if (await onBackupClick(btn, draw)) return;
    if (act === 'close') close();
    else if (act === 'add-library') addLibrary();
    else if (act === 'relocate') relocateLibrary(id);
    else if (act === 'remove') {
      await removeLibrary(id);
      openSettings();
    } else if (act === 'cleanup') {
      const data = await call('cleanup_missing');
      if (data) {
        applyData(data);
        toast(t('Záznamy o chybějících souborech byly odstraněny.'), 'ok');
      }
    } else if (act === 'remove-key') {
      const s = await call('set_tmdb_key', { key: '' });
      if (s) {
        state.settings = s;
        await draw();
        render();
      }
    } else if (act === 'fetch-missing') {
      startMetadata(null, false, true);
    } else if (act === 'fetch-all') {
      const ok = await modal({
        title: t('Znovu vyhledat všechny filmy a seriály?'),
        text: t('Informace a plakáty všech filmů a seriálů se znovu načtou z TMDB. Ručně vybrané shody mohou být nahrazeny automatickými.'),
        okLabel: t('Vyhledat'),
      });
      if (ok) startMetadata(null, true, true);
      else openSettings();
    } else if (act === 'pick-vlc') {
      const file = await dialog.open({ title: t('Vyberte vlc.exe'), multiple: false, directory: false, filters: [{ name: 'VLC', extensions: ['exe'] }] });
      if (file && (await call('set_vlc_path', { path: file }))) draw();
    } else if (act === 'auto-vlc') {
      if (await call('set_vlc_path', { path: null })) draw();
    } else if (act === 'install-vlc') {
      call('install_vlc');
    } else if (act === 'refresh') {
      draw();
    } else if (act === 'grant-storage') {
      if (await call('storage_access', { request: true })) {
        await draw();
        rescan();
      } else {
        draw();
      }
    }
  });
}

// ---------- technical filters ----------

function renderTechFilters(items) {
  const html = TECH_GROUPS.map((g) => {
    const counts = new Map();
    for (const m of items) for (const v of techValues(m, g.id)) counts.set(v, (counts.get(v) || 0) + 1);
    const selected = state.tech[g.id] || new Set();
    // A selected value stays visible even when nothing in this view has it.
    for (const v of selected) if (!counts.has(v)) counts.set(v, 0);
    if (!counts.size) return '';
    const rank = (v) => (g.order ? (g.order.indexOf(v) + 1 || 99) : TECH_LABELS[v] ? 1 : 0);
    const values = [...counts.keys()].sort((a, b) => rank(a) - rank(b) || counts.get(b) - counts.get(a) || collator.compare(techLabel(g.id, a), techLabel(g.id, b)));
    return `
      <div class="tech-group">
        <div class="tech-label">${t(g.label)}</div>
        <div class="tech-chips">${values.map((v) => `
          <button class="chip toggle small ${selected.has(v) ? 'on' : ''}" data-tech="${g.id}" data-value="${esc(v)}">${esc(techLabel(g.id, v))}<span class="n">${counts.get(v)}</span></button>`).join('')}
        </div>
      </div>`;
  }).join('');
  $('#tech-section').classList.toggle('hidden', !html);
  $('#tech-filters').innerHTML = html;
}

/** Number of active filters of each collapsible section. */
const SECTION_FILTERS = {
  saved: () => +state.savedFilters.some((s) => sameFilter(s.filter, currentFilter())),
  status: () => +(state.status !== 'all'),
  libraries: () => +(state.libraryFilter !== null),
  cats: () => state.selectedCats.size + state.uncategorized,
  tech: () => techCount(),
};

/** Collapsed sections hide their content; their active filters stay visible as a count. */
function renderSectionToggles() {
  for (const section of document.querySelectorAll('.sidebar [data-section]')) {
    const id = section.dataset.section;
    const collapsed = state.collapsed.has(id);
    section.classList.toggle('collapsed', collapsed);
    const toggle = section.querySelector('[data-toggle]');
    toggle.setAttribute('aria-expanded', String(!collapsed));
    toggle.querySelector('.badge-n').textContent = collapsed ? SECTION_FILTERS[id]() || '' : '';
  }
}

// ---------- tracks ----------

function channelsLabel(n) {
  if (n >= 8) return '7.1';
  if (n === 6) return '5.1';
  if (n === 2) return 'stereo';
  if (n === 1) return 'mono';
  return n ? t('{n} kanálů', { n }) : '';
}

function audioLabel(track, i, numbered = true) {
  return [
    numbered ? `${i + 1}.` : null,
    track.language ? langName(track.language) : t('neznámý jazyk'),
    [track.codec, channelsLabel(track.channels)].filter(Boolean).join(' '),
    track.name,
  ].filter(Boolean).join(' · ');
}

function subLabel(track, i, numbered = true) {
  return [
    numbered ? `${i + 1}.` : null,
    track.language ? langName(track.language) : t('neznámý jazyk'),
    track.forced ? t('vynucené') : null,
    track.codec,
    track.name,
  ].filter(Boolean).join(' · ');
}

function extLabel(x) {
  return [x.language ? langName(x.language) : t('neznámý jazyk'), x.forced ? t('vynucené') : null, x.fileName].filter(Boolean).join(' · ');
}

/** Lines of a track list; long lists are shortened. */
function trackLines(labels, limit = 8) {
  const shown = labels.slice(0, limit).map((l) => `<div>${esc(l)}</div>`).join('');
  const more = labels.length > limit ? `<div class="more">${t('… a dalších {n}', { n: labels.length - limit })}</div>` : '';
  return `<div class="track-list">${shown}${more}</div>`;
}

/** Video format and tracks of a movie, from the file headers or its name. */
function techInfoHtml(m) {
  const media = m.media;
  const tags = m.tags;
  const rows = [];
  const byName = `<span class="muted">(${t('podle názvu souboru')})</span>`;
  if (media?.video) {
    const v = media.video;
    const video = [v.codec, v.width ? `${v.width}×${v.height}` : null, tags.resolution, tags.hdr, v.bitDepth ? `${v.bitDepth} bit` : null];
    rows.push(`<div><span class="k">${t('Obraz')}</span>${esc(video.filter(Boolean).join(' · '))}</div>`);
  } else if (tags.resolution || tags.videoCodec || tags.hdr) {
    rows.push(`<div><span class="k">${t('Obraz')}</span>${esc([tags.videoCodec, tags.resolution, tags.hdr].filter(Boolean).join(' · '))} ${byName}</div>`);
  }
  if (media?.audio.length) {
    rows.push(`<div><span class="k">${t('Zvuk')}</span>${media.audio.length} ${plural(media.audio.length, 'stopa', 'stopy', 'stop')}</div>`);
    rows.push(trackLines(media.audio.map((a, i) => audioLabel(a, i))));
  } else if (tags.audioLanguages.length) {
    rows.push(`<div><span class="k">${t('Zvuk')}</span>${esc(tags.audioLanguages.map(langName).join(', '))} ${byName}</div>`);
  }
  const subs = [
    ...(media?.subtitles || []).map((s, i) => subLabel(s, i)),
    ...m.externalSubtitles.map((x) => `${t('soubor')}: ${extLabel(x)}`),
  ];
  if (subs.length) {
    rows.push(`<div><span class="k">${t('Titulky')}</span>${subs.length} ${plural(subs.length, 'stopa', 'stopy', 'stop')}</div>`);
    rows.push(trackLines(subs));
  } else if (media) {
    rows.push(`<div><span class="k">${t('Titulky')}</span>${t('žádné')}</div>`);
  }
  if (media) {
    rows.push(`<div><span class="k">${t('Soubor')}</span>${esc([media.container.toUpperCase(), media.duration ? fmtTime(media.duration) : null, fmtSize(m.size)].filter(Boolean).join(' · '))}</div>`);
  }
  return rows.length ? `<div class="tech-info">${rows.join('')}</div>` : `<div class="muted">${t('Technické údaje se nepodařilo zjistit.')}</div>`;
}

const SOURCE_TEXT = { movie: 'zvoleno u filmu', show: 'nastaveno u seriálu', rules: 'podle nastavení', file: 'jak určuje soubor' };

function playAudioText(m) {
  const p = m.playTracks;
  const audio = m.media?.audio || [];
  if (p.audio != null && audio[p.audio]) return audioLabel(audio[p.audio], p.audio, false);
  if (p.audioLanguages.length) return t('{langs} (je-li ve filmu)', { langs: p.audioLanguages.map(langName).join(', ') });
  const def = audio.find((track) => track.default) || audio[0];
  return def ? audioLabel(def, 0, false) : t('výchozí stopa souboru');
}

function playSubText(m) {
  const p = m.playTracks;
  switch (p.subtitle) {
    case 'off': return t('vypnuté');
    case 'track': return subLabel(m.media.subtitles[p.subtitleIndex], p.subtitleIndex, false);
    case 'file': return t('soubor {file}', { file: p.subtitleFile });
    case 'languages': return t('{langs} (jsou-li)', { langs: p.subtitleLanguages.map(langName).join(', ') });
    default:
      return m.externalSubtitles.length ? t('titulky vedle filmu (VLC je zapne samo)') : t('jak určí soubor');
  }
}

/** Option list value of a pick: "" = automatic, "off", "t:<index>", "f:<file>", "l:<language>". */
function pickValue(pick) {
  if (!pick) return '';
  if (pick.kind === 'off') return 'off';
  if (pick.kind === 'track') return `t:${pick.index}`;
  if (pick.kind === 'file') return `f:${pick.name}`;
  return `l:${pick.code}`;
}

function pickFromValue(v) {
  if (!v) return null;
  if (v === 'off') return { kind: 'off' };
  if (v.startsWith('t:')) return { kind: 'track', index: +v.slice(2) };
  if (v.startsWith('f:')) return { kind: 'file', name: v.slice(2) };
  return { kind: 'language', code: v.slice(2) };
}

const option = (value, label, current) => `<option value="${esc(value)}" ${value === current ? 'selected' : ''}>${esc(label)}</option>`;

/** Track choice and what will play, in the movie detail. */
function tracksHtml(m) {
  const media = m.media;
  const audioValue = pickValue(m.tracks?.audio);
  const subValue = pickValue(m.tracks?.subtitle);
  const audio = media && media.audio.length > 1
    ? `<label>${t('Zvuk')}<select data-track="audio">${option('', t('Automaticky'), audioValue)}${media.audio.map((track, i) => option(`t:${i}`, audioLabel(track, i), audioValue)).join('')}</select></label>`
    : '';
  const subs = [option('', t('Automaticky'), subValue), option('off', t('Vypnuté'), subValue)];
  if (media) subs.push(...media.subtitles.map((track, i) => option(`t:${i}`, subLabel(track, i), subValue)));
  subs.push(...m.externalSubtitles.map((x) => option(`f:${x.fileName}`, `${t('Soubor')}: ${extLabel(x)}`, subValue)));
  const p = m.playTracks;
  return `
    <div class="tracks-form">
      ${audio}
      <label>${t('Titulky')}<select data-track="subtitle">${subs.join('')}</select></label>
    </div>
    <div class="play-summary">${t('Přehraje se: zvuk {audio} {audioSource}<br>titulky {subtitle} {subtitleSource}', {
      audio: `<b>${esc(playAudioText(m))}</b>`,
      audioSource: `<span class="src">(${t(SOURCE_TEXT[p.audioSource])})</span>`,
      subtitle: `<b>${esc(playSubText(m))}</b>`,
      subtitleSource: `<span class="src">(${t(SOURCE_TEXT[p.subtitleSource])})</span>`,
    })}</div>
    ${media ? '' : `<div class="muted" style="margin-top:6px">${t('Stopy tohoto souboru se nepodařilo zjistit (např. AVI) – VLC vybere podle jazyků z nastavení.')}</div>`}`;
}

async function setMovieTracks(m, select) {
  const tracks = { audio: m.tracks?.audio ?? null, subtitle: m.tracks?.subtitle ?? null };
  tracks[select.dataset.track] = pickFromValue(select.value);
  const updated = await call('set_movie_tracks', { id: m.id, tracks });
  if (updated) updateMovie(updated);
}

/** Languages for all episodes of a show. */
function showTracksHtml(s) {
  const audio = new Set();
  const subs = new Set();
  for (const e of s.episodes) {
    for (const track of e.media?.audio || []) if (track.language) audio.add(track.language);
    for (const track of e.media?.subtitles || []) if (track.language) subs.add(track.language);
    for (const x of e.externalSubtitles) if (x.language) subs.add(x.language);
  }
  const sorted = (set) => [...set].sort((a, b) => collator.compare(langName(a), langName(b)));
  const audioValue = pickValue(s.tracks?.audio);
  const subValue = pickValue(s.tracks?.subtitle);
  return `
    <div class="tracks-form">
      <label>${t('Jazyk zvuku')}<select data-show-track="audio">
        ${option('', t('Podle nastavení'), audioValue)}${sorted(audio).map((l) => option(`l:${l}`, langName(l), audioValue)).join('')}
      </select></label>
      <label>${t('Titulky')}<select data-show-track="subtitle">
        ${option('', t('Podle nastavení'), subValue)}${option('off', t('Vypnuté'), subValue)}${sorted(subs).map((l) => option(`l:${l}`, langName(l), subValue)).join('')}
      </select></label>
    </div>
    <div class="muted" style="margin-top:8px">${t('U každé epizody se najde stopa v tomto jazyce; když ji epizoda nemá, platí globální nastavení.')}</div>`;
}

async function setShowTracks(s, select) {
  const tracks = { audio: s.tracks?.audio ?? null, subtitle: s.tracks?.subtitle ?? null };
  tracks[select.dataset.showTrack] = pickFromValue(select.value);
  const data = await call('set_show_tracks', { id: s.id, tracks });
  if (data) applyData(data);
}

// ---------- settings: track rules ----------

function langListHtml(list, which) {
  const items = list.map((code, i) => `
    <span class="lang-item"><span class="ord">${i + 1}.</span>${esc(langName(code))}
      ${i > 0 ? `<button data-act="lang-up" data-list="${which}" data-i="${i}" title="${t('Posunout dopředu')}">${ICONS.left}</button>` : ''}
      <button data-act="lang-remove" data-list="${which}" data-i="${i}" title="${t('Odebrat')}">${ICONS.close}</button>
    </span>`).join('');
  const options = (state.settings?.languages || [])
    .filter(([code]) => !list.includes(code))
    .sort((a, b) => collator.compare(a[1], b[1]))
    .map(([code, name]) => `<option value="${code}">${esc(name)}</option>`)
    .join('');
  return `
    <div class="lang-list">
      ${items || `<span class="muted">${t('žádný')}</span>`}
      <select data-lang-add="${which}"><option value="">+ ${t('Přidat jazyk…')}</option>${options}</select>
    </div>`;
}

const SUBTITLE_MODES = [
  ['auto', 'Podle souboru', 'Jak určí soubor a VLC (titulky uložené vedle filmu VLC zapne samo).'],
  ['off', 'Vypnuté', 'Filmy se pouští bez titulků; ve VLC je jde kdykoli zapnout.'],
  ['always', 'Vždy', 'V prvním dostupném jazyce ze seznamu níže; když žádný není, bez titulků.'],
  ['foreign', 'Jen u cizojazyčného zvuku', 'Když zvuk není v žádném z preferovaných jazyků zvuku. Jinak jen vynucené titulky (překlad cizojazyčných pasáží), pokud je film má.'],
];

function trackRulesHtml(r) {
  return `
    <div class="setting">
      <div class="title">${t('Zvuk a titulky při přehrávání')}</div>
      <div class="muted">${t('Platí pro všechny filmy a seriály. Přednost má volba u seriálu a nejvyšší volba u konkrétního filmu (v jeho detailu).')}</div>
      <div style="margin-top:12px"><b>${t('Preferované jazyky zvuku')}</b> <span class="muted">– ${t('první dostupný se pustí')}</span>${langListHtml(r.audioLanguages, 'audio')}</div>
      <div style="margin-top:12px"><b>${t('Titulky')}</b>
        ${SUBTITLE_MODES.map(([id, label, desc]) => `
          <label class="radio-row"><input type="radio" name="sub-mode" value="${id}" ${r.subtitles === id ? 'checked' : ''} />
            <span>${t(label)}<small>${t(desc)}</small></span></label>`).join('')}
      </div>
      ${['always', 'foreign'].includes(r.subtitles) ? `<div style="margin-top:12px"><b>${t('Preferované jazyky titulků')}</b>${langListHtml(r.subtitleLanguages, 'subs')}</div>` : ''}
    </div>`;
}

async function saveTrackRules(rules) {
  const data = await call('set_track_rules', { rules });
  if (!data) return;
  state.settings.tracks = rules;
  // Redraws the open settings too.
  applyData(data);
}

const ruleList = (which) => (which === 'audio' ? 'audioLanguages' : 'subtitleLanguages');

/** Handles a change in the track rules; true when the event was one of them. */
async function onTrackRulesChange(e) {
  const rules = structuredClone(state.settings.tracks);
  if (e.target.name === 'sub-mode') {
    rules.subtitles = e.target.value;
    // Subtitles in the preferred audio languages are a sensible start.
    if (['always', 'foreign'].includes(rules.subtitles) && !rules.subtitleLanguages.length) {
      rules.subtitleLanguages = rules.audioLanguages.length ? [rules.audioLanguages[0]] : [LANG];
    }
  } else if (e.target.dataset.langAdd) {
    if (!e.target.value) return true;
    const key = ruleList(e.target.dataset.langAdd);
    rules[key] = [...rules[key], e.target.value];
  } else {
    return false;
  }
  await saveTrackRules(rules);
  return true;
}

async function onTrackRulesClick(btn) {
  const act = btn.dataset.act;
  if (act !== 'lang-up' && act !== 'lang-remove') return false;
  const rules = structuredClone(state.settings.tracks);
  const list = rules[ruleList(btn.dataset.list)];
  const i = +btn.dataset.i;
  if (act === 'lang-remove') list.splice(i, 1);
  else [list[i - 1], list[i]] = [list[i], list[i - 1]];
  await saveTrackRules(rules);
  return true;
}

// ---------- settings: backups ----------

function fmtFileSize(bytes) {
  return bytes < 1024 ** 2 ? `${Math.max(1, Math.round(bytes / 1024))} kB` : fmtSize(bytes);
}

const BACKUP_KINDS = { auto: 'automatická', 'before-restore': 'před obnovením', manual: 'ruční' };

async function backupsHtml(android) {
  const list = (await call('list_backups')) || [];
  const rows = list.slice(0, 10).map((b) => `
    <div class="backup-row">
      <div class="p">${fmtDate(b.created)} · ${t(BACKUP_KINDS[b.kind])}
        <small>${b.movies} ${plural(b.movies, 'video', 'videa', 'videí')} · ${fmtFileSize(b.size)} · ${esc(b.name)}</small>
      </div>
      <button class="btn small" data-act="restore" data-path="${esc(b.path)}" data-created="${b.created}">${t('Obnovit')}</button>
    </div>`).join('');
  return `
    <div class="setting">
      <div class="title">${t('Záloha a obnova dat')}</div>
      <div class="muted">${t('Záloha obsahuje složky, kategorie, pozice přehrávání, historii, informace o filmech a nastavení. Plakáty ne – po obnovení se stáhnou znovu. Automatická záloha se dělá jednou denně, uchovává se posledních 7.')}</div>
      <div class="row" style="margin-top:10px">
        <button class="btn small primary" data-act="export-backup">${android ? t('Uložit zálohu do Stažených') : t('Exportovat zálohu…')}</button>
        ${android ? '' : `
          <button class="btn small" data-act="import-backup">${t('Obnovit ze souboru…')}</button>
          <button class="btn small" data-act="open-backups">${t('Otevřít složku záloh')}</button>`}
      </div>
      ${rows ? `<div style="margin-top:10px">${rows}</div>` : ''}
    </div>`;
}

/** Handles the backup buttons of the settings; true when the click was one of them. */
async function onBackupClick(btn, draw) {
  const act = btn.dataset.act;
  if (act === 'export-backup') {
    let path = null;
    if (!IS_ANDROID) {
      path = await dialog.save({
        title: t('Uložit zálohu Moviary'),
        defaultPath: `Moviary-backup-${new Date().toISOString().slice(0, 10)}.json`,
        filters: [{ name: t('Záloha Moviary'), extensions: ['json'] }],
      });
      if (!path) return true;
    }
    const written = await call('export_backup', { path });
    if (written) {
      toast(t('Záloha uložena: {path}', { path: written }), 'ok');
      draw();
    }
  } else if (act === 'import-backup') {
    const file = await dialog.open({
      title: t('Vyberte zálohu Moviary'),
      multiple: false,
      directory: false,
      filters: [{ name: t('Záloha Moviary'), extensions: ['json'] }],
    });
    if (file) await restoreBackup(file, null);
  } else if (act === 'restore') {
    await restoreBackup(btn.dataset.path, +btn.dataset.created);
  } else if (act === 'open-backups') {
    call('open_backups_folder');
  } else {
    return false;
  }
  return true;
}

async function restoreBackup(path, created) {
  const ok = await modal({
    title: t('Obnovit data ze zálohy?'),
    text: `${created ? `${t('Záloha z {date}.', { date: fmtDate(created) })} ` : ''}${t('Všechna současná data (složky, kategorie, pozice, historie a nastavení) se nahradí obsahem zálohy. Současný stav se předtím uloží jako záloha, takže se k němu můžete vrátit.')}`,
    okLabel: t('Obnovit'),
  });
  if (!ok) return;
  const data = await withLoading(t('Obnovuji data a prohledávám složky…'), () => call('restore_backup', { path }));
  if (!data) return;
  state.settings = (await call('get_settings')) || state.settings;
  // The restored data may have another language.
  if (state.settings?.language && state.settings.language !== LANG) await setLanguage(state.settings.language, false);
  applyData(data);
  toast(t('Data byla obnovena ze zálohy.'), 'ok');
  autoMetadata();
}

// ---------- statistics ----------

function fmtHours(seconds) {
  const h = seconds / 3600;
  if (h < 1) return `${Math.round(seconds / 60)} min`;
  if (h >= 100) return `${Math.round(h)} h`;
  return `${num(h, 1)} h`;
}

function fmtBytes(bytes) {
  return bytes >= 1024 ** 4 ? `${num(bytes / 1024 ** 4, 2)} TB` : fmtSize(bytes);
}

const fmtCount = (n) => n.toLocaleString(locale());

/** Horizontal bars of one series; `rows` = [{ label, value }]. */
function hbarsHtml(rows, fmt = fmtCount) {
  if (!rows.length) return `<div class="empty-note">${t('Zatím žádná data.')}</div>`;
  const max = Math.max(...rows.map((r) => r.value)) || 1;
  return `<div class="hbars">${rows.map((r) => `
    <div class="hbar" data-tip="${esc(`${r.label}: ${fmt(r.value)}`)}">
      <span class="name">${esc(r.label)}</span>
      <div class="track"><div class="fill" style="width:${(r.value / max) * 100}%"></div></div>
      <span class="val">${fmt(r.value)}</span>
    </div>`).join('')}</div>`;
}

/** Counts of values; `valuesOf(item)` gives the values of one item. Biggest first. */
function countBy(items, valuesOf, label = (v) => v, limit = 10) {
  const counts = new Map();
  for (const item of items) for (const v of valuesOf(item)) counts.set(v, (counts.get(v) || 0) + 1);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || collator.compare(label(a[0]), label(b[0])))
    .slice(0, limit)
    .map(([v, value]) => ({ label: label(v), value }));
}

function statCard(title, body, wide = false) {
  return `<div class="stat-card ${wide ? 'wide' : ''}"><h3>${title}</h3>${body}</div>`;
}

function statsHtml(history) {
  const files = state.movies;
  const episodes = files.filter((m) => m.showId != null);
  const titles = [...movieItems, ...showItems];
  const watchedMovies = movieItems.filter((m) => m.watched).length;
  const watchedEpisodes = episodes.filter((e) => e.watched).length;
  const totalSize = files.reduce((sum, m) => sum + m.size, 0);
  const totalLength = files.reduce((sum, m) => sum + (m.length || 0), 0);
  const unknownLength = files.filter((m) => !m.length).length;
  const watchedSeconds = history.reduce((sum, h) => sum + h.seconds, 0);
  const now = new Date();
  const monthStart = new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000;
  const thisMonth = history.filter((h) => h.at >= monthStart).reduce((sum, h) => sum + h.seconds, 0);
  const inProgress = files.filter((m) => m.position > 0).length;
  const pct = (a, b) => (b ? Math.round((a / b) * 100) : 0);

  const kpis = [
    [fmtCount(movieItems.length), t('filmů'), t('{n} zhlédnuto ({pct} %)', { n: watchedMovies, pct: pct(watchedMovies, movieItems.length) })],
    [fmtCount(showItems.length), t('seriálů'), t('{n} epizod · {watched} zhlédnuto', { n: episodes.length, watched: watchedEpisodes })],
    [fmtBytes(totalSize), t('velikost knihovny'), `${fmtCount(files.length)} ${plural(files.length, 'soubor', 'soubory', 'souborů')}`],
    [fmtHours(totalLength), t('délka všech videí'), unknownLength
      ? t('bez {n} s neznámou délkou', { n: unknownLength })
      : t('≈ {n} dní v kuse', { n: Math.round(totalLength / 86400) })],
    [fmtHours(watchedSeconds), t('nakoukáno v Moviary'), t('tento měsíc {time}', { time: fmtHours(thisMonth) })],
    [fmtCount(inProgress), t('rozkoukaných videí'), t('{n} kdy přehraných', { n: files.filter((m) => m.lastPlayed).length })],
  ];

  // Hours watched in each of the last 12 months.
  const months = [];
  for (let i = 11; i >= 0; i--) {
    const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
    const from = d.getTime() / 1000;
    const to = new Date(d.getFullYear(), d.getMonth() + 1, 1).getTime() / 1000;
    const events = history.filter((h) => h.at >= from && h.at < to);
    months.push({
      label: d.toLocaleString(locale(), { month: 'short' }),
      name: d.toLocaleString(locale(), { month: 'long', year: 'numeric' }),
      seconds: events.reduce((sum, h) => sum + h.seconds, 0),
      plays: events.length,
    });
  }
  const maxMonth = Math.max(...months.map((m) => m.seconds));
  const monthsHtml = history.length
    ? `<div class="cols">${months.map((m, i) => `
        <div class="col" data-tip="${esc(`${m.name}: ${fmtHours(m.seconds)} (${m.plays} ${plural(m.plays, 'přehrání', 'přehrání', 'přehrání')})`)}">
          ${m.seconds && (m.seconds === maxMonth || i === 11) ? `<span class="top">${fmtHours(m.seconds)}</span>` : ''}
          <div class="fill" style="height:${maxMonth ? (m.seconds / maxMonth) * 100 : 0}%"></div>
        </div>`).join('')}</div>
      <div class="col-labels">${months.map((m) => `<span>${m.label}</span>`).join('')}</div>`
    : `<div class="empty-note">${t('Statistika sledování se začne plnit po prvním přehrání filmu nebo epizody (počítá se, když se přehraje alespoň minuta).')}</div>`;

  const decades = countBy(titles.filter((m) => m.year), (m) => [Math.floor(m.year / 10) * 10], (d) => `${d}–${d + 9}`, 20)
    .sort((a, b) => collator.compare(a.label, b.label));
  const unknown = t('neznámé');
  const resolutionOrder = ['4K', '1080p', '720p', 'SD', unknown];
  const resolutions = countBy(files, (m) => [m.tags.resolution || unknown])
    .sort((a, b) => resolutionOrder.indexOf(a.label) - resolutionOrder.indexOf(b.label));
  const audio = countBy(files, (m) => (m.tags.audioLanguages.length ? m.tags.audioLanguages : ['?']), (l) => (l === '?' ? t('neznámý') : langName(l)));
  const subs = countBy(files, (m) => (m.tags.hasSubtitles ? (m.tags.subtitleLanguages.length ? m.tags.subtitleLanguages : ['?']) : ['-']),
    (l) => (l === '-' ? t('bez titulků') : l === '?' ? t('neznámý jazyk') : langName(l)));
  const folders = state.libraries
    .map((l) => ({ label: l.name, value: files.filter((m) => m.libraryId === l.id).reduce((sum, m) => sum + m.size, 0) }))
    .filter((r) => r.value)
    .sort((a, b) => b.value - a.value);

  const recent = [...history].reverse().slice(0, 8).map((h) => {
    const m = movieById(h.id);
    if (!m) return '';
    const show = m.showId != null ? showById(m.showId) : null;
    const title = show ? `${show.title} ${epCode(m)}` : m.title;
    return `<div class="row"><span title="${esc(title)}">${esc(title)}${h.finished ? ' ✓' : ''}</span><span>${fmtDate(h.at)} · ${fmtHours(h.seconds)}</span></div>`;
  }).join('');
  const top = titles.filter((m) => m.rating).sort((a, b) => b.rating - a.rating).slice(0, 8)
    .map((m) => `<div class="row"><span>${esc(m.title)}${m.year ? ` (${m.year})` : ''}</span><span>★ ${num(m.rating, 1)}</span></div>`).join('');

  return `
    <h2>${t('Statistiky')}</h2>
    <div class="kpis">${kpis.map(([v, l, s]) => `<div class="kpi"><div class="v">${v}</div><div class="l">${l}</div><div class="s">${s}</div></div>`).join('')}</div>
    <div class="stat-grid">
      ${statCard(t('Nakoukáno za posledních 12 měsíců'), monthsHtml, true)}
      ${statCard(t('Kategorie'), hbarsHtml(countBy(titles, (m) => m.categories)))}
      ${statCard(t('Žánry (TMDB)'), hbarsHtml(countBy(titles, (m) => m.genres)))}
      ${statCard(t('Podle roku vydání'), hbarsHtml(decades))}
      ${statCard(t('Rozlišení videí'), hbarsHtml(resolutions))}
      ${statCard(t('Jazyk zvuku'), hbarsHtml(audio))}
      ${statCard(t('Titulky'), hbarsHtml(subs))}
      ${statCard(t('Složky podle velikosti'), hbarsHtml(folders, fmtBytes))}
      ${statCard(t('Naposledy přehrané'), recent ? `<div class="stat-list">${recent}</div>` : `<div class="empty-note">${t('Zatím nic.')}</div>`)}
      ${statCard(t('Nejlépe hodnocené (TMDB)'), top ? `<div class="stat-list">${top}</div>` : `<div class="empty-note">${t('Hodnocení se načte s informacemi z TMDB.')}</div>`)}
    </div>
    <div class="modal-actions"><button class="btn primary" data-act="close">${t('Zavřít')}</button></div>`;
}

// ---------- about, credits and licenses ----------

/** The notice TMDB's terms of use require, word for word. */
const TMDB_NOTICE = 'This application uses TMDB and the TMDB APIs but is not endorsed, certified, or otherwise approved by TMDB.';

/** TMDB logo with the required notice (in English, with a Czech translation in Czech). */
function tmdbNoticeHtml() {
  return `
    <div class="tmdb-notice">
      <img src="assets/tmdb-logo.svg" alt="The Movie Database (TMDB)" />
      <div>
        <div>${TMDB_NOTICE}</div>
        ${LANG === 'cs' ? '<div class="muted">Tato aplikace používá TMDB a TMDB API, ale TMDB ji nepodporuje, necertifikuje ani jinak neschvaluje.</div>' : ''}
      </div>
    </div>`;
}

/** Credit for the intro and credits times (TheIntroDB asks for its name and a link). */
function introDbNoticeHtml() {
  return `
    <div class="muted" style="margin-top:10px">${t('Časy intra a titulků poskytuje TheIntroDB, komunitní databáze, do které je zadávají sami diváci, takže nemusí vždy sedět přesně. Stahují se podle TMDB ID seriálu (jen se zapnutým přeskakováním) po načtení složek, když pustíte epizodu, nebo tlačítkem na stránce seriálu. Moviary s TheIntroDB nijak nesouvisí.')}</div>
    <div class="row" style="margin-top:10px">
      <button class="btn small" type="button" data-link="theintrodb">theintrodb.org</button>
      <button class="btn small" type="button" data-link="theintrodb-terms">${t('Podmínky TheIntroDB')}</button>
    </div>`;
}

async function openAbout() {
  const s = state.settings || (await call('get_settings')) || {};
  const android = s.platform === 'android';
  const { el, close } = openModal(`
    <div class="about-head">
      <svg viewBox="0 0 24 24" class="brand-icon" aria-hidden="true"><rect x="2" y="3" width="20" height="18" rx="4" /><path d="M10 8.5v7l6-3.5z" fill="#fff" /></svg>
      <div>
        <h2>${t('Moviary')}</h2>
        <div class="muted">${t('Verze {version}', { version: esc(s.version || '') })}</div>
      </div>
    </div>
    <p>${t('Kategorizace filmů a seriálů a jejich přehrávání ve VLC s pamatováním pozice.')}</p>

    <div class="setting">
      <div class="title">${t('Informace o filmech a seriálech')}</div>
      ${tmdbNoticeHtml()}
      <div class="muted" style="margin-top:10px">${t('Názvy, popisy, plakáty, herci a hodnocení pocházejí z The Movie Database (TMDB) a stahují se s vaším vlastním API klíčem. Podle podmínek TMDB se uložené údaje obnovují, než jsou starší než 6 měsíců.')}</div>
      <div class="row" style="margin-top:10px">
        <button class="btn small" data-link="tmdb">themoviedb.org</button>
        <button class="btn small" data-link="tmdb-terms">${t('Podmínky TMDB API')}</button>
      </div>
    </div>

    <div class="setting">
      <div class="title">${t('Časy intra a titulků')}</div>
      ${introDbNoticeHtml()}
    </div>

    <div class="setting">
      <div class="title">${t('Přehrávač')}</div>
      <div class="muted">${android
        ? t('Videa přehrává VLC pro Android, které se instaluje zvlášť z Google Play. VLC je ochranná známka VideoLAN; Moviary s VideoLAN nijak nesouvisí.')
        : t('Videa přehrává VLC media player, který se instaluje zvlášť. VLC je ochranná známka VideoLAN; Moviary s VideoLAN nijak nesouvisí.')}</div>
      <div class="row" style="margin-top:10px"><button class="btn small" data-link="videolan">videolan.org</button></div>
    </div>

    <div class="setting">
      <div class="title">${t('Soukromí')}</div>
      <div class="muted">${t('Moviary nemá účty, reklamy ani sběr statistik. Do internetu se připojuje jen k TMDB (s vaším klíčem), k TheIntroDB kvůli časům intra a titulků (jen se zapnutým přeskakováním nebo na vyžádání) a ke GitHubu kvůli kontrole nových verzí (lze vypnout v Nastavení). Kategorie, pozice přehrávání a ostatní data zůstávají v tomto zařízení.')}</div>
    </div>

    <div class="setting">
      <div class="title">${t('Open source')}</div>
      <div class="muted">${t('Moviary je svobodný software pod licencí MIT. Je postavená na frameworku Tauri a dalších open-source knihovnách; jejich licence najdete níže.')}</div>
      <div class="row" style="margin-top:10px">
        <button class="btn small" data-act="licenses">${t('Licence a knihovny třetích stran')}</button>
        <button class="btn small" data-link="tauri">tauri.app</button>
      </div>
    </div>
    <div class="modal-actions">
      <button class="btn" data-link="repo" style="margin-right:auto">GitHub</button>
      <button class="btn" data-act="check-update">${t('Zkontrolovat aktualizace')}</button>
      <button class="btn primary" data-act="close">${t('Zavřít')}</button>
    </div>`, { wide: true, extraClass: 'about' });

  el.addEventListener('click', (e) => {
    const link = e.target.closest('[data-link]');
    if (link) return openLink(link.dataset.link);
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'close') close();
    else if (act === 'licenses') openLicenses();
    else if (act === 'check-update') checkForUpdate(true);
  });
}

// ---------- updates (GitHub Releases, see update.rs) ----------

/** Asks GitHub for a newer version; `manual` also reports when there is none. */
async function checkForUpdate(manual) {
  let info;
  try {
    info = manual
      ? await withLoading(t('Hledám novou verzi…'), () => invoke('check_update', { manual }))
      : await invoke('check_update', { manual });
  } catch (err) {
    if (manual) toast(String(err), 'error');
    return;
  }
  if (info) openUpdate(info);
  else if (manual) toast(t('Máte nejnovější verzi {version}.', { version: state.settings?.version || '' }), 'ok');
}

function openUpdate(info) {
  const android = IS_ANDROID;
  const installLabel = !info.canInstall
    ? t('Otevřít stránku ke stažení')
    : android ? t('Stáhnout a nainstalovat') : t('Aktualizovat a restartovat');
  const { el, close } = openModal(`
    <h2>${t('Je k dispozici nová verze {version}', { version: esc(info.version) })}</h2>
    <p>${t('Teď máte verzi {current}.', { current: esc(info.current) })}
      ${info.canInstall ? (android
        ? t('Aktualizace se stáhne ({size}) a Android se zeptá na její instalaci; data zůstanou zachovaná.', { size: fmtSize(info.size) })
        : t('Aktualizace se stáhne ({size}), program se nahradí novou verzí a znovu spustí; data zůstanou zachovaná.', { size: fmtSize(info.size) }))
      : t('Novou verzi stáhněte ze stránky vydání.')}</p>
    ${info.notes ? `<div class="title">${t('Co je nového')}</div><pre class="release-notes">${esc(info.notes)}</pre>` : ''}
    <div class="update-progress" hidden><div class="bar"><div></div></div><div class="muted"></div></div>
    <div class="modal-actions">
      <button class="btn" data-act="skip" style="margin-right:auto">${t('Přeskočit tuto verzi')}</button>
      ${info.canInstall ? `<button class="btn" data-link="releases">${t('Stránka vydání')}</button>` : ''}
      <button class="btn" data-act="later">${t('Později')}</button>
      <button class="btn primary" data-act="install">${installLabel}</button>
    </div>`, { wide: true, extraClass: 'update' });

  const progress = el.querySelector('.update-progress');
  const buttons = [...el.querySelectorAll('.modal-actions button')];
  state.updateProgress = (p) => {
    progress.hidden = false;
    progress.querySelector('.bar > div').style.width = `${p.total ? Math.round((p.done / p.total) * 100) : 0}%`;
    progress.querySelector('.muted').textContent = `${fmtSize(p.done)} / ${fmtSize(p.total)}`;
  };

  el.addEventListener('click', async (e) => {
    const link = e.target.closest('[data-link]');
    if (link) return openLink(link.dataset.link);
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'later') close();
    else if (act === 'skip') {
      await call('skip_update', { version: info.version });
      close();
    } else if (act === 'install') {
      if (!info.canInstall) {
        openLink('releases');
        return close();
      }
      buttons.forEach((b) => { b.disabled = true; });
      state.updateProgress({ done: 0, total: info.size });
      try {
        await invoke('install_update');
        // Windows restarts into the new version; Android shows its installer.
        progress.querySelector('.muted').textContent = android
          ? t('Potvrďte instalaci v okně Androidu.')
          : t('Spouštím novou verzi…');
      } catch (err) {
        toast(String(err), 'error');
        progress.hidden = true;
        buttons.forEach((b) => { b.disabled = false; });
      }
    }
  });
}

/** Opens one of the web pages the backend knows by name (see LINKS in lib.rs). */
async function openLink(name) {
  try {
    await invoke('open_link', { link: name });
  } catch (err) {
    toast(String(err), 'error');
  }
}

/** The license file is generated by scripts/licenses.js during the build. */
async function openLicenses() {
  let text = '';
  try {
    const response = await fetch('third-party-licenses.txt');
    if (response.ok) text = await response.text();
  } catch {}
  const { el, close } = openModal(`
    <h2>${t('Licence a knihovny třetích stran')}</h2>
    ${text ? `<pre class="licenses">${esc(text)}</pre>` : `<p>${t('Seznam licencí se vytváří při sestavení aplikace (npm run build).')}</p>`}
    <div class="modal-actions">
      <button class="btn" data-act="back">${t('Zpět')}</button>
      <button class="btn primary" data-act="close">${t('Zavřít')}</button>
    </div>`, { wide: true, extraClass: 'licenses-modal' });
  el.addEventListener('click', (e) => {
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'close') close();
    else if (act === 'back') openAbout();
  });
}

async function openStats() {
  const history = (await call('get_history')) || [];
  const { el, close } = openModal(statsHtml(history), { extraClass: 'stats' });
  el.addEventListener('click', (e) => {
    if (e.target.closest('[data-act="close"]')) close();
  });
}

// ---------- duplicates ----------

const DUPLICATE_REASONS = {
  file: 'Stejný soubor (kopie)',
  episode: 'Stejná epizoda',
  movie: 'Stejný film podle TMDB',
  title: 'Stejný název a rok',
};

function duplicateItemHtml(m, largest) {
  const show = m.showId != null ? showById(m.showId) : null;
  const title = show ? `${show.title} – ${epCode(m)} · ${m.title}` : `${m.title}${m.year ? ` (${m.year})` : ''}`;
  const tags = m.tags;
  const facts = [
    fmtSize(m.size),
    tags.resolution,
    tags.videoCodec,
    tags.hdr,
    tags.audioLanguages.length ? t('zvuk {langs}', { langs: tags.audioLanguages.map(langName).join(', ') }) : null,
    tags.subtitleLanguages.length ? t('titulky {langs}', { langs: tags.subtitleLanguages.map(langName).join(', ') }) : null,
    m.watched ? `✓ ${t('zhlédnuto')}` : m.position > 0 ? t('rozkoukáno {pct} %', { pct: Math.round(percent(m)) }) : null,
    m.available ? null : t('nedostupné'),
  ].filter(Boolean).join(' · ');
  const poster = m.poster || show?.poster;
  return `
    <div class="dup-item">
      ${poster ? `<img class="thumb" src="${posterUrl(poster)}" alt="" />` : '<div class="thumb"></div>'}
      <div class="info">
        <div class="t">${esc(title)}</div>
        <div class="p">${esc(m.path)}</div>
        <div class="f">${largest ? `<span class="largest">${t('největší')}</span> · ` : ''}${esc(facts)}</div>
      </div>
      <div class="acts">
        <button class="btn small" data-act="dup-open" data-id="${m.id}">${t('Detail')}</button>
        ${IS_ANDROID ? '' : `<button class="btn small" data-act="dup-reveal" data-id="${m.id}" ${m.available ? '' : 'disabled'}>${t('Ve složce')}</button>`}
      </div>
    </div>`;
}

async function openDuplicates() {
  const groups = (await call('find_duplicates')) || [];
  let reclaim = 0;
  let files = 0;
  const html = groups.map((g) => {
    const items = g.ids.map(movieById).filter(Boolean).sort((a, b) => b.size - a.size);
    if (items.length < 2) return '';
    reclaim += items.slice(1).reduce((sum, m) => sum + m.size, 0);
    files += items.length;
    return `
      <div class="dup-group">
        <h3>${t(DUPLICATE_REASONS[g.reason])} · ${items.length}×</h3>
        ${items.map((m, i) => duplicateItemHtml(m, i === 0)).join('')}
      </div>`;
  }).join('');
  const summary = html
    ? `<p>${t('{groups}, {files}. Kdybyste v každé skupině ponechali jen největší soubor, uvolní se <b>{size}</b>. Moviary nic nemaže – soubory odstraňte sami{hint}.', {
      groups: `${groups.length} ${plural(groups.length, 'skupina', 'skupiny', 'skupin')}`,
      files: `${files} ${plural(files, 'soubor', 'soubory', 'souborů')}`,
      size: fmtBytes(reclaim),
      hint: IS_ANDROID ? '' : t(' (tlačítko „Ve složce“)'),
    })}</p>`
    : `<p>${t('Žádné duplicity – každý film a epizoda je v knihovně jen jednou.')}</p>`;
  const { el, close } = openModal(`
    <h2>${t('Duplicity')}</h2>
    ${summary}
    ${html}
    <div class="modal-actions"><button class="btn primary" data-act="close">${t('Zavřít')}</button></div>`, { extraClass: 'dups' });
  el.addEventListener('click', (e) => {
    const btn = e.target.closest('[data-act]');
    if (!btn) return;
    const m = btn.dataset.id ? movieById(+btn.dataset.id) : null;
    if (btn.dataset.act === 'close') {
      close();
    } else if (btn.dataset.act === 'dup-reveal' && m) {
      call('reveal_in_explorer', { id: m.id });
    } else if (btn.dataset.act === 'dup-open' && m) {
      close();
      if (m.showId != null) {
        switchView('shows');
        openShowPage(m.showId);
        state.season = m.season;
        render();
      } else {
        switchView('movies');
        openDetail(m.id);
      }
    }
  });
}

// ---------- language ----------

/** Switches texts, number and date formats and sorting; `save` stores the choice (and tells the backend). */
async function setLanguage(lang, save = true) {
  LANG = lang === 'en' ? 'en' : 'cs';
  collator = new Intl.Collator(LANG, { sensitivity: 'base', numeric: true });
  $('#lang-code').textContent = LANG.toUpperCase();
  translatePage();
  if (IS_ANDROID) $('#search').placeholder = t('Hledat film, seriál, režiséra…');
  if (save) {
    const s = await call('set_language', { language: LANG });
    if (s) state.settings = s;
  }
}

/** Language chosen by the user: everything is drawn again (episode titles from the backend too). */
async function switchLanguage(lang) {
  if (lang === LANG) return;
  await setLanguage(lang);
  const data = await call('get_data');
  if (data) applyData(data);
  else render();
  // TMDB information loaded by older versions is Czech only: fetch the English part.
  const untranslated = [...state.movies, ...state.shows].some((m) => m.tmdbId && !m.translated);
  if (LANG === 'en' && untranslated && state.settings?.tmdbKey) startMetadata(null, false);
}

// ---------- home rows, "new" badge ----------

/** Rows at most this long. */
const ROW_LIMIT = 20;
/** Days a video counts as new. */
const NEW_DAYS = 3;

/** New for a few days after it was added – but not everything from the first scan. */
function isNew(m) {
  const now = Date.now() / 1000;
  return firstAdded !== null && m.added - firstAdded > 12 * 3600 && now - m.added < NEW_DAYS * 86400;
}

/** Small card of a home row or a "similar" list; `sub` is the line under the title. */
function rowCardHtml(m, sub = '') {
  return `
    <div class="row-card ${m.available ? '' : 'offline'}" data-kind="${m.isShow ? 'show' : 'movie'}" data-id="${m.id}" title="${esc(m.title)}">
      ${posterHtml(m, true, false)}
      <div class="rc-title">${esc(m.title)}</div>
      <div class="rc-sub">${sub ? esc(sub) : m.year || '&nbsp;'}</div>
    </div>`;
}

function rowHtml(title, items, sub) {
  return `
    <div class="home-row">
      <h3>${esc(title)}</h3>
      <div class="row-scroll">${items.map((m) => rowCardHtml(m, sub(m))).join('')}</div>
    </div>`;
}

/** Category counts of what has been watched (or is being watched), most watched first. */
function favoriteCategories() {
  const counts = new Map();
  for (const m of [...movieItems, ...showItems]) {
    if (!(m.watched || m.position > 0 || m.watchedCount > 0)) continue;
    for (const c of m.categories) counts.set(c, (counts.get(c) || 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3).map(([c]) => c);
}

/** Rows above the grid when nothing is filtered: continue watching, new, unwatched favourites. */
function homeRowsHtml() {
  if (!state.homeRows || hasFilters() || state.selection.size) return '';
  const rows = [];
  const continuing = [...movieItems.filter((m) => m.position > 0), ...showItems.filter((s) => s.inProgress && nextEpisode(s))]
    .filter((m) => m.available)
    .sort((a, b) => (b.lastPlayed || 0) - (a.lastPlayed || 0))
    .slice(0, ROW_LIMIT);
  if (continuing.length) {
    rows.push(rowHtml(t('Pokračovat ve sledování'), continuing, (m) => {
      if (!m.isShow) return m.duration ? t('zbývá {time}', { time: fmtTime(m.duration - m.position) }) : fmtTime(m.position);
      const next = nextEpisode(m);
      return `${epCode(next)} · ${next.title}`;
    }));
  }

  const upcoming = upcomingShows().slice(0, ROW_LIMIT);
  if (upcoming.length) {
    rows.push(rowHtml(t('Nové díly seriálů'), upcoming, (s) => {
      const n = s.nextEpisode;
      const days = daysUntil(n.airDate);
      return `${epCode({ season: n.season, episode: n.episode })} · ${days < 0 ? t('vyšel {date}', { date: airDateLabel(n.airDate) }) : airDateLabel(n.airDate)}`;
    }));
  }

  const items = viewItems();
  const wished = items
    .filter((m) => m.wishlist && !m.watched)
    .sort((a, b) => (b.myRating || 0) - (a.myRating || 0) || b.added - a.added)
    .slice(0, ROW_LIMIT);
  if (wished.length) {
    rows.push(rowHtml(t('Chci vidět'), wished, (m) => (m.year ? String(m.year) : '')));
  }

  const since = Date.now() / 1000 - 60 * 86400;
  const added = items
    .filter((m) => m.added >= since && firstAdded !== null && m.added - firstAdded > 12 * 3600)
    .sort((a, b) => b.added - a.added)
    .slice(0, ROW_LIMIT);
  if (added.length) {
    rows.push(rowHtml(t('Nově přidané'), added, (m) => new Date(m.added * 1000).toLocaleDateString(locale())));
  }

  const favorites = favoriteCategories();
  if (favorites.length) {
    const unwatched = items
      .filter((m) => (m.isShow ? !m.watchedCount && !m.inProgress : !m.watched && m.position <= 0))
      .filter((m) => m.categories.some((c) => favorites.includes(c)))
      .sort((a, b) => (b.rating || 0) - (a.rating || 0) || b.added - a.added)
      .slice(0, ROW_LIMIT);
    if (unwatched.length) {
      rows.push(rowHtml(t('Neviděné z oblíbených kategorií ({categories})', { categories: favorites.join(', ') }), unwatched,
        (m) => m.categories.filter((c) => favorites.includes(c)).join(', ')));
    }
  }
  if (!rows.length) return '';
  return `<div class="home-rows">${rows.join('')}</div><div class="grid-head">${t(VIEWS[state.view].all)}</div>`;
}

/** Opens the detail of a movie or the page of a show, switching the tab when needed. */
function openItem(m) {
  if (m.isShow) {
    switchView('shows');
    openShowPage(m.id);
  } else {
    switchView('movies');
    openDetail(m.id);
  }
}

/** Handles a click on a small card; true when it was one. */
function onRowCardClick(e) {
  const card = e.target.closest('.row-card');
  if (!card) return false;
  const id = +card.dataset.id;
  const m = card.dataset.kind === 'show' ? showById(id) : movieById(id);
  if (!m) return true;
  if (e.target.closest('[data-act="play"]')) {
    if (m.isShow) playShow(m);
    else play(m.id, false);
  } else {
    openItem(m);
  }
  return true;
}

// ---------- people, collections, similar titles ----------

/** Movies and shows filtered to a person (actor, director, creator) or a movie series. */
function matchesFocus(m, f = state.focus) {
  if (f.kind === 'collection') return !m.isShow && m.collection?.id === f.id;
  return [...(m.cast || []), ...(m.directors || m.creatorList || [])].some((p) => p.id === f.id);
}

function setFocus(kind, id, name) {
  state.focus = { kind, id, name };
  state.detailId = null;
  state.showId = null;
  state.selection.clear();
  if (kind === 'collection') state.view = 'movies';
  render();
  $('#grid').scrollTop = 0;
}

function renderFocusChip() {
  const chip = $('#focus-chip');
  const f = state.focus;
  chip.classList.toggle('hidden', !f);
  if (!f) {
    chip.innerHTML = '';
    return;
  }
  // The person may also be in the other tab.
  const other = f.kind === 'person' ? (state.view === 'movies' ? showItems : movieItems).filter(matchesFocus).length : 0;
  const otherNoun = state.view === 'movies' ? plural(other, 'seriál', 'seriály', 'seriálů') : plural(other, 'film', 'filmy', 'filmů');
  chip.innerHTML = `
    <span class="name">${f.kind === 'collection' ? t('Série') : t('Osoba')}: <b>${esc(f.name)}</b></span>
    ${other ? `<button class="other" data-act="focus-other">+ ${other} ${otherNoun}</button>` : ''}
    <button class="x" data-act="focus-clear" title="${t('Zrušit')}">${ICONS.close}</button>`;
}

const personButton = (p) =>
  `<button class="person" data-person="${p.id}" data-name="${esc(p.name)}"><b>${esc(p.name)}</b>${p.character ? `<small>${esc(p.character)}</small>` : ''}</button>`;

const personLink = (p) => `<button class="person-link" data-person="${p.id}" data-name="${esc(p.name)}">${esc(p.name)}</button>`;

/** Cast as clickable chips. */
function castHtml(cast) {
  return cast.length ? `<div class="people">${cast.slice(0, 12).map(personButton).join('')}</div>` : '';
}

/** Titles of the library TMDB recommends for `m`, in TMDB's order. */
function similarItems(m) {
  if (!m.similar?.length) return [];
  const pool = m.isShow ? showItems : movieItems;
  return m.similar
    .map((tmdbId) => pool.find((x) => x.tmdbId === tmdbId && x.id !== m.id))
    .filter(Boolean)
    .slice(0, 12);
}

function similarHtml(m) {
  const items = similarItems(m);
  return items.length ? `<div class="row-scroll">${items.map((x) => rowCardHtml(x)).join('')}</div>` : '';
}

/** Parts of the movie series `m` belongs to, with what the library has and what was watched. */
function collectionHtml(m) {
  const link = m.collection;
  const c = state.collections.find((x) => x.id === link.id);
  const parts = c?.parts || [];
  const inLibrary = (p) => movieItems.find((x) => x.tmdbId === p.tmdbId);
  const next = parts.map(inLibrary).find((x) => x && !x.watched && x.available);
  const rows = parts.map((p, i) => {
    const own = inLibrary(p);
    let status;
    if (!own) status = `<span class="s">${t('chybí v knihovně')}</span>`;
    else if (own.watched) status = `<span class="s ok">✓ ${t('zhlédnuto')}</span>`;
    else if (own.position > 0) status = `<span class="s progress-s">${Math.round(percent(own))} %</span>`;
    else status = `<span class="s">${t('v knihovně')}</span>`;
    const classes = ['part', own ? 'in' : 'missing', own?.id === m.id ? 'current' : ''].join(' ');
    return `
      <div class="${classes}" ${own ? `data-movie="${own.id}"` : ''}>
        <span class="n">${i + 1}.</span>
        <span class="t">${esc(p.title)}${p.year ? ` (${p.year})` : ''}</span>
        ${status}
      </div>`;
  }).join('');
  return `
    <div class="muted" style="margin-bottom:8px">${esc(link.name)}</div>
    ${rows ? `<div class="parts">${rows}</div>` : ''}
    <div class="collection-actions">
      ${next && next.id !== m.id ? `<button class="btn small primary" data-act="collection-next" data-id="${next.id}">${ICONS.play.replace('<svg', '<svg style="fill:#fff;stroke:none;width:14px;height:14px"')} ${t('Pokračovat dalším dílem: {title}', { title: esc(next.title) })}</button>` : ''}
      <button class="btn small" data-collection="${link.id}" data-name="${esc(link.name)}">${t('Zobrazit celou sérii')}</button>
    </div>`;
}

/** Clicks on people, series and small cards in the detail or on a show page; true when handled. */
function onLinkClick(e) {
  if (onRowCardClick(e)) return true;
  const person = e.target.closest('[data-person]');
  if (person) {
    setFocus('person', +person.dataset.person, person.dataset.name);
    return true;
  }
  const collection = e.target.closest('[data-collection]');
  if (collection) {
    setFocus('collection', +collection.dataset.collection, collection.dataset.name);
    return true;
  }
  const part = e.target.closest('[data-movie]');
  if (part) {
    openDetail(+part.dataset.movie);
    return true;
  }
  const next = e.target.closest('[data-act="collection-next"]');
  if (next) {
    play(+next.dataset.id, false);
    return true;
  }
  return false;
}

// ---------- "What tonight?" ----------

/** A random unwatched movie (or show) from the current selection. */
function openRandomPick() {
  const pool = visible.filter((m) => m.available && (m.isShow ? !m.watchedCount && !m.inProgress : !m.watched && m.position <= 0));
  if (!pool.length) {
    toast(state.view === 'shows' ? t('Ve výběru není žádný neviděný seriál.') : t('Ve výběru není žádný neviděný film.'));
    return;
  }
  const seen = new Set();
  const { el, close } = openModal('', { extraClass: 'random' });
  let current = null;

  const draw = () => {
    let left = pool.filter((m) => !seen.has(m.id));
    if (!left.length) {
      seen.clear();
      left = pool;
    }
    current = left[Math.floor(Math.random() * left.length)];
    seen.add(current.id);
    const m = current;
    const facts = [
      m.rating ? `★ ${num(m.rating, 1)}` : null,
      m.year,
      m.isShow ? `${m.episodeCount} ${plural(m.episodeCount, 'epizoda', 'epizody', 'epizod')}` : m.runtime ? fmtRuntime(m.runtime) : m.length ? fmtTime(m.length) : null,
    ].filter(Boolean).map((f) => `<span>${f}</span>`).join('');
    el.innerHTML = `
      <div class="muted" style="margin-bottom:12px">${t('Co dnes?')} · ${t('náhodně z {n} neviděných', { n: pool.length })}</div>
      <div class="random-pick">
        ${posterHtml(m, false)}
        <div class="info">
          <h2>${esc(m.title)}</h2>
          <div class="facts">${facts}</div>
          ${m.genres.length ? `<div class="genres">${m.genres.map((g) => `<span class="chip">${esc(g)}</span>`).join('')}</div>` : ''}
          ${m.overview ? `<div class="overview">${esc(m.overview)}</div>` : ''}
        </div>
      </div>
      <div class="modal-actions">
        <button class="btn" data-act="close">${t('Zavřít')}</button>
        <button class="btn" data-act="detail">${t('Detail')}</button>
        <button class="btn" data-act="another" ${pool.length > 1 ? '' : 'disabled'}>${t('Jiný')}</button>
        <button class="btn primary" data-act="play">${ICONS.play.replace('<svg', '<svg style="fill:#fff;stroke:none;width:14px;height:14px"')} ${t('Přehrát')}</button>
      </div>`;
  };

  el.addEventListener('click', (e) => {
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'close') close();
    else if (act === 'another') draw();
    else if (act === 'detail') {
      close();
      openItem(current);
    } else if (act === 'play') {
      close();
      if (current.isShow) playShow(current);
      else play(current.id, false);
    }
  });
  draw();
}

// ---------- actions ----------

/**
 * Resolves to `{ path, label }` or null. On Android `path` can be a folder URI (USB drives),
 * named by `label`.
 */
async function pickFolder(title, defaultPath) {
  if (IS_ANDROID) {
    // Without "access to all files" the folder is still usable through the document provider.
    await ensureStorage();
    return (await call('pick_folder')) || null;
  }
  const path = await dialog.open({ directory: true, multiple: false, title, defaultPath: defaultPath || undefined });
  return path ? { path, label: null } : null;
}

/** Android: the app needs "access to all files" to read the videos. Asks for it when missing. */
async function ensureStorage() {
  if (!IS_ANDROID || (await call('storage_access', { request: false }))) return true;
  const ok = await modal({
    title: t('Povolit přístup k souborům?'),
    text: t('Moviary potřebuje přístup ke všem souborům, aby mohla procházet složky s filmy a seriály (i na SD kartě nebo USB disku). Soubory jen čte, nic nemění. V následujícím okně zapněte „Povolit přístup ke správě všech souborů“ a vraťte se zpět.'),
    okLabel: t('Otevřít nastavení'),
  });
  if (!ok) return false;
  return !!(await call('storage_access', { request: true }));
}

async function addLibrary() {
  const dir = await pickFolder(t('Vyberte složku s filmy'));
  if (!dir) return;
  const episodes = (list) => list.filter((m) => m.showId != null).length;
  const before = { movies: state.movies.length - episodes(state.movies), episodes: episodes(state.movies) };
  const data = await withLoading(t('Prohledávám složku…'), () => call('add_library', { path: dir.path, label: dir.label ?? null }));
  if (!data) return;
  applyData(data);
  const newEpisodes = episodes(data.movies) - before.episodes;
  const newMovies = data.movies.length - newEpisodes - before.episodes - before.movies;
  const found = [`${newMovies} ${plural(newMovies, 'nový film', 'nové filmy', 'nových filmů')}`];
  if (newEpisodes) found.push(t('{n} {episodes} seriálů', { n: newEpisodes, episodes: plural(newEpisodes, 'epizodu', 'epizody', 'epizod') }));
  toast(t('Nalezeno {what}.', { what: found.join(` ${t('a')} `) }), 'ok');
  autoMetadata();
}

async function relocateLibrary(id) {
  const lib = state.libraries.find((l) => l.id === id);
  if (!lib) return;
  const dir = await pickFolder(t('Nové umístění složky „{name}“', { name: lib.name }), lib.online ? lib.path : undefined);
  if (!dir) return;
  const data = await withLoading(t('Prohledávám složku…'), () => call('relocate_library', { id, path: dir.path, label: dir.label ?? null }));
  if (data) {
    applyData(data);
    toast(t('Umístění složky bylo změněno.'), 'ok');
    autoMetadata();
  }
}

async function removeLibrary(id) {
  const lib = state.libraries.find((l) => l.id === id);
  if (!lib) return;
  const ok = await modal({
    title: t('Odebrat složku?'),
    text: t('Složka „{path}“ bude odebrána z Moviary. Soubory na disku zůstanou beze změny a kategorie i pozice filmů zůstanou uložené – obnoví se, pokud složku přidáte znovu.', { path: lib.path }),
    okLabel: t('Odebrat'),
    danger: true,
  });
  if (ok) {
    const data = await call('remove_library', { id });
    if (data) applyData(data);
  }
}

async function rescan() {
  if (!state.libraries.length) return;
  const data = await withLoading(t('Prohledávám složky…'), () => call('scan'));
  if (data) {
    applyData(data);
    autoMetadata();
  }
}

/** Runs the metadata job in the background after a scan when enabled. */
function autoMetadata() {
  // The metadata job downloads the intro and credits times after itself.
  if (state.settings?.autoMetadata) startMetadata(null, false);
  else invoke('fetch_segments_auto').catch(() => {});
}

async function startMetadata(ids, force, announce = false) {
  try {
    const count = await invoke('fetch_metadata', { ids, force });
    if (!count && announce) toast(t('Všechny filmy i seriály už mají informace i náhledy.'));
  } catch (err) {
    if (announce) toast(String(err), 'error');
  }
}

function onJobProgress(p) {
  state.job = p.running ? p : null;
  renderJob();
  if (!p.running) {
    if (p.error) toast(p.error, 'error');
    else if (p.done && state.settings?.tmdbKey) toast(t('Informace načteny: nalezeno {matched} z {done}.', { matched: p.matched, done: p.done }), 'ok');
    refreshSettings?.();
  }
}

async function play(id, fromStart) {
  const m = await call('play_movie', { id, fromStart });
  if (m) {
    updateMovie(m);
    toast(fromStart || !canResume(m) ? t('Spouštím VLC…') : t('Spouštím VLC od {time}…', { time: fmtTime(Math.max(0, m.position - 5)) }));
  }
}

async function createCategory(initial = '') {
  const name = await modal({ title: t('Nová kategorie'), input: initial, okLabel: t('Vytvořit') });
  if (!name || !name.trim()) return null;
  const data = await call('create_category', { name });
  if (!data) return null;
  applyData(data);
  return findCategory(name);
}

async function renameCategory(name) {
  const next = await modal({ title: t('Přejmenovat kategorii'), input: name, okLabel: t('Přejmenovat') });
  if (!next || next.trim() === name) return;
  const data = await call('rename_category', { old: name, new: next });
  if (!data) return;
  if (state.selectedCats.delete(name)) state.selectedCats.add(next.trim().replace(/\s+/g, ' '));
  applyData(data);
  savePrefs();
}

async function deleteCategory(name) {
  const count = (items) => items.filter((m) => m.categories.includes(name)).length;
  const [movies, shows] = [count(movieItems), count(showItems)];
  const from = [
    movies ? `${movies} ${plural(movies, 'filmu', 'filmů', 'filmů')}` : '',
    shows ? `${shows} ${plural(shows, 'seriálu', 'seriálů', 'seriálů')}` : '',
  ].filter(Boolean).join(` ${t('a')} `);
  const ok = await modal({
    title: t('Smazat kategorii?'),
    text: from
      ? t('Kategorie „{name}“ bude odstraněna ze {from}. Filmy a seriály samotné zůstanou beze změny.', { name, from })
      : t('Kategorie „{name}“ bude odstraněna. Filmy a seriály samotné zůstanou beze změny.', { name }),
    okLabel: t('Smazat'),
    danger: true,
  });
  if (!ok) return;
  const data = await call('delete_category', { name });
  if (data) {
    applyData(data);
    savePrefs();
  }
}

async function setCategories(m, categories) {
  if (m.isShow) {
    const updated = await call('set_show_categories', { id: m.id, categories });
    if (updated) updateShow(updated);
  } else {
    const updated = await call('set_movie_categories', { id: m.id, categories });
    if (updated) updateMovie(updated);
  }
}

function toggleCategory(m, name) {
  return setCategories(m, m.categories.includes(name) ? m.categories.filter((c) => c !== name) : [...m.categories, name]);
}

/** Adds the category typed into `input` to the movie or show, creating it when needed. */
async function addTypedCategory(input, getItem) {
  const raw = input.value.trim();
  if (!getItem() || !raw) return;
  let name = findCategory(raw);
  if (!name) {
    const data = await call('create_category', { name: raw });
    if (!data) return;
    applyData(data);
    name = findCategory(raw);
  }
  document.getElementById(input.id).value = '';
  const current = getItem();
  if (name && current && !current.categories.includes(name)) await setCategories(current, [...current.categories, name]);
  document.getElementById(input.id)?.focus();
}

/** Plays the episode to continue with, or the first one when all is watched. */
function playShow(s) {
  const next = nextEpisode(s) || s.episodes.find((e) => e.season !== 0) || s.episodes[0];
  if (!next.available) toast(t('Epizoda {code} je na odpojeném disku.', { code: epCode(next) }), 'error');
  else play(next.id, false);
}

function openShowPage(id) {
  state.showId = id;
  state.season = null;
  state.selection.clear();
  render();
  $('#show-page').scrollTop = 0;
}

function switchView(view) {
  if (state.view === view) {
    // Clicking the active tab returns from a show page to the list.
    if (state.showId !== null) {
      state.showId = null;
      render();
    }
    return;
  }
  state.view = view;
  state.selection.clear();
  lastClickedIndex = null;
  savePrefs();
  render();
}

async function bulkApply(add) {
  const select = $('#bulk-cat');
  let category = sortedCats[+select.value];
  if (select.value === 'new') category = await createCategory();
  if (!category) {
    if (select.value === '') toast(t('Nejprve zvolte kategorii.'));
    return;
  }
  const ids = [...state.selection];
  const data = await call('bulk_set_category', { ids, category, add });
  if (data) {
    applyData(data);
    const params = { category, n: `${ids.length} ${nounFor(ids.length)}` };
    toast(add ? t('Přidáno do „{category}“: {n}.', params) : t('Odebráno z „{category}“: {n}.', params), 'ok');
  }
}

function toggleSelect(index, range) {
  const m = visible[index];
  if (range && lastClickedIndex !== null && visible[lastClickedIndex]) {
    const [from, to] = [Math.min(lastClickedIndex, index), Math.max(lastClickedIndex, index)];
    for (let i = from; i <= to; i++) state.selection.add(visible[i].id);
  } else if (state.selection.has(m.id)) {
    state.selection.delete(m.id);
  } else {
    state.selection.add(m.id);
  }
  lastClickedIndex = index;
  render();
}

function openDetail(id) {
  state.detailId = id;
  render();
  $('#drawer').scrollTop = 0;
}

function setFiltersOpen(open) {
  state.filtersOpen = open && compactQuery.matches;
  $('.sidebar').classList.toggle('open', state.filtersOpen);
  $('#filters-backdrop').classList.toggle('open', state.filtersOpen);
  syncHistory();
}

/** Moves the search field to the toolbar on small screens. */
function applyLayout() {
  const compact = compactQuery.matches;
  document.body.classList.toggle('compact', compact);
  const search = $('.search');
  if (compact) $('.toolbar').prepend(search);
  else $('.sidebar-head').after(search);
  if (!compact) setFiltersOpen(false);
}

// ---------- Android back button ----------
// While a dialog, the filter panel, a selection, a show page or a movie detail is open, one
// history entry exists, so the system back button closes that instead of leaving the app.
let backGuard = false;
let ignorePop = false;
let syncPending = false;

const layerOpen = () =>
  !!closeCurrentModal || state.filtersOpen || state.selection.size > 0 || !!openShow() || (state.view === 'movies' && state.detailId !== null);

/** Adds or removes the history entry; deferred so a dialog replacing another one counts as one. */
function syncHistory() {
  if (!IS_ANDROID || syncPending) return;
  syncPending = true;
  setTimeout(() => {
    syncPending = false;
    if (ignorePop) return;
    const open = layerOpen();
    if (open && !backGuard) {
      history.pushState({ guard: true }, '');
      backGuard = true;
    } else if (!open && backGuard) {
      backGuard = false;
      ignorePop = true;
      history.back();
    }
  }, 0);
}

function closeTopLayer() {
  if (closeCurrentModal) closeCurrentModal();
  else if (state.filtersOpen) setFiltersOpen(false);
  else if (state.selection.size) {
    state.selection.clear();
    render();
  } else if (openShow()) {
    state.showId = null;
    render();
  } else if (state.detailId !== null) {
    state.detailId = null;
    render();
  }
}

window.addEventListener('popstate', () => {
  backGuard = false;
  if (ignorePop) ignorePop = false;
  else closeTopLayer();
  syncHistory();
});

function clearSearch() {
  state.search = '';
  $('#search').value = '';
  render();
}

// ---------- events ----------

function bindEvents() {
  $('#btn-add-library').addEventListener('click', addLibrary);
  $('#btn-add-library-2').addEventListener('click', addLibrary);
  $('#btn-rescan').addEventListener('click', rescan);
  $('#btn-settings').addEventListener('click', openSettings);
  $('#btn-stats').addEventListener('click', openStats);
  $('#btn-about').addEventListener('click', openAbout);
  $('#btn-duplicates').addEventListener('click', openDuplicates);
  $('#btn-random').addEventListener('click', openRandomPick);
  $('#focus-chip').addEventListener('click', (e) => {
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'focus-clear') {
      state.focus = null;
      render();
    } else if (act === 'focus-other') {
      switchView(state.view === 'movies' ? 'shows' : 'movies');
    }
  });
  $('#btn-lang').addEventListener('click', () => switchLanguage(LANG === 'cs' ? 'en' : 'cs'));
  $('.sidebar').addEventListener('click', (e) => {
    const toggle = e.target.closest('[data-toggle]');
    if (!toggle) return;
    const id = toggle.dataset.toggle;
    if (!state.collapsed.delete(id)) state.collapsed.add(id);
    savePrefs();
    renderSectionToggles();
  });

  $('#tech-filters').addEventListener('click', (e) => {
    const chip = e.target.closest('[data-tech]');
    if (!chip) return;
    const { tech, value } = chip.dataset;
    const selected = state.tech[tech] || new Set();
    if (!selected.delete(value)) selected.add(value);
    if (selected.size) state.tech[tech] = selected;
    else delete state.tech[tech];
    state.showId = null;
    savePrefs();
    render();
  });

  // Tooltips of the statistics charts.
  document.addEventListener('mouseover', (e) => {
    const target = e.target.closest('[data-tip]');
    const tip = $('#tooltip');
    if (!target) {
      tip.classList.add('hidden');
      return;
    }
    tip.textContent = target.dataset.tip;
    tip.classList.remove('hidden');
    const r = target.getBoundingClientRect();
    const left = Math.min(window.innerWidth - tip.offsetWidth - 8, Math.max(8, r.left + r.width / 2 - tip.offsetWidth / 2));
    tip.style.left = `${left}px`;
    tip.style.top = `${Math.max(8, r.top - tip.offsetHeight - 6)}px`;
  });
  $('#btn-add-cat').addEventListener('click', () => createCategory());
  $('#btn-save-filter').addEventListener('click', saveCurrentFilter);
  $('#saved-list').addEventListener('click', onSavedFilterClick);
  $('#btn-clear-filters').addEventListener('click', clearFilters);
  $('#btn-filters').addEventListener('click', () => setFiltersOpen(true));
  $('#btn-close-filters').addEventListener('click', () => setFiltersOpen(false));
  $('#filters-backdrop').addEventListener('click', () => setFiltersOpen(false));
  compactQuery.addEventListener('change', applyLayout);

  $('#job').addEventListener('click', (e) => {
    if (e.target.closest('[data-act="cancel-job"]')) {
      invoke('cancel_metadata');
      toast(t('Načítání informací se zastaví po aktuálním filmu.'));
    }
  });

  // Changing a filter on a show page goes back to the list it filters.
  $('.sidebar').addEventListener('click', (e) => {
    if (e.target.closest('.filter') && !e.target.closest('[data-act]')) state.showId = null;
  }, true);

  $('#search').addEventListener('input', (e) => {
    state.search = e.target.value;
    if (openShow()) {
      state.showId = null;
      render();
    } else {
      renderGrid();
    }
    $('#btn-clear-filters').classList.toggle('hidden', !hasFilters());
  });

  $('#sort').addEventListener('change', (e) => {
    state.sort = e.target.value;
    savePrefs();
    render();
  });

  $('#status-filters').addEventListener('click', (e) => {
    const btn = e.target.closest('[data-status]');
    if (!btn) return;
    state.status = btn.dataset.status;
    savePrefs();
    render();
  });

  $('#library-list').addEventListener('click', (e) => {
    const row = e.target.closest('[data-lib]');
    if (!row) return;
    const id = +row.dataset.lib;
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'relocate') return relocateLibrary(id);
    if (act === 'remove') return removeLibrary(id);
    state.libraryFilter = state.libraryFilter === id ? null : id;
    savePrefs();
    render();
  });

  $('#cat-mode').addEventListener('click', (e) => {
    const btn = e.target.closest('[data-mode]');
    if (!btn) return;
    state.catMode = btn.dataset.mode;
    savePrefs();
    render();
  });

  $('#cat-list').addEventListener('click', (e) => {
    if (e.target.closest('[data-uncategorized]')) {
      state.uncategorized = !state.uncategorized;
      if (state.uncategorized) state.selectedCats.clear();
      savePrefs();
      render();
      return;
    }
    const row = e.target.closest('[data-cat-i]');
    if (!row) return;
    const name = sortedCats[+row.dataset.catI];
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'rename') return renameCategory(name);
    if (act === 'delete') return deleteCategory(name);
    if (!state.selectedCats.delete(name)) state.selectedCats.add(name);
    state.uncategorized = false;
    savePrefs();
    render();
  });

  const grid = $('#grid');
  grid.addEventListener('click', (e) => {
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'add-library') return addLibrary();
    if (act === 'clear-filters') return clearFilters();
    if (onRowCardClick(e)) return;
    const card = e.target.closest('.card');
    if (!card) return;
    const index = +card.dataset.i;
    const m = visible[index];
    if (act === 'play') return m.isShow ? playShow(m) : play(m.id, false);
    if (act === 'select' || e.ctrlKey || e.shiftKey || state.selection.size) return toggleSelect(index, e.shiftKey);
    lastClickedIndex = index;
    if (m.isShow) openShowPage(m.id);
    else openDetail(m.id);
  });
  // Long press on a touch screen starts selecting (like Ctrl+click).
  grid.addEventListener('contextmenu', (e) => {
    const card = e.target.closest('.card');
    if (!card || !IS_ANDROID) return;
    e.preventDefault();
    navigator.vibrate?.(15);
    toggleSelect(+card.dataset.i, false);
  });
  grid.addEventListener('dblclick', (e) => {
    const card = e.target.closest('.card');
    if (!card || e.target.closest('[data-act]') || state.selection.size) return;
    const m = visible[+card.dataset.i];
    if (!m.isShow && m.available) play(m.id, false);
  });

  $('#mode-tabs').addEventListener('click', (e) => {
    const btn = e.target.closest('[data-view]');
    if (btn) switchView(btn.dataset.view);
  });

  const page = $('#show-page');
  page.addEventListener('click', async (e) => {
    const s = openShow();
    if (!s) return;
    if (onLinkClick(e)) return;
    if (onPersonalClick(e, s)) return;
    const chip = e.target.closest('[data-cat-i]');
    if (chip) return toggleCategory(s, sortedCats[+chip.dataset.catI]);
    const tab = e.target.closest('[data-season]');
    if (tab) {
      state.season = +tab.dataset.season;
      return render();
    }
    const link = e.target.closest('[data-link]');
    if (link) return openLink(link.dataset.link);
    const act = e.target.closest('[data-act]')?.dataset.act;
    const ep = s.episodes.find((x) => x.id === +e.target.closest('[data-ep]')?.dataset.ep);
    const season = s.seasons.find((x) => x.number === state.season);
    if (act === 'fetch-segments') {
      fetchSegments(s);
    } else if (act === 'back') {
      state.showId = null;
      render();
    } else if (act === 'play-next') {
      playShow(s);
    } else if (act === 'play-first') {
      const first = s.episodes.find((x) => x.season !== 0) || s.episodes[0];
      if (first.available) play(first.id, true);
      else toast(t('Epizoda {code} je na odpojeném disku.', { code: epCode(first) }), 'error');
    } else if (act === 'season-watched' && season) {
      const watched = !season.episodes.every((x) => x.watched);
      const data = await call('bulk_set_watched', { ids: season.episodes.map((x) => x.id), watched });
      if (data) applyData(data);
    } else if (act === 'ep-play' && ep) {
      play(ep.id, false);
    } else if (act === 'ep-restart' && ep) {
      play(ep.id, true);
    } else if (act === 'ep-reset' && ep) {
      const u = await call('reset_progress', { id: ep.id });
      if (u) updateMovie(u);
    } else if (act === 'ep-watched' && ep) {
      const u = await call('set_watched', { id: ep.id, watched: !ep.watched });
      if (u) updateMovie(u);
    } else if (act === 'ep-times' && ep) {
      openSkipTimesEditor(ep);
    } else if (act === 'ep-reveal' && ep) {
      call('reveal_in_explorer', { id: ep.id });
    } else if (act === 'match') {
      openMatchPicker(s);
    } else if (act === 'refetch') {
      startMetadata([s.id], true, true);
    } else if (act === 'clear-meta') {
      const u = await call('clear_show_metadata', { id: s.id });
      if (u) updateShow(u);
    } else if (act === 'settings') {
      openSettings();
    }
  });
  page.addEventListener('dblclick', (e) => {
    const row = e.target.closest('[data-ep]');
    if (!row || e.target.closest('[data-act]')) return;
    const ep = state.movies.find((m) => m.id === +row.dataset.ep);
    if (ep?.available && !ep.playing) play(ep.id, false);
  });
  page.addEventListener('submit', (e) => {
    e.preventDefault();
    addTypedCategory($('#show-cat-input'), openShow);
  });
  page.addEventListener('change', (e) => {
    const s = openShow();
    if (s && e.target.dataset.showTrack) setShowTracks(s, e.target);
  });
  for (const type of ['input', 'focusout']) page.addEventListener(type, onNoteEvent);

  const drawer = $('#drawer-content');
  drawer.addEventListener('click', async (e) => {
    const m = movieById(state.detailId);
    if (!m) return;
    if (onLinkClick(e)) return;
    if (onPersonalClick(e, m)) return;
    const chip = e.target.closest('[data-cat-i]');
    if (chip) return toggleCategory(m, sortedCats[+chip.dataset.catI]);
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'close') {
      state.detailId = null;
      render();
    } else if (act === 'resume') {
      play(m.id, false);
    } else if (act === 'restart') {
      play(m.id, true);
    } else if (act === 'reveal') {
      call('reveal_in_explorer', { id: m.id });
    } else if (act === 'toggle-watched') {
      const u = await call('set_watched', { id: m.id, watched: !m.watched });
      if (u) updateMovie(u);
    } else if (act === 'reset') {
      const u = await call('reset_progress', { id: m.id });
      if (u) updateMovie(u);
    } else if (act === 'match') {
      openMatchPicker(m);
    } else if (act === 'refetch') {
      startMetadata([m.id], true, true);
    } else if (act === 'clear-meta') {
      const u = await call('clear_metadata', { id: m.id });
      if (u) updateMovie(u);
    } else if (act === 'settings') {
      openSettings();
    }
  });
  drawer.addEventListener('submit', (e) => {
    e.preventDefault();
    const id = state.detailId;
    addTypedCategory($('#new-cat-input'), () => movieById(id));
  });
  drawer.addEventListener('change', (e) => {
    const m = movieById(state.detailId);
    if (m && e.target.dataset.track) setMovieTracks(m, e.target);
  });
  for (const type of ['input', 'focusout']) drawer.addEventListener(type, onNoteEvent);

  $('#bulkbar').addEventListener('click', (e) => {
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'bulk-add') bulkApply(true);
    if (act === 'bulk-remove') bulkApply(false);
    if (act === 'bulk-meta') startMetadata([...state.selection], true, true);
    if (act === 'bulk-wish') {
      call('bulk_set_wishlist', { ids: [...state.selection], wishlist: !selectionWished() }).then((data) => data && applyData(data));
    }
    if (act === 'select-all') {
      for (const m of visible) state.selection.add(m.id);
      render();
    }
    if (act === 'clear-selection') {
      state.selection.clear();
      render();
    }
  });
  $('#bulkbar').addEventListener('change', async (e) => {
    if (e.target.id === 'bulk-cat' && e.target.value === 'new') {
      const name = await createCategory();
      const index = sortedCats.indexOf(name);
      if ($('#bulk-cat')) $('#bulk-cat').value = index >= 0 ? String(index) : '';
    }
  });

  document.addEventListener('keydown', (e) => {
    if ($('#modal-root').childElementCount) return;
    if (e.ctrlKey && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      $('#search').focus();
      $('#search').select();
    } else if (e.key === 'F5') {
      e.preventDefault();
      rescan();
    } else if (e.key === 'Escape') {
      if (document.activeElement === $('#search') && state.search) {
        clearSearch();
      } else if (state.selection.size) {
        state.selection.clear();
        render();
      } else if (openShow()) {
        state.showId = null;
        render();
      } else if (state.detailId !== null) {
        state.detailId = null;
        render();
      }
    }
  });

  // Disable the browser context menu – it makes no sense in a desktop app.
  document.addEventListener('contextmenu', (e) => {
    if (!e.target.closest('input')) e.preventDefault();
  });
}

async function init() {
  loadPrefs();
  applyLayout();
  bindEvents();
  await listen('movie-progress', (e) => updateMovie(e.payload));
  await listen('movie-updated', (e) => updateMovie(e.payload));
  await listen('show-updated', (e) => updateShow(e.payload));
  await listen('categories-changed', (e) => {
    state.categories = e.payload;
    render();
  });
  await listen('metadata-progress', (e) => onJobProgress(e.payload));
  await listen('play-error', (e) => toast(String(e.payload), 'error'));
  await listen('autoplay-started', (e) => {
    updateMovie(e.payload);
    toast(t('Další díl: {code} · {title}', { code: epCode(e.payload), title: e.payload.title }));
  });
  await listen('segments-updated', async () => {
    const data = await call('get_data');
    if (data) applyData(data);
  });
  await listen('segments-progress', (e) => {
    const { showId, done, total } = e.payload;
    if (!segmentsProgress.has(showId)) return;
    segmentsProgress.set(showId, `${done}/${total}`);
    const button = state.showId === showId && $('#show-page [data-act="fetch-segments"]');
    if (button) button.textContent = t('Stahuji {progress}…', { progress: `${done}/${total}` });
  });
  await listen('update-progress', (e) => state.updateProgress?.(e.payload));
  await listen('library-changed', async (e) => {
    const data = await call('get_data');
    if (data) applyData(data);
    const { added, removed } = e.payload;
    const changes = [];
    if (added) changes.push(t('přibylo {n}', { n: `${added} ${plural(added, 'video', 'videa', 'videí')}` }));
    if (removed) changes.push(t('ubylo {n}', { n: `${removed} ${plural(removed, 'video', 'videa', 'videí')}` }));
    toast(t('Složky se změnily: {changes}.', { changes: changes.join(', ') }), 'ok');
    if (added) autoMetadata();
  });
  await listen('scan-progress', (e) => {
    $('#loading-text').textContent = t('Zpracovávám nové soubory… {done} / {total}', e.payload);
  });
  state.settings = (await call('get_settings')) || null;
  // Until chosen, the system language decides (Czech and Slovak -> Czech, others -> English).
  // Android stores it each start: its player plugin keeps it only while running.
  const lang = state.settings?.language || (/^(cs|sk)\b/i.test(navigator.language) ? 'cs' : 'en');
  await setLanguage(lang, !state.settings?.language || IS_ANDROID);
  const data = await call('get_data');
  if (data) applyData(data);
  if (state.libraries.length) {
    // Folders on USB drives (uri) do not need the permission.
    if (state.libraries.some((l) => !l.uri)) await ensureStorage();
    await rescan();
  }
  // Quietly, a bit after start; at most once a day (decided by the backend).
  setTimeout(() => checkForUpdate(false), 5000);
}

init();
