// First, so `tr!` is available in every module.
#[macro_use]
mod i18n;
mod backup;
mod db;
mod library;
mod media;
mod metadata;
#[cfg(mobile)]
mod mobile;
mod playback;
mod scanner;
mod tmdb;
mod tracks;
mod update;
mod view;
#[cfg(desktop)]
mod vlc;
mod watch;

// The player: VLC on Windows, VLC for Android on Android. Both modules offer the same
// functions (`detected`, `play`, `Thumbnailer`).
#[cfg(mobile)]
use mobile as player;
#[cfg(desktop)]
use vlc as player;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::http::Response;
use tauri::{AppHandle, Emitter, Manager, State};

use db::Db;
use library::ScanState;
use view::{AppData, Movie};

/// Positions shorter than this are not worth resuming.
const RESUME_MIN_SECS: f64 = 10.0;
/// When resuming, start a few seconds earlier to regain context.
const RESUME_REWIND_SECS: f64 = 5.0;

pub struct AppState {
    pub db: Mutex<Db>,
    pub db_path: PathBuf,
    /// Downloaded posters and generated thumbnails.
    pub posters_dir: PathBuf,
    /// Automatic backups and those made before a restore.
    pub backups_dir: PathBuf,
    pub scan: Mutex<ScanState>,
    /// Serializes scans.
    scan_lock: Mutex<()>,
    /// Ids of records currently open in VLC.
    pub playing: Mutex<HashSet<u64>>,
    /// Position (seconds) each running playback started from, for the history.
    pub sessions: Mutex<HashMap<u64, f64>>,
    pub job: metadata::Job,
    pub watch: watch::Watch,
    pub update: update::Updater,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    /// "android" or "desktop".
    platform: &'static str,
    /// Version of the app.
    version: String,
    /// "cs" or "en"; empty until chosen (the frontend picks by the system language).
    language: String,
    vlc_path: Option<String>,
    vlc_detected: Option<String>,
    data_path: String,
    tmdb_key: Option<String>,
    auto_metadata: bool,
    genres_as_categories: bool,
    video_thumbnails: bool,
    watch_folders: bool,
    check_updates: bool,
    tracks: tracks::TrackRules,
    /// Code and Czech name of the known languages.
    languages: Vec<(&'static str, &'static str)>,
    backups_dir: String,
}

/// Blocks on Android (asks the plugin about VLC) – call only from async commands.
fn settings_view(app: &AppHandle) -> SettingsView {
    let vlc_detected = player::detected(app);
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    SettingsView {
        platform: if cfg!(target_os = "android") { "android" } else { "desktop" },
        version: app.package_info().version.to_string(),
        language: db.settings.language.clone(),
        vlc_path: db.settings.vlc_path.clone(),
        vlc_detected,
        data_path: state.db_path.to_string_lossy().to_string(),
        tmdb_key: db.settings.tmdb_key.clone(),
        auto_metadata: db.settings.auto_metadata,
        genres_as_categories: db.settings.genres_as_categories,
        video_thumbnails: db.settings.video_thumbnails,
        watch_folders: db.settings.watch_folders,
        check_updates: db.settings.check_updates,
        tracks: db.settings.tracks.clone(),
        languages: media::language_names(),
        backups_dir: state.backups_dir.to_string_lossy().to_string(),
    }
}

/// Applies `change` to a record, saves and returns the updated movie.
fn update_record(state: &AppState, id: u64, change: impl FnOnce(&mut db::Record)) -> Result<Movie, String> {
    {
        let mut db = state.db.lock().unwrap();
        let record = db.records.get_mut(&id).ok_or_else(|| tr!("Film nebyl nalezen.", "Movie not found."))?;
        change(record);
        db.save(&state.db_path)?;
    }
    view::movie_by_id(state, id)
}

fn update_show(state: &AppState, id: u64, change: impl FnOnce(&mut db::Show)) -> Result<view::Show, String> {
    {
        let mut db = state.db.lock().unwrap();
        let show = db.shows.get_mut(&id).ok_or_else(|| tr!("Seriál nebyl nalezen.", "Series not found."))?;
        change(show);
        db.save(&state.db_path)?;
    }
    view::show_by_id(state, id)
}

fn clean_category_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(tr!("Název kategorie nesmí být prázdný.", "The category name must not be empty."));
    }
    Ok(name)
}

fn check_folder(state: &AppState, path: &str, except: Option<u64>) -> Result<(), String> {
    // A folder URI comes straight from the Android folder picker.
    if !db::is_uri(path) && !Path::new(path).is_dir() {
        return Err(tr!("Složka „{path}“ neexistuje.", "Folder “{path}” does not exist."));
    }
    let db = state.db.lock().unwrap();
    if let Some(other) = db
        .libraries
        .iter()
        .find(|l| Some(l.id) != except && library::folders_overlap(&l.path, path))
    {
        let other = &other.path;
        return Err(tr!(
            "Složka se překrývá s již přidanou složkou „{other}“.",
            "The folder overlaps with the already added folder “{other}”."
        ));
    }
    Ok(())
}

/// Scans all library folders (also used by the folder watcher).
pub(crate) fn run_scan(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _guard = state.scan_lock.lock().unwrap();
    let libraries = state.db.lock().unwrap().libraries.clone();
    #[cfg(desktop)]
    let (files, online) = library::scan_folders(&libraries, library::scan_library);
    #[cfg(mobile)]
    let (files, online) = library::scan_folders(&libraries, |l| mobile::scan_library(app, l));
    let pending = {
        let mut db = state.db.lock().unwrap();
        library::match_paths(&mut db, files)
    };
    #[cfg(desktop)]
    let open = library::open_path;
    #[cfg(mobile)]
    let open = |path: &Path| mobile::open_file(app, path);
    let hashed = library::compute_fingerprints(pending, open, |done, total| {
        let _ = app.emit("scan-progress", serde_json::json!({ "done": done, "total": total }));
    });
    let found = {
        let mut db = state.db.lock().unwrap();
        let found = library::apply_fingerprints(&mut db, hashed);
        library::update_shows(&mut db);
        db.save(&state.db_path)?;
        found
    };
    let subtitles = library::external_subtitles(&found);
    *state.scan.lock().unwrap() = ScanState {
        scanned: true,
        found,
        online,
        subtitles,
    };
    drop(_guard);
    watch::update(app);
    Ok(())
}

async fn scan_and_report(app: AppHandle) -> Result<AppData, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_scan(&handle))
        .await
        .map_err(|e| e.to_string())??;
    Ok(view::app_data(&app.state()))
}

// ---------- library folders ----------

#[tauri::command]
fn get_data(state: State<AppState>) -> AppData {
    view::app_data(&state)
}

#[tauri::command]
async fn scan(app: AppHandle) -> Result<AppData, String> {
    scan_and_report(app).await
}

/// `label` names a folder URI (Android).
#[tauri::command]
async fn add_library(app: AppHandle, path: String, label: Option<String>) -> Result<AppData, String> {
    {
        let state = app.state::<AppState>();
        check_folder(&state, &path, None)?;
        let mut db = state.db.lock().unwrap();
        let id = db.new_id();
        db.libraries.push(db::LibraryDir { id, path, label });
        db.save(&state.db_path)?;
    }
    scan_and_report(app).await
}

#[tauri::command]
async fn relocate_library(app: AppHandle, id: u64, path: String, label: Option<String>) -> Result<AppData, String> {
    {
        let state = app.state::<AppState>();
        check_folder(&state, &path, Some(id))?;
        let mut db = state.db.lock().unwrap();
        let library = db
            .libraries
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| tr!("Složka nebyla nalezena.", "Folder not found."))?;
        library.path = path;
        library.label = label;
        db.save(&state.db_path)?;
    }
    scan_and_report(app).await
}

/// Records stay stored, so adding the folder again restores everything.
#[tauri::command]
fn remove_library(state: State<AppState>, id: u64) -> Result<AppData, String> {
    {
        let mut db = state.db.lock().unwrap();
        db.libraries.retain(|l| l.id != id);
        db.save(&state.db_path)?;
    }
    state.scan.lock().unwrap().online.remove(&id);
    Ok(view::app_data(&state))
}

#[tauri::command]
fn cleanup_missing(state: State<AppState>) -> Result<AppData, String> {
    {
        let _guard = state.scan_lock.lock().unwrap();
        let mut db = state.db.lock().unwrap();
        let scan = state.scan.lock().unwrap();
        if !scan.scanned {
            return Err(tr!("Složky ještě nebyly načteny.", "The folders have not been scanned yet."));
        }
        for id in view::missing_ids(&db, &scan) {
            if let Some(thumb) = db.records.remove(&id).and_then(|r| r.thumbnail) {
                let _ = fs::remove_file(state.posters_dir.join(thumb));
            }
        }
        for thumb in library::prune_shows(&mut db) {
            let _ = fs::remove_file(state.posters_dir.join(thumb));
        }
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

// ---------- categories ----------

#[tauri::command]
fn create_category(state: State<AppState>, name: String) -> Result<AppData, String> {
    let name = clean_category_name(&name)?;
    {
        let mut db = state.db.lock().unwrap();
        if db.find_category(&name).is_some() {
            return Err(tr!("Kategorie „{name}“ už existuje.", "Category “{name}” already exists."));
        }
        db.categories.push(name);
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

#[tauri::command]
fn rename_category(state: State<AppState>, old: String, new: String) -> Result<AppData, String> {
    let new = clean_category_name(&new)?;
    {
        let mut db = state.db.lock().unwrap();
        let index = db
            .find_category(&old)
            .ok_or_else(|| tr!("Kategorie „{old}“ neexistuje.", "Category “{old}” does not exist."))?;
        if db.find_category(&new).is_some_and(|i| i != index) {
            return Err(tr!("Kategorie „{new}“ už existuje.", "Category “{new}” already exists."));
        }
        let old = std::mem::replace(&mut db.categories[index], new.clone());
        for categories in db.all_categories_mut() {
            for category in categories.iter_mut() {
                if *category == old {
                    *category = new.clone();
                }
            }
        }
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

#[tauri::command]
fn delete_category(state: State<AppState>, name: String) -> Result<AppData, String> {
    {
        let mut db = state.db.lock().unwrap();
        db.categories.retain(|c| *c != name);
        for categories in db.all_categories_mut() {
            categories.retain(|c| *c != name);
        }
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

/// Known categories from `categories`, without duplicates.
fn valid_categories(state: &AppState, categories: Vec<String>) -> Vec<String> {
    let known = state.db.lock().unwrap().categories.clone();
    let mut valid: Vec<String> = Vec::new();
    for category in categories {
        if known.contains(&category) && !valid.contains(&category) {
            valid.push(category);
        }
    }
    valid
}

#[tauri::command]
fn set_movie_categories(state: State<AppState>, id: u64, categories: Vec<String>) -> Result<Movie, String> {
    let valid = valid_categories(&state, categories);
    update_record(&state, id, |r| r.categories = valid)
}

#[tauri::command]
fn set_show_categories(state: State<AppState>, id: u64, categories: Vec<String>) -> Result<view::Show, String> {
    let valid = valid_categories(&state, categories);
    update_show(&state, id, |s| s.categories = valid)
}

/// `ids` are movies or shows.
#[tauri::command]
fn bulk_set_category(state: State<AppState>, ids: Vec<u64>, category: String, add: bool) -> Result<AppData, String> {
    {
        let mut db = state.db.lock().unwrap();
        if !db.categories.contains(&category) {
            return Err(tr!("Kategorie „{category}“ neexistuje.", "Category “{category}” does not exist."));
        }
        for id in &ids {
            let Some(categories) = db.categories_mut(*id) else {
                continue;
            };
            let has = categories.contains(&category);
            if add && !has {
                categories.push(category.clone());
            } else if !add && has {
                categories.retain(|c| *c != category);
            }
        }
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

// ---------- playback ----------

/// Own information of a movie or series: valid rating, note limited in length.
fn personal(wishlist: bool, rating: u8, note: String) -> db::Personal {
    db::Personal { wishlist, rating: rating.min(5), note: note.trim().chars().take(4000).collect() }
}

#[tauri::command]
fn set_movie_personal(state: State<AppState>, id: u64, wishlist: bool, rating: u8, note: String) -> Result<Movie, String> {
    let value = personal(wishlist, rating, note);
    update_record(&state, id, |r| r.personal = value)
}

#[tauri::command]
fn set_show_personal(state: State<AppState>, id: u64, wishlist: bool, rating: u8, note: String) -> Result<view::Show, String> {
    let value = personal(wishlist, rating, note);
    update_show(&state, id, |s| s.personal = value)
}

/// Adds or removes movies and shows (`ids`) from the "want to watch" list.
#[tauri::command]
fn bulk_set_wishlist(state: State<AppState>, ids: Vec<u64>, wishlist: bool) -> Result<AppData, String> {
    {
        let mut db = state.db.lock().unwrap();
        for id in &ids {
            if let Some(r) = db.records.get_mut(id) {
                r.personal.wishlist = wishlist;
            } else if let Some(s) = db.shows.get_mut(id) {
                s.personal.wishlist = wishlist;
            }
        }
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

// ---------- saved filters ----------

/// Saves `filter` under `name` (replacing one of the same name); returns all saved filters.
#[tauri::command]
fn save_filter(state: State<AppState>, name: String, filter: serde_json::Value) -> Result<Vec<db::SavedFilter>, String> {
    let name: String = name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(tr!("Zadejte název filtru.", "Enter a name for the filter."));
    }
    let mut db = state.db.lock().unwrap();
    let saved = db::SavedFilter { name: name.clone(), filter };
    match db.saved_filters.iter_mut().find(|f| f.name.to_lowercase() == name.to_lowercase()) {
        Some(existing) => *existing = saved,
        None => db.saved_filters.push(saved),
    }
    db.save(&state.db_path)?;
    Ok(db.saved_filters.clone())
}

#[tauri::command]
fn rename_filter(state: State<AppState>, name: String, new_name: String) -> Result<Vec<db::SavedFilter>, String> {
    let new_name: String = new_name.trim().chars().take(60).collect();
    if new_name.is_empty() {
        return Err(tr!("Zadejte název filtru.", "Enter a name for the filter."));
    }
    let mut db = state.db.lock().unwrap();
    if db.saved_filters.iter().any(|f| f.name != name && f.name.to_lowercase() == new_name.to_lowercase()) {
        return Err(tr!("Filtr „{new_name}“ už existuje.", "A filter named “{new_name}” already exists."));
    }
    if let Some(f) = db.saved_filters.iter_mut().find(|f| f.name == name) {
        f.name = new_name;
    }
    db.save(&state.db_path)?;
    Ok(db.saved_filters.clone())
}

#[tauri::command]
fn delete_filter(state: State<AppState>, name: String) -> Result<Vec<db::SavedFilter>, String> {
    let mut db = state.db.lock().unwrap();
    db.saved_filters.retain(|f| f.name != name);
    db.save(&state.db_path)?;
    Ok(db.saved_filters.clone())
}

#[tauri::command]
fn set_watched(state: State<AppState>, id: u64, watched: bool) -> Result<Movie, String> {
    update_record(&state, id, |r| {
        r.watched = watched;
        if watched {
            r.position = 0.0;
        }
    })
}

#[tauri::command]
fn bulk_set_watched(state: State<AppState>, ids: Vec<u64>, watched: bool) -> Result<AppData, String> {
    {
        let mut db = state.db.lock().unwrap();
        for id in &ids {
            if let Some(record) = db.records.get_mut(id) {
                record.watched = watched;
                record.position = 0.0;
            }
        }
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

#[tauri::command]
fn reset_progress(state: State<AppState>, id: u64) -> Result<Movie, String> {
    update_record(&state, id, |r| r.position = 0.0)
}

#[tauri::command]
async fn play_movie(app: AppHandle, id: u64, from_start: bool) -> Result<Movie, String> {
    tauri::async_runtime::spawn_blocking(move || start_playback(app, id, from_start))
        .await
        .map_err(|e| e.to_string())?
}

fn start_playback(app: AppHandle, id: u64, from_start: bool) -> Result<Movie, String> {
    let state = app.state::<AppState>();
    let path = state
        .scan
        .lock()
        .unwrap()
        .found
        .get(&id)
        .cloned()
        .filter(|p| p.to_str().is_some_and(db::is_uri) || p.is_file())
        .ok_or_else(|| tr!(
            "Soubor není dostupný – je disk připojený? Zkuste složky načíst znovu.",
            "The file is not available – is the drive connected? Try scanning the folders again."
        ))?;
    let (settings, position) = {
        let db = state.db.lock().unwrap();
        let position = db.records.get(&id).map(|r| r.position).unwrap_or(0.0);
        (db.settings.clone(), position)
    };
    let start = if !from_start && position >= RESUME_MIN_SECS {
        (position - RESUME_REWIND_SECS).max(0.0)
    } else {
        0.0
    };
    let title = view::player_title(&state, id);
    let tracks = view::resolved_tracks(&state, id);

    // On Android the system may drop the running VLC without telling us, so starting the
    // movie again is always allowed there.
    if !state.playing.lock().unwrap().insert(id) && cfg!(desktop) {
        return Err(tr!("Tento film už se přehrává ve VLC.", "This movie is already playing in VLC."));
    }
    state.sessions.lock().unwrap().insert(id, start);
    if let Err(e) = player::play(app.clone(), &settings, id, &path, start, &title, &tracks) {
        state.playing.lock().unwrap().remove(&id);
        state.sessions.lock().unwrap().remove(&id);
        return Err(e);
    }
    view::movie_by_id(&state, id)
}

// ---------- tracks ----------

#[tauri::command]
fn set_movie_tracks(state: State<AppState>, id: u64, tracks: tracks::TrackChoice) -> Result<Movie, String> {
    update_record(&state, id, |r| r.tracks = tracks)
}

/// Applies to every episode, so all data is returned.
#[tauri::command]
fn set_show_tracks(state: State<AppState>, id: u64, tracks: tracks::TrackChoice) -> Result<AppData, String> {
    update_show(&state, id, |s| s.tracks = tracks)?;
    Ok(view::app_data(&state))
}

#[tauri::command]
async fn set_track_rules(app: AppHandle, rules: tracks::TrackRules) -> Result<AppData, String> {
    let state = app.state::<AppState>();
    {
        let mut db = state.db.lock().unwrap();
        db.settings.tracks = rules;
        db.save(&state.db_path)?;
    }
    Ok(view::app_data(&state))
}

// ---------- statistics, duplicates ----------

#[tauri::command]
fn get_history(state: State<AppState>) -> Vec<db::PlayEvent> {
    state.db.lock().unwrap().history.clone()
}

#[tauri::command]
fn find_duplicates(state: State<AppState>) -> Vec<view::DuplicateGroup> {
    view::duplicates(&state)
}

// ---------- backups ----------

#[tauri::command]
fn list_backups(state: State<AppState>) -> Vec<backup::BackupInfo> {
    let mut list = backup::list_dir(&state.backups_dir);
    // Exports in Downloads (Android has no save dialog).
    if cfg!(mobile) {
        list.extend(backup::list_dir(Path::new(ANDROID_DOWNLOADS)));
        list.sort_by_key(|b| std::cmp::Reverse(b.created));
    }
    list
}

/// Public Downloads folder of Android, writable with "access to all files".
const ANDROID_DOWNLOADS: &str = "/storage/emulated/0/Download";

/// Writes a backup to `path`; without a path (Android) into Downloads. Returns the file.
#[tauri::command]
fn export_backup(state: State<AppState>, path: Option<String>) -> Result<String, String> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => Path::new(ANDROID_DOWNLOADS).join(backup::manual_file_name()),
    };
    backup::write(&state.db.lock().unwrap(), &path)?;
    Ok(path.to_string_lossy().to_string())
}

/// Replaces all data with a backup (the current data is backed up first) and scans again.
#[tauri::command]
async fn restore_backup(app: AppHandle, path: String) -> Result<AppData, String> {
    {
        let state = app.state::<AppState>();
        let (mut restored, _) = backup::read(Path::new(&path))?;
        let _guard = state.scan_lock.lock().unwrap();
        if !state.playing.lock().unwrap().is_empty() {
            return Err(tr!("Nejprve zavřete přehrávaný film.", "Close the playing movie first."));
        }
        if state.job.running.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(tr!("Počkejte na dokončení načítání informací o filmech.", "Wait until loading movie information finishes."));
        }
        backup::reset_missing_images(&mut restored, &state.posters_dir);
        let mut db = state.db.lock().unwrap();
        backup::before_restore(&state.backups_dir, &db)?;
        *db = restored;
        db.save(&state.db_path)?;
        *state.scan.lock().unwrap() = ScanState::default();
    }
    scan_and_report(app).await
}

#[cfg(desktop)]
#[tauri::command]
fn open_backups_folder(state: State<AppState>) -> Result<(), String> {
    fs::create_dir_all(&state.backups_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&state.backups_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(mobile)]
#[tauri::command]
fn open_backups_folder() -> Result<(), String> {
    Err(tr!("Na Androidu není k dispozici.", "Not available on Android."))
}

#[cfg(desktop)]
#[tauri::command]
fn reveal_in_explorer(state: State<AppState>, id: u64) -> Result<(), String> {
    use std::process::Command;
    let path = state
        .scan
        .lock()
        .unwrap()
        .found
        .get(&id)
        .cloned()
        .ok_or_else(|| tr!("Soubor není dostupný.", "The file is not available."))?;
    let mut cmd = Command::new("explorer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/select,\"{}\"", path.display()));
    }
    #[cfg(not(windows))]
    cmd.arg(&path);
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(mobile)]
#[tauri::command]
fn reveal_in_explorer(id: u64) -> Result<(), String> {
    let _ = id;
    Err(tr!("Na Androidu není k dispozici.", "Not available on Android."))
}

/// Web pages the app links to (the frontend names them, so it cannot open anything else).
const LINKS: &[(&str, &str)] = &[
    ("tmdb", "https://www.themoviedb.org/"),
    ("tmdb-api", "https://www.themoviedb.org/settings/api"),
    ("tmdb-terms", "https://www.themoviedb.org/api-terms-of-use"),
    ("videolan", "https://www.videolan.org/"),
    ("tauri", "https://tauri.app/"),
    ("repo", "https://github.com/tomasbaier/Moviary"),
    ("releases", "https://github.com/tomasbaier/Moviary/releases/latest"),
];

/// Opens one of [`LINKS`] in the default browser.
#[tauri::command]
async fn open_link(app: AppHandle, link: String) -> Result<(), String> {
    let url = LINKS
        .iter()
        .find(|(name, _)| *name == link)
        .map(|(_, url)| *url)
        .ok_or_else(|| format!("Unknown link: {link}"))?;
    #[cfg(mobile)]
    return tauri::async_runtime::spawn_blocking(move || mobile::open_url(&app, url))
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    {
        let _ = app;
        // Explorer hands a URL to the default browser.
        std::process::Command::new("explorer").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

// ---------- updates ----------

/// Newer release on GitHub, or None. `manual` ignores the setting, the daily interval and a
/// skipped version.
#[tauri::command]
async fn check_update(app: AppHandle, manual: bool) -> Result<Option<update::UpdateInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || update::check(&app, manual))
        .await
        .map_err(|e| e.to_string())?
}

/// Downloads and installs the release found by the last check (restarts the app on Windows).
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || update::install(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn skip_update(app: AppHandle, version: String) -> Result<(), String> {
    update::skip(&app, version)
}

// ---------- Android ----------

/// Folder picker of Android (on Windows the frontend uses the dialog plugin). Returns the
/// path, or a folder URI with a label for drives Android does not expose as files.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    #[cfg(mobile)]
    return tauri::async_runtime::spawn_blocking(move || mobile::pick_folder(&app))
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    {
        let _ = app;
        Err(tr!("Výběr složky zajišťuje dialog.", "Folders are chosen with the dialog."))
    }
}

/// Whether the app may read the video files; with `request` asks the user first.
#[tauri::command]
async fn storage_access(app: AppHandle, request: bool) -> Result<bool, String> {
    #[cfg(mobile)]
    return tauri::async_runtime::spawn_blocking(move || mobile::storage_access(&app, request))
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    {
        let _ = (app, request);
        Ok(true)
    }
}

/// Opens VLC for Android in Google Play.
#[tauri::command]
async fn install_vlc(app: AppHandle) -> Result<(), String> {
    #[cfg(mobile)]
    return tauri::async_runtime::spawn_blocking(move || mobile::install_vlc(&app))
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    {
        let _ = app;
        Err(tr!("Nainstalujte VLC z videolan.org.", "Install VLC from videolan.org."))
    }
}

// ---------- metadata ----------

#[tauri::command]
fn fetch_metadata(app: AppHandle, ids: Option<Vec<u64>>, force: bool) -> Result<usize, String> {
    metadata::start(&app, ids, force)
}

#[tauri::command]
fn cancel_metadata(state: State<AppState>) {
    state.job.cancel();
}

fn tmdb_client(state: &AppState) -> Result<tmdb::Client, String> {
    let db = state.db.lock().unwrap();
    let key = db
        .settings
        .tmdb_key
        .as_deref()
        .ok_or_else(|| tr!("Nejprve zadejte TMDB klíč v Nastavení.", "Enter a TMDB key in the Settings first."))?;
    Ok(tmdb::Client::new(key))
}

/// Searches movies, or TV series when `tv` is set.
#[tauri::command]
async fn search_tmdb(app: AppHandle, query: String, year: Option<u32>, tv: bool) -> Result<Vec<tmdb::Candidate>, String> {
    let client = tmdb_client(&app.state())?;
    tauri::async_runtime::spawn_blocking(move || client.search(query.trim(), year, tv).map_err(String::from))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn apply_tmdb_match(app: AppHandle, id: u64, tmdb_id: u64) -> Result<Movie, String> {
    let client = tmdb_client(&app.state())?;
    let posters_dir = app.state::<AppState>().posters_dir.clone();
    let handle = app.clone();
    let meta = tauri::async_runtime::spawn_blocking(move || {
        let meta = client.details(tmdb_id, &posters_dir)?;
        metadata::fetch_collection(&handle.state::<AppState>(), &client, &meta);
        Ok::<_, tmdb::Error>(meta)
    })
    .await
    .map_err(|e| e.to_string())??;
    metadata::apply(&app, id, Some(Some(metadata::Info::Movie(meta))), None)?;
    view::movie_by_id(&app.state(), id)
}

#[tauri::command]
async fn apply_tmdb_show_match(app: AppHandle, id: u64, tmdb_id: u64) -> Result<view::Show, String> {
    let client = tmdb_client(&app.state())?;
    let state = app.state::<AppState>();
    let posters_dir = state.posters_dir.clone();
    let seasons = library::show_seasons(&state.db.lock().unwrap(), id);
    let info = tauri::async_runtime::spawn_blocking(move || client.show_details(tmdb_id, &seasons, &posters_dir))
        .await
        .map_err(|e| e.to_string())??;
    metadata::apply(&app, id, Some(Some(metadata::Info::Show(info))), None)?;
    view::show_by_id(&state, id)
}

#[tauri::command]
fn clear_metadata(state: State<AppState>, id: u64) -> Result<Movie, String> {
    update_record(&state, id, |r| {
        r.meta = None;
        r.meta_checked = true;
    })
}

#[tauri::command]
fn clear_show_metadata(state: State<AppState>, id: u64) -> Result<view::Show, String> {
    update_show(&state, id, |s| {
        s.meta = None;
        s.seasons.clear();
        s.episodes.clear();
        s.status = None;
        s.next_episode = None;
        s.meta_checked = true;
    })
}

// ---------- settings ----------

#[tauri::command]
async fn get_settings(app: AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command]
async fn set_vlc_path(app: AppHandle, path: Option<String>) -> Result<SettingsView, String> {
    if let Some(p) = &path {
        if !Path::new(p).is_file() {
            return Err(tr!("Soubor „{p}“ neexistuje.", "File “{p}” does not exist."));
        }
    }
    {
        let state = app.state::<AppState>();
        let mut db = state.db.lock().unwrap();
        db.settings.vlc_path = path;
        db.save(&state.db_path)?;
    }
    Ok(settings_view(&app))
}

/// Verifies the key with TMDB before saving it. An empty key removes it.
#[tauri::command]
async fn set_tmdb_key(app: AppHandle, key: String) -> Result<SettingsView, String> {
    let key = key.trim().to_string();
    if !key.is_empty() {
        let client = tmdb::Client::new(&key);
        tauri::async_runtime::spawn_blocking(move || client.validate())
            .await
            .map_err(|e| e.to_string())?
            .map_err(String::from)?;
    }
    let state = app.state::<AppState>();
    {
        let mut db = state.db.lock().unwrap();
        db.settings.tmdb_key = (!key.is_empty()).then_some(key);
        if db.settings.tmdb_key.is_some() {
            // Movies checked without a key get another chance.
            for record in db.records.values_mut().filter(|r| r.meta.is_none()) {
                record.meta_checked = false;
            }
            for show in db.shows.values_mut().filter(|s| s.meta.is_none()) {
                show.meta_checked = false;
            }
        }
        db.save(&state.db_path)?;
    }
    Ok(settings_view(&app))
}

/// Language of the app: "cs" or "en".
#[tauri::command]
async fn set_language(app: AppHandle, language: String) -> Result<SettingsView, String> {
    if language != "cs" && language != "en" {
        return Err(format!("Unknown language: {language}"));
    }
    {
        let state = app.state::<AppState>();
        let mut db = state.db.lock().unwrap();
        db.settings.language = language.clone();
        db.save(&state.db_path)?;
    }
    i18n::set_language(&language);
    #[cfg(mobile)]
    mobile::set_language(&app, &language);
    Ok(settings_view(&app))
}

#[tauri::command]
async fn set_options(
    app: AppHandle,
    auto_metadata: bool,
    genres_as_categories: bool,
    video_thumbnails: bool,
    watch_folders: bool,
    check_updates: Option<bool>,
) -> Result<SettingsView, String> {
    {
        let state = app.state::<AppState>();
        let mut db = state.db.lock().unwrap();
        db.settings.auto_metadata = auto_metadata;
        db.settings.genres_as_categories = genres_as_categories;
        db.settings.video_thumbnails = video_thumbnails;
        db.settings.watch_folders = watch_folders;
        if let Some(check_updates) = check_updates {
            db.settings.check_updates = check_updates;
        }
        db.save(&state.db_path)?;
    }
    watch::update(&app);
    Ok(settings_view(&app))
}

/// Serves posters and thumbnails: `http://poster.localhost/<file>` on Windows and Android.
fn poster_response(app: &AppHandle, uri_path: &str) -> Response<Vec<u8>> {
    let name = uri_path.trim_start_matches('/');
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    let bytes = valid
        .then(|| fs::read(app.state::<AppState>().posters_dir.join(name)).ok())
        .flatten();
    match bytes {
        Some(bytes) => Response::builder()
            .header("Content-Type", "image/jpeg")
            .body(bytes)
            .unwrap(),
        None => Response::builder().status(404).body(Vec::new()).unwrap(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(mobile)]
    let builder = builder.plugin(mobile::init());
    builder
        .register_uri_scheme_protocol("poster", |ctx, request| {
            poster_response(ctx.app_handle(), request.uri().path())
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db_path = data_dir.join("library.json");
            let backups_dir = data_dir.join("backups");
            let db = Db::load(&db_path);
            i18n::set_language(&db.settings.language);
            // A failed automatic backup must not stop the app.
            let _ = backup::auto(&backups_dir, &db);
            app.manage(AppState {
                db: Mutex::new(db),
                db_path,
                posters_dir: data_dir.join("posters"),
                backups_dir,
                scan: Mutex::new(ScanState::default()),
                scan_lock: Mutex::new(()),
                playing: Mutex::new(HashSet::new()),
                sessions: Mutex::new(HashMap::new()),
                job: metadata::Job::default(),
                watch: watch::Watch::default(),
                update: update::Updater::default(),
            });
            watch::start(app.handle());
            update::cleanup(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_data,
            open_link,
            set_movie_personal,
            set_show_personal,
            bulk_set_wishlist,
            save_filter,
            rename_filter,
            delete_filter,
            check_update,
            install_update,
            skip_update,
            scan,
            add_library,
            relocate_library,
            remove_library,
            cleanup_missing,
            create_category,
            rename_category,
            delete_category,
            set_movie_categories,
            set_show_categories,
            bulk_set_category,
            set_watched,
            bulk_set_watched,
            reset_progress,
            play_movie,
            reveal_in_explorer,
            fetch_metadata,
            cancel_metadata,
            search_tmdb,
            apply_tmdb_match,
            apply_tmdb_show_match,
            clear_metadata,
            clear_show_metadata,
            get_settings,
            set_vlc_path,
            set_tmdb_key,
            set_options,
            set_language,
            pick_folder,
            storage_access,
            install_vlc,
            set_movie_tracks,
            set_show_tracks,
            set_track_rules,
            get_history,
            find_duplicates,
            list_backups,
            export_backup,
            restore_backup,
            open_backups_folder,
        ])
        .run(tauri::generate_context!())
        .expect("chyba při spouštění aplikace");
}
