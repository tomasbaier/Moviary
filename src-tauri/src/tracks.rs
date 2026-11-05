//! Choosing the audio and subtitle track a video starts with.
//!
//! Three levels, the more specific wins:
//! 1. Global rules ([`TrackRules`]): preferred audio languages and when to show subtitles
//!    in which languages – e.g. "Czech, else English; subtitles only for foreign audio".
//! 2. A show's choice ([`TrackChoice`] with languages): applies to all its episodes, whose
//!    tracks are usually numbered the same but need not be.
//! 3. A movie's or episode's choice: a concrete track of that file (or subtitles off).
//!
//! The result ([`Resolved`]) becomes VLC arguments; without the file's track list (formats
//! the parser does not read) it falls back to VLC's own language matching.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::media::{ExternalSubtitle, MediaInfo};

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleMode {
    /// As the file (or VLC) decides.
    #[default]
    Auto,
    Off,
    /// Always, in the first available preferred language.
    Always,
    /// Only when the audio is not in a preferred audio language (plus forced subtitles in a
    /// preferred language, which translate foreign passages).
    Foreign,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct TrackRules {
    /// Preferred audio languages (ISO 639-1), best first; empty = as the file decides.
    pub audio_languages: Vec<String>,
    pub subtitles: SubtitleMode,
    /// Preferred subtitle languages, best first.
    pub subtitle_languages: Vec<String>,
}

/// One choice of the user for a movie, episode or show.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Pick {
    /// Subtitles off (not offered for audio).
    Off,
    /// Index among the file's audio or embedded subtitle tracks.
    Track { index: u32 },
    Language { code: String },
    /// External subtitle file next to the video, by file name.
    File { name: String },
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct TrackChoice {
    /// None = by the show or the global rules.
    pub audio: Option<Pick>,
    pub subtitle: Option<Pick>,
}

impl TrackChoice {
    pub fn is_empty(&self) -> bool {
        self.audio.is_none() && self.subtitle.is_none()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum AudioSelection {
    Default,
    Track(u32),
    /// The track list is unknown: let VLC match these languages.
    Languages(Vec<String>),
}

#[derive(Clone, PartialEq, Debug)]
pub enum SubtitleSelection {
    Default,
    Off,
    Track(u32),
    File(PathBuf),
    Languages(Vec<String>),
}

/// Where a selection comes from, shown to the user.
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Movie,
    Show,
    Rules,
    File,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Resolved {
    pub audio: AudioSelection,
    pub audio_source: Source,
    pub subtitle: SubtitleSelection,
    pub subtitle_source: Source,
}

impl Resolved {
    /// VLC command line options (verified with VLC 3: `--sub-track` counts embedded
    /// subtitles first, `--sub-language=none` turns off even autodetected files).
    pub fn vlc_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        match &self.audio {
            AudioSelection::Default => {}
            AudioSelection::Track(i) => args.push(format!("--audio-track={i}")),
            AudioSelection::Languages(l) => args.push(format!("--audio-language={}", l.join(","))),
        }
        match &self.subtitle {
            SubtitleSelection::Default => {}
            SubtitleSelection::Off => args.push("--sub-language=none".into()),
            SubtitleSelection::Track(i) => args.push(format!("--sub-track={i}")),
            SubtitleSelection::File(path) => {
                args.push(format!("--sub-file={}", path.display()));
                // Otherwise the same file would be listed twice.
                args.push("--no-sub-autodetect-file".into());
            }
            SubtitleSelection::Languages(l) => args.push(format!("--sub-language={}", l.join(","))),
        }
        args
    }
}

fn audio_index(media: &MediaInfo, language: &str) -> Option<u32> {
    media
        .audio
        .iter()
        .position(|t| t.language.as_deref() == Some(language))
        .map(|i| i as u32)
}

/// Subtitles in `language`: a full embedded track, a full external file, then forced ones
/// (or only forced ones with `forced_only`).
fn subtitle_in(media: &MediaInfo, external: &[ExternalSubtitle], language: &str, forced_only: bool) -> Option<SubtitleSelection> {
    let lang = |l: &Option<String>| l.as_deref() == Some(language);
    let embedded = |forced: bool| {
        media
            .subtitles
            .iter()
            .position(|t| lang(&t.language) && t.forced == forced)
            .map(|i| SubtitleSelection::Track(i as u32))
    };
    let file = |forced: bool| {
        external
            .iter()
            .find(|s| lang(&s.language) && s.forced == forced)
            .map(|s| SubtitleSelection::File(s.path.clone()))
    };
    if forced_only {
        embedded(true).or_else(|| file(true))
    } else {
        embedded(false).or_else(|| file(false)).or_else(|| embedded(true)).or_else(|| file(true))
    }
}

fn first_subtitle(media: &MediaInfo, external: &[ExternalSubtitle], languages: &[String], forced_only: bool) -> Option<SubtitleSelection> {
    languages.iter().find_map(|l| subtitle_in(media, external, l, forced_only))
}

/// Picks the tracks for a video. `media` is None when the file's tracks are unknown.
pub fn resolve(
    rules: &TrackRules,
    show: Option<&TrackChoice>,
    own: &TrackChoice,
    media: Option<&MediaInfo>,
    external: &[ExternalSubtitle],
) -> Resolved {
    let (audio_pick, audio_source) = match (&own.audio, show.and_then(|s| s.audio.as_ref())) {
        (Some(pick), _) => (Some(pick), Source::Movie),
        (None, Some(pick)) => (Some(pick), Source::Show),
        _ => (None, Source::Rules),
    };
    let (subtitle_pick, subtitle_source) = match (&own.subtitle, show.and_then(|s| s.subtitle.as_ref())) {
        (Some(pick), _) => (Some(pick), Source::Movie),
        (None, Some(pick)) => (Some(pick), Source::Show),
        _ => (None, Source::Rules),
    };

    let Some(media) = media else {
        // Unknown tracks: VLC matches languages itself.
        let audio = match audio_pick {
            Some(Pick::Language { code }) => AudioSelection::Languages(vec![code.clone()]),
            _ if !rules.audio_languages.is_empty() => AudioSelection::Languages(rules.audio_languages.clone()),
            _ => AudioSelection::Default,
        };
        let subtitle = match subtitle_pick {
            Some(Pick::Off) => SubtitleSelection::Off,
            Some(Pick::Language { code }) => SubtitleSelection::Languages(vec![code.clone()]),
            Some(Pick::File { name }) => external
                .iter()
                .find(|s| &s.file_name == name)
                .map_or(SubtitleSelection::Default, |s| SubtitleSelection::File(s.path.clone())),
            _ => match rules.subtitles {
                SubtitleMode::Auto => SubtitleSelection::Default,
                SubtitleMode::Off => SubtitleSelection::Off,
                // Whether the audio is foreign is unknown here.
                SubtitleMode::Always | SubtitleMode::Foreign if !rules.subtitle_languages.is_empty() => {
                    SubtitleSelection::Languages(rules.subtitle_languages.clone())
                }
                _ => SubtitleSelection::Default,
            },
        };
        return Resolved {
            audio,
            audio_source,
            subtitle,
            subtitle_source,
        };
    };

    // ----- audio -----
    let picked_audio = match audio_pick {
        Some(Pick::Track { index }) if (*index as usize) < media.audio.len() => Some(*index),
        Some(Pick::Language { code }) => audio_index(media, code),
        _ => None,
    };
    let (audio, audio_source) = match picked_audio {
        Some(i) => (AudioSelection::Track(i), audio_source),
        None => match rules.audio_languages.iter().find_map(|l| audio_index(media, l)) {
            Some(i) => (AudioSelection::Track(i), Source::Rules),
            None => (AudioSelection::Default, Source::File),
        },
    };
    // Language of the audio that will play (the default track when VLC decides).
    let playing = match audio {
        AudioSelection::Track(i) => media.audio.get(i as usize),
        _ => media.audio.iter().find(|t| t.default).or(media.audio.first()),
    };
    let understood = playing
        .and_then(|t| t.language.as_ref())
        .is_some_and(|l| rules.audio_languages.contains(l));

    // ----- subtitles -----
    let picked_subtitle = match subtitle_pick {
        Some(Pick::Off) => Some(SubtitleSelection::Off),
        Some(Pick::Track { index }) if (*index as usize) < media.subtitles.len() => Some(SubtitleSelection::Track(*index)),
        Some(Pick::File { name }) => external
            .iter()
            .find(|s| &s.file_name == name)
            .map(|s| SubtitleSelection::File(s.path.clone())),
        Some(Pick::Language { code }) => subtitle_in(media, external, code, false),
        _ => None,
    };
    let (subtitle, subtitle_source) = match picked_subtitle {
        Some(selection) => (selection, subtitle_source),
        None => {
            let by_rules = match rules.subtitles {
                SubtitleMode::Auto => None,
                SubtitleMode::Off => Some(SubtitleSelection::Off),
                SubtitleMode::Always => {
                    Some(first_subtitle(media, external, &rules.subtitle_languages, false).unwrap_or(SubtitleSelection::Off))
                }
                SubtitleMode::Foreign => Some(
                    first_subtitle(media, external, &rules.subtitle_languages, understood)
                        .unwrap_or(SubtitleSelection::Off),
                ),
            };
            match by_rules {
                Some(selection) => (selection, Source::Rules),
                None => (SubtitleSelection::Default, Source::File),
            }
        }
    };

    Resolved {
        audio,
        audio_source,
        subtitle,
        subtitle_source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{AudioTrack, SubtitleTrack};

    fn media() -> MediaInfo {
        let audio = |lang: &str, default: bool| AudioTrack { language: Some(lang.into()), default, ..Default::default() };
        let sub = |lang: &str, forced: bool| SubtitleTrack { language: Some(lang.into()), forced, ..Default::default() };
        MediaInfo {
            audio: vec![audio("en", true), audio("cs", false)],
            subtitles: vec![sub("cs", true), sub("en", false), sub("cs", false)],
            ..Default::default()
        }
    }

    fn rules(audio: &[&str], subtitles: SubtitleMode, subs: &[&str]) -> TrackRules {
        TrackRules {
            audio_languages: audio.iter().map(|s| s.to_string()).collect(),
            subtitles,
            subtitle_languages: subs.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn external(name: &str, lang: &str) -> ExternalSubtitle {
        ExternalSubtitle { path: PathBuf::from(name), file_name: name.into(), language: Some(lang.into()), forced: false }
    }

    #[test]
    fn global_rules() {
        let none = TrackChoice::default();
        let m = media();
        // Czech audio preferred; subtitles only for foreign audio -> Czech audio, forced Czech subs.
        let r = resolve(&rules(&["cs", "en"], SubtitleMode::Foreign, &["cs"]), None, &none, Some(&m), &[]);
        assert_eq!((r.audio.clone(), r.subtitle.clone()), (AudioSelection::Track(1), SubtitleSelection::Track(0)));
        // English audio preferred, subtitles for foreign audio: English is understood too.
        let r = resolve(&rules(&["en"], SubtitleMode::Foreign, &["cs"]), None, &none, Some(&m), &[]);
        assert_eq!(r.audio, AudioSelection::Track(0));
        assert_eq!(r.subtitle, SubtitleSelection::Track(0));
        // Japanese preferred (not present): default English plays, it is foreign -> full Czech subs.
        let r = resolve(&rules(&["ja"], SubtitleMode::Foreign, &["cs"]), None, &none, Some(&m), &[]);
        assert_eq!((r.audio.clone(), r.audio_source), (AudioSelection::Default, Source::File));
        assert_eq!(r.subtitle, SubtitleSelection::Track(2));
        // Never subtitles.
        let r = resolve(&rules(&[], SubtitleMode::Off, &[]), None, &none, Some(&m), &[]);
        assert_eq!(r.subtitle, SubtitleSelection::Off);
        assert_eq!(r.vlc_args(), ["--sub-language=none"]);
        // Always Slovak subtitles: an external file when not embedded.
        let ext = [external("Film.sk.srt", "sk")];
        let r = resolve(&rules(&[], SubtitleMode::Always, &["sk", "cs"]), None, &none, Some(&m), &ext);
        assert_eq!(r.subtitle, SubtitleSelection::File(PathBuf::from("Film.sk.srt")));
    }

    #[test]
    fn movie_beats_show_beats_rules() {
        let m = media();
        let rules = rules(&["cs"], SubtitleMode::Off, &[]);
        let show = TrackChoice {
            audio: Some(Pick::Language { code: "en".into() }),
            subtitle: Some(Pick::Language { code: "en".into() }),
        };
        let r = resolve(&rules, Some(&show), &TrackChoice::default(), Some(&m), &[]);
        assert_eq!((r.audio.clone(), r.audio_source), (AudioSelection::Track(0), Source::Show));
        assert_eq!(r.subtitle, SubtitleSelection::Track(1));
        let own = TrackChoice {
            audio: Some(Pick::Track { index: 1 }),
            subtitle: Some(Pick::Off),
        };
        let r = resolve(&rules, Some(&show), &own, Some(&m), &[]);
        assert_eq!((r.audio.clone(), r.audio_source, r.subtitle.clone()), (AudioSelection::Track(1), Source::Movie, SubtitleSelection::Off));
        assert_eq!(r.vlc_args(), ["--audio-track=1", "--sub-language=none"]);
        // A track index the file does not have falls back to the rules.
        let own = TrackChoice { audio: Some(Pick::Track { index: 7 }), subtitle: None };
        assert_eq!(resolve(&rules, None, &own, Some(&m), &[]).audio, AudioSelection::Track(1));
    }

    #[test]
    fn unknown_tracks_use_vlc_languages() {
        let r = resolve(&rules(&["cs", "en"], SubtitleMode::Always, &["cs"]), None, &TrackChoice::default(), None, &[]);
        assert_eq!(r.vlc_args(), ["--audio-language=cs,en", "--sub-language=cs"]);
        assert!(resolve(&TrackRules::default(), None, &TrackChoice::default(), None, &[]).vlc_args().is_empty());
    }
}
