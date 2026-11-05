//! Minimal client for The Movie Database (TMDB) API v3.
//! Accepts both the v3 "API key" and the v4 "API Read Access Token".

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::db::{self, NextEpisode, Collection, CollectionPart, CollectionRef, EpisodeMeta, Meta, Person, SeasonMeta, META_VERSION};

const API: &str = "https://api.themoviedb.org/3";
const IMAGES: &str = "https://image.tmdb.org/t/p";
const LANGUAGE: &str = "cs-CZ";
const ENGLISH: &str = "en-US";
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// Extra parts of a details response: people and related titles in one request.
const EXTRAS: &str = "credits,recommendations,similar";
/// Actors kept per movie or series.
const CAST_LIMIT: usize = 15;
/// Recommended and similar titles kept.
const SIMILAR_LIMIT: usize = 40;

pub enum Error {
    /// Invalid key – no point in continuing.
    Auth(String),
    Other(String),
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        match e {
            Error::Auth(msg) | Error::Other(msg) => msg,
        }
    }
}

/// A TV series with the seasons and the episodes asked for.
pub struct ShowInfo {
    pub meta: Meta,
    pub seasons: Vec<SeasonMeta>,
    pub episodes: Vec<EpisodeMeta>,
    /// "Returning Series", "Ended"…
    pub status: String,
    pub next_episode: Option<NextEpisode>,
}

/// Search result offered to the user when picking a match by hand.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: u64,
    pub title: String,
    pub original_title: String,
    pub year: Option<u32>,
    pub overview: String,
    pub poster_url: Option<String>,
}

pub struct Client {
    key: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(key: &str) -> Client {
        Client {
            key: key.trim().to_string(),
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(20))
                .user_agent(concat!("Moviary/", env!("CARGO_PKG_VERSION")))
                .build(),
        }
    }

    fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value, Error> {
        let mut request = self.agent.get(&format!("{API}{path}"));
        request = if self.key.starts_with("eyJ") {
            request.set("Authorization", &format!("Bearer {}", self.key))
        } else {
            request.query("api_key", &self.key)
        };
        for (name, value) in params {
            request = request.query(name, value);
        }
        match request.call() {
            Ok(response) => response
                .into_json()
                .map_err(|e| Error::Other(tr!("Neplatná odpověď TMDB: {e}", "Invalid response from TMDB: {e}"))),
            Err(ureq::Error::Status(code, response)) => {
                let message = response
                    .into_json::<Value>()
                    .ok()
                    .and_then(|v| v["status_message"].as_str().map(String::from))
                    .unwrap_or_else(|| format!("HTTP {code}"));
                if code == 401 {
                    Err(Error::Auth(tr!("Neplatný TMDB klíč ({message})", "Invalid TMDB key ({message})")))
                } else {
                    Err(Error::Other(tr!("TMDB vrátilo chybu: {message}", "TMDB returned an error: {message}")))
                }
            }
            Err(e) => Err(Error::Other(tr!("Nelze se připojit k TMDB: {e}", "Cannot connect to TMDB: {e}"))),
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.get("/configuration", &[]).map(|_| ())
    }

    /// Searches movies, or TV series when `tv` is set; titles in the app's language.
    pub fn search(&self, query: &str, year: Option<u32>, tv: bool) -> Result<Vec<Candidate>, Error> {
        let year = year.map(|y| y.to_string());
        let mut params = vec![
            ("query", query),
            ("language", if crate::i18n::english() { ENGLISH } else { LANGUAGE }),
            ("include_adult", "false"),
        ];
        if let Some(year) = &year {
            params.push((if tv { "first_air_date_year" } else { "year" }, year));
        }
        let response = self.get(if tv { "/search/tv" } else { "/search/movie" }, &params)?;
        Ok(response["results"]
            .as_array()
            .map(|results| results.iter().map(candidate).collect())
            .unwrap_or_default())
    }

    /// TMDB id of the most likely match for a title parsed from a file name.
    pub fn find(&self, title: &str, year: Option<u32>, tv: bool) -> Result<Option<u64>, Error> {
        let mut candidates = self.search(title, year, tv)?;
        if candidates.is_empty() && year.is_some() {
            candidates = self.search(title, None, tv)?;
        }
        Ok(best_match(&candidates, title, year).map(|c| c.id))
    }

    /// Full details in Czech and English; the poster is downloaded into `posters_dir`.
    pub fn details(&self, id: u64, posters_dir: &Path) -> Result<Meta, Error> {
        let path = format!("/movie/{id}");
        let v = self.get(&path, &[("language", LANGUAGE), ("append_to_response", EXTRAS)])?;
        let en = English::fetch(self, &path, "title");
        let directors: Vec<Person> = v["credits"]["crew"]
            .as_array()
            .map(|crew| crew.iter().filter(|c| c["job"] == "Director").map(person).collect())
            .unwrap_or_default();
        let director = Some(names(&directors)).filter(|n| !n.is_empty());
        let collection = v["belongs_to_collection"]["id"].as_u64().map(|id| CollectionRef {
            id,
            name: text(&v["belongs_to_collection"]["name"]),
            name_en: en.collection.clone(),
        });
        let poster = v["poster_path"]
            .as_str()
            .and_then(|p| self.download_poster(format!("tmdb-{id}.jpg"), p, posters_dir).ok());

        Ok(Meta {
            tmdb_id: id,
            title: text(&v["title"]),
            original_title: text(&v["original_title"]),
            year: year_of(&text(&v["release_date"])),
            // No Czech translation – English instead.
            overview: or_english(text(&v["overview"]), &en.overview),
            genres: genres(&v),
            runtime: v["runtime"].as_u64().unwrap_or(0) as u32,
            rating: v["vote_average"].as_f64().unwrap_or(0.0),
            director,
            poster,
            bilingual: en.fetched,
            title_en: en.title,
            overview_en: en.overview,
            genres_en: en.genres,
            cast: cast(&v),
            directors,
            collection,
            similar: similar(&v),
            version: META_VERSION,
            fetched: db::now(),
        })
    }

    /// Parts of a movie series in release order, with Czech and English titles.
    pub fn collection(&self, id: u64) -> Result<Collection, Error> {
        let path = format!("/collection/{id}");
        let v = self.get(&path, &[("language", LANGUAGE)])?;
        let en = self.get(&path, &[("language", ENGLISH)]).ok();
        let en_title = |part: u64| {
            en.as_ref()
                .and_then(|en| en["parts"].as_array())
                .and_then(|parts| parts.iter().find(|p| p["id"].as_u64() == Some(part)))
                .map(|p| text(&p["title"]))
                .unwrap_or_default()
        };
        let mut parts: Vec<CollectionPart> = v["parts"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| {
                        let id = p["id"].as_u64()?;
                        Some(CollectionPart {
                            tmdb_id: id,
                            title: text(&p["title"]),
                            title_en: en_title(id),
                            year: year_of(&text(&p["release_date"])),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Unreleased parts (no date) last.
        parts.sort_by_key(|p| p.year.unwrap_or(u32::MAX));
        Ok(Collection {
            name: text(&v["name"]),
            name_en: en.as_ref().map(|en| text(&en["name"])).unwrap_or_default(),
            parts,
            fetched: db::now(),
        })
    }

    /// Details of a TV series with the episodes of the given seasons; the poster is
    /// downloaded into `posters_dir`. The creators are stored as `director`.
    pub fn show_details(&self, id: u64, seasons: &[u32], posters_dir: &Path) -> Result<ShowInfo, Error> {
        let path = format!("/tv/{id}");
        let v = self.get(&path, &[("language", LANGUAGE), ("append_to_response", EXTRAS)])?;
        let en = English::fetch(self, &path, "name");
        let creator_list: Vec<Person> = v["created_by"]
            .as_array()
            .map(|people| people.iter().map(person).collect())
            .unwrap_or_default();
        let creators = Some(names(&creator_list)).filter(|n| !n.is_empty());
        let poster = v["poster_path"]
            .as_str()
            .and_then(|p| self.download_poster(format!("tmdb-tv-{id}.jpg"), p, posters_dir).ok());
        let season_list: Vec<SeasonMeta> = v["seasons"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|s| SeasonMeta {
                        number: s["season_number"].as_u64().unwrap_or(0) as u32,
                        name: text(&s["name"]),
                        episode_count: s["episode_count"].as_u64().unwrap_or(0) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut episodes = Vec::new();
        for season in seasons {
            if season_list.iter().any(|s| s.number == *season) {
                // A failed season only means fewer episode titles.
                episodes.extend(self.season_episodes(id, *season).unwrap_or_default());
            }
        }

        Ok(ShowInfo {
            meta: Meta {
                tmdb_id: id,
                title: text(&v["name"]),
                original_title: text(&v["original_name"]),
                year: year_of(&text(&v["first_air_date"])),
                overview: or_english(text(&v["overview"]), &en.overview),
                genres: genres(&v),
                runtime: v["episode_run_time"][0].as_u64().unwrap_or(0) as u32,
                rating: v["vote_average"].as_f64().unwrap_or(0.0),
                director: creators,
                poster,
                bilingual: en.fetched,
                title_en: en.title,
                overview_en: en.overview,
                genres_en: en.genres,
                cast: cast(&v),
                directors: creator_list,
                collection: None,
                similar: similar(&v),
                version: META_VERSION,
                fetched: db::now(),
            },
            seasons: season_list,
            episodes,
            status: text(&v["status"]),
            next_episode: next_episode(&v["next_episode_to_air"], en.next_episode),
        })
    }

    /// Episodes of a season in Czech and English.
    fn season_episodes(&self, id: u64, season: u32) -> Result<Vec<EpisodeMeta>, Error> {
        let path = format!("/tv/{id}/season/{season}");
        let mut episodes = episode_list(&self.get(&path, &[("language", LANGUAGE)])?, season);
        if let Ok(en) = self.get(&path, &[("language", ENGLISH)]) {
            let en = episode_list(&en, season);
            for episode in episodes.iter_mut() {
                let Some(en) = en.iter().find(|e| e.episode == episode.episode) else {
                    continue;
                };
                episode.title_en = en.title.clone();
                episode.overview_en = en.overview.clone();
                // Without a Czech translation TMDB returns "Epizoda 5" and no overview.
                if episode.title.is_empty() {
                    episode.title = en.title.clone();
                }
                if episode.overview.is_empty() {
                    episode.overview = en.overview.clone();
                }
            }
        }
        Ok(episodes)
    }

    /// Returns the file name of the poster in `dir`.
    fn download_poster(&self, name: String, poster_path: &str, dir: &Path) -> Result<String, String> {
        let target = dir.join(&name);
        // A poster is kept as long as the rest of the data (see `db::TMDB_MAX_AGE`).
        let fresh = fs::metadata(&target)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() <= db::TMDB_MAX_AGE);
        if fresh {
            return Ok(name);
        }
        let response = self
            .agent
            .get(&format!("{IMAGES}/w342{poster_path}"))
            .call()
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_IMAGE_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("{name}.tmp"));
        fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        Ok(name)
    }
}

/// English texts of a movie or series (`title_field`: "title" for movies, "name" for series).
struct English {
    fetched: bool,
    title: String,
    overview: String,
    genres: Vec<String>,
    /// Name of the movie series.
    collection: String,
    /// Name of the next episode of a series.
    next_episode: String,
}

impl English {
    fn fetch(client: &Client, path: &str, title_field: &str) -> English {
        match client.get(path, &[("language", ENGLISH)]) {
            Ok(v) => English {
                fetched: true,
                title: text(&v[title_field]),
                overview: text(&v["overview"]),
                genres: genres(&v),
                collection: text(&v["belongs_to_collection"]["name"]),
                next_episode: text(&v["next_episode_to_air"]["name"]),
            },
            // Missing English is filled in later by the metadata job.
            Err(_) => English {
                fetched: false,
                title: String::new(),
                overview: String::new(),
                genres: Vec::new(),
                collection: String::new(),
                next_episode: String::new(),
            },
        }
    }
}

/// The announced next episode, when TMDB knows its date.
fn next_episode(v: &Value, name_en: String) -> Option<NextEpisode> {
    let air_date = text(&v["air_date"]);
    if air_date.len() < 10 {
        return None;
    }
    Some(NextEpisode {
        season: v["season_number"].as_u64()? as u32,
        episode: v["episode_number"].as_u64()? as u32,
        name: text(&v["name"]),
        name_en,
        air_date,
    })
}

fn person(v: &Value) -> Person {
    Person {
        id: v["id"].as_u64().unwrap_or(0),
        name: text(&v["name"]),
        character: text(&v["character"]),
    }
}

fn names(people: &[Person]) -> String {
    people.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
}

/// Main cast from the credits of a details response.
fn cast(v: &Value) -> Vec<Person> {
    v["credits"]["cast"]
        .as_array()
        .map(|cast| cast.iter().take(CAST_LIMIT).map(person).filter(|p| p.id > 0).collect())
        .unwrap_or_default()
}

/// Ids of recommended, then similar titles of a details response.
fn similar(v: &Value) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    for list in ["recommendations", "similar"] {
        for id in v[list]["results"].as_array().into_iter().flatten().filter_map(|r| r["id"].as_u64()) {
            if !ids.contains(&id) && ids.len() < SIMILAR_LIMIT {
                ids.push(id);
            }
        }
    }
    ids
}

fn or_english(czech: String, english: &str) -> String {
    if czech.is_empty() {
        english.to_string()
    } else {
        czech
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().trim().to_string()
}

fn year_of(date: &str) -> Option<u32> {
    date.get(..4)?.parse().ok()
}

fn genres(v: &Value) -> Vec<String> {
    v["genres"]
        .as_array()
        .map(|g| g.iter().map(|g| text(&g["name"])).filter(|n| !n.is_empty()).collect())
        .unwrap_or_default()
}

/// Episodes of a season response; placeholder titles ("Epizoda 5", "Episode 5") are left empty.
fn episode_list(v: &Value, season: u32) -> Vec<EpisodeMeta> {
    let Some(list) = v["episodes"].as_array() else {
        return Vec::new();
    };
    list.iter()
        .map(|e| {
            let episode = e["episode_number"].as_u64().unwrap_or(0) as u32;
            let title = text(&e["name"]);
            let placeholder = [format!("Epizoda {episode}"), format!("Episode {episode}")];
            EpisodeMeta {
                season,
                episode,
                title: if placeholder.contains(&title) { String::new() } else { title },
                overview: text(&e["overview"]),
                runtime: e["runtime"].as_u64().unwrap_or(0) as u32,
                ..Default::default()
            }
        })
        .collect()
}

/// Movies have `title` and `release_date`, TV series `name` and `first_air_date`.
fn candidate(v: &Value) -> Candidate {
    let field = |movie: &str, tv: &str| {
        let value = text(&v[movie]);
        if value.is_empty() {
            text(&v[tv])
        } else {
            value
        }
    };
    Candidate {
        id: v["id"].as_u64().unwrap_or(0),
        title: field("title", "name"),
        original_title: field("original_title", "original_name"),
        year: year_of(&field("release_date", "first_air_date")),
        overview: text(&v["overview"]),
        poster_url: v["poster_path"].as_str().map(|p| format!("{IMAGES}/w92{p}")),
    }
}

/// Lowercase words without punctuation and leading articles: "The Philosopher's Stone" ->
/// ["philosophers", "stone"].
fn words(title: &str) -> Vec<String> {
    let cleaned: String = title
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '’'))
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .filter(|w| !matches!(*w, "the" | "a" | "an"))
        .map(String::from)
        .collect()
}

/// Share of words in common, 0..=1.
fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let common = a.iter().filter(|w| b.contains(w)).count();
    common as f64 / a.len().max(b.len()) as f64
}

/// Picks the candidate whose title is similar enough; a wrong match is worse than none.
pub fn best_match<'a>(
    candidates: &'a [Candidate],
    title: &str,
    year: Option<u32>,
) -> Option<&'a Candidate> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(rank, c)| {
            let sim = similarity(title, &c.title).max(similarity(title, &c.original_title));
            let year_diff = match (year, c.year) {
                (Some(want), Some(got)) => Some(want.abs_diff(got)),
                _ => None,
            };
            let required = match year_diff {
                Some(d) if d > 1 => 0.8,
                _ => 0.5,
            };
            if sim < required {
                return None;
            }
            let bonus = match year_diff {
                Some(0) => 0.3,
                Some(1) => 0.15,
                _ => 0.0,
            };
            // TMDB orders by relevance, prefer earlier results on a tie.
            Some((sim + bonus - rank as f64 * 0.01, c))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: u64, title: &str, original: &str, year: u32) -> Candidate {
        Candidate {
            id,
            title: title.into(),
            original_title: original.into(),
            year: Some(year),
            overview: String::new(),
            poster_url: None,
        }
    }

    #[test]
    fn picks_sensible_matches() {
        let hp = [
            c(1, "Harry Potter a Kámen mudrců", "Harry Potter and the Philosopher's Stone", 2001),
            c(2, "Harry Potter a Tajemná komnata", "Harry Potter and the Chamber of Secrets", 2002),
        ];
        assert_eq!(best_match(&hp, "Harry Potter And The Sorcerers Stone", Some(2001)).unwrap().id, 1);

        let matrix = [c(5, "Matrix Reloaded", "The Matrix Reloaded", 2003), c(6, "Matrix", "The Matrix", 1999)];
        assert_eq!(best_match(&matrix, "Matrix", Some(1999)).unwrap().id, 6);
        assert_eq!(best_match(&matrix, "The Matrix", None).unwrap().id, 6);

        let random = [c(9, "Úplně jiný film", "Something Else", 2024)];
        assert!(best_match(&random, "AFK Journey", Some(2024)).is_none());
    }
}
