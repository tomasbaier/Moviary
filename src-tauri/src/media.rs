//! Technical information read from the headers of video files: resolution, HDR, codecs and
//! the audio and subtitle tracks with their languages. Understands Matroska (MKV, WebM) and
//! MP4 / QuickTime; other formats fall back to tags in the file name ([`tags_from_name`]).
//!
//! Only the track headers are read (usually well under a megabyte, found by seeking), never
//! the whole file.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bump to re-read files probed by an older version of the parser.
pub const PROBE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaInfo {
    /// "mkv" or "mp4".
    pub container: String,
    /// Seconds, 0 when unknown.
    pub duration: f64,
    pub video: Option<VideoInfo>,
    /// In file order: the n-th entry is VLC's audio track n.
    pub audio: Vec<AudioTrack>,
    /// Embedded subtitles in file order: the n-th entry is VLC's subtitle track n.
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct VideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// "HDR10", "HLG" or "Dolby Vision".
    pub hdr: Option<String>,
    pub bit_depth: u32,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioTrack {
    pub codec: String,
    /// ISO 639-1 code when known ("cs"), otherwise the code from the file.
    pub language: Option<String>,
    pub channels: u32,
    pub name: String,
    pub default: bool,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub codec: String,
    pub language: Option<String>,
    pub name: String,
    pub default: bool,
    pub forced: bool,
}

/// Subtitle file next to a video ("Film.cs.srt").
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSubtitle {
    #[serde(skip)]
    pub path: PathBuf,
    pub file_name: String,
    pub language: Option<String>,
    pub forced: bool,
}

/// Reads the track headers; None for unknown formats or damaged files.
pub fn probe(file: File) -> Option<MediaInfo> {
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut magic = [0u8; 12];
    reader.read_exact(&mut magic).ok()?;
    reader.seek(SeekFrom::Start(0)).ok()?;
    if magic[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        mkv::probe(&mut reader)
    } else if matches!(&magic[4..8], b"ftyp" | b"moov" | b"mdat" | b"free" | b"wide" | b"skip") {
        mp4::probe(&mut reader)
    } else {
        None
    }
}

// ---------- languages ----------

/// ISO 639-1 code, Czech name, English name, other spellings (639-2/B and /T codes, English names, tags
/// used in file names).
const LANGUAGES: &[(&str, &str, &str, &[&str])] = &[
    ("cs", "čeština", "Czech", &["cze", "ces", "cz", "czech", "cesky", "česky", "cestina", "čeština", "czdab", "czsub", "cztit"]),
    ("sk", "slovenština", "Slovak", &["slo", "slk", "slovak", "slovensky", "slovenčina", "slovencina", "skdab", "sksub"]),
    ("en", "angličtina", "English", &["eng", "english", "anglicky"]),
    ("de", "němčina", "German", &["ger", "deu", "german", "deutsch", "nemecky"]),
    ("fr", "francouzština", "French", &["fre", "fra", "french", "francais", "français"]),
    ("es", "španělština", "Spanish", &["spa", "spanish", "espanol", "español", "castellano"]),
    ("it", "italština", "Italian", &["ita", "italian", "italiano"]),
    ("pl", "polština", "Polish", &["pol", "polish", "polski"]),
    ("hu", "maďarština", "Hungarian", &["hun", "hungarian", "magyar"]),
    ("ru", "ruština", "Russian", &["rus", "russian"]),
    ("uk", "ukrajinština", "Ukrainian", &["ukr", "ukrainian"]),
    ("pt", "portugalština", "Portuguese", &["por", "portuguese", "portugues", "português"]),
    ("nl", "nizozemština", "Dutch", &["dut", "nld", "dutch", "nederlands"]),
    ("sv", "švédština", "Swedish", &["swe", "swedish", "svenska"]),
    ("da", "dánština", "Danish", &["dan", "danish", "dansk"]),
    ("no", "norština", "Norwegian", &["nor", "nob", "nno", "norwegian", "norsk"]),
    ("fi", "finština", "Finnish", &["fin", "finnish", "suomi"]),
    ("hr", "chorvatština", "Croatian", &["hrv", "scr", "croatian", "hrvatski"]),
    ("sr", "srbština", "Serbian", &["srp", "scc", "serbian"]),
    ("sl", "slovinština", "Slovenian", &["slv", "slovenian", "slovene"]),
    ("ro", "rumunština", "Romanian", &["rum", "ron", "romanian"]),
    ("bg", "bulharština", "Bulgarian", &["bul", "bulgarian"]),
    ("el", "řečtina", "Greek", &["gre", "ell", "greek"]),
    ("tr", "turečtina", "Turkish", &["tur", "turkish"]),
    ("he", "hebrejština", "Hebrew", &["heb", "hebrew"]),
    ("ar", "arabština", "Arabic", &["ara", "arabic"]),
    ("hi", "hindština", "Hindi", &["hin", "hindi"]),
    ("th", "thajština", "Thai", &["tha", "thai"]),
    ("vi", "vietnamština", "Vietnamese", &["vie", "vietnamese"]),
    ("ja", "japonština", "Japanese", &["jpn", "japanese"]),
    ("ko", "korejština", "Korean", &["kor", "korean"]),
    ("zh", "čínština", "Chinese", &["chi", "zho", "chinese", "mandarin", "cantonese"]),
    ("lt", "litevština", "Lithuanian", &["lit", "lithuanian"]),
    ("lv", "lotyština", "Latvian", &["lav", "latvian"]),
    ("et", "estonština", "Estonian", &["est", "estonian"]),
    ("is", "islandština", "Icelandic", &["ice", "isl", "icelandic"]),
    ("id", "indonéština", "Indonesian", &["ind", "indonesian"]),
    ("ms", "malajština", "Malay", &["may", "msa", "malay"]),
    ("fa", "perština", "Persian", &["per", "fas", "persian", "farsi"]),
    ("ca", "katalánština", "Catalan", &["cat", "catalan"]),
];

/// Code -> name of every known language in the app's language (for the settings).
pub fn language_names() -> Vec<(&'static str, &'static str)> {
    let english = crate::i18n::english();
    LANGUAGES
        .iter()
        .map(|(code, cs, en, _)| (*code, if english { *en } else { *cs }))
        .collect()
}

/// "cze", "ces", "cs-CZ", "Czech" -> "cs". Unknown codes are kept, "und" and empty give None.
pub fn normalize_language(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase();
    let base = lower.split(['-', '_']).next().unwrap_or_default();
    if base.is_empty() || matches!(base, "und" | "mis" | "zxx" | "mul" | "qaa") {
        return None;
    }
    LANGUAGES
        .iter()
        .find(|(code, _, _, other)| *code == base || other.contains(&base))
        .map(|(code, _, _, _)| code.to_string())
        .or_else(|| (base.len() <= 3 && base.chars().all(|c| c.is_ascii_alphabetic())).then(|| base.to_string()))
}

/// Language named in a track name ("Czech", "English (SDH)") when the track has none.
fn language_in_name(name: &str) -> Option<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .find_map(language_word)
}

/// Language named by a word of a file name ("cz", "eng", "czech"), stricter than
/// [`normalize_language`]: unknown words are not languages.
fn language_word(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    LANGUAGES
        .iter()
        .find(|(code, _, _, other)| *code == lower || other.contains(&lower.as_str()))
        .map(|(code, _, _, _)| code.to_string())
}

// ---------- tags for filters ----------

/// Summary used by the technical filters, from the headers or the file name.
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tags {
    /// "4K", "1080p", "720p" or "SD".
    pub resolution: Option<&'static str>,
    pub hdr: Option<String>,
    pub video_codec: Option<String>,
    pub audio_languages: Vec<String>,
    /// Embedded and external subtitles.
    pub subtitle_languages: Vec<String>,
    pub has_subtitles: bool,
    /// Most channels of an audio track (6 = 5.1).
    pub channels: u32,
}

fn resolution_of(width: u32, height: u32) -> &'static str {
    // Widescreen movies are wide rather than tall (3840×1606 is still 4K).
    if width >= 3200 || height >= 1800 {
        "4K"
    } else if width >= 1700 || height >= 1000 {
        "1080p"
    } else if width >= 1200 || height >= 700 {
        "720p"
    } else {
        "SD"
    }
}

fn push_unique(list: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value {
        if !list.contains(&value) {
            list.push(value);
        }
    }
}

pub fn tags(media: Option<&MediaInfo>, external: &[ExternalSubtitle], file_name: &str) -> Tags {
    let mut tags = tags_from_name(file_name);
    if let Some(media) = media {
        if let Some(video) = &media.video {
            if video.width > 0 {
                tags.resolution = Some(resolution_of(video.width, video.height));
            }
            if !video.codec.is_empty() {
                tags.video_codec = Some(video.codec.clone());
            }
            // The name may claim HDR that the headers do not show (MP4 rarely has them).
            if video.hdr.is_some() {
                tags.hdr = video.hdr.clone();
            }
        }
        if !media.audio.is_empty() {
            tags.audio_languages.clear();
            tags.channels = 0;
            for track in &media.audio {
                push_unique(&mut tags.audio_languages, track.language.clone());
                tags.channels = tags.channels.max(track.channels);
            }
        }
        tags.subtitle_languages.clear();
        for track in &media.subtitles {
            push_unique(&mut tags.subtitle_languages, track.language.clone());
        }
        tags.has_subtitles = !media.subtitles.is_empty();
    }
    for sub in external {
        push_unique(&mut tags.subtitle_languages, sub.language.clone());
    }
    tags.has_subtitles |= !external.is_empty();
    tags
}

/// Tags in release names: "2160p", "x265", "HDR", "DV", "5.1", "CZ dabing", "CZ titulky".
pub fn tags_from_name(file_name: &str) -> Tags {
    let lower = file_name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |w: &str| words.contains(&w);
    let mut tags = Tags::default();
    tags.resolution = if has("2160p") || has("4k") || has("uhd") {
        Some("4K")
    } else if has("1080p") || has("1080i") {
        Some("1080p")
    } else if has("720p") {
        Some("720p")
    } else if has("480p") || has("576p") || has("dvdrip") {
        Some("SD")
    } else {
        None
    };
    tags.video_codec = if has("x265") || has("h265") || has("hevc") {
        Some("HEVC".into())
    } else if has("x264") || has("h264") || has("avc") {
        Some("H.264".into())
    } else if has("av1") {
        Some("AV1".into())
    } else if has("xvid") || has("divx") {
        Some("MPEG-4".into())
    } else {
        None
    };
    tags.hdr = if has("dv") || has("dovi") || lower.contains("dolby vision") {
        Some("Dolby Vision".into())
    } else if has("hdr") || has("hdr10") || lower.contains("hdr10+") {
        Some("HDR10".into())
    } else {
        None
    };
    if lower.contains("7.1") {
        tags.channels = 8;
    } else if lower.contains("5.1") {
        tags.channels = 6;
    }
    // "CZ dabing", "CZdab", "CZ titulky", "CZsub"
    for (i, word) in words.iter().enumerate() {
        let next = words.get(i + 1).copied().unwrap_or_default();
        let lang = match *word {
            "czdab" | "czsub" | "cztit" => Some("cs".to_string()),
            "skdab" | "sksub" => Some("sk".to_string()),
            "cz" | "sk" => language_word(word),
            _ => None,
        };
        let Some(lang) = lang else {
            continue;
        };
        if word.ends_with("sub") || word.ends_with("tit") || matches!(next, "titulky" | "tit" | "sub" | "subs") {
            push_unique(&mut tags.subtitle_languages, Some(lang));
            tags.has_subtitles = true;
        } else {
            push_unique(&mut tags.audio_languages, Some(lang));
        }
    }
    tags
}

// ---------- external subtitles ----------

const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "sub", "vtt", "smi"];
const SUBTITLE_FOLDERS: &[&str] = &["subs", "sub", "subtitles", "titulky"];

pub fn is_subtitle(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUBTITLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Subtitle files belonging to `video`: in its folder starting with the video's name, any
/// subtitles when the video is alone in its folder, and those in a "Subs" folder next to it.
pub fn find_external_subtitles(video: &Path, videos_in_folder: usize) -> Vec<ExternalSubtitle> {
    let Some(dir) = video.parent() else {
        return Vec::new();
    };
    let stem = video
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let alone = videos_in_folder <= 1;
    let mut found = Vec::new();
    let mut add = |path: PathBuf, rest: &str| {
        let words: Vec<&str> = rest
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        found.push(ExternalSubtitle {
            file_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            language: words.iter().rev().find_map(|w| language_word(w)),
            forced: words.iter().any(|w| w.eq_ignore_ascii_case("forced")),
            path,
        });
    };

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if path.is_dir() {
                if alone && SUBTITLE_FOLDERS.contains(&name.as_str()) {
                    for sub in std::fs::read_dir(&path).into_iter().flatten().flatten() {
                        let sub_path = sub.path();
                        if is_subtitle(&sub_path) {
                            let sub_name = sub.file_name().to_string_lossy().to_lowercase();
                            add(sub_path, &sub_name);
                        }
                    }
                }
                continue;
            }
            if !is_subtitle(&path) {
                continue;
            }
            let sub_stem = Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if let Some(rest) = sub_stem.strip_prefix(&stem) {
                add(path, rest);
            } else if alone {
                add(path, &sub_stem);
            }
        }
    }
    found.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    found
}

// ---------- Matroska ----------

mod mkv {
    use super::*;

    const SEGMENT: u32 = 0x1853_8067;
    const SEEK_HEAD: u32 = 0x114D_9B74;
    const SEEK: u32 = 0x4DBB;
    const SEEK_ID: u32 = 0x53AB;
    const SEEK_POSITION: u32 = 0x53AC;
    const INFO: u32 = 0x1549_A966;
    const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
    const DURATION: u32 = 0x4489;
    const TRACKS: u32 = 0x1654_AE6B;
    const TRACK_ENTRY: u32 = 0xAE;
    const TRACK_TYPE: u32 = 0x83;
    const FLAG_DEFAULT: u32 = 0x88;
    const FLAG_FORCED: u32 = 0x55AA;
    const NAME: u32 = 0x536E;
    const LANGUAGE: u32 = 0x22_B59C;
    const LANGUAGE_IETF: u32 = 0x22_B59D;
    const CODEC_ID: u32 = 0x86;
    const CODEC_PRIVATE: u32 = 0x63A2;
    const VIDEO: u32 = 0xE0;
    const PIXEL_WIDTH: u32 = 0xB0;
    const PIXEL_HEIGHT: u32 = 0xBA;
    const COLOUR: u32 = 0x55B0;
    const BITS_PER_CHANNEL: u32 = 0x55B2;
    const TRANSFER: u32 = 0x55BA;
    const AUDIO: u32 = 0xE1;
    const CHANNELS: u32 = 0x9F;
    const BLOCK_ADDITION_MAPPING: u32 = 0x41E4;
    const BLOCK_ADD_ID_TYPE: u32 = 0x41E7;
    const CLUSTER: u32 = 0x1F43_B675;

    /// Elements larger than this are not read into memory.
    const MAX_ELEMENT: u64 = 16 * 1024 * 1024;

    struct Header {
        id: u32,
        /// None = unknown size (live streams).
        size: Option<u64>,
        /// Offset of the data.
        start: u64,
    }

    fn read_vint<R: Read>(r: &mut R, keep_marker: bool) -> Option<(u64, usize)> {
        let mut first = [0u8; 1];
        r.read_exact(&mut first).ok()?;
        let len = first[0].leading_zeros() as usize + 1;
        if len > 8 {
            return None;
        }
        let mut value = if keep_marker { u64::from(first[0]) } else { u64::from(first[0]) & (0xFF >> len) };
        let mut all_ones = value == (0xFF >> len);
        for _ in 1..len {
            let mut b = [0u8; 1];
            r.read_exact(&mut b).ok()?;
            all_ones &= b[0] == 0xFF;
            value = (value << 8) | u64::from(b[0]);
        }
        if !keep_marker && all_ones {
            return Some((u64::MAX, len));
        }
        Some((value, len))
    }

    fn header<R: Read + Seek>(r: &mut R) -> Option<Header> {
        let (id, _) = read_vint(r, true)?;
        let (size, _) = read_vint(r, false)?;
        let start = r.stream_position().ok()?;
        Some(Header {
            id: id as u32,
            size: (size != u64::MAX).then_some(size),
            start,
        })
    }

    /// Child elements of an element read into memory: (id, data).
    fn children(data: &[u8]) -> Vec<(u32, &[u8])> {
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(data);
        while (cursor.position() as usize) < data.len() {
            let Some(h) = header(&mut cursor) else {
                break;
            };
            let start = h.start as usize;
            let end = h.size.map_or(data.len(), |s| start.saturating_add(s as usize)).min(data.len());
            out.push((h.id, &data[start..end]));
            cursor.set_position(end as u64);
        }
        out
    }

    fn uint(data: &[u8]) -> u64 {
        data.iter().take(8).fold(0, |acc, b| (acc << 8) | u64::from(*b))
    }

    fn float(data: &[u8]) -> f64 {
        match data.len() {
            4 => f64::from(f32::from_be_bytes(data.try_into().unwrap())),
            8 => f64::from_be_bytes(data.try_into().unwrap()),
            _ => 0.0,
        }
    }

    fn text(data: &[u8]) -> String {
        String::from_utf8_lossy(data).trim_end_matches('\0').trim().to_string()
    }

    fn read_element<R: Read + Seek>(r: &mut R, h: &Header) -> Option<Vec<u8>> {
        let size = h.size.filter(|s| *s <= MAX_ELEMENT)?;
        let mut data = vec![0u8; size as usize];
        r.seek(SeekFrom::Start(h.start)).ok()?;
        r.read_exact(&mut data).ok()?;
        Some(data)
    }

    pub fn probe<R: Read + Seek>(r: &mut R) -> Option<MediaInfo> {
        let ebml = header(r)?;
        r.seek(SeekFrom::Start(ebml.start + ebml.size?)).ok()?;
        let segment = header(r)?;
        if segment.id != SEGMENT {
            return None;
        }
        let segment_end = segment.size.map(|s| segment.start + s);

        let mut info: Option<Vec<u8>> = None;
        let mut tracks: Option<Vec<u8>> = None;
        let mut seek_targets: Vec<(u32, u64)> = Vec::new();
        let mut pos = segment.start;
        // Top-level elements up to the first cluster (where Info and Tracks normally are).
        for _ in 0..64 {
            if segment_end.is_some_and(|end| pos >= end) || (info.is_some() && tracks.is_some()) {
                break;
            }
            r.seek(SeekFrom::Start(pos)).ok()?;
            let Some(h) = header(r) else {
                break;
            };
            match h.id {
                INFO => info = read_element(r, &h),
                TRACKS => tracks = read_element(r, &h),
                SEEK_HEAD => {
                    for (id, seek) in children(&read_element(r, &h).unwrap_or_default()) {
                        if id != SEEK {
                            continue;
                        }
                        let fields = children(seek);
                        let target = fields.iter().find(|(id, _)| *id == SEEK_ID).map(|(_, d)| uint(d) as u32);
                        let position = fields.iter().find(|(id, _)| *id == SEEK_POSITION).map(|(_, d)| uint(d));
                        if let (Some(target), Some(position)) = (target, position) {
                            seek_targets.push((target, segment.start + position));
                        }
                    }
                }
                CLUSTER => break,
                _ => {}
            }
            let Some(size) = h.size else {
                break;
            };
            pos = h.start + size;
        }
        // Info or Tracks after the clusters: follow the SeekHead.
        for (target, position) in seek_targets {
            let slot = match target {
                INFO if info.is_none() => &mut info,
                TRACKS if tracks.is_none() => &mut tracks,
                _ => continue,
            };
            r.seek(SeekFrom::Start(position)).ok()?;
            if let Some(h) = header(r).filter(|h| h.id == target) {
                *slot = read_element(r, &h);
            }
        }

        let mut media = MediaInfo {
            container: "mkv".into(),
            ..Default::default()
        };
        if let Some(info) = info {
            let fields = children(&info);
            let scale = fields
                .iter()
                .find(|(id, _)| *id == TIMESTAMP_SCALE)
                .map_or(1_000_000, |(_, d)| uint(d));
            if let Some((_, d)) = fields.iter().find(|(id, _)| *id == DURATION) {
                media.duration = float(d) * scale as f64 / 1e9;
            }
        }
        for (id, entry) in children(&tracks?) {
            if id == TRACK_ENTRY {
                add_track(&mut media, entry);
            }
        }
        Some(media)
    }

    fn video_codec(id: &str) -> String {
        match id {
            "V_MPEGH/ISO/HEVC" => "HEVC",
            "V_MPEG4/ISO/AVC" => "H.264",
            "V_AV1" => "AV1",
            "V_VP9" => "VP9",
            "V_VP8" => "VP8",
            "V_MPEG2" => "MPEG-2",
            id if id.starts_with("V_MPEG4/ISO") || id.starts_with("V_MS/VFW") => "MPEG-4",
            id => id.trim_start_matches("V_"),
        }
        .to_string()
    }

    fn audio_codec(id: &str) -> String {
        match id {
            id if id.starts_with("A_AAC") => "AAC",
            "A_AC3" => "AC3",
            "A_EAC3" => "E-AC3",
            "A_TRUEHD" => "TrueHD",
            id if id.starts_with("A_DTS") => "DTS",
            "A_FLAC" => "FLAC",
            "A_OPUS" => "Opus",
            "A_VORBIS" => "Vorbis",
            "A_MPEG/L3" => "MP3",
            "A_MPEG/L2" => "MP2",
            id if id.starts_with("A_PCM") => "PCM",
            id => id.trim_start_matches("A_"),
        }
        .to_string()
    }

    fn subtitle_codec(id: &str) -> String {
        match id {
            "S_TEXT/UTF8" | "S_TEXT/ASCII" => "SRT",
            "S_TEXT/ASS" | "S_TEXT/SSA" | "S_ASS" | "S_SSA" => "ASS",
            "S_TEXT/WEBVTT" => "WebVTT",
            "S_HDMV/PGS" => "PGS",
            "S_VOBSUB" => "VobSub",
            "S_DVBSUB" => "DVB",
            id => id.trim_start_matches("S_"),
        }
        .to_string()
    }

    fn add_track(media: &mut MediaInfo, entry: &[u8]) {
        let fields = children(entry);
        let get = |id: u32| fields.iter().find(|(i, _)| *i == id).map(|(_, d)| *d);
        let codec_id = get(CODEC_ID).map(text).unwrap_or_default();
        // Language defaults to English in Matroska; the IETF tag wins when present.
        let name = get(NAME).map(text).unwrap_or_default();
        let language = match get(LANGUAGE_IETF) {
            Some(tag) => normalize_language(&text(tag)),
            None => normalize_language(&get(LANGUAGE).map(text).unwrap_or_else(|| "eng".into())),
        }
        .or_else(|| language_in_name(&name));
        let default = get(FLAG_DEFAULT).is_none_or(|d| uint(d) != 0);
        match get(TRACK_TYPE).map(uint) {
            Some(1) if media.video.is_none() => {
                let video = get(VIDEO).map(children).unwrap_or_default();
                let vget = |id: u32| video.iter().find(|(i, _)| *i == id).map(|(_, d)| *d);
                let colour = vget(COLOUR).map(children).unwrap_or_default();
                let cget = |id: u32| colour.iter().find(|(i, _)| *i == id).map(|(_, d)| uint(d));
                let dolby_vision = fields
                    .iter()
                    .filter(|(id, _)| *id == BLOCK_ADDITION_MAPPING)
                    .flat_map(|(_, d)| children(d))
                    .any(|(id, d)| id == BLOCK_ADD_ID_TYPE && matches!(&uint(d).to_be_bytes()[4..], b"dvcC" | b"dvvC"));
                let hdr = if dolby_vision {
                    Some("Dolby Vision")
                } else {
                    match cget(TRANSFER) {
                        Some(16) => Some("HDR10"),
                        Some(18) => Some("HLG"),
                        _ => None,
                    }
                };
                media.video = Some(VideoInfo {
                    codec: video_codec(&codec_id),
                    width: vget(PIXEL_WIDTH).map_or(0, uint) as u32,
                    height: vget(PIXEL_HEIGHT).map_or(0, uint) as u32,
                    hdr: hdr.map(String::from),
                    // HEVC: from the configuration record when the colour info is missing.
                    bit_depth: cget(BITS_PER_CHANNEL).map(|b| b as u32).unwrap_or_else(|| {
                        get(CODEC_PRIVATE)
                            .filter(|_| codec_id == "V_MPEGH/ISO/HEVC")
                            .and_then(|d| d.get(18))
                            .map_or(0, |b| u32::from(b & 7) + 8)
                    }),
                });
            }
            Some(2) => {
                let audio = get(AUDIO).map(children).unwrap_or_default();
                let channels = audio.iter().find(|(i, _)| *i == CHANNELS).map_or(1, |(_, d)| uint(d)) as u32;
                media.audio.push(AudioTrack {
                    codec: audio_codec(&codec_id),
                    language,
                    channels,
                    name,
                    default,
                });
            }
            Some(17) => media.subtitles.push(SubtitleTrack {
                codec: subtitle_codec(&codec_id),
                language,
                name,
                default,
                forced: get(FLAG_FORCED).is_some_and(|d| uint(d) != 0),
            }),
            _ => {}
        }
    }
}

// ---------- MP4 / QuickTime ----------

mod mp4 {
    use super::*;

    /// Boxes larger than this are not read into memory.
    const MAX_BOX: u64 = 8 * 1024 * 1024;
    const CONTAINERS: &[&[u8; 4]] = &[b"trak", b"mdia", b"minf", b"stbl"];

    struct BoxHeader {
        kind: [u8; 4],
        /// Offset of the content.
        start: u64,
        end: u64,
    }

    fn box_header<R: Read + Seek>(r: &mut R, limit: u64) -> Option<BoxHeader> {
        let pos = r.stream_position().ok()?;
        let mut head = [0u8; 8];
        r.read_exact(&mut head).ok()?;
        let mut size = u64::from(u32::from_be_bytes(head[..4].try_into().unwrap()));
        let kind: [u8; 4] = head[4..].try_into().unwrap();
        let mut header_len = 8;
        if size == 1 {
            let mut large = [0u8; 8];
            r.read_exact(&mut large).ok()?;
            size = u64::from_be_bytes(large);
            header_len = 16;
        } else if size == 0 {
            size = limit.saturating_sub(pos);
        }
        if size < header_len {
            return None;
        }
        Some(BoxHeader {
            kind,
            start: pos + header_len,
            end: (pos + size).min(limit),
        })
    }

    fn be16(d: &[u8], at: usize) -> u32 {
        d.get(at..at + 2).map_or(0, |b| u32::from(u16::from_be_bytes([b[0], b[1]])))
    }

    fn be32(d: &[u8], at: usize) -> u64 {
        d.get(at..at + 4).map_or(0, |b| u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])))
    }

    fn be64(d: &[u8], at: usize) -> u64 {
        d.get(at..at + 8).map_or(0, |b| u64::from_be_bytes(b.try_into().unwrap()))
    }

    /// Child boxes of a box read into memory: (kind, content).
    fn children(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
        let mut out = Vec::new();
        let mut at = 0usize;
        while at + 8 <= data.len() {
            let size = be32(data, at) as usize;
            let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
            let (header, size) = match size {
                1 => (16, be64(data, at + 8) as usize),
                0 => (8, data.len() - at),
                s => (8, s),
            };
            if size < header || at + size > data.len() {
                break;
            }
            out.push((kind, &data[at + header..at + size]));
            at += size;
        }
        out
    }

    #[derive(Default)]
    struct Track {
        handler: [u8; 4],
        language: Option<String>,
        /// Extended language (elng), wins over the packed one.
        elng: Option<String>,
        enabled: bool,
        name: String,
        /// First sample description: codec and its content.
        sample: Option<([u8; 4], Vec<u8>)>,
    }

    pub fn probe<R: Read + Seek>(r: &mut R) -> Option<MediaInfo> {
        let file_end = r.seek(SeekFrom::End(0)).ok()?;
        r.seek(SeekFrom::Start(0)).ok()?;
        // Find moov among the top-level boxes (it may follow the media data).
        let moov = loop {
            let h = box_header(r, file_end)?;
            if &h.kind == b"moov" {
                break h;
            }
            r.seek(SeekFrom::Start(h.end)).ok()?;
        };

        let mut media = MediaInfo {
            container: "mp4".into(),
            ..Default::default()
        };
        r.seek(SeekFrom::Start(moov.start)).ok()?;
        while r.stream_position().ok()? + 8 <= moov.end {
            let h = box_header(r, moov.end)?;
            match &h.kind {
                b"mvhd" => {
                    let d = read_box(r, &h)?;
                    let (scale, duration) = if d.first() == Some(&1) {
                        (be32(&d, 20), be64(&d, 24))
                    } else {
                        (be32(&d, 12), be32(&d, 16))
                    };
                    if scale > 0 {
                        media.duration = duration as f64 / scale as f64;
                    }
                }
                b"trak" => {
                    let mut track = Track { enabled: true, ..Default::default() };
                    read_trak(r, &h, &mut track)?;
                    add_track(&mut media, track);
                }
                _ => {}
            }
            r.seek(SeekFrom::Start(h.end)).ok()?;
        }
        Some(media)
    }

    fn read_box<R: Read + Seek>(r: &mut R, h: &BoxHeader) -> Option<Vec<u8>> {
        let size = h.end.checked_sub(h.start).filter(|s| *s <= MAX_BOX)?;
        let mut data = vec![0u8; size as usize];
        r.seek(SeekFrom::Start(h.start)).ok()?;
        r.read_exact(&mut data).ok()?;
        Some(data)
    }

    /// Walks trak/mdia/minf/stbl by seeking, so the large sample tables are skipped.
    fn read_trak<R: Read + Seek>(r: &mut R, parent: &BoxHeader, track: &mut Track) -> Option<()> {
        r.seek(SeekFrom::Start(parent.start)).ok()?;
        while r.stream_position().ok()? + 8 <= parent.end {
            let h = box_header(r, parent.end)?;
            match &h.kind {
                kind if CONTAINERS.contains(&kind) => read_trak(r, &h, track)?,
                b"tkhd" => {
                    let d = read_box(r, &h)?;
                    track.enabled = d.get(3).is_none_or(|flags| flags & 1 != 0);
                }
                b"mdhd" => {
                    let d = read_box(r, &h)?;
                    let at = if d.first() == Some(&1) { 32 } else { 20 };
                    let packed = be16(&d, at);
                    if packed != 0 && packed != 0x7FFF {
                        let code: String = [(packed >> 10) & 31, (packed >> 5) & 31, packed & 31]
                            .iter()
                            .map(|c| char::from((c + 0x60) as u8))
                            .collect();
                        track.language = normalize_language(&code);
                    }
                }
                b"elng" => {
                    let d = read_box(r, &h)?;
                    track.elng = normalize_language(&String::from_utf8_lossy(d.get(4..).unwrap_or_default()).trim_end_matches('\0').to_string());
                }
                b"hdlr" => {
                    let d = read_box(r, &h)?;
                    track.handler = d.get(8..12).and_then(|k| k.try_into().ok()).unwrap_or_default();
                    // Name after 24 bytes; QuickTime prefixes it with its length.
                    let raw = d.get(24..).unwrap_or_default();
                    let raw = match raw.first() {
                        Some(len) if usize::from(*len) == raw.len() - 1 => &raw[1..],
                        _ => raw,
                    };
                    track.name = String::from_utf8_lossy(raw).trim_end_matches('\0').trim().to_string();
                }
                b"stsd" => {
                    let d = read_box(r, &h)?;
                    // version/flags (4), entry count (4), then sample entries.
                    if let Some((kind, content)) = children(d.get(8..).unwrap_or_default()).into_iter().next() {
                        track.sample = Some((kind, content.to_vec()));
                    }
                }
                _ => {}
            }
            r.seek(SeekFrom::Start(h.end)).ok()?;
        }
        Some(())
    }

    /// Number of channels of AAC (from the esds descriptors) or AC3 (dac3); the sample entry
    /// often says 2 regardless.
    fn real_channels(kind: &[u8; 4], entry: &[u8]) -> Option<u32> {
        // Child boxes follow the sound sample entry, which is longer in QuickTime v1 / v2.
        let offset = match be16(entry, 8) {
            1 => 44,
            2 => 64,
            _ => 28,
        };
        let boxes = children(entry.get(offset..)?);
        match kind {
            b"mp4a" => {
                let esds = boxes.iter().find(|(k, _)| k == b"esds")?.1;
                let config = descriptor(esds.get(4..)?, 0x03)
                    .and_then(|es| {
                        let flags = *es.get(2)?;
                        let mut at = 3;
                        if flags & 0x80 != 0 {
                            at += 2;
                        }
                        if flags & 0x40 != 0 {
                            at += 1 + usize::from(*es.get(at)?);
                        }
                        if flags & 0x20 != 0 {
                            at += 2;
                        }
                        descriptor(es.get(at..)?, 0x04)
                    })
                    .and_then(|dc| descriptor(dc.get(13..)?, 0x05))?;
                // AudioSpecificConfig: object type (5 bits), frequency index (4), with index 15
                // an explicit frequency (24), then the channel configuration (4).
                let mut bytes = [0u8; 8];
                for (i, b) in config.iter().take(5).enumerate() {
                    bytes[3 + i] = *b;
                }
                let bits = u64::from_be_bytes(bytes);
                if bits >> 35 == 31 {
                    return None;
                }
                let freq_index = (bits >> 31) & 0xF;
                let channel_config = if freq_index == 15 { (bits >> 3) & 0xF } else { (bits >> 27) & 0xF } as u32;
                match channel_config {
                    0 => None,
                    7 => Some(8),
                    n => Some(n),
                }
            }
            b"ac-3" => {
                let d = boxes.iter().find(|(k, _)| k == b"dac3")?.1;
                let v = u32::from_be_bytes([0, *d.first()?, *d.get(1)?, *d.get(2)?]);
                let acmod = (v >> 11) & 7;
                let lfe = (v >> 10) & 1;
                Some([2, 1, 2, 3, 3, 4, 4, 5][acmod as usize] + lfe)
            }
            _ => None,
        }
    }

    /// Content of the first MPEG-4 descriptor with `tag` in `data`.
    fn descriptor(data: &[u8], tag: u8) -> Option<&[u8]> {
        let mut at = 0;
        while at < data.len() {
            let this = data[at];
            at += 1;
            let mut len = 0usize;
            for _ in 0..4 {
                let b = *data.get(at)?;
                at += 1;
                len = (len << 7) | usize::from(b & 0x7F);
                if b & 0x80 == 0 {
                    break;
                }
            }
            let body = data.get(at..at + len)?;
            if this == tag {
                return Some(body);
            }
            at += len;
        }
        None
    }

    const VIDEO_CODECS: &[(&[u8; 4], &str)] = &[
        (b"avc1", "H.264"), (b"avc3", "H.264"), (b"hvc1", "HEVC"), (b"hev1", "HEVC"), (b"dvh1", "HEVC"),
        (b"dvhe", "HEVC"), (b"av01", "AV1"), (b"vp09", "VP9"), (b"mp4v", "MPEG-4"),
    ];
    const AUDIO_CODECS: &[(&[u8; 4], &str)] = &[
        (b"mp4a", "AAC"), (b"ac-3", "AC3"), (b"ec-3", "E-AC3"), (b"dtsc", "DTS"), (b"dtsh", "DTS"),
        (b"dtsl", "DTS"), (b"dtse", "DTS"), (b"Opus", "Opus"), (b"fLaC", "FLAC"), (b".mp3", "MP3"),
        (b"alac", "ALAC"),
    ];
    const SUBTITLE_CODECS: &[(&[u8; 4], &str)] = &[(b"tx3g", "Text"), (b"wvtt", "WebVTT"), (b"stpp", "TTML"), (b"mp4s", "VobSub")];

    /// Name of a codec by its sample entry type; unknown ones by the type itself.
    fn codec(kind: &[u8; 4], table: &[(&[u8; 4], &str)]) -> String {
        table
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or_else(|| String::from_utf8_lossy(kind).trim().to_string(), |(_, name)| name.to_string())
    }

    fn add_track(media: &mut MediaInfo, track: Track) {
        let Some((kind, entry)) = track.sample else {
            return;
        };
        let language = track.elng.or(track.language);
        // Generic handler names written by muxers say nothing.
        let name = if ["SoundHandler", "VideoHandler", "SubtitleHandler", "TextHandler", "Core Media Audio", "Core Media Video"]
            .iter()
            .any(|generic| track.name.starts_with(generic))
            || track.name.to_lowercase().contains("handler")
        {
            String::new()
        } else {
            track.name
        };
        match &track.handler {
            b"vide" if media.video.is_none() => {
                // Visual sample entry: 78 bytes of fields, then child boxes.
                let width = be16(&entry, 24);
                let height = be16(&entry, 26);
                let boxes = children(entry.get(78..).unwrap_or_default());
                let dolby_vision = matches!(&kind, b"dvh1" | b"dvhe" | b"dva1" | b"dvav")
                    || boxes.iter().any(|(k, _)| matches!(k, b"dvcC" | b"dvvC"));
                let transfer = boxes
                    .iter()
                    .find(|(k, d)| k == b"colr" && d.get(..4) == Some(b"nclx"))
                    .map(|(_, d)| be16(d, 6));
                let hdr = if dolby_vision {
                    Some("Dolby Vision")
                } else {
                    match transfer {
                        Some(16) => Some("HDR10"),
                        Some(18) => Some("HLG"),
                        _ => None,
                    }
                };
                // HEVC: bit depth from the configuration record.
                let bit_depth = boxes
                    .iter()
                    .find(|(k, _)| k == b"hvcC")
                    .and_then(|(_, d)| d.get(18))
                    .map_or(0, |b| u32::from(b & 7) + 8);
                media.video = Some(VideoInfo {
                    codec: codec(&kind, VIDEO_CODECS),
                    width,
                    height,
                    hdr: hdr.map(String::from),
                    bit_depth,
                });
            }
            // Disabled (alternate) tracks are listed too, VLC numbers them as well.
            b"soun" => media.audio.push(AudioTrack {
                codec: codec(&kind, AUDIO_CODECS),
                language,
                channels: real_channels(&kind, &entry).unwrap_or_else(|| be16(&entry, 16).max(1)),
                name,
                default: track.enabled,
            }),
            b"sbtl" | b"text" | b"subt" | b"subp" => media.subtitles.push(SubtitleTrack {
                codec: codec(&kind, SUBTITLE_CODECS),
                language,
                name,
                default: false,
                forced: false,
            }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ----- builders for synthetic files -----

    fn vint_size(size: usize) -> Vec<u8> {
        let mut out = vec![0x01, 0, 0, 0, 0, 0, 0, 0];
        out[1..].copy_from_slice(&(size as u64).to_be_bytes()[1..]);
        out
    }

    fn el(id: u32, data: &[u8]) -> Vec<u8> {
        let id_bytes = id.to_be_bytes();
        let skip = id_bytes.iter().position(|b| *b != 0).unwrap_or(3);
        let mut out = id_bytes[skip..].to_vec();
        out.extend(vint_size(data.len()));
        out.extend_from_slice(data);
        out
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    fn mkv_track(kind: u8, codec: &str, lang: Option<&str>, extra: Vec<u8>) -> Vec<u8> {
        let mut fields = vec![el(0x83, &[kind]), el(0x86, codec.as_bytes())];
        if let Some(lang) = lang {
            fields.push(el(0x22B59C, lang.as_bytes()));
        }
        fields.push(extra);
        el(0xAE, &cat(&fields))
    }

    #[test]
    fn reads_matroska_tracks() {
        let video = el(0xE0, &cat(&[
            el(0xB0, &3840u16.to_be_bytes()),
            el(0xBA, &1606u16.to_be_bytes()),
            el(0x55B0, &cat(&[el(0x55B2, &[10]), el(0x55BA, &[16])])),
        ]));
        let tracks = el(0x1654AE6B, &cat(&[
            mkv_track(1, "V_MPEGH/ISO/HEVC", None, video),
            mkv_track(2, "A_EAC3", Some("cze"), cat(&[el(0xE1, &el(0x9F, &[6])), el(0x536E, b"CZ dabing")])),
            mkv_track(2, "A_AAC", None, cat(&[el(0x88, &[0]), el(0xE1, &el(0x9F, &[2]))])),
            mkv_track(17, "S_TEXT/UTF8", Some("cze"), el(0x55AA, &[1])),
            mkv_track(17, "S_HDMV/PGS", Some("und"), Vec::new()),
        ]));
        let info = el(0x1549A966, &cat(&[el(0x2AD7B1, &1_000_000u32.to_be_bytes()), el(0x4489, &7_200_000f64.to_be_bytes())]));
        let file = cat(&[el(0x1A45DFA3, &el(0x4282, b"matroska")), el(0x18538067, &cat(&[info, tracks, el(0x1F43B675, &[0; 16])]))]);

        let media = mkv::probe(&mut Cursor::new(file)).unwrap();
        assert_eq!(media.duration, 7200.0);
        let video = media.video.unwrap();
        assert_eq!((video.codec.as_str(), video.width, video.height, video.bit_depth), ("HEVC", 3840, 1606, 10));
        assert_eq!(video.hdr.as_deref(), Some("HDR10"));
        assert_eq!(media.audio.len(), 2);
        assert_eq!((media.audio[0].language.as_deref(), media.audio[0].channels, media.audio[0].codec.as_str()), (Some("cs"), 6, "E-AC3"));
        assert_eq!(media.audio[0].name, "CZ dabing");
        // No Language element = English; FlagDefault 0.
        assert_eq!((media.audio[1].language.as_deref(), media.audio[1].default), (Some("en"), false));
        assert_eq!(media.subtitles.len(), 2);
        assert!(media.subtitles[0].forced);
        assert_eq!(media.subtitles[0].language.as_deref(), Some("cs"));
        assert_eq!((media.subtitles[1].language.as_deref(), media.subtitles[1].codec.as_str()), (None, "PGS"));
    }

    fn mp4_box(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = ((data.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out
    }

    fn mp4_trak(handler: &[u8; 4], lang: &str, entry: Vec<u8>) -> Vec<u8> {
        let l = lang.as_bytes();
        let packed = ((u16::from(l[0]) - 0x60) << 10) | ((u16::from(l[1]) - 0x60) << 5) | (u16::from(l[2]) - 0x60);
        let mut mdhd = vec![0u8; 24];
        mdhd[20..22].copy_from_slice(&packed.to_be_bytes());
        let mut hdlr = vec![0u8; 24];
        hdlr[8..12].copy_from_slice(handler);
        let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stsd.extend(entry);
        let stbl = mp4_box(b"stbl", &cat(&[mp4_box(b"stsd", &stsd), mp4_box(b"stsz", &[0; 64])]));
        let mdia = mp4_box(b"mdia", &cat(&[mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr), mp4_box(b"minf", &stbl)]));
        mp4_box(b"trak", &cat(&[mp4_box(b"tkhd", &[0, 0, 0, 1]), mdia]))
    }

    #[test]
    fn reads_mp4_tracks_after_media_data() {
        let mut visual = vec![0u8; 78];
        visual[24..26].copy_from_slice(&1920u16.to_be_bytes());
        visual[26..28].copy_from_slice(&800u16.to_be_bytes());
        let mut colr = b"nclx".to_vec();
        colr.extend([0, 9, 0, 18, 0, 9, 0]);
        visual.extend(mp4_box(b"colr", &colr));
        let mut sound = vec![0u8; 28];
        sound[16..18].copy_from_slice(&6u16.to_be_bytes());
        let mut mvhd = vec![0u8; 20];
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[16..20].copy_from_slice(&5_400_000u32.to_be_bytes());
        let moov = mp4_box(b"moov", &cat(&[
            mp4_box(b"mvhd", &mvhd),
            mp4_trak(b"vide", "und", mp4_box(b"avc1", &visual)),
            mp4_trak(b"soun", "eng", mp4_box(b"mp4a", &sound)),
            mp4_trak(b"soun", "ces", mp4_box(b"ac-3", &sound)),
            mp4_trak(b"sbtl", "ces", mp4_box(b"tx3g", &[0; 40])),
        ]));
        let file = cat(&[mp4_box(b"ftyp", b"isom\0\0\0\0"), mp4_box(b"mdat", &[0; 1000]), moov]);

        let media = mp4::probe(&mut Cursor::new(file)).unwrap();
        assert_eq!(media.duration, 5400.0);
        let video = media.video.unwrap();
        assert_eq!((video.codec.as_str(), video.width, video.height), ("H.264", 1920, 800));
        assert_eq!(video.hdr.as_deref(), Some("HLG"));
        let langs: Vec<_> = media.audio.iter().map(|a| a.language.clone().unwrap()).collect();
        assert_eq!(langs, ["en", "cs"]);
        assert_eq!((media.audio[1].codec.as_str(), media.audio[1].channels), ("AC3", 6));
        assert_eq!(media.subtitles[0].language.as_deref(), Some("cs"));
    }

    #[test]
    fn normalizes_languages() {
        assert_eq!(normalize_language("cze").as_deref(), Some("cs"));
        assert_eq!(normalize_language("ces").as_deref(), Some("cs"));
        assert_eq!(normalize_language("cs-CZ").as_deref(), Some("cs"));
        assert_eq!(normalize_language("und"), None);
        assert_eq!(normalize_language("tlh").as_deref(), Some("tlh"));
    }

    #[test]
    fn tags_from_release_names() {
        let t = tags_from_name("Pelisky.1999.2160p.HDR.x265.CZ.dabing.5.1.mkv");
        assert_eq!((t.resolution, t.video_codec.as_deref(), t.hdr.as_deref(), t.channels), (Some("4K"), Some("HEVC"), Some("HDR10"), 6));
        assert_eq!(t.audio_languages, ["cs"]);
        let t = tags_from_name("Matrix.1999.1080p.x264.CZ.titulky.mkv");
        assert_eq!(t.subtitle_languages, ["cs"]);
        assert!(t.audio_languages.is_empty());
        // Headers override the name, external subtitles are added.
        let media = MediaInfo {
            video: Some(VideoInfo { codec: "HEVC".into(), width: 1920, height: 1080, ..Default::default() }),
            audio: vec![AudioTrack { language: Some("en".into()), channels: 2, ..Default::default() }],
            ..Default::default()
        };
        let external = [ExternalSubtitle { path: PathBuf::new(), file_name: "a.srt".into(), language: Some("cs".into()), forced: false }];
        let t = tags(Some(&media), &external, "Film.2160p.mkv");
        assert_eq!((t.resolution, t.audio_languages.clone(), t.subtitle_languages.clone(), t.has_subtitles), (Some("1080p"), vec!["en".to_string()], vec!["cs".to_string()], true));
    }

    #[test]
    fn finds_external_subtitles() {
        let dir = std::env::temp_dir().join(format!("filmoteka-subs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Subs")).unwrap();
        for name in ["Film.2010.mkv", "Film.2010.cs.srt", "Film.2010.forced.en.srt", "Film.2010.CZE.ass", "Subs/Czech.srt"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let subs = find_external_subtitles(&dir.join("Film.2010.mkv"), 1);
        let summary: Vec<_> = subs.iter().map(|s| (s.file_name.as_str(), s.language.as_deref(), s.forced)).collect();
        assert_eq!(summary, [
            ("Czech.srt", Some("cs"), false),
            ("Film.2010.CZE.ass", Some("cs"), false),
            ("Film.2010.cs.srt", Some("cs"), false),
            ("Film.2010.forced.en.srt", Some("en"), true),
        ]);
        // With more videos in the folder only subtitles named after this video count.
        assert_eq!(find_external_subtitles(&dir.join("Film.2010.mkv"), 2).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

