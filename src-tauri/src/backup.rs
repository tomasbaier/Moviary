//! Backups of the data file: exported by the user, made automatically once a day (the last
//! [`AUTO_KEEP`] are kept) and before a restore. A backup is the data in a small wrapper;
//! a plain `library.json` (of any version) can be restored too.
//!
//! Posters and thumbnails are not part of a backup – they are downloaded or created again.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::db::{self, Db};

const KIND: &str = "filmoteka-backup";
const AUTO_PREFIX: &str = "auto-";
const BEFORE_RESTORE_PREFIX: &str = "pred-obnovou-";
/// Automatic backups kept.
const AUTO_KEEP: usize = 7;
const DAY: u64 = 24 * 60 * 60;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub name: String,
    /// "auto", "before-restore" or "manual".
    pub kind: &'static str,
    /// Unix timestamp.
    pub created: u64,
    pub size: u64,
    pub movies: usize,
}

fn wrap(db: &Db) -> Value {
    json!({
        "kind": KIND,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "created": db::now(),
        "data": db,
    })
}

/// Writes a backup of `db` to `path` (through a temporary file).
pub fn write(db: &Db, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| tr!("Nelze vytvořit složku pro zálohu: {e}", "Cannot create the backup folder: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&wrap(db)).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json).map_err(|e| tr!("Zálohu nelze uložit: {e}", "Cannot save the backup: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| tr!("Zálohu nelze uložit: {e}", "Cannot save the backup: {e}"))
}

/// Reads a backup or a data file; returns the data and when the backup was made.
pub fn read(path: &Path) -> Result<(Db, Option<u64>), String> {
    let text = fs::read_to_string(path).map_err(|e| tr!("Soubor nelze přečíst: {e}", "Cannot read the file: {e}"))?;
    parse(&text)
}

fn parse(text: &str) -> Result<(Db, Option<u64>), String> {
    let invalid = || tr!("Soubor není záloha Filmotéky.", "The file is not a Filmotéka backup.");
    let value: Value = serde_json::from_str(text).map_err(|_| invalid())?;
    if value["kind"] == KIND {
        let created = value["created"].as_u64();
        let db = Db::from_json(Ok(value["data"].clone())).map_err(|_| invalid())?;
        return Ok((db, created));
    }
    // A data file: it must look like one, not like any JSON.
    let looks_like_data = ["records", "libraries", "libraryPath", "movies"]
        .iter()
        .any(|key| value.get(key).is_some());
    if !looks_like_data {
        return Err(invalid());
    }
    Db::from_json(Ok(value)).map(|db| (db, None)).map_err(|_| invalid())
}

fn date_stamp(at: u64) -> String {
    // Days since 1970 -> civil date (Howard Hinnant's algorithm), UTC is fine for a file name.
    let days = (at / DAY) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// File name of a backup made now: "Filmoteka-zaloha-2026-11-04.json" and similar.
pub fn file_name(prefix: &str) -> String {
    let now = db::now();
    format!("{prefix}{}-{:05}.json", date_stamp(now), now % DAY)
}

pub fn manual_file_name() -> String {
    format!("Filmoteka-zaloha-{}.json", date_stamp(db::now()))
}

/// Makes the daily automatic backup when the last one is older than a day; removes the
/// oldest ones beyond [`AUTO_KEEP`].
pub fn auto(dir: &Path, db: &Db) -> Result<(), String> {
    if db.records.is_empty() && db.libraries.is_empty() {
        return Ok(());
    }
    let mut autos: Vec<BackupInfo> = list_dir(dir).into_iter().filter(|b| b.kind == "auto").collect();
    autos.sort_by_key(|b| std::cmp::Reverse(b.created));
    if autos.first().is_some_and(|b| db::now().saturating_sub(b.created) < DAY) {
        return Ok(());
    }
    write(db, &dir.join(file_name(AUTO_PREFIX)))?;
    for old in autos.iter().skip(AUTO_KEEP - 1) {
        let _ = fs::remove_file(&old.path);
    }
    Ok(())
}

/// Saves the current data before it is replaced by a restore.
pub fn before_restore(dir: &Path, db: &Db) -> Result<PathBuf, String> {
    let path = dir.join(file_name(BEFORE_RESTORE_PREFIX));
    write(db, &path)?;
    Ok(path)
}

/// Backups in `dir`, newest first.
pub fn list_dir(dir: &Path) -> Vec<BackupInfo> {
    let mut list: Vec<BackupInfo> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_lowercase().ends_with(".json") {
                return None;
            }
            let size = entry.metadata().map(|m| m.len()).ok()?;
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let kind = if name.starts_with(AUTO_PREFIX) {
                "auto"
            } else if name.starts_with(BEFORE_RESTORE_PREFIX) {
                "before-restore"
            } else if name.starts_with("Filmoteka-zaloha") {
                "manual"
            } else {
                return None;
            };
            // Reading the whole file only for the movie count is fine for a handful of files.
            let (db, created) = read(&path).ok()?;
            Some(BackupInfo {
                path: path.to_string_lossy().to_string(),
                name,
                kind,
                created: created.unwrap_or(modified),
                size,
                movies: db.records.len(),
            })
        })
        .collect();
    list.sort_by_key(|b| std::cmp::Reverse(b.created));
    list
}

/// After a restore: posters and thumbnails that are not on this computer are fetched or
/// created again by the metadata job (a known TMDB match is kept, see
/// [`crate::metadata`]).
pub fn reset_missing_images(db: &mut Db, posters_dir: &Path) {
    let missing = |name: &Option<String>| name.as_ref().is_some_and(|n| !posters_dir.join(n).is_file());
    for record in db.records.values_mut() {
        if let Some(meta) = record.meta.as_mut().filter(|m| missing(&m.poster)) {
            meta.poster = None;
            record.meta_checked = false;
        }
        if missing(&record.thumbnail) {
            record.thumbnail = None;
            record.thumb_checked = false;
        }
    }
    for show in db.shows.values_mut() {
        if let Some(meta) = show.meta.as_mut().filter(|m| missing(&m.poster)) {
            meta.poster = None;
            show.meta_checked = false;
        }
        if missing(&show.thumbnail) {
            show.thumbnail = None;
            show.thumb_checked = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_rejects_other_json() {
        let mut db = Db::default();
        db.categories.push("Komedie".into());
        let dir = std::env::temp_dir().join(format!("filmoteka-backup-{}", std::process::id()));
        let path = dir.join(manual_file_name());
        write(&db, &path).unwrap();
        let (restored, created) = read(&path).unwrap();
        assert_eq!(restored.categories, ["Komedie"]);
        assert!(created.is_some());
        assert_eq!(list_dir(&dir).len(), 1);
        assert!(parse(r#"{"foo": 1}"#).is_err());
        assert!(parse("not json").is_err());
        // A data file of version 1 is accepted too.
        assert!(parse(r#"{"libraryPath": "F:\\Filmy", "movies": {}}"#).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates() {
        assert_eq!(date_stamp(0), "1970-01-01");
        assert_eq!(date_stamp(1_793_664_000), "2026-11-03");
    }
}
