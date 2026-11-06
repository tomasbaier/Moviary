//! Android: playing in VLC for Android, folder picker, storage permission, video
//! thumbnails and folders on USB drives, implemented by the Kotlin plugin `FilmotekaPlugin`
//! (`gen/android/app/src/main/java/cz/filmoteka/app/FilmotekaPlugin.kt`).
//!
//! VLC is started with an intent and returns the position when it is closed, so the
//! position is stored once, after playback (there is no live progress like on Windows).
//!
//! USB drives are usually not visible as files to apps, only through the system document
//! provider. Such a folder is stored as a `content://` tree URI: it is listed by the plugin,
//! files are read through a file descriptor from it, and VLC gets the file's URI.
//!
//! Plugin calls block until Kotlin answers; they must never run on the main thread
//! (synchronous Tauri commands), so every command using them is `async`.

use std::fs::{self, File};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::thread;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Wry};

use crate::db::{self, LibraryDir, Settings};
use crate::library::{self, FoundFile};
use crate::{playback, scanner};

struct Native(PluginHandle<Wry>);

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("filmoteka")
        .setup(|app, api| {
            let handle = api.register_android_plugin("cz.filmoteka.app", "FilmotekaPlugin")?;
            app.manage(Native(handle));
            Ok(())
        })
        .build()
}

fn call<T: DeserializeOwned>(app: &AppHandle, command: &str, payload: serde_json::Value) -> Result<T, String> {
    app.state::<Native>()
        .0
        .run_mobile_plugin(command, payload)
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VlcInfo {
    installed: bool,
    version: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayArgs {
    path: String,
    /// Milliseconds; 0 = from the start.
    position: i64,
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayResult {
    /// Milliseconds, missing when VLC sent none.
    position: Option<i64>,
    duration: Option<i64>,
    /// VLC closed itself at the end of the video.
    ended: bool,
    error: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Picked {
    path: Option<String>,
    /// Name of a folder URI.
    label: Option<String>,
}

#[derive(Deserialize)]
struct TreeFile {
    /// Path relative to the folder, `/` separated.
    rel: String,
    size: u64,
    uri: String,
}

#[derive(Deserialize)]
struct Tree {
    files: Vec<TreeFile>,
}

#[derive(Deserialize)]
struct Fd {
    fd: i32,
}

#[derive(Deserialize)]
struct Granted {
    granted: bool,
}

#[derive(Deserialize)]
struct Frame {
    ok: bool,
}

fn vlc_info(app: &AppHandle) -> Option<VlcInfo> {
    call::<VlcInfo>(app, "checkVlc", json!({})).ok()
}

/// VLC for Android cannot be asked while it plays.
pub fn current_time(_id: u64) -> Option<f64> {
    None
}

/// Player shown in the settings.
pub fn detected(app: &AppHandle) -> Option<String> {
    vlc_info(app).filter(|v| v.installed).map(|v| match v.version {
        Some(version) => tr!("VLC pro Android {version}", "VLC for Android {version}"),
        None => tr!("VLC pro Android", "VLC for Android"),
    })
}

/// Opens `movie` in VLC for Android; the position is stored when VLC is closed. The intent
/// cannot choose tracks, VLC for Android uses its own language preferences.
#[allow(clippy::too_many_arguments)]
pub fn play(
    app: AppHandle,
    _settings: &Settings,
    id: u64,
    movie: &Path,
    start: f64,
    title: &str,
    _tracks: &crate::tracks::Resolved,
    plan: crate::segments::Plan,
    _fullscreen: bool,
) -> Result<(), String> {
    if !vlc_info(&app).is_some_and(|v| v.installed) {
        return Err(tr!(
            "VLC pro Android není nainstalovaný. Nainstalujte jej z Google Play (Nastavení → Přehrávač).",
            "VLC for Android is not installed. Install it from Google Play (Settings → Player)."
        ));
    }
    let args = PlayArgs {
        path: movie.to_string_lossy().to_string(),
        position: (start * 1000.0) as i64,
        title: title.to_string(),
    };
    let payload = serde_json::to_value(args).map_err(|e| e.to_string())?;
    thread::spawn(move || match call::<PlayResult>(&app, "playVideo", payload) {
        Ok(PlayResult { error: Some(error), .. }) => playback::failed(&app, id, error),
        Ok(result) => {
            let seconds = |ms: Option<i64>| ms.filter(|ms| *ms > 0).map(|ms| ms as f64 / 1000.0);
            let length = seconds(result.duration).unwrap_or(0.0);
            playback::record(&app, id, seconds(result.position), length, true, result.ended);
            // VLC for Android cannot be controlled while it plays: the intro is skipped only
            // at the start (see `Plan::opening_end`) and the next episode follows the end.
            if plan.autoplay && result.ended {
                crate::play_next(&app, id, false);
            }
        }
        Err(error) => playback::failed(&app, id, tr!("VLC se nepodařilo spustit: {error}", "VLC could not be started: {error}")),
    });
    Ok(())
}

/// Lets the user choose a folder; returns `{path, label}`, None when cancelled.
pub fn pick_folder(app: &AppHandle) -> Result<Option<serde_json::Value>, String> {
    let picked = call::<Picked>(app, "pickFolder", json!({}))?;
    Ok(picked.path.is_some().then(|| json!(picked)))
}

/// Video files of a library; a folder URI is listed by the plugin. None when unreachable
/// (drive disconnected, permission gone).
pub fn scan_library(app: &AppHandle, library: &LibraryDir) -> Option<Vec<FoundFile>> {
    if !db::is_uri(&library.path) {
        return library::scan_library(library);
    }
    let tree = call::<Tree>(app, "listTree", json!({ "uri": library.path })).ok()?;
    Some(
        tree.files
            .into_iter()
            .filter(|f| scanner::is_video(f.rel.rsplit('/').next().unwrap_or(&f.rel), f.size))
            .map(|f| FoundFile {
                library: library.id,
                path: PathBuf::from(f.uri),
                rel: f.rel,
                size: f.size,
            })
            .collect(),
    )
}

/// Opens a found file for reading: a path directly, a `content://` URI through the plugin.
pub fn open_file(app: &AppHandle, path: &Path) -> Option<File> {
    let uri = path.to_str().filter(|p| db::is_uri(p));
    let Some(uri) = uri else {
        return library::open_path(path);
    };
    let fd = call::<Fd>(app, "openFile", json!({ "uri": uri })).ok()?;
    // The plugin detached the descriptor; the File owns and closes it.
    (fd.fd >= 0).then(|| unsafe { File::from_raw_fd(fd.fd) })
}

/// Whether the app may read all files; with `request` asks the user first.
pub fn storage_access(app: &AppHandle, request: bool) -> Result<bool, String> {
    call::<Granted>(app, "storageAccess", json!({ "request": request })).map(|g| g.granted)
}

pub fn install_vlc(app: &AppHandle) -> Result<(), String> {
    call::<serde_json::Value>(app, "openVlcStore", json!({})).map(|_| ())
}

/// Opens Android's package installer for a downloaded update.
pub fn install_apk(app: &AppHandle, apk: &Path) -> Result<(), String> {
    call::<serde_json::Value>(app, "installApk", json!({ "uri": apk.to_string_lossy() })).map(|_| ())
}

/// Opens a web page in the browser.
pub fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    call::<serde_json::Value>(app, "openUrl", json!({ "uri": url })).map(|_| ())
}

/// Language of the plugin's messages ("cs" or "en").
pub fn set_language(app: &AppHandle, language: &str) {
    let _ = call::<serde_json::Value>(app, "setLanguage", json!({ "language": language }));
}

/// Creates thumbnails with Android's MediaMetadataRetriever.
pub struct Thumbnailer(AppHandle);

impl Thumbnailer {
    pub fn new(app: &AppHandle, _settings: &Settings) -> Option<Thumbnailer> {
        Some(Thumbnailer(app.clone()))
    }

    /// Saves a frame of the movie as JPEG `dir/name`.
    pub fn snapshot(&self, movie: &Path, dir: &Path, name: &str) -> bool {
        if fs::create_dir_all(dir).is_err() {
            return false;
        }
        let target = dir.join(name);
        let payload = json!({
            "path": movie.to_string_lossy(),
            "target": target.to_string_lossy(),
        });
        call::<Frame>(&self.0, "videoFrame", payload).is_ok_and(|f| f.ok) && target.is_file()
    }
}
