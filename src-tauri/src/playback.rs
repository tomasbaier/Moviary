//! Storing the playback position reported by the player (VLC on Windows, VLC for Android).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{now, PlayEvent, HISTORY_LIMIT};
use crate::AppState;

/// Shorter playbacks are not worth a history entry (a look into the file).
const HISTORY_MIN_SECS: f64 = 60.0;

/// Payload of the `movie-progress` event sent to the frontend.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    id: u64,
    position: f64,
    duration: f64,
    watched: bool,
    last_played: Option<u64>,
    playing: bool,
}

/// Watched when at least 95 % or all but the last 30 seconds were played.
fn is_finished(time: f64, length: f64) -> bool {
    length > 0.0 && (time >= length * 0.95 || length - time <= 30.0)
}

/// Stores the position `time` (seconds) of a movie. `done` = the player was closed;
/// `ended` = the player reported that the video played to the end.
pub fn record(app: &AppHandle, id: u64, time: Option<f64>, length: f64, done: bool, ended: bool) {
    let state = app.state::<AppState>();
    let progress = {
        let mut db = state.db.lock().unwrap();
        let Some(data) = db.records.get_mut(&id) else {
            state.playing.lock().unwrap().remove(&id);
            return;
        };
        if length > 0.0 {
            data.duration = length;
        }
        let finished = done && (ended || time.is_some_and(|t| is_finished(t, data.duration)));
        if finished {
            data.watched = true;
            data.position = 0.0;
        } else if let Some(time) = time {
            data.position = time;
        }
        data.last_played = Some(now());
        let data = data.clone();
        if done {
            // Played from where it started to where it was closed (the end when finished).
            let start = state.sessions.lock().unwrap().remove(&id).unwrap_or(0.0);
            let end = if finished { data.duration.max(time.unwrap_or(0.0)) } else { time.unwrap_or(start) };
            let seconds = end - start;
            if seconds >= HISTORY_MIN_SECS {
                db.history.push(PlayEvent { id, at: now(), seconds, finished });
                let excess = db.history.len().saturating_sub(HISTORY_LIMIT);
                db.history.drain(..excess);
            }
        }
        let _ = db.save(&state.db_path);
        if done {
            state.playing.lock().unwrap().remove(&id);
        }
        Progress {
            id,
            position: data.position,
            duration: data.duration,
            watched: data.watched,
            last_played: data.last_played,
            playing: !done,
        }
    };
    let _ = app.emit("movie-progress", progress);
}

/// Playback ended without a position (the player failed to start); `error` is shown.
#[cfg_attr(desktop, allow(dead_code))]
pub fn failed(app: &AppHandle, id: u64, error: String) {
    let state = app.state::<AppState>();
    state.playing.lock().unwrap().remove(&id);
    state.sessions.lock().unwrap().remove(&id);
    if let Ok(movie) = crate::view::movie_by_id(&state, id) {
        let _ = app.emit("movie-updated", &movie);
    }
    let _ = app.emit("play-error", error);
}
