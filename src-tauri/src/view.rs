//! Data sent to the frontend, combined from stored records and the last scan.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::db::{self, Db, Person, Record, SeasonMeta};
use crate::library::{self, ScanState};
use crate::media::{self, ExternalSubtitle, MediaInfo, Tags};
use crate::scanner;
use crate::tracks::{self, AudioSelection, Resolved, Source, SubtitleSelection, TrackChoice};
use crate::AppState;

/// A movie, or an episode when `show_id` is set.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Movie {
    id: u64,
    library_id: u64,
    path: String,
    file_name: String,
    folder: String,
    title: String,
    original_title: Option<String>,
    year: Option<u32>,
    size: u64,
    added: u64,
    categories: Vec<String>,
    position: f64,
    duration: f64,
    watched: bool,
    last_played: Option<u64>,
    playing: bool,
    /// False when the folder (disk) is not connected.
    available: bool,
    /// File name served by the `poster` protocol.
    poster: Option<String>,
    /// "tmdb" (real poster) or "thumb" (frame from the video).
    poster_kind: Option<&'static str>,
    tmdb_id: Option<u64>,
    overview: Option<String>,
    genres: Vec<String>,
    rating: Option<f64>,
    runtime: Option<u32>,
    director: Option<String>,
    meta_checked: bool,
    /// TMDB information exists in both languages (or there is none).
    translated: bool,
    show_id: Option<u64>,
    season: Option<u32>,
    episode: Option<u32>,
    last_episode: Option<u32>,
    /// Length in seconds: from VLC, or from the file headers before the first playback.
    length: f64,
    /// For the technical filters.
    tags: Tags,
    /// Tracks and video format from the file headers.
    media: Option<MediaInfo>,
    external_subtitles: Vec<ExternalSubtitle>,
    /// Tracks chosen for this file.
    tracks: TrackChoice,
    /// What will play (global rules, show and own choice applied).
    play_tracks: PlayTracks,
    fingerprint: String,
    /// Main cast and directors from TMDB.
    cast: Vec<Person>,
    directors: Vec<Person>,
    /// Movie series (collection) the movie belongs to.
    collection: Option<CollectionLink>,
    /// TMDB ids of recommended and similar movies.
    similar: Vec<u64>,
    /// Want to watch, own rating (0–5) and note.
    wishlist: bool,
    my_rating: u8,
    note: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionLink {
    id: u64,
    name: String,
}

/// A movie series with its parts in release order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInfo {
    id: u64,
    name: String,
    parts: Vec<CollectionPartInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionPartInfo {
    tmdb_id: u64,
    title: String,
    year: Option<u32>,
}

/// Tracks a playback will use.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayTracks {
    /// Index of the audio track; None = as the file decides (or by `audio_languages`).
    audio: Option<u32>,
    audio_languages: Vec<String>,
    audio_source: Source,
    /// "default", "off", "track", "file" or "languages".
    subtitle: &'static str,
    subtitle_index: Option<u32>,
    subtitle_file: Option<String>,
    subtitle_languages: Vec<String>,
    subtitle_source: Source,
}

impl From<&Resolved> for PlayTracks {
    fn from(r: &Resolved) -> Self {
        let (audio, audio_languages) = match &r.audio {
            AudioSelection::Default => (None, Vec::new()),
            AudioSelection::Track(i) => (Some(*i), Vec::new()),
            AudioSelection::Languages(l) => (None, l.clone()),
        };
        let (subtitle, subtitle_index, subtitle_file, subtitle_languages) = match &r.subtitle {
            SubtitleSelection::Default => ("default", None, None, Vec::new()),
            SubtitleSelection::Off => ("off", None, None, Vec::new()),
            SubtitleSelection::Track(i) => ("track", Some(*i), None, Vec::new()),
            SubtitleSelection::File(path) => (
                "file",
                None,
                path.file_name().map(|n| n.to_string_lossy().to_string()),
                Vec::new(),
            ),
            SubtitleSelection::Languages(l) => ("languages", None, None, l.clone()),
        };
        PlayTracks {
            audio,
            audio_languages,
            audio_source: r.audio_source,
            subtitle,
            subtitle_index,
            subtitle_file,
            subtitle_languages,
            subtitle_source: r.subtitle_source,
        }
    }
}

/// Movies or episodes that are probably the same video.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// "file" (identical copies), "episode", "movie" (same TMDB movie) or "title".
    reason: &'static str,
    ids: Vec<u64>,
}

/// A TV series; its episodes are the movies with its `show_id`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Show {
    id: u64,
    title: String,
    original_title: Option<String>,
    year: Option<u32>,
    added: u64,
    categories: Vec<String>,
    poster: Option<String>,
    poster_kind: Option<&'static str>,
    tmdb_id: Option<u64>,
    overview: Option<String>,
    genres: Vec<String>,
    rating: Option<f64>,
    /// Typical episode length in minutes.
    runtime: Option<u32>,
    creators: Option<String>,
    meta_checked: bool,
    /// TMDB information exists in both languages (or there is none).
    translated: bool,
    /// Seasons on TMDB (empty without metadata).
    seasons: Vec<SeasonMeta>,
    /// Audio and subtitle languages for all episodes.
    tracks: TrackChoice,
    cast: Vec<Person>,
    /// Creators with their TMDB ids.
    creator_list: Vec<Person>,
    /// TMDB ids of recommended and similar series.
    similar: Vec<u64>,
    wishlist: bool,
    my_rating: u8,
    note: String,
    /// New episodes can still come (TMDB status).
    airing: bool,
    /// TMDB status: "Returning Series", "Ended", "Canceled"…
    status: String,
    next_episode: Option<NextEpisodeView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextEpisodeView {
    season: u32,
    episode: u32,
    /// In the app's language.
    title: String,
    /// "YYYY-MM-DD".
    air_date: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    id: u64,
    /// Path, or the label of a folder URI.
    path: String,
    /// Folder reached through the Android document provider (USB drive).
    uri: bool,
    name: String,
    online: bool,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
    libraries: Vec<LibraryInfo>,
    categories: Vec<String>,
    /// Movies and episodes.
    movies: Vec<Movie>,
    shows: Vec<Show>,
    /// Records whose file is gone from a reachable folder (or whose folder was removed).
    missing: usize,
    scanned: bool,
    /// Movie series of the shown movies.
    collections: Vec<CollectionInfo>,
    saved_filters: Vec<db::SavedFilter>,
}

fn movie(
    id: u64,
    rec: &Record,
    db: &Db,
    scan: &ScanState,
    playing: &HashSet<u64>,
    shows: &HashMap<String, u64>,
) -> Option<Movie> {
    let library = db.libraries.iter().find(|l| l.id == rec.library)?;
    let available = scan.found.contains_key(&id);
    if !available && scan.online.contains(&library.id) {
        return None;
    }
    // Folder URIs are shown by their label: "USB disk/Filmy/Matrix.mkv".
    let path = match scan.found.get(&id) {
        Some(path) if !db::is_uri(&library.path) => path.to_string_lossy().to_string(),
        _ if db::is_uri(&library.path) => format!("{}/{}", library.display_path(), rec.rel_path),
        _ => db::join_rel(&library.path, &rec.rel_path).to_string_lossy().to_string(),
    };
    let (folder, file_name) = match rec.rel_path.rsplit_once('/') {
        Some((dir, name)) => (dir.replace('/', std::path::MAIN_SEPARATOR_STR), name.to_string()),
        None => (String::new(), rec.rel_path.clone()),
    };
    let episode = scanner::parse_episode(&rec.rel_path)
        .and_then(|e| shows.get(&scanner::show_key(&e.show)).map(|show_id| (*show_id, e)));
    let external = scan.subtitles.get(&id).map(Vec::as_slice).unwrap_or_default();
    let show_tracks = episode.as_ref().and_then(|(show_id, _)| db.shows.get(show_id)).map(|s| &s.tracks);
    let resolved = tracks::resolve(&db.settings.tracks, show_tracks, &rec.tracks, rec.media.as_ref(), external);
    let length = if rec.duration > 0.0 { rec.duration } else { rec.media.as_ref().map_or(0.0, |m| m.duration) };
    let base = |title: String| Movie {
        id,
        library_id: library.id,
        path: path.clone(),
        file_name: file_name.clone(),
        folder: folder.clone(),
        title,
        original_title: None,
        year: None,
        size: rec.size,
        added: rec.added,
        categories: rec.categories.clone(),
        position: rec.position,
        duration: rec.duration,
        watched: rec.watched,
        last_played: rec.last_played,
        playing: playing.contains(&id),
        available,
        poster: rec.thumbnail.clone(),
        poster_kind: rec.thumbnail.as_ref().map(|_| "thumb"),
        tmdb_id: None,
        overview: None,
        genres: Vec::new(),
        rating: None,
        runtime: None,
        director: None,
        meta_checked: rec.meta_checked,
        translated: rec.meta.as_ref().is_none_or(|m| m.bilingual),
        show_id: None,
        season: None,
        episode: None,
        last_episode: None,
        length,
        tags: media::tags(rec.media.as_ref(), external, &file_name),
        media: rec.media.clone(),
        external_subtitles: external.to_vec(),
        tracks: rec.tracks.clone(),
        play_tracks: PlayTracks::from(&resolved),
        fingerprint: rec.fingerprint.clone(),
        cast: Vec::new(),
        directors: Vec::new(),
        collection: None,
        similar: Vec::new(),
        wishlist: rec.personal.wishlist,
        my_rating: rec.personal.rating,
        note: rec.personal.note.clone(),
    };

    if let Some((show_id, ep)) = episode {
        let info = db.shows.get(&show_id).and_then(|show| {
            show.episodes
                .iter()
                .find(|m| m.season == ep.season && m.episode == ep.episode)
        });
        let title = info
            .map(|m| db::localized(&m.title, &m.title_en).to_string())
            .filter(|t| !t.is_empty())
            .or(ep.title)
            .unwrap_or_else(|| tr!("Epizoda {}", "Episode {}", ep.episode));
        return Some(Movie {
            overview: info
                .map(|m| db::localized(&m.overview, &m.overview_en).to_string())
                .filter(|o| !o.is_empty()),
            runtime: info.map(|m| m.runtime).filter(|r| *r > 0),
            show_id: Some(show_id),
            season: Some(ep.season),
            episode: Some(ep.episode),
            last_episode: ep.last_episode,
            ..base(title)
        });
    }

    let stem = Path::new(&file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let (parsed_title, parsed_year) = scanner::parse_title(&stem);
    let meta = rec.meta.as_ref();
    let title = meta
        .map(|m| m.title().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or(parsed_title);
    let original_title = meta
        .map(|m| m.original_title.clone())
        .filter(|t| !t.is_empty() && *t != title);
    let (poster, poster_kind) = match (meta.and_then(|m| m.poster.clone()), &rec.thumbnail) {
        (Some(p), _) => (Some(p), Some("tmdb")),
        (None, Some(t)) => (Some(t.clone()), Some("thumb")),
        _ => (None, None),
    };

    Some(Movie {
        original_title,
        year: meta.and_then(|m| m.year).or(parsed_year),
        poster,
        poster_kind,
        tmdb_id: meta.map(|m| m.tmdb_id),
        overview: meta.map(|m| m.overview().to_string()).filter(|o| !o.is_empty()),
        genres: meta.map(|m| m.genres().to_vec()).unwrap_or_default(),
        rating: meta.map(|m| m.rating).filter(|r| *r > 0.0),
        runtime: meta.map(|m| m.runtime).filter(|r| *r > 0),
        director: meta.and_then(|m| m.director.clone()),
        cast: meta.map(|m| m.cast.clone()).unwrap_or_default(),
        directors: meta.map(|m| m.directors.clone()).unwrap_or_default(),
        collection: meta.and_then(|m| m.collection.as_ref()).map(|c| CollectionLink {
            id: c.id,
            name: collection_name(db, c),
        }),
        similar: meta.map(|m| m.similar.clone()).unwrap_or_default(),
        ..base(title)
    })
}

fn show(id: u64, show: &db::Show) -> Show {
    let meta = show.meta.as_ref();
    let title = meta
        .map(|m| m.title().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| show.name.clone());
    let original_title = meta
        .map(|m| m.original_title.clone())
        .filter(|t| !t.is_empty() && *t != title);
    let (poster, poster_kind) = match (meta.and_then(|m| m.poster.clone()), &show.thumbnail) {
        (Some(p), _) => (Some(p), Some("tmdb")),
        (None, Some(t)) => (Some(t.clone()), Some("thumb")),
        _ => (None, None),
    };
    Show {
        id,
        title,
        original_title,
        year: meta.and_then(|m| m.year).or(show.year),
        added: show.added,
        categories: show.categories.clone(),
        poster,
        poster_kind,
        tmdb_id: meta.map(|m| m.tmdb_id),
        overview: meta.map(|m| m.overview().to_string()).filter(|o| !o.is_empty()),
        genres: meta.map(|m| m.genres().to_vec()).unwrap_or_default(),
        rating: meta.map(|m| m.rating).filter(|r| *r > 0.0),
        runtime: meta.map(|m| m.runtime).filter(|r| *r > 0),
        creators: meta.and_then(|m| m.director.clone()),
        meta_checked: show.meta_checked,
        translated: meta.is_none_or(|m| m.bilingual),
        seasons: show.seasons.clone(),
        tracks: show.tracks.clone(),
        cast: meta.map(|m| m.cast.clone()).unwrap_or_default(),
        creator_list: meta.map(|m| m.directors.clone()).unwrap_or_default(),
        similar: meta.map(|m| m.similar.clone()).unwrap_or_default(),
        wishlist: show.personal.wishlist,
        my_rating: show.personal.rating,
        note: show.personal.note.clone(),
        airing: show.airing(),
        status: show.status.clone().unwrap_or_default(),
        next_episode: show.next_episode.as_ref().map(|n| NextEpisodeView {
            season: n.season,
            episode: n.episode,
            title: db::localized(&n.name, &n.name_en).to_string(),
            air_date: n.air_date.clone(),
        }),
    }
}

/// Name of a movie series in the app's language; the stored series knows both names.
fn collection_name(db: &Db, link: &db::CollectionRef) -> String {
    match db.collections.get(&link.id) {
        Some(c) if !c.name.is_empty() => db::localized(&c.name, &c.name_en).to_string(),
        _ => db::localized(&link.name, &link.name_en).to_string(),
    }
}

pub fn app_data(state: &AppState) -> AppData {
    let db = state.db.lock().unwrap();
    let scan = state.scan.lock().unwrap();
    let playing = state.playing.lock().unwrap();
    let show_ids = library::show_ids(&db);

    let movies: Vec<Movie> = if scan.scanned {
        db.records
            .iter()
            .filter_map(|(id, rec)| movie(*id, rec, &db, &scan, &playing, &show_ids))
            .collect()
    } else {
        Vec::new()
    };
    // Only shows with an episode to display.
    let shown: HashSet<u64> = movies.iter().filter_map(|m| m.show_id).collect();
    let shows = db
        .shows
        .iter()
        .filter(|(id, _)| shown.contains(id))
        .map(|(id, s)| show(*id, s))
        .collect();
    let missing = if scan.scanned { missing_ids(&db, &scan).len() } else { 0 };
    let libraries = db
        .libraries
        .iter()
        .map(|l| LibraryInfo {
            id: l.id,
            path: l.display_path(),
            uri: db::is_uri(&l.path),
            name: Path::new(&l.display_path())
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| l.display_path()),
            online: scan.online.contains(&l.id),
            count: movies.iter().filter(|m| m.library_id == l.id).count(),
        })
        .collect();

    let mut collection_ids: Vec<u64> = movies.iter().filter_map(|m| m.collection.as_ref().map(|c| c.id)).collect();
    collection_ids.sort_unstable();
    collection_ids.dedup();
    let collections = collection_ids
        .into_iter()
        .filter_map(|id| {
            let c = db.collections.get(&id)?;
            Some(CollectionInfo {
                id,
                name: db::localized(&c.name, &c.name_en).to_string(),
                parts: c
                    .parts
                    .iter()
                    .map(|p| CollectionPartInfo {
                        tmdb_id: p.tmdb_id,
                        title: db::localized(&p.title, &p.title_en).to_string(),
                        year: p.year,
                    })
                    .collect(),
            })
        })
        .collect();

    AppData {
        libraries,
        categories: db.categories.clone(),
        movies,
        shows,
        missing,
        scanned: scan.scanned,
        collections,
        saved_filters: db.saved_filters.clone(),
    }
}

/// Records that are not shown: file gone from a reachable folder, or folder removed.
pub fn missing_ids(db: &Db, scan: &ScanState) -> Vec<u64> {
    db.records
        .iter()
        .filter(|(id, rec)| {
            let library_exists = db.libraries.iter().any(|l| l.id == rec.library);
            !library_exists || (!scan.found.contains_key(id) && scan.online.contains(&rec.library))
        })
        .map(|(id, _)| *id)
        .collect()
}

pub fn movie_by_id(state: &AppState, id: u64) -> Result<Movie, String> {
    let db = state.db.lock().unwrap();
    let scan = state.scan.lock().unwrap();
    let playing = state.playing.lock().unwrap();
    db.records
        .get(&id)
        .and_then(|rec| movie(id, rec, &db, &scan, &playing, &library::show_ids(&db)))
        .ok_or_else(|| tr!("Film nebyl nalezen. Zkuste složky načíst znovu.", "Movie not found. Try scanning the folders again."))
}

pub fn show_by_id(state: &AppState, id: u64) -> Result<Show, String> {
    let db = state.db.lock().unwrap();
    db.shows
        .get(&id)
        .map(|s| show(id, s))
        .ok_or_else(|| tr!("Seriál nebyl nalezen. Zkuste složky načíst znovu.", "Series not found. Try scanning the folders again."))
}

/// Tracks to play for a record (rules, show and own choice applied).
pub fn resolved_tracks(state: &AppState, id: u64) -> Resolved {
    let db = state.db.lock().unwrap();
    let scan = state.scan.lock().unwrap();
    let Some(rec) = db.records.get(&id) else {
        return tracks::resolve(&db.settings.tracks, None, &TrackChoice::default(), None, &[]);
    };
    let show = scanner::parse_episode(&rec.rel_path)
        .and_then(|e| library::show_ids(&db).get(&scanner::show_key(&e.show)).copied())
        .and_then(|show_id| db.shows.get(&show_id))
        .map(|s| &s.tracks);
    let external = scan.subtitles.get(&id).map(Vec::as_slice).unwrap_or_default();
    tracks::resolve(&db.settings.tracks, show, &rec.tracks, rec.media.as_ref(), external)
}

/// Groups of movies and episodes that are probably the same video, most certain first.
pub fn duplicates(state: &AppState) -> Vec<DuplicateGroup> {
    let movies = app_data(state).movies;
    // Index = rank of a reason: identical files are the surest.
    const REASONS: [&str; 4] = ["title", "movie", "episode", "file"];
    let mut keys: HashMap<(usize, String), Vec<usize>> = HashMap::new();
    for (i, m) in movies.iter().enumerate() {
        if !m.fingerprint.is_empty() {
            keys.entry((3, m.fingerprint.clone())).or_default().push(i);
        }
        match (m.show_id, m.season, m.episode) {
            (Some(show), Some(season), Some(episode)) => {
                keys.entry((2, format!("{show}/{season}/{episode}"))).or_default().push(i);
            }
            _ => {
                if let Some(tmdb) = m.tmdb_id {
                    keys.entry((1, tmdb.to_string())).or_default().push(i);
                }
                let title: String = m.title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
                keys.entry((0, format!("{title}/{}", m.year.unwrap_or(0)))).or_default().push(i);
            }
        }
    }

    // Union-find over the movies sharing a key; a group keeps its best reason.
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..movies.len()).collect();
    let mut reasons: Vec<(usize, usize)> = Vec::new();
    for ((reason, _), members) in keys.iter().filter(|(_, m)| m.len() > 1) {
        let first = root(&mut parent, members[0]);
        for &other in &members[1..] {
            let r = root(&mut parent, other);
            parent[r] = first;
        }
        reasons.push((members[0], *reason));
    }
    let mut groups: HashMap<usize, (usize, Vec<u64>)> = HashMap::new();
    for (i, movie) in movies.iter().enumerate() {
        let r = root(&mut parent, i);
        groups.entry(r).or_insert((0, Vec::new())).1.push(movie.id);
    }
    for (member, reason) in reasons {
        let r = root(&mut parent, member);
        if let Some(group) = groups.get_mut(&r) {
            group.0 = group.0.max(reason);
        }
    }
    let mut out: Vec<(usize, Vec<u64>)> = groups.into_values().filter(|(_, ids)| ids.len() > 1).collect();
    out.sort_by_key(|(reason, _)| std::cmp::Reverse(*reason));
    out.into_iter()
        .map(|(reason, ids)| DuplicateGroup { reason: REASONS[reason], ids })
        .collect()
}

/// Title for the player: "Show – S01E02 – Episode" for an episode.
pub fn player_title(state: &AppState, id: u64) -> String {
    let Ok(movie) = movie_by_id(state, id) else {
        return String::new();
    };
    let show = movie.show_id.and_then(|show_id| show_by_id(state, show_id).ok());
    match (show, movie.season, movie.episode) {
        (Some(show), Some(season), Some(episode)) => {
            format!("{} – S{season:02}E{episode:02} – {}", show.title, movie.title)
        }
        _ => movie.title,
    }
}
