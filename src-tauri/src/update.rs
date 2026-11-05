//! Updates from GitHub Releases.
//!
//! The latest release of [`REPO`] is checked at most once a day (when enabled) or on request.
//! Its asset `Moviary-<version>-windows.exe` / `-android.apk` is downloaded and checked
//! against the size and SHA-256 digest GitHub reports for it.
//! - Windows: the app is a single exe. The running exe is renamed to `<name>.old` (Windows
//!   allows renaming a running program), the new one takes its place and is started, and the
//!   old one is deleted by the new instance ([`cleanup`]). Shortcuts keep working.
//! - Android: the APK goes to the cache folder and Android's package installer is opened; the
//!   user confirms the update there (it must be signed with the same key).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
#[cfg(desktop)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::{db, AppState};

/// GitHub repository the releases come from.
pub const REPO: &str = "tomasbaier/Moviary";
/// Automatic checks happen at most this often.
const CHECK_INTERVAL: u64 = 24 * 3600;
/// Larger downloads are refused.
const MAX_SIZE: u64 = 300 * 1024 * 1024;

/// A newer release, as shown to the user.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    /// Release notes (Markdown, shown as plain text).
    pub notes: String,
    /// Size of the download in bytes (0 when there is no file for this platform).
    pub size: u64,
    /// The app can download and install the update itself.
    pub can_install: bool,
}

#[derive(Clone)]
struct Asset {
    url: String,
    size: u64,
    /// Lowercase hex; GitHub reports it for files uploaded since mid-2025.
    sha256: Option<String>,
}

#[derive(Default)]
pub struct Updater {
    /// The release found by the last check; installing uses only this, never a URL from
    /// the frontend.
    found: Mutex<Option<(UpdateInfo, Option<Asset>)>>,
    busy: AtomicBool,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: u64,
}

/// Where releases are looked up. The test build (identifier `*.e2etest`) can use a local
/// server instead of GitHub via the `MOVIARY_UPDATE_API` environment variable.
struct Source {
    api: String,
    /// Download URLs must start with this.
    downloads: String,
}

fn source(app: &AppHandle) -> Source {
    if app.config().identifier.ends_with(".e2etest") {
        if let Ok(base) = std::env::var("MOVIARY_UPDATE_API") {
            return Source { api: format!("{base}/repos/{REPO}/releases/latest"), downloads: format!("{base}/") };
        }
    }
    Source {
        api: format!("https://api.github.com/repos/{REPO}/releases/latest"),
        downloads: format!("https://github.com/{REPO}/releases/download/"),
    }
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(timeout))
        .user_agent(concat!("Moviary/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// "v1.12.0" -> (1, 12, 0); pre-release suffixes ("-beta") are ignored.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

fn asset_name(version: &str) -> String {
    let platform = if cfg!(target_os = "android") { "android.apk" } else { "windows.exe" };
    format!("Moviary-{version}-{platform}")
}

/// Looks for a newer release. Automatic checks (`manual` false) respect the setting, the
/// daily interval and a skipped version; a manual check always asks GitHub.
pub fn check(app: &AppHandle, manual: bool) -> Result<Option<UpdateInfo>, String> {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock().unwrap();
        if !manual
            && (!db.settings.check_updates || db::now().saturating_sub(db.settings.update_checked) < CHECK_INTERVAL)
        {
            return Ok(None);
        }
    }
    let src = source(app);
    let release: Option<Value> = match agent(20).get(&src.api).set("Accept", "application/vnd.github+json").call() {
        Ok(response) => Some(response.into_json().map_err(|e| e.to_string())?),
        // No release published yet.
        Err(ureq::Error::Status(404, _)) => None,
        Err(e) => {
            return Err(tr!("Nepodařilo se zjistit, jestli je nová verze: {e}", "Could not check for a new version: {e}"))
        }
    };
    let skipped = {
        let mut db = state.db.lock().unwrap();
        db.settings.update_checked = db::now();
        db.save(&state.db_path)?;
        db.settings.skipped_version.clone()
    };
    let current = app.package_info().version.to_string();
    let found = release.and_then(|release| {
        let tag = release["tag_name"].as_str().unwrap_or_default();
        let newer = matches!((parse_version(tag), parse_version(&current)), (Some(new), Some(old)) if new > old);
        if !newer || release["draft"].as_bool() == Some(true) || release["prerelease"].as_bool() == Some(true) {
            return None;
        }
        let version = tag.trim_start_matches(['v', 'V']).to_string();
        let name = asset_name(&version);
        let asset = release["assets"]
            .as_array()
            .and_then(|assets| assets.iter().find(|a| a["name"].as_str() == Some(name.as_str())))
            .and_then(|a| {
                let url = a["browser_download_url"].as_str().filter(|u| u.starts_with(&src.downloads))?;
                Some(Asset {
                    url: url.to_string(),
                    size: a["size"].as_u64().filter(|s| *s > 0 && *s <= MAX_SIZE)?,
                    sha256: a["digest"]
                        .as_str()
                        .and_then(|d| d.strip_prefix("sha256:"))
                        .map(|d| d.to_ascii_lowercase()),
                })
            });
        let info = UpdateInfo {
            version,
            current: current.clone(),
            notes: release["body"].as_str().unwrap_or_default().chars().take(20_000).collect(),
            size: asset.as_ref().map_or(0, |a| a.size),
            can_install: asset.is_some() && installable(),
        };
        Some((info, asset))
    });
    let info = found.as_ref().map(|(info, _)| info.clone());
    *state.update.found.lock().unwrap() = found;
    Ok(info.filter(|info| manual || info.version != skipped))
}

/// An automatic check no longer offers `version`.
pub fn skip(app: &AppHandle, version: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut db = state.db.lock().unwrap();
    db.settings.skipped_version = version;
    db.save(&state.db_path)
}

/// Downloads and installs the release found by the last check. On Windows the app restarts
/// into the new version (the call does not return then); on Android the system installer opens.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.update.busy.swap(true, Ordering::SeqCst) {
        return Err(tr!("Aktualizace už probíhá.", "The update is already running."));
    }
    let result = install_found(app);
    state.update.busy.store(false, Ordering::SeqCst);
    result
}

fn install_found(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let found = state.update.found.lock().unwrap().clone();
    let Some((info, Some(asset))) = found else {
        return Err(tr!("Nejdřív zkontrolujte aktualizace.", "Check for updates first."));
    };
    #[cfg(desktop)]
    {
        let _ = info;
        if !state.playing.lock().unwrap().is_empty() {
            return Err(tr!(
                "Nejdřív zavřete přehrávání ve VLC, ať se uloží pozice.",
                "Close the playback in VLC first so the position is saved."
            ));
        }
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        let download = current.with_file_name(DOWNLOAD_NAME);
        fetch(app, &asset, &download)?;
        swap_and_restart(app, &current, &download)
    }
    #[cfg(mobile)]
    {
        let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let apk = dir.join(asset_name(&info.version));
        fetch(app, &asset, &apk)?;
        crate::mobile::install_apk(app, &apk)
    }
}

/// Downloads `asset` into `target`, reporting `update-progress` events, and checks it.
fn fetch(app: &AppHandle, asset: &Asset, target: &Path) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| tr!("Stažení se nepodařilo: {e}", "The download failed: {e}");
    let response = agent(60).get(&asset.url).call().map_err(|e| failed(&e))?;
    let mut reader = response.into_reader().take(MAX_SIZE + 1);
    let mut file = File::create(target).map_err(|e| failed(&e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut reported = 0u64;
    let result = (|| {
        loop {
            let n = reader.read(&mut buffer).map_err(|e| failed(&e))?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n]).map_err(|e| failed(&e))?;
            hasher.update(&buffer[..n]);
            done += n as u64;
            if done - reported >= asset.size / 100 || done == asset.size {
                reported = done;
                let _ = app.emit("update-progress", Progress { done, total: asset.size });
            }
        }
        file.sync_all().map_err(|e| failed(&e))?;
        if done != asset.size {
            return Err(tr!(
                "Stažený soubor má jinou velikost, než má mít.",
                "The downloaded file does not have the expected size."
            ));
        }
        if let Some(expected) = &asset.sha256 {
            if format!("{:x}", hasher.finalize()) != *expected {
                return Err(tr!(
                    "Stažený soubor neodpovídá kontrolnímu součtu z GitHubu.",
                    "The downloaded file does not match the checksum from GitHub."
                ));
            }
        }
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}

/// Name of the download next to the exe while updating.
#[cfg(desktop)]
const DOWNLOAD_NAME: &str = ".moviary-update.download";

#[cfg(desktop)]
fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// The folder of the exe can be written to (not e.g. Program Files without admin rights).
fn installable() -> bool {
    #[cfg(desktop)]
    {
        let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) else {
            return false;
        };
        let probe = dir.join(".moviary-write-test");
        let ok = File::create(&probe).is_ok();
        let _ = fs::remove_file(&probe);
        ok
    }
    #[cfg(mobile)]
    {
        true
    }
}

#[cfg(desktop)]
fn swap_and_restart(app: &AppHandle, current: &Path, download: &Path) -> Result<(), String> {
    let old = old_path(current);
    let _ = fs::remove_file(&old);
    let cannot = |e: std::io::Error| tr!("Nelze nahradit program: {e}", "Cannot replace the program: {e}");
    fs::rename(current, &old).map_err(cannot)?;
    if let Err(e) = fs::rename(download, current) {
        let _ = fs::rename(&old, current);
        let _ = fs::remove_file(download);
        return Err(cannot(e));
    }
    if let Err(e) = std::process::Command::new(current).spawn() {
        // Back to the version that is running, so the next start works the same.
        let _ = fs::rename(current, download);
        let _ = fs::rename(&old, current);
        let _ = fs::remove_file(download);
        return Err(tr!("Novou verzi nelze spustit: {e}", "Cannot start the new version: {e}"));
    }
    app.exit(0);
    Ok(())
}

/// Removes what an update left behind: the previous exe (once its process has ended) and an
/// unfinished download; on Android the downloaded APK.
pub fn cleanup(app: &AppHandle) {
    #[cfg(desktop)]
    {
        let _ = app;
        let Ok(current) = std::env::current_exe() else {
            return;
        };
        let _ = fs::remove_file(current.with_file_name(DOWNLOAD_NAME));
        let old = old_path(&current);
        if old.exists() {
            std::thread::spawn(move || {
                // The previous process may still be closing.
                for _ in 0..40 {
                    if fs::remove_file(&old).is_ok() || !old.exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            });
        }
    }
    #[cfg(mobile)]
    if let Ok(dir) = app.path().app_cache_dir() {
        let _ = fs::remove_dir_all(dir.join("updates"));
    }
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.5.0"), Some((1, 5, 0)));
        assert_eq!(parse_version("1.10"), Some((1, 10, 0)));
        assert_eq!(parse_version("v2.0.1-beta.1"), Some((2, 0, 1)));
        assert_eq!(parse_version("latest"), None);
        assert!(parse_version("v1.10.0") > parse_version("v1.9.9"));
    }
}
