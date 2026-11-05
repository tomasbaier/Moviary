//! Persistent data: library folders, settings, categories and one record per movie file.
//! Stored as a single JSON file in the app data directory.
//!
//! A record is tied to its folder (library) and a path relative to that folder, plus a
//! fingerprint of the file content, so data survives a changed drive letter, a moved
//! library folder and renamed or moved files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::media::MediaInfo;
use crate::tracks::{TrackChoice, TrackRules};

pub const VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Db {
    pub version: u32,
    pub next_id: u64,
    pub libraries: Vec<LibraryDir>,
    pub settings: Settings,
    pub categories: Vec<String>,
    pub records: BTreeMap<u64, Record>,
    /// TV series; their episodes are records whose file name marks an episode. Ids are
    /// shared with records, so an id names either a record or a show.
    pub shows: BTreeMap<u64, Show>,
    /// Finished playbacks, oldest first (for the statistics).
    pub history: Vec<PlayEvent>,
    /// Movie series (collections) on TMDB that movies of the library belong to, by TMDB id.
    pub collections: BTreeMap<u64, Collection>,
}

/// Version of [`Meta`]: older metadata is refreshed by the metadata job (by its known TMDB
/// match). 2 = cast, directors, collection and similar titles.
pub const META_VERSION: u32 = 2;

/// TMDB data may be kept at most 6 months (TMDB API Terms of Use, 1.C); older data is
/// refreshed by the metadata job a bit before that.
pub const TMDB_MAX_AGE: u64 = 150 * 24 * 3600;

/// Data fetched at `fetched` (Unix seconds; 0 = unknown) is due for a refresh.
pub fn tmdb_stale(fetched: u64) -> bool {
    now().saturating_sub(fetched) > TMDB_MAX_AGE
}

/// A movie series on TMDB ("Harry Potter Collection").
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Collection {
    pub name: String,
    pub name_en: String,
    /// In release order.
    pub parts: Vec<CollectionPart>,
    /// Unix timestamp of the download from TMDB.
    pub fetched: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CollectionPart {
    pub tmdb_id: u64,
    pub title: String,
    pub title_en: String,
    pub year: Option<u32>,
}

/// A person on TMDB (the id is the same for movies and series).
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Person {
    pub id: u64,
    pub name: String,
    /// Role of an actor; empty for directors and creators.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub character: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CollectionRef {
    pub id: u64,
    pub name: String,
    pub name_en: String,
}

/// Most history entries kept.
pub const HISTORY_LIMIT: usize = 20_000;

/// One playback of a movie or episode.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayEvent {
    pub id: u64,
    /// Unix timestamp of the end.
    pub at: u64,
    /// Seconds of the video played.
    pub seconds: f64,
    /// Played to the end.
    pub finished: bool,
}

impl Default for Db {
    fn default() -> Self {
        Db {
            version: VERSION,
            next_id: 1,
            libraries: Vec::new(),
            settings: Settings::default(),
            categories: Vec::new(),
            records: BTreeMap::new(),
            shows: BTreeMap::new(),
            history: Vec::new(),
            collections: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub vlc_path: Option<String>,
    pub tmdb_key: Option<String>,
    /// Fetch metadata for new movies automatically after a scan.
    pub auto_metadata: bool,
    /// Add TMDB genres to the movie as categories.
    pub genres_as_categories: bool,
    /// Generate a thumbnail with VLC for movies without a TMDB poster.
    pub video_thumbnails: bool,
    /// Which audio and subtitle tracks to play.
    pub tracks: TrackRules,
    /// Language of the app: "cs" or "en"; empty until chosen.
    pub language: String,
    /// Scan automatically when files in the library folders change or a drive is connected.
    pub watch_folders: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            vlc_path: None,
            tmdb_key: None,
            auto_metadata: true,
            genres_as_categories: true,
            video_thumbnails: true,
            tracks: TrackRules::default(),
            language: String::new(),
            watch_folders: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryDir {
    pub id: u64,
    /// Folder path, or on Android a `content://` folder URI (USB drives are only reachable
    /// through the system document provider, see [`is_uri`]).
    pub path: String,
    /// Name shown for a `content://` folder ("USB disk/Filmy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl LibraryDir {
    /// Path or label shown to the user.
    pub fn display_path(&self) -> String {
        match &self.label {
            Some(label) if is_uri(&self.path) => label.clone(),
            _ => self.path.clone(),
        }
    }
}

/// A `content://` URI of the Android document provider instead of a file path.
pub fn is_uri(path: &str) -> bool {
    path.starts_with("content://")
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Record {
    pub library: u64,
    /// Path relative to the library folder, `/` separated.
    pub rel_path: String,
    pub size: u64,
    /// Size + hash of samples of the content; empty until computed.
    pub fingerprint: String,
    /// Unix timestamp when the file was first found.
    pub added: u64,
    pub categories: Vec<String>,
    /// Last playback position in seconds (0 = not started / finished).
    pub position: f64,
    /// Length in seconds as reported by VLC.
    pub duration: f64,
    pub watched: bool,
    /// Unix timestamp (seconds) of the last playback.
    pub last_played: Option<u64>,
    pub meta: Option<Meta>,
    /// TMDB lookup was done (with or without a match) – don't repeat it automatically.
    pub meta_checked: bool,
    /// File name of a thumbnail generated by VLC (in the posters folder).
    pub thumbnail: Option<String>,
    pub thumb_checked: bool,
    /// Tracks and video format read from the file headers.
    pub media: Option<MediaInfo>,
    /// [`crate::media::PROBE_VERSION`] that read `media`; 0 = not read yet.
    pub media_probe: u32,
    /// Audio and subtitles chosen for this file.
    #[serde(skip_serializing_if = "TrackChoice::is_empty")]
    pub tracks: TrackChoice,
}

/// Movie information from TMDB.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Meta {
    pub tmdb_id: u64,
    pub title: String,
    pub original_title: String,
    pub year: Option<u32>,
    pub overview: String,
    pub genres: Vec<String>,
    /// Minutes.
    pub runtime: u32,
    pub rating: f64,
    pub director: Option<String>,
    /// File name of the downloaded poster (in the posters folder).
    pub poster: Option<String>,
    /// English title, overview and genres. The fields above are Czech (with English where
    /// TMDB has no Czech translation); the app shows those of its language.
    pub title_en: String,
    pub overview_en: String,
    pub genres_en: Vec<String>,
    /// Both languages were fetched (metadata from version 1.3 and older is Czech only and
    /// gets the English part added by the metadata job).
    pub bilingual: bool,
    /// Main cast, in billing order.
    pub cast: Vec<Person>,
    /// Directors of a movie, creators of a series.
    pub directors: Vec<Person>,
    /// Movie series the movie belongs to.
    pub collection: Option<CollectionRef>,
    /// TMDB ids of recommended and similar titles (movies for a movie, series for a series).
    pub similar: Vec<u64>,
    /// [`META_VERSION`] of this metadata; 0 = from version 1.3.2 or older.
    pub version: u32,
    /// Unix timestamp of the download from TMDB; 0 = from version 1.4 or older.
    pub fetched: u64,
}

impl Meta {
    /// Title in the app's language.
    pub fn title(&self) -> &str {
        localized(&self.title, &self.title_en)
    }

    pub fn overview(&self) -> &str {
        localized(&self.overview, &self.overview_en)
    }

    pub fn genres(&self) -> &[String] {
        if crate::i18n::english() && !self.genres_en.is_empty() {
            &self.genres_en
        } else {
            &self.genres
        }
    }
}

/// The English text when the app is English and there is one, otherwise the Czech one.
pub fn localized<'a>(cs: &'a str, en: &'a str) -> &'a str {
    if crate::i18n::english() && !en.is_empty() {
        en
    } else {
        cs
    }
}

/// A TV series. Episodes belong to it by the show name in their path (see
/// [`crate::scanner::show_key`]), so the data survives renamed or moved episode files.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Show {
    pub key: String,
    /// Name as found in the file or folder names.
    pub name: String,
    pub year: Option<u32>,
    pub added: u64,
    pub categories: Vec<String>,
    /// For a show `director` holds the creators.
    pub meta: Option<Meta>,
    pub seasons: Vec<SeasonMeta>,
    /// Episode details of the seasons found on disk.
    pub episodes: Vec<EpisodeMeta>,
    pub meta_checked: bool,
    pub thumbnail: Option<String>,
    pub thumb_checked: bool,
    /// Audio and subtitle languages for all episodes.
    #[serde(skip_serializing_if = "TrackChoice::is_empty")]
    pub tracks: TrackChoice,
}

/// Season of a show on TMDB.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonMeta {
    pub number: u32,
    pub name: String,
    pub episode_count: u32,
}

/// Episode of a show on TMDB.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodeMeta {
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub overview: String,
    pub title_en: String,
    pub overview_en: String,
    /// Minutes.
    pub runtime: u32,
}

impl Db {
    pub fn load(path: &Path) -> Db {
        let Ok(text) = fs::read_to_string(path) else {
            return Db::default();
        };
        match Db::from_json(serde_json::from_str::<Value>(&text)) {
            Ok(db) => db,
            Err(_) => {
                // Keep the unreadable file around instead of silently overwriting it.
                let _ = fs::rename(path, path.with_extension(format!("corrupt-{}.json", now())));
                Db::default()
            }
        }
    }

    /// Data of any version (the current one or version 1).
    pub fn from_json(value: serde_json::Result<Value>) -> serde_json::Result<Db> {
        value
            .and_then(|value| {
                if value.get("version").is_some() {
                    serde_json::from_value::<Db>(value)
                } else {
                    serde_json::from_value::<LegacyDb>(value).map(migrate)
                }
            })
            .map(|mut db| {
                db.estimate_fetched();
                db
            })
    }

    /// Data from version 1.4 and older has no download time. The movie or show was added
    /// before its data was downloaded, so its `added` time is a safe (early) estimate; a
    /// movie series gets the earliest one of its movies.
    fn estimate_fetched(&mut self) {
        let mut series: BTreeMap<u64, u64> = BTreeMap::new();
        for rec in self.records.values_mut() {
            if let Some(meta) = rec.meta.as_mut() {
                if meta.fetched == 0 {
                    meta.fetched = rec.added;
                }
                if let Some(collection) = &meta.collection {
                    let earliest = series.entry(collection.id).or_insert(meta.fetched);
                    *earliest = (*earliest).min(meta.fetched);
                }
            }
        }
        for show in self.shows.values_mut() {
            if let Some(meta) = show.meta.as_mut().filter(|m| m.fetched == 0) {
                meta.fetched = show.added;
            }
        }
        for (id, collection) in self.collections.iter_mut().filter(|(_, c)| c.fetched == 0) {
            collection.fetched = series.get(id).copied().unwrap_or(0);
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| tr!("Nelze uložit data: {e}", "Cannot save the data: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| tr!("Nelze uložit data: {e}", "Cannot save the data: {e}"))
    }

    pub fn new_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn find_category(&self, name: &str) -> Option<usize> {
        let lower = name.to_lowercase();
        self.categories.iter().position(|c| c.to_lowercase() == lower)
    }

    /// Categories of a movie record or a show.
    pub fn categories_mut(&mut self, id: u64) -> Option<&mut Vec<String>> {
        match self.records.get_mut(&id) {
            Some(record) => Some(&mut record.categories),
            None => self.shows.get_mut(&id).map(|show| &mut show.categories),
        }
    }

    pub fn all_categories_mut(&mut self) -> impl Iterator<Item = &mut Vec<String>> {
        let records = self.records.values_mut().map(|r| &mut r.categories);
        records.chain(self.shows.values_mut().map(|s| &mut s.categories))
    }

    /// Returns the stored spelling of a category, creating it if needed.
    /// The flag says whether it was created.
    pub fn ensure_category(&mut self, name: &str) -> (String, bool) {
        match self.find_category(name) {
            Some(i) => (self.categories[i].clone(), false),
            None => {
                self.categories.push(name.to_string());
                (name.to_string(), true)
            }
        }
    }
}

/// `a\b\c.mkv` -> `a/b/c.mkv`
pub fn rel_to_string(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn join_rel(root: &str, rel: &str) -> PathBuf {
    let mut path = PathBuf::from(root);
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
    }
    path
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------- format of version 1 (single folder, keyed by absolute path) ----------

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct LegacyDb {
    library_path: Option<String>,
    vlc_path: Option<String>,
    categories: Vec<String>,
    movies: BTreeMap<String, LegacyMovie>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct LegacyMovie {
    categories: Vec<String>,
    position: f64,
    duration: f64,
    watched: bool,
    last_played: Option<u64>,
}

fn migrate(old: LegacyDb) -> Db {
    let mut db = Db::default();
    db.settings.vlc_path = old.vlc_path;
    db.categories = old.categories;
    let Some(root) = old.library_path else {
        return db;
    };
    let library = db.new_id();
    db.libraries.push(LibraryDir {
        id: library,
        path: root.clone(),
        label: None,
    });
    for (abs, movie) in old.movies {
        let Ok(rel) = Path::new(&abs).strip_prefix(&root) else {
            continue;
        };
        let id = db.new_id();
        // Size and fingerprint are filled in by the next scan.
        db.records.insert(
            id,
            Record {
                library,
                rel_path: rel_to_string(rel),
                added: now(),
                categories: movie.categories,
                position: movie.position,
                duration: movie.duration,
                watched: movie.watched,
                last_played: movie.last_played,
                ..Default::default()
            },
        );
    }
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_version_1() {
        let old = r#"{"libraryPath":"F:\\Movies","vlcPath":null,"categories":["Komedie"],
            "movies":{"F:\\Movies\\Pelisky\\Pelisky.mkv":{"categories":["Komedie"],"position":120.0,
            "duration":6000.0,"watched":false,"lastPlayed":5}}}"#;
        let db: Db = serde_json::from_str::<Value>(old)
            .and_then(serde_json::from_value::<LegacyDb>)
            .map(migrate)
            .unwrap();
        assert_eq!(db.libraries.len(), 1);
        let rec = db.records.values().next().unwrap();
        assert_eq!(rec.rel_path, "Pelisky/Pelisky.mkv");
        assert_eq!(rec.position, 120.0);
        assert_eq!(rec.categories, vec!["Komedie".to_string()]);
    }
}
