//! Scanning the library folders and matching found files to stored records.
//!
//! Matching runs in three steps so the database lock is not held during disk reads:
//! 1. [`match_paths`] – files at a known relative path keep their record (fast, no reads).
//! 2. [`compute_fingerprints`] – fingerprints for new files and files that changed size,
//!    and the track headers ([`media::probe`]) of files not read yet.
//! 3. [`apply_fingerprints`] – a new file whose fingerprint equals that of a record whose
//!    file disappeared takes over that record (renamed / moved file), otherwise it gets a
//!    new record.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::db::{self, Db, LibraryDir, Record, Show};
use crate::media::{self, ExternalSubtitle, MediaInfo};
use crate::scanner;

pub struct FoundFile {
    pub library: u64,
    pub path: PathBuf,
    pub rel: String,
    pub size: u64,
}

/// Result of the last scan, kept in memory.
#[derive(Default)]
pub struct ScanState {
    pub scanned: bool,
    /// Record id -> absolute path of its file.
    pub found: HashMap<u64, PathBuf>,
    /// Libraries whose folder was reachable.
    pub online: HashSet<u64>,
    /// Subtitle files next to the videos, by record id.
    pub subtitles: HashMap<u64, Vec<ExternalSubtitle>>,
}

pub struct Pending {
    matched: HashMap<u64, PathBuf>,
    /// Known records whose fingerprint (and tracks) must be read again: id, path, size.
    refresh: Vec<(u64, PathBuf, u64)>,
    /// Known records whose tracks were not read yet.
    probe: Vec<(u64, PathBuf)>,
    unknown: Vec<FoundFile>,
}

pub struct Hashed {
    matched: HashMap<u64, PathBuf>,
    refresh: Vec<(u64, String, Option<MediaInfo>)>,
    probed: Vec<(u64, Option<MediaInfo>)>,
    unknown: Vec<(FoundFile, String, Option<MediaInfo>)>,
}

/// Video files of a folder given by its path; None when the folder is not reachable.
pub fn scan_library(library: &LibraryDir) -> Option<Vec<FoundFile>> {
    let root = Path::new(&library.path);
    let found = scanner::scan(root).ok()?;
    Some(
        found
            .into_iter()
            .map(|file| FoundFile {
                library: library.id,
                rel: file.path.strip_prefix(root).map(db::rel_to_string).unwrap_or_default(),
                path: file.path,
                size: file.size,
            })
            .collect(),
    )
}

/// Scans every library with `scan` (see [`scan_library`]); returns the files and the
/// libraries that were reachable.
pub fn scan_folders(
    libraries: &[LibraryDir],
    scan: impl Fn(&LibraryDir) -> Option<Vec<FoundFile>>,
) -> (Vec<FoundFile>, HashSet<u64>) {
    let mut files = Vec::new();
    let mut online = HashSet::new();
    for library in libraries {
        if let Some(found) = scan(library) {
            online.insert(library.id);
            files.extend(found);
        }
    }
    (files, online)
}

/// Opens a file given by its path (the default for [`compute_fingerprints`]).
pub fn open_path(path: &Path) -> Option<File> {
    File::open(path).ok()
}

pub fn match_paths(db: &mut Db, files: Vec<FoundFile>) -> Pending {
    // Windows paths are case-insensitive.
    let index: HashMap<(u64, String), u64> = db
        .records
        .iter()
        .map(|(id, r)| ((r.library, r.rel_path.to_lowercase()), *id))
        .collect();

    let mut pending = Pending {
        matched: HashMap::new(),
        refresh: Vec::new(),
        probe: Vec::new(),
        unknown: Vec::new(),
    };
    for file in files {
        let id = index
            .get(&(file.library, file.rel.to_lowercase()))
            .copied()
            .filter(|id| !pending.matched.contains_key(id));
        let Some((id, record)) = id.and_then(|id| db.records.get_mut(&id).map(|r| (id, r))) else {
            pending.unknown.push(file);
            continue;
        };
        // Same path but different size: probably replaced by another version of the same
        // movie – keep the data, just refresh the fingerprint.
        if record.size != file.size || record.fingerprint.is_empty() {
            pending.refresh.push((id, file.path.clone(), file.size));
        } else if record.media_probe < media::PROBE_VERSION {
            pending.probe.push((id, file.path.clone()));
        }
        record.size = file.size;
        record.rel_path = file.rel;
        pending.matched.insert(id, file.path);
    }
    pending
}

/// `open` opens a found file for reading (on Android also `content://` files).
pub fn compute_fingerprints(
    pending: Pending,
    open: impl Fn(&Path) -> Option<File>,
    progress: impl Fn(usize, usize),
) -> Hashed {
    let total = pending.refresh.len() + pending.probe.len() + pending.unknown.len();
    let mut done = 0;
    let mut tick = || {
        done += 1;
        if done % 10 == 0 || done == total {
            progress(done, total);
        }
    };

    let probe = |path: &Path| open(path).and_then(media::probe);

    let mut refresh = Vec::new();
    for (id, path, size) in pending.refresh {
        let fp = open(&path).and_then(|file| scanner::fingerprint(file, size));
        refresh.push((id, fp.unwrap_or_default(), probe(&path)));
        tick();
    }
    let mut probed = Vec::new();
    for (id, path) in pending.probe {
        probed.push((id, probe(&path)));
        tick();
    }
    let mut unknown = Vec::new();
    for file in pending.unknown {
        let fp = open(&file.path)
            .and_then(|f| scanner::fingerprint(f, file.size))
            .unwrap_or_default();
        let info = probe(&file.path);
        unknown.push((file, fp, info));
        tick();
    }
    Hashed {
        matched: pending.matched,
        refresh,
        probed,
        unknown,
    }
}

/// Returns record id -> path for every found file.
pub fn apply_fingerprints(db: &mut Db, hashed: Hashed) -> HashMap<u64, PathBuf> {
    let mut found = hashed.matched;
    for (id, fp, info) in hashed.refresh {
        if let Some(record) = db.records.get_mut(&id) {
            record.fingerprint = fp;
            set_media(record, info);
        }
    }
    for (id, info) in hashed.probed {
        if let Some(record) = db.records.get_mut(&id) {
            set_media(record, info);
        }
    }

    // Records whose file was not found (including those in offline or removed folders).
    let mut orphans: HashMap<String, Vec<u64>> = HashMap::new();
    for (id, record) in &db.records {
        if !found.contains_key(id) && !record.fingerprint.is_empty() {
            orphans.entry(record.fingerprint.clone()).or_default().push(*id);
        }
    }

    for (file, fp, info) in hashed.unknown {
        let reuse = if fp.is_empty() {
            None
        } else {
            orphans.get_mut(&fp).and_then(|ids| ids.pop())
        };
        let id = match reuse.and_then(|id| db.records.get_mut(&id).map(|r| (id, r))) {
            Some((id, record)) => {
                record.library = file.library;
                record.rel_path = file.rel;
                record.size = file.size;
                set_media(record, info);
                id
            }
            None => {
                let id = db.new_id();
                let mut record = Record {
                    library: file.library,
                    rel_path: file.rel,
                    size: file.size,
                    fingerprint: fp,
                    added: db::now(),
                    ..Default::default()
                };
                set_media(&mut record, info);
                db.records.insert(id, record);
                id
            }
        };
        found.insert(id, file.path);
    }
    found
}

fn set_media(record: &mut Record, info: Option<MediaInfo>) {
    record.media = info;
    record.media_probe = media::PROBE_VERSION;
}

/// Subtitle files next to the found videos (not for folders reached through a URI).
pub fn external_subtitles(found: &HashMap<u64, PathBuf>) -> HashMap<u64, Vec<ExternalSubtitle>> {
    let mut per_folder: HashMap<&Path, usize> = HashMap::new();
    for path in found.values() {
        if let Some(dir) = path.parent() {
            *per_folder.entry(dir).or_default() += 1;
        }
    }
    found
        .iter()
        .filter(|(_, path)| !path.to_str().is_some_and(db::is_uri))
        .filter_map(|(id, path)| {
            let videos = path.parent().and_then(|dir| per_folder.get(dir)).copied().unwrap_or(1);
            let subs = media::find_external_subtitles(path, videos);
            (!subs.is_empty()).then_some((*id, subs))
        })
        .collect()
}

/// Show key -> show id.
pub fn show_ids(db: &Db) -> HashMap<String, u64> {
    db.shows.iter().map(|(id, s)| (s.key.clone(), *id)).collect()
}

/// Creates a show for every series among the records that has none yet. Categories given to
/// episodes while they were treated as movies move to their show.
pub fn update_shows(db: &mut Db) {
    let mut index = show_ids(db);
    let episodes: Vec<(u64, scanner::EpisodeName)> = db
        .records
        .iter()
        .filter_map(|(id, r)| scanner::parse_episode(&r.rel_path).map(|e| (*id, e)))
        .collect();
    for (id, episode) in episodes {
        let key = scanner::show_key(&episode.show);
        let show_id = match index.get(&key) {
            Some(show_id) => *show_id,
            None => {
                let show_id = db.new_id();
                let show = Show {
                    key: key.clone(),
                    name: episode.show,
                    year: episode.year,
                    added: db::now(),
                    ..Default::default()
                };
                db.shows.insert(show_id, show);
                index.insert(key, show_id);
                show_id
            }
        };
        let moved = db.records.get_mut(&id).map(|r| std::mem::take(&mut r.categories)).unwrap_or_default();
        let show = db.shows.get_mut(&show_id).expect("inserted above");
        for category in moved {
            if !show.categories.contains(&category) {
                show.categories.push(category);
            }
        }
    }
}

/// Season numbers of a show's episode records.
pub fn show_seasons(db: &Db, show_id: u64) -> Vec<u32> {
    let Some(show) = db.shows.get(&show_id) else {
        return Vec::new();
    };
    let mut seasons: Vec<u32> = db
        .records
        .values()
        .filter_map(|r| scanner::parse_episode(&r.rel_path))
        .filter(|e| scanner::show_key(&e.show) == show.key)
        .map(|e| e.season)
        .collect();
    seasons.sort_unstable();
    seasons.dedup();
    seasons
}

/// An episode as TheIntroDB looks it up.
pub struct EpisodeLookup {
    /// TMDB id of the series.
    pub tmdb_id: u64,
    pub season: u32,
    pub episode: u32,
    /// Length of the file in seconds (0 = unknown).
    pub length: f64,
}

/// TheIntroDB lookup of an episode record of a series matched on TMDB. None for movies and
/// for files with more episodes (their times would not fit).
pub fn episode_lookup(db: &Db, id: u64) -> Option<EpisodeLookup> {
    let rec = db.records.get(&id)?;
    let ep = scanner::parse_episode(&rec.rel_path).filter(|e| e.last_episode.is_none())?;
    let key = scanner::show_key(&ep.show);
    let tmdb_id = db.shows.values().find(|s| s.key == key)?.meta.as_ref()?.tmdb_id;
    let length = if rec.duration > 0.0 { rec.duration } else { rec.media.as_ref().map_or(0.0, |m| m.duration) };
    (tmdb_id > 0).then_some(EpisodeLookup { tmdb_id, season: ep.season, episode: ep.episode, length })
}

/// Episode records of a show, by season and episode.
pub fn show_episodes(db: &Db, show_id: u64) -> Vec<u64> {
    let Some(show) = db.shows.get(&show_id) else {
        return Vec::new();
    };
    let mut episodes: Vec<(u32, u32, u64)> = db
        .records
        .iter()
        .filter_map(|(id, r)| scanner::parse_episode(&r.rel_path).map(|e| (*id, e)))
        .filter(|(_, e)| scanner::show_key(&e.show) == show.key)
        .map(|(id, e)| (e.season, e.episode, id))
        .collect();
    episodes.sort_unstable();
    episodes.into_iter().map(|(_, _, id)| id).collect()
}

/// The episode after `id` in its series whose file is reachable (`found`): the next one of
/// the season or the first of a following season. Specials (season 0) lead only to specials.
pub fn next_episode(db: &Db, found: &HashMap<u64, PathBuf>, id: u64) -> Option<u64> {
    let current = scanner::parse_episode(&db.records.get(&id)?.rel_path)?;
    let key = scanner::show_key(&current.show);
    let after = (current.season, current.last_episode.unwrap_or(current.episode));
    db.records
        .iter()
        .filter(|(other, _)| found.contains_key(other))
        .filter_map(|(other, r)| scanner::parse_episode(&r.rel_path).map(|e| (*other, e)))
        .filter(|(_, e)| scanner::show_key(&e.show) == key)
        .filter(|(_, e)| (e.season == 0) == (current.season == 0) && (e.season, e.episode) > after)
        .min_by_key(|(other, e)| (e.season, e.episode, *other))
        .map(|(other, _)| other)
}

/// Removes shows that have no episode records left; returns their thumbnails.
pub fn prune_shows(db: &mut Db) -> Vec<String> {
    let keys: HashSet<String> = db
        .records
        .values()
        .filter_map(|r| scanner::parse_episode(&r.rel_path))
        .map(|e| scanner::show_key(&e.show))
        .collect();
    let gone: Vec<u64> = db
        .shows
        .iter()
        .filter(|(_, s)| !keys.contains(&s.key))
        .map(|(id, _)| *id)
        .collect();
    gone.into_iter()
        .filter_map(|id| db.shows.remove(&id).and_then(|s| s.thumbnail))
        .collect()
}

/// `F:\Movies\` and `f:/movies` compare equal; used to reject nested library folders.
/// Folder URIs are compared by their decoded form ("…/tree/1234-5678:Filmy/Akce").
fn normalized_dir(path: &str) -> String {
    let decoded;
    let path = if db::is_uri(path) {
        decoded = path.replace("%3A", ":").replace("%3a", ":").replace("%2F", "/").replace("%2f", "/");
        decoded.as_str()
    } else {
        path
    };
    let mut dir = path
        .trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    dir.push('\\');
    dir
}

pub fn folders_overlap(a: &str, b: &str) -> bool {
    let (a, b) = (normalized_dir(a), normalized_dir(b));
    a.starts_with(&b) || b.starts_with(&a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scan(db: &mut Db) -> HashMap<u64, PathBuf> {
        let (files, _) = scan_folders(&db.libraries, scan_library);
        let pending = match_paths(db, files);
        let hashed = compute_fingerprints(pending, open_path, |_, _| {});
        apply_fingerprints(db, hashed)
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn keeps_data_for_moved_renamed_and_relocated_files() {
        let tmp = std::env::temp_dir().join(format!("moviary-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let root = tmp.join("disk-f");
        write(&root.join("A/Pelisky.mkv"), "pelisky content");
        write(&root.join("Matrix.mp4"), "matrix content");

        let mut db = Db::default();
        let lib = db.new_id();
        db.libraries.push(LibraryDir { id: lib, path: root.to_string_lossy().into(), label: None });
        let found = scan(&mut db);
        assert_eq!(found.len(), 2);
        let pelisky = *found.keys().find(|id| db.records[id].rel_path == "A/Pelisky.mkv").unwrap();
        db.records.get_mut(&pelisky).unwrap().categories = vec!["Komedie".into()];

        // Rename + move within the library.
        fs::create_dir_all(root.join("B")).unwrap();
        fs::rename(root.join("A/Pelisky.mkv"), root.join("B/Pelisky (1999).mkv")).unwrap();
        let found = scan(&mut db);
        assert_eq!(found.len(), 2);
        assert_eq!(db.records[&pelisky].rel_path, "B/Pelisky (1999).mkv");
        assert!(found.contains_key(&pelisky));

        // Whole folder moved (e.g. different drive letter) and the library relocated.
        let moved = tmp.join("disk-g");
        fs::rename(&root, &moved).unwrap();
        let found = scan(&mut db);
        assert!(found.is_empty(), "folder is offline");
        db.libraries[0].path = moved.to_string_lossy().into();
        let found = scan(&mut db);
        assert_eq!(found.len(), 2);
        assert_eq!(db.records[&pelisky].categories, vec!["Komedie".to_string()]);

        // A copy of a known file gets its own record; the original keeps its data.
        fs::copy(moved.join("Matrix.mp4"), moved.join("Matrix copy.mp4")).unwrap();
        let found = scan(&mut db);
        assert_eq!(found.len(), 3);
        assert_eq!(db.records.len(), 3);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn groups_episodes_into_shows() {
        let mut db = Db::default();
        for (rel, categories) in [
            ("Serialy/Lost/Season 1/Lost.S01E01.mkv", vec!["Drama"]),
            ("Serialy/Lost/Season 2/lost.2x03.avi", vec!["Oblíbené"]),
            ("Filmy/Matrix.mkv", vec!["Sci-fi"]),
        ] {
            let id = db.new_id();
            let categories = categories.into_iter().map(String::from).collect();
            db.records.insert(id, Record { rel_path: rel.into(), categories, ..Default::default() });
        }
        update_shows(&mut db);
        update_shows(&mut db);
        assert_eq!(db.shows.len(), 1);
        let show = db.shows.values().next().unwrap();
        assert_eq!(show.name, "Lost");
        assert_eq!(show.categories, vec!["Drama".to_string(), "Oblíbené".to_string()]);
        assert!(db.records.values().any(|r| r.categories == vec!["Sci-fi".to_string()]));

        db.records.retain(|_, r| !r.rel_path.contains("Lost"));
        prune_shows(&mut db);
        assert!(db.shows.is_empty());
    }

    #[test]
    fn detects_nested_folders() {
        assert!(folders_overlap(r"F:\Movies", r"f:/movies/Sci-fi/"));
        assert!(folders_overlap(r"F:\", r"F:\Movies"));
        assert!(!folders_overlap(r"F:\Movies", r"F:\Movies2"));
        let tree = "content://com.android.externalstorage.documents/tree/1A2B-3C4D%3AFilmy";
        assert!(folders_overlap(tree, &format!("{tree}%2FAkce")));
        assert!(!folders_overlap(tree, &format!("{tree}2")));
    }
}
