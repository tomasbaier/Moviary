//! Intro, recap and credits times of episodes from TheIntroDB (theintrodb.org), a community
//! database looked up by the TMDB id of the series, what a playback skips by them, and the
//! background job that downloads them for the episodes of the library.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{self, Settings};
use crate::library::{self, EpisodeLookup};
use crate::AppState;

const API: &str = "https://api.theintrodb.org/v4/media";
/// Found times are asked again after this long, as the community corrects them.
const FOUND_MAX_AGE: u64 = 60 * 24 * 3600;
/// An episode without times is asked again sooner, as new submissions keep coming.
const MISSING_MAX_AGE: u64 = 7 * 24 * 3600;
/// Pause between the requests of a batch: the API allows 30 requests in 10 seconds
/// without a key.
pub const PACE: Duration = Duration::from_millis(400);
/// Waiting for the times when an episode starts; the episode then plays without them.
pub const PLAY_TIMEOUT: Duration = Duration::from_secs(4);
/// Waiting for one answer in a batch.
const BATCH_TIMEOUT: Duration = Duration::from_secs(15);
/// The background job saves the data after this many episodes.
const SAVE_EVERY: usize = 25;
/// Shorter parts are not worth a jump.
const MIN_SKIP: f64 = 3.0;

/// Times of one episode file, in seconds.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Segments {
    pub intro: Vec<Span>,
    pub recap: Vec<Span>,
    pub credits: Vec<Span>,
    /// "Next time on…" after the credits.
    pub preview: Vec<Span>,
    /// Unix timestamp of the download; the times may be empty (none submitted yet).
    pub fetched: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Debug)]
#[serde(default)]
pub struct Span {
    pub start: f64,
    /// None = to the end of the file.
    pub end: Option<f64>,
}

impl Segments {
    pub fn found(&self) -> bool {
        !(self.intro.is_empty() && self.recap.is_empty() && self.credits.is_empty() && self.preview.is_empty())
    }

    pub fn stale(&self) -> bool {
        let max_age = if self.found() { FOUND_MAX_AGE } else { MISSING_MAX_AGE };
        db::now().saturating_sub(self.fetched) > max_age
    }
}

/// What the player does with the times during one playback.
#[derive(Clone, Default, Debug)]
pub struct Plan {
    /// Parts at the start to jump over (intro, recap), in order.
    pub skip: Vec<(f64, f64)>,
    /// Where the credits start, if they are to be skipped.
    pub credits: Option<f64>,
    /// Where the playback continues after the credits; None = the episode ends there.
    pub after_credits: Option<f64>,
    /// Start the next episode when this one ends.
    pub autoplay: bool,
}

impl Plan {
    /// What to skip in a file of `length` seconds (0 = unknown) by the user's settings.
    pub fn new(settings: &Settings, segments: Option<&Segments>, length: f64) -> Plan {
        let mut plan = Plan { autoplay: settings.autoplay_next, ..Plan::default() };
        let Some(segments) = segments else {
            return plan;
        };
        let end_of = |span: &Span| span.end.unwrap_or(f64::INFINITY);
        if settings.skip_intro {
            plan.skip = segments
                .recap
                .iter()
                .chain(&segments.intro)
                .filter_map(|s| s.end.map(|end| (s.start, end)))
                .filter(|(start, end)| end - start >= MIN_SKIP)
                .collect();
            plan.skip.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        if settings.skip_credits {
            // The end credits: the last credits in the second half (a movie-like episode can
            // have opening credits too).
            let credits = segments
                .credits
                .iter()
                .filter(|s| length <= 0.0 || s.start >= length * 0.5)
                .max_by(|a, b| a.start.total_cmp(&b.start));
            if let Some(credits) = credits {
                // A preview right after the credits goes too.
                let mut end = end_of(credits);
                if let Some(preview) = segments.preview.iter().find(|p| (p.start - end).abs() <= 3.0) {
                    end = end_of(preview);
                }
                plan.credits = Some(credits.start);
                // Only a real scene after the credits is worth staying for.
                plan.after_credits = (end.is_finite() && (length <= 0.0 || end < length - 10.0)).then_some(end);
            }
        }
        plan
    }

    /// Where a playback from the beginning starts: after the parts that open the file.
    pub fn opening_end(&self) -> f64 {
        let mut position = 0.0;
        for (start, end) in &self.skip {
            if *start > position + 2.0 {
                break;
            }
            position = end.max(position);
        }
        position
    }
}

/// Downloads the times of an episode of the series `tmdb_id`. `length` (seconds, 0 =
/// unknown) picks the release whose length matches the file.
pub fn fetch(tmdb_id: u64, season: u32, episode: u32, length: f64, timeout: Duration) -> Result<Segments, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .user_agent(concat!("Moviary/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut request = agent
        .get(API)
        .set("Accept", "application/json")
        .query("tmdb_id", &tmdb_id.to_string())
        .query("season", &season.to_string())
        .query("episode", &episode.to_string());
    if length > 0.0 {
        request = request.query("duration_ms", &((length * 1000.0) as u64).to_string());
    }
    let json: Value = match request.call() {
        Ok(response) => response
            .into_json()
            .map_err(|e| tr!("Neplatná odpověď TheIntroDB: {e}", "Invalid response from TheIntroDB: {e}"))?,
        // No times submitted yet, or TheIntroDB does not know the episode.
        Err(ureq::Error::Status(404, _)) => Value::Null,
        Err(ureq::Error::Status(429, _)) => {
            return Err(tr!(
                "TheIntroDB odmítá další požadavky, zkuste to později.",
                "TheIntroDB refuses more requests, try again later."
            ))
        }
        Err(ureq::Error::Status(code, _)) => {
            return Err(tr!("TheIntroDB vrátilo chybu HTTP {code}.", "TheIntroDB returned HTTP error {code}."))
        }
        Err(e) => return Err(tr!("Nelze se připojit k TheIntroDB: {e}", "Cannot connect to TheIntroDB: {e}")),
    };
    Ok(parse(&json))
}

/// Downloads run one at a time (the API allows few requests per second): the background job
/// for the whole library, or the download of one series asked for on its page.
#[derive(Default)]
pub struct Job {
    running: AtomicBool,
    cancel: AtomicBool,
}

/// Holds [`Job::running`] while a download runs.
struct Running<'a>(&'a Job);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

impl Job {
    fn try_run(&self) -> Option<Running<'_>> {
        (!self.running.swap(true, Ordering::SeqCst)).then(|| {
            self.cancel.store(false, Ordering::SeqCst);
            Running(self)
        })
    }

    /// Stops the background job (if it runs) and takes its place.
    fn take_over(&self) -> Option<Running<'_>> {
        let deadline = Instant::now() + BATCH_TIMEOUT + Duration::from_secs(5);
        loop {
            self.cancel.store(true, Ordering::SeqCst);
            if let Some(running) = self.try_run() {
                return Some(running);
            }
            if Instant::now() > deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Payload of the `segments-updated` event: the background job stored new times.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Updated {
    done: usize,
    found: usize,
}

/// Starts the background download of the times of episodes that have none yet or old ones,
/// when skipping is on and automatic download too. Runs after a scan and after the TMDB job
/// (which may have matched a series). Unwatched episodes go first.
pub fn start_auto(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut lookups: Vec<(bool, u64, EpisodeLookup)> = {
        let db = state.db.lock().unwrap();
        if !(db.settings.auto_segments && db.settings.uses_segments()) {
            return;
        }
        db.records
            .iter()
            .filter(|(_, r)| r.segments.as_ref().is_none_or(Segments::stale))
            .filter_map(|(id, r)| library::episode_lookup(&db, *id).map(|l| (r.watched, *id, l)))
            .collect()
    };
    if lookups.is_empty() {
        return;
    }
    lookups.sort_by_key(|(watched, id, l)| (*watched, l.tmdb_id, l.season, l.episode, *id));
    let app = app.clone();
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let Some(_running) = state.segments.try_run() else {
            return;
        };
        let (mut done, mut found) = (0, 0);
        for (_, id, lookup) in &lookups {
            if state.segments.cancel.load(Ordering::SeqCst) {
                break;
            }
            if done > 0 {
                thread::sleep(PACE);
            }
            // A failure (no connection, the daily limit) ends the job; the next scan retries.
            let Ok(times) = fetch(lookup.tmdb_id, lookup.season, lookup.episode, lookup.length, BATCH_TIMEOUT) else {
                break;
            };
            found += usize::from(times.found());
            done += 1;
            let mut db = state.db.lock().unwrap();
            if let Some(record) = db.records.get_mut(id) {
                record.segments = Some(times);
            }
            if done % SAVE_EVERY == 0 {
                let _ = db.save(&state.db_path);
            }
        }
        if done > 0 {
            let _ = state.db.lock().unwrap().save(&state.db_path);
            let _ = app.emit("segments-updated", Updated { done, found });
        }
    });
}

/// Result of downloading the times of a series' episodes.
pub struct ShowResult {
    /// Episodes asked about and those TheIntroDB has times for.
    pub total: usize,
    pub found: usize,
    /// Why the download stopped early.
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    show_id: u64,
    done: usize,
    total: usize,
}

/// Downloads the times of all episodes of a series (asked for on its page), in place of the
/// background job.
pub fn fetch_show(app: &AppHandle, show_id: u64) -> Result<ShowResult, String> {
    let state = app.state::<AppState>();
    let lookups: Vec<(u64, EpisodeLookup)> = {
        let db = state.db.lock().unwrap();
        if db.shows.get(&show_id).and_then(|s| s.meta.as_ref()).is_none() {
            return Err(tr!(
                "Časy se hledají podle TMDB – seriál nejdřív přiřaďte v TMDB.",
                "The times are looked up by TMDB – match the series on TMDB first."
            ));
        }
        library::show_episodes(&db, show_id)
            .into_iter()
            .filter_map(|ep| library::episode_lookup(&db, ep).map(|l| (ep, l)))
            .collect()
    };
    let _running = state.segments.take_over().ok_or_else(|| {
        tr!("Časy se právě stahují, zkuste to za chvíli.", "The times are being downloaded, try again in a moment.")
    })?;
    let total = lookups.len();
    let (mut found, mut error) = (0, None);
    for (i, (ep, lookup)) in lookups.iter().enumerate() {
        if i > 0 {
            thread::sleep(PACE);
        }
        match fetch(lookup.tmdb_id, lookup.season, lookup.episode, lookup.length, BATCH_TIMEOUT) {
            Ok(times) => {
                found += usize::from(times.found());
                if let Some(record) = state.db.lock().unwrap().records.get_mut(ep) {
                    record.segments = Some(times);
                }
            }
            Err(e) => {
                error = Some(e);
                break;
            }
        }
        let _ = app.emit("segments-progress", Progress { show_id, done: i + 1, total });
    }
    state.db.lock().unwrap().save(&state.db_path)?;
    match error {
        Some(error) if found == 0 => Err(error),
        error => Ok(ShowResult { total, found, error }),
    }
}

fn parse(json: &Value) -> Segments {
    // Intro and recap: start null = from the beginning; credits and preview: end null = to
    // the end of the file. 0/0 marks a part confirmed absent.
    let spans = |key: &str, open_start: bool| -> Vec<Span> {
        let mut spans: Vec<Span> = json[key]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| {
                let ms = |v: &Value| v.as_f64().map(|ms| ms / 1000.0);
                let start = match ms(&s["start_ms"]) {
                    Some(start) => start,
                    None if open_start => 0.0,
                    None => return None,
                };
                let end = match ms(&s["end_ms"]) {
                    Some(end) if end > start => Some(end),
                    None if !open_start => None,
                    _ => return None,
                };
                Some(Span { start, end })
            })
            .collect();
        spans.sort_by(|a, b| a.start.total_cmp(&b.start));
        spans
    };
    Segments {
        intro: spans("intro", true),
        recap: spans("recap", true),
        credits: spans("credits", false),
        preview: spans("preview", false),
        fetched: db::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(intro: bool, credits: bool) -> Settings {
        Settings { skip_intro: intro, skip_credits: credits, autoplay_next: true, ..Settings::default() }
    }

    #[test]
    fn parses_open_ends_and_drops_absent_parts() {
        let s = parse(&json!({
            "intro": [{ "start_ms": null, "end_ms": 23000 }, { "start_ms": 0, "end_ms": 0 }],
            "recap": [{ "start_ms": 25000, "end_ms": 134000 }],
            "credits": [{ "start_ms": 2500000, "end_ms": null }],
        }));
        assert_eq!(s.intro, vec![Span { start: 0.0, end: Some(23.0) }]);
        assert_eq!(s.recap, vec![Span { start: 25.0, end: Some(134.0) }]);
        assert_eq!(s.credits, vec![Span { start: 2500.0, end: None }]);
        assert!(s.preview.is_empty());
        assert!(s.found());
        assert!(!parse(&Value::Null).found());
    }

    #[test]
    fn plan_skips_intro_and_ends_at_credits() {
        let segments = Segments {
            intro: vec![Span { start: 60.0, end: Some(120.0) }],
            recap: vec![Span { start: 0.0, end: Some(40.0) }],
            credits: vec![Span { start: 10.0, end: Some(30.0) }, Span { start: 2500.0, end: None }],
            ..Segments::default()
        };
        let plan = Plan::new(&settings(true, true), Some(&segments), 2600.0);
        assert_eq!(plan.skip, vec![(0.0, 40.0), (60.0, 120.0)]);
        assert_eq!(plan.opening_end(), 40.0);
        assert_eq!(plan.credits, Some(2500.0));
        assert_eq!(plan.after_credits, None);

        let plan = Plan::new(&settings(false, false), Some(&segments), 2600.0);
        assert!(plan.skip.is_empty() && plan.credits.is_none() && plan.autoplay);
    }

    #[test]
    fn plan_keeps_a_scene_after_the_credits() {
        let segments = Segments {
            credits: vec![Span { start: 2400.0, end: Some(2500.0) }],
            preview: vec![Span { start: 2501.0, end: Some(2530.0) }],
            ..Segments::default()
        };
        let plan = Plan::new(&settings(false, true), Some(&segments), 2600.0);
        assert_eq!((plan.credits, plan.after_credits), (Some(2400.0), Some(2530.0)));

        // Credits and preview to the end of the file: nothing to stay for.
        let plan = Plan::new(&settings(false, true), Some(&segments), 2535.0);
        assert_eq!(plan.after_credits, None);
    }
}
