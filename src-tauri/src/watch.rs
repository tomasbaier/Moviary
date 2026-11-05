//! Watching the library folders. A new, removed or renamed video (or subtitle file) triggers
//! a scan in the background once the folder has been quiet for a moment, so copying a large
//! file causes one scan at the end, not hundreds. A periodic check notices a drive with a
//! library folder being connected or removed. The frontend is told by `library-changed`.
//!
//! Folders reached through the Android document provider (`content://`) cannot be watched;
//! they are covered by the periodic check only partially (when the folder goes missing).

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::{db, media, scanner, AppState};

/// Quiet time after the last change before scanning.
const DEBOUNCE: Duration = Duration::from_secs(4);
/// How often the reachability of the library folders is checked.
const CHECK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Watch {
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// Folders the watcher currently watches.
    folders: Mutex<Vec<String>>,
    trigger: Mutex<Option<Sender<()>>>,
}

/// Starts the background thread that scans after changes.
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    *app.state::<AppState>().watch.trigger.lock().unwrap() = Some(tx);
    let app = app.clone();
    thread::spawn(move || loop {
        match rx.recv_timeout(CHECK_INTERVAL) {
            Ok(()) => {
                // Wait until the changes stop (a file being copied changes for a while).
                while rx.recv_timeout(DEBOUNCE).is_ok() {}
                rescan(&app);
            }
            Err(RecvTimeoutError::Timeout) => {
                if reachability_changed(&app) {
                    rescan(&app);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    });
}

fn enabled(state: &AppState) -> bool {
    state.db.lock().unwrap().settings.watch_folders
}

/// A library folder became reachable or unreachable since the last scan.
fn reachability_changed(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    if !enabled(&state) {
        return false;
    }
    let libraries = state.db.lock().unwrap().libraries.clone();
    let scan = state.scan.lock().unwrap();
    if !scan.scanned {
        return false;
    }
    libraries.iter().any(|l| {
        let reachable = if db::is_uri(&l.path) {
            // Only a folder that went missing can be noticed without asking Android.
            scan.online.contains(&l.id)
        } else {
            Path::new(&l.path).is_dir()
        };
        reachable != scan.online.contains(&l.id)
    })
}

/// Scans again and reports how many videos appeared and disappeared.
fn rescan(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !enabled(&state) || !state.scan.lock().unwrap().scanned {
        return;
    }
    let before: HashSet<u64> = state.scan.lock().unwrap().found.keys().copied().collect();
    if crate::run_scan(app).is_err() {
        return;
    }
    let after: HashSet<u64> = state.scan.lock().unwrap().found.keys().copied().collect();
    let added = after.difference(&before).count();
    let removed = before.difference(&after).count();
    if added > 0 || removed > 0 {
        let _ = app.emit("library-changed", json!({ "added": added, "removed": removed }));
    }
}

/// A change worth a scan: a video or subtitle file, or a folder (no extension) appearing,
/// disappearing or being renamed. Writes into a video count too, so a copy in progress is
/// scanned again when it finishes.
fn relevant(event: &notify::Event) -> bool {
    let interesting = |path: &Path| {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        scanner::is_video(&name, u64::MAX) || media::is_subtitle(path) || path.extension().is_none()
    };
    matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_))
        && event.paths.iter().any(|p| interesting(p))
}

/// Watches the reachable library folders (called after every scan and when the setting
/// changes). Folders already watched are kept as they are.
pub fn update(app: &AppHandle) {
    let state = app.state::<AppState>();
    let wanted: Vec<String> = if enabled(&state) {
        let online = state.scan.lock().unwrap().online.clone();
        let db = state.db.lock().unwrap();
        db.libraries
            .iter()
            .filter(|l| online.contains(&l.id) && !db::is_uri(&l.path))
            .map(|l| l.path.clone())
            .collect()
    } else {
        Vec::new()
    };
    let mut folders = state.watch.folders.lock().unwrap();
    if *folders == wanted {
        return;
    }
    let mut watcher = state.watch.watcher.lock().unwrap();
    *watcher = None;
    folders.clear();
    if wanted.is_empty() {
        return;
    }
    let Some(trigger) = state.watch.trigger.lock().unwrap().clone() else {
        return;
    };
    let created = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| relevant(&e)) {
            let _ = trigger.send(());
        }
    });
    let Ok(mut new_watcher) = created else {
        return;
    };
    for folder in &wanted {
        // A folder that cannot be watched (some network drives) is still scanned by hand.
        if new_watcher.watch(Path::new(folder), RecursiveMode::Recursive).is_ok() {
            folders.push(folder.clone());
        }
    }
    *watcher = Some(new_watcher);
}
