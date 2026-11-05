//! Intro, recap and credits times of episodes from TheIntroDB (theintrodb.org), a community
//! database looked up by the TMDB id of the series, and what a playback skips by them.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{self, Settings};

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
