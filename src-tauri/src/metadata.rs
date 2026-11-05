//! Background job that fetches TMDB metadata and posters, and creates thumbnails (frames
//! from the video, see [`crate::player::Thumbnailer`]) for movies and shows without a poster.
//! Episodes get their details together with their show.
//! Progress is reported with `metadata-progress` events and every updated movie with a
//! `movie-updated` event (`show-updated` for a show).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{self, Collection, Meta, META_VERSION};
use crate::player::Thumbnailer;
use crate::{library, scanner, tmdb, view, AppState};

#[derive(Default)]
pub struct Job {
    pub running: AtomicBool,
    cancel: AtomicBool,
}

impl Job {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct JobProgress {
    running: bool,
    done: usize,
    total: usize,
    title: String,
    matched: usize,
    error: Option<String>,
}

/// Details found on TMDB.
pub enum Info {
    Movie(Meta),
    Show(tmdb::ShowInfo),
}

impl Info {
    fn meta(&self) -> &Meta {
        match self {
            Info::Movie(meta) => meta,
            Info::Show(show) => &show.meta,
        }
    }
}

enum Kind {
    Movie {
        /// Known match that only needs refreshing (posters missing after a restore).
        tmdb_id: Option<u64>,
    },
    Show {
        /// Seasons found on disk.
        seasons: Vec<u32>,
        /// Known match whose episode list only needs completing (or whose poster is
        /// missing after a restore).
        tmdb_id: Option<u64>,
    },
}

struct Task {
    id: u64,
    /// The movie, or the first episode of a show (for the thumbnail).
    path: PathBuf,
    title: String,
    year: Option<u32>,
    lookup: bool,
    thumbnail: bool,
    has_poster: bool,
    kind: Kind,
}

/// Episodes of a show found by the last scan.
#[derive(Default)]
struct ShowFiles {
    seasons: Vec<u32>,
    /// Order (specials last) and path of the first episode.
    first: Option<((u32, u32), PathBuf)>,
}

/// Starts the job for the given movies and shows (all available ones when `None`).
/// `force` repeats lookups that were already done. Returns the number of queued items.
pub fn start(app: &AppHandle, ids: Option<Vec<u64>>, force: bool) -> Result<usize, String> {
    let state = app.state::<AppState>();
    if state.job.running.swap(true, Ordering::SeqCst) {
        return Err(tr!("Načítání informací už probíhá.", "Loading information is already running."));
    }
    let tasks = collect_tasks(app, &state, ids, force);
    if tasks.is_empty() {
        state.job.running.store(false, Ordering::SeqCst);
        return Ok(0);
    }
    state.job.cancel.store(false, Ordering::SeqCst);
    let count = tasks.len();
    let app = app.clone();
    thread::spawn(move || run(app, tasks));
    Ok(count)
}

fn collect_tasks(app: &AppHandle, state: &AppState, ids: Option<Vec<u64>>, force: bool) -> Vec<Task> {
    let db = state.db.lock().unwrap();
    let scan = state.scan.lock().unwrap();
    let has_key = db.settings.tmdb_key.is_some();
    let thumbnails = db.settings.video_thumbnails && Thumbnailer::new(app, &db.settings).is_some();
    let wanted = |id: &u64| ids.as_ref().is_none_or(|ids| ids.contains(id));
    let show_ids = library::show_ids(&db);
    let mut show_files: HashMap<u64, ShowFiles> = HashMap::new();

    let mut tasks = Vec::new();
    for (id, rec) in &db.records {
        if let Some(episode) = scanner::parse_episode(&rec.rel_path) {
            if let Some(show_id) = show_ids.get(&scanner::show_key(&episode.show)) {
                if let Some(path) = scan.found.get(id) {
                    let files = show_files.entry(*show_id).or_default();
                    if !files.seasons.contains(&episode.season) {
                        files.seasons.push(episode.season);
                    }
                    let season = if episode.season == 0 { u32::MAX } else { episode.season };
                    let order = (season, episode.episode);
                    if files.first.as_ref().is_none_or(|(first, _)| order < *first) {
                        files.first = Some((order, path.clone()));
                    }
                }
                continue;
            }
        }
        if !wanted(id) {
            continue;
        }
        let Some(path) = scan.found.get(id) else {
            continue;
        };
        let has_poster = rec.meta.as_ref().is_some_and(|m| m.poster.is_some());
        let lookup = has_key && (force || !rec.meta_checked || outdated(rec.meta.as_ref(), &db.collections));
        let thumbnail = thumbnails && rec.thumbnail.is_none() && (force || !rec.thumb_checked);
        if !lookup && !(thumbnail && !has_poster) {
            continue;
        }
        let stem = std::path::Path::new(&rec.rel_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let (title, year) = scanner::parse_title(&stem);
        tasks.push(Task {
            id: *id,
            path: path.clone(),
            title,
            year,
            lookup,
            thumbnail,
            has_poster,
            kind: Kind::Movie {
                tmdb_id: rec.meta.as_ref().filter(|_| !force).map(|m| m.tmdb_id),
            },
        });
    }

    for (id, show) in &db.shows {
        if !wanted(id) {
            continue;
        }
        let Some(ShowFiles { seasons, first: Some((_, path)) }) = show_files.remove(id) else {
            continue;
        };
        let has_poster = show.meta.as_ref().is_some_and(|m| m.poster.is_some());
        // A new season on disk needs its episode titles.
        let new_season = show.meta.as_ref().filter(|_| {
            seasons.iter().any(|n| {
                show.seasons.iter().any(|s| s.number == *n) && !show.episodes.iter().any(|e| e.season == *n)
            })
        });
        let lookup = has_key
            && (force
                || !show.meta_checked
                || new_season.is_some()
                || outdated(show.meta.as_ref(), &db.collections)
                || airing_due(show));
        let thumbnail = thumbnails && show.thumbnail.is_none() && (force || !show.thumb_checked);
        if !lookup && !(thumbnail && !has_poster) {
            continue;
        }
        tasks.push(Task {
            id: *id,
            path,
            title: show.name.clone(),
            year: show.year,
            lookup,
            thumbnail,
            has_poster,
            kind: Kind::Show {
                seasons,
                tmdb_id: show.meta.as_ref().filter(|_| !force).map(|m| m.tmdb_id),
            },
        });
    }
    tasks
}

/// Metadata of an older version (Czech only, or without cast and related titles) and
/// metadata close to the TMDB limit of 6 months (including that of the movie's series) is
/// refreshed by its known match.
fn outdated(meta: Option<&Meta>, collections: &std::collections::BTreeMap<u64, Collection>) -> bool {
    meta.is_some_and(|m| {
        let series_stale = m
            .collection
            .as_ref()
            .and_then(|c| collections.get(&c.id))
            .is_some_and(|c| db::tmdb_stale(c.fetched));
        !m.bilingual || m.version < META_VERSION || db::tmdb_stale(m.fetched) || series_stale
    })
}

/// A running series is refreshed weekly, and a day after its announced next episode aired,
/// so the next episode shown stays current. Series loaded by version 1.5 or older have no
/// status yet and are refreshed once.
fn airing_due(show: &db::Show) -> bool {
    const DAY: u64 = 24 * 3600;
    let Some(meta) = &show.meta else {
        return false;
    };
    if show.status.is_none() {
        return true;
    }
    let age = db::now().saturating_sub(meta.fetched);
    if !show.airing() || age < DAY {
        return false;
    }
    let today = crate::backup::date_stamp(db::now());
    age > 7 * DAY || show.next_episode.as_ref().is_some_and(|n| n.air_date < today)
}

/// Fetches the parts of the movie series `meta` belongs to when they are not stored yet
/// or are due for a refresh.
pub fn fetch_collection(state: &AppState, client: &tmdb::Client, meta: &Meta) {
    let Some(collection) = &meta.collection else {
        return;
    };
    let stored = state.db.lock().unwrap().collections.get(&collection.id).map(|c| c.fetched);
    // Several movies of one series refresh together; one download is enough.
    if stored.is_some_and(|fetched| !db::tmdb_stale(fetched)) {
        return;
    }
    // Without the parts the series is still shown by its name.
    if let Ok(parts) = client.collection(collection.id) {
        state.db.lock().unwrap().collections.insert(collection.id, parts);
    }
}

fn run(app: AppHandle, tasks: Vec<Task>) {
    let state = app.state::<AppState>();
    let (key, thumbnailer) = {
        let db = state.db.lock().unwrap();
        (db.settings.tmdb_key.clone(), Thumbnailer::new(&app, &db.settings))
    };
    let client = key.map(|k| tmdb::Client::new(&k));
    let total = tasks.len();
    let mut progress = JobProgress {
        running: true,
        done: 0,
        total,
        title: String::new(),
        matched: 0,
        error: None,
    };

    for task in &tasks {
        if state.job.cancel.load(Ordering::SeqCst) {
            break;
        }
        progress.title = task.title.clone();
        let _ = app.emit("metadata-progress", progress.clone());

        // None = lookup not done (skipped or failed temporarily), Some(None) = no match.
        let mut found: Option<Option<Info>> = None;
        if let (true, Some(client)) = (task.lookup, &client) {
            let dir = &state.posters_dir;
            let result = match &task.kind {
                Kind::Movie { tmdb_id } => tmdb_id
                    .map_or_else(|| client.find(&task.title, task.year, false), |id| Ok(Some(id)))
                    .and_then(|id| id.map(|id| client.details(id, dir).map(Info::Movie)).transpose()),
                Kind::Show { seasons, tmdb_id } => tmdb_id
                    .map_or_else(|| client.find(&task.title, task.year, true), |id| Ok(Some(id)))
                    .and_then(|id| {
                        id.map(|id| client.show_details(id, seasons, dir).map(Info::Show))
                            .transpose()
                    }),
            };
            match result {
                Ok(info) => {
                    if let Some(Info::Movie(meta)) = &info {
                        fetch_collection(&state, client, meta);
                    }
                    if info.is_some() {
                        progress.matched += 1;
                    }
                    found = Some(info);
                }
                Err(tmdb::Error::Auth(message)) => {
                    progress.error = Some(message);
                    break;
                }
                Err(tmdb::Error::Other(message)) => progress.error = Some(message),
            }
        }

        let has_poster = task.has_poster
            || found.as_ref().is_some_and(|i| i.as_ref().is_some_and(|i| i.meta().poster.is_some()));
        let mut thumbnail = None;
        if let (true, false, Some(thumbnailer)) = (task.thumbnail, has_poster, &thumbnailer) {
            let name = format!("thumb-{}.jpg", task.id);
            thumbnail = Some(thumbnailer.snapshot(&task.path, &state.posters_dir, &name).then_some(name));
        }

        // The record may have been removed meanwhile; nothing to report then.
        let _ = apply(&app, task.id, found, thumbnail);
        progress.done += 1;
    }

    state.job.running.store(false, Ordering::SeqCst);
    progress.running = false;
    let _ = app.emit("metadata-progress", progress);
}

/// Stores the results for one movie or show and emits the update.
/// `info`: None = leave as is; Some(None) = looked up without a match.
pub fn apply(
    app: &AppHandle,
    id: u64,
    info: Option<Option<Info>>,
    thumbnail: Option<Option<String>>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut categories_changed = false;
    let is_show = {
        let mut db = state.db.lock().unwrap();
        let is_show = db.shows.contains_key(&id);
        if !is_show && !db.records.contains_key(&id) {
            return Err(tr!("Film nebyl nalezen.", "Movie not found."));
        }
        // Genres become categories with a new match only; refreshing a known one must not bring
        // back categories the user removed.
        let known = if is_show {
            db.shows.get(&id).and_then(|s| s.meta.as_ref())
        } else {
            db.records.get(&id).and_then(|r| r.meta.as_ref())
        }
        .map(|m| m.tmdb_id);
        let mut genre_categories = Vec::new();
        if let Some(Some(info)) = &info {
            if db.settings.genres_as_categories && known != Some(info.meta().tmdb_id) {
                for genre in &info.meta().genres {
                    let (name, created) = db.ensure_category(genre);
                    categories_changed |= created;
                    genre_categories.push(name);
                }
            }
        }
        if is_show {
            let show = db.shows.get_mut(&id).expect("checked above");
            if let Some(info) = info {
                show.meta_checked = true;
                if let Some(Info::Show(info)) = info {
                    show.meta = Some(info.meta);
                    show.seasons = info.seasons;
                    show.episodes = info.episodes;
                    show.status = Some(info.status);
                    show.next_episode = info.next_episode;
                }
            }
            add_results(&mut show.categories, genre_categories, &mut show.thumbnail, &mut show.thumb_checked, thumbnail);
        } else {
            let record = db.records.get_mut(&id).expect("checked above");
            if let Some(info) = info {
                record.meta_checked = true;
                if let Some(Info::Movie(meta)) = info {
                    record.meta = Some(meta);
                }
            }
            add_results(&mut record.categories, genre_categories, &mut record.thumbnail, &mut record.thumb_checked, thumbnail);
        }
        db.save(&state.db_path)?;
        if categories_changed {
            let _ = app.emit("categories-changed", db.categories.clone());
        }
        is_show
    };
    if is_show {
        let _ = app.emit("show-updated", &view::show_by_id(&state, id)?);
    } else {
        let _ = app.emit("movie-updated", &view::movie_by_id(&state, id)?);
    }
    Ok(())
}

fn add_results(
    categories: &mut Vec<String>,
    genres: Vec<String>,
    thumbnail: &mut Option<String>,
    thumb_checked: &mut bool,
    new_thumbnail: Option<Option<String>>,
) {
    for name in genres {
        if !categories.contains(&name) {
            categories.push(name);
        }
    }
    if let Some(new_thumbnail) = new_thumbnail {
        *thumb_checked = true;
        if new_thumbnail.is_some() {
            *thumbnail = new_thumbnail;
        }
    }
}
