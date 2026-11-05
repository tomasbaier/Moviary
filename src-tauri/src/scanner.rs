//! Recursive search for video files, file fingerprints and title/year extraction from file names.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "wmv", "m4v", "mpg", "mpeg", "ts", "m2ts", "mts", "webm", "flv",
    "vob", "ogv", "3gp", "divx", "rmvb",
];

/// Release tags that mark the end of the actual title in a file name.
const JUNK_TOKENS: &[&str] = &[
    "480p", "576p", "720p", "1080p", "1080i", "2160p", "4k", "uhd", "hdr", "hdr10", "bluray",
    "blu-ray", "brrip", "bdrip", "bdremux", "remux", "webrip", "web-dl", "webdl", "hdrip",
    "dvdrip", "dvdscr", "dvd", "hdtv", "tvrip", "camrip", "x264", "x265", "h264", "h265", "hevc",
    "avc", "xvid", "divx", "aac", "ac3", "dts", "ddp5", "dd5", "atmos", "truehd", "10bit",
    "proper", "repack", "extended", "unrated", "remastered", "cz", "czdab", "czsub", "cztit",
    "dabing", "titulky", "multi", "multisub", "eng",
];

pub struct ScannedFile {
    pub path: PathBuf,
    pub size: u64,
}

pub fn scan(root: &Path) -> Result<Vec<ScannedFile>, String> {
    if !root.is_dir() {
        return Err(format!(
            "Složka „{}“ neexistuje nebo není dostupná.",
            root.display()
        ));
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    Ok(out)
}

/// Folders that are never searched (hidden and system folders).
pub fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || name.eq_ignore_ascii_case("System Volume Information")
}

/// A video file worth listing (samples are left out).
pub fn is_video(name: &str, size: u64) -> bool {
    let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) else {
        return false;
    };
    VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        && !(name.to_lowercase().contains("sample") && size < 300 * 1024 * 1024)
}

fn walk(dir: &Path, out: &mut Vec<ScannedFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if file_type.is_dir() {
            if !skip_dir(&name) {
                walk(&path, out);
            }
            continue;
        }
        if !(file_type.is_file() || (file_type.is_symlink() && path.is_file())) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if is_video(&name, size) {
            out.push(ScannedFile { path, size });
        }
    }
}

/// Identifies a file by its size and a hash of three 64 KiB samples (start, middle, end),
/// so it can be recognised after being renamed or moved. Cheap even for huge files.
pub fn fingerprint(mut file: File, size: u64) -> Option<String> {
    const SAMPLE: u64 = 64 * 1024;
    let samples: Vec<(u64, u64)> = if size <= 3 * SAMPLE {
        vec![(0, size)]
    } else {
        vec![(0, SAMPLE), (size / 2, SAMPLE), (size - SAMPLE, SAMPLE)]
    };

    // FNV-1a: stable across Rust versions, unlike std's DefaultHasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut buf = vec![0u8; (3 * SAMPLE) as usize];
    for (offset, len) in samples {
        file.seek(SeekFrom::Start(offset)).ok()?;
        let chunk = &mut buf[..len as usize];
        file.read_exact(chunk).ok()?;
        for byte in chunk.iter() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    Some(format!("{size:x}-{hash:016x}"))
}

fn strip_brackets(token: &str) -> &str {
    token.trim_matches(|c| "()[]{}".contains(c))
}

fn as_year(token: &str) -> Option<u32> {
    let t = strip_brackets(token);
    if t.len() != 4 {
        return None;
    }
    t.parse().ok().filter(|y| (1900..=2099).contains(y))
}

fn is_junk(token: &str) -> bool {
    let t = strip_brackets(token).to_lowercase();
    JUNK_TOKENS.contains(&t.as_str())
        || t.split('-')
            .next()
            .is_some_and(|head| head.len() > 1 && JUNK_TOKENS.contains(&head))
}

/// "Blade.Runner.2049.(2017).1080p.BluRay.x264" -> ("Blade Runner 2049", Some(2017))
pub fn parse_title(stem: &str) -> (String, Option<u32>) {
    let cleaned = stem.replace(['.', '_'], " ");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();

    let junk_at = tokens
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, t)| is_junk(t))
        .map(|(i, _)| i)
        .unwrap_or(tokens.len());
    // The last year-like token before the junk is the release year (the first token
    // is never a year, so titles like "1917" or "2001 ..." survive).
    let year_at = (1..junk_at).rev().find(|&i| as_year(tokens[i]).is_some());

    let end = year_at.unwrap_or(junk_at);
    let title = tokens[..end]
        .join(" ")
        .trim_matches(|c: char| c == '-' || c.is_whitespace())
        .to_string();
    let title = if title.is_empty() {
        stem.to_string()
    } else {
        title
    };
    (title, year_at.and_then(|i| as_year(tokens[i])))
}

// ---------- TV series ----------

const SEASON_WORDS: &[&str] = &[
    "season", "sezona", "sezóna", "serie", "série", "series", "řada", "rada", "staffel", "saison",
    "temporada",
];
const SPECIALS_WORDS: &[&str] = &["specials", "special", "speciály", "specialy"];
/// Words in front of an episode number in files inside a season folder ("Díl 5", "Ep 05").
const EPISODE_WORDS: &[&str] = &["episode", "epizoda", "díl", "dil", "část", "cast", "part", "ep"];

/// An episode recognised from its path, see [`parse_episode`].
#[derive(Debug, PartialEq)]
pub struct EpisodeName {
    pub show: String,
    pub year: Option<u32>,
    /// 0 = specials.
    pub season: u32,
    pub episode: u32,
    /// Last episode of a multi-episode file ("S01E01E02").
    pub last_episode: Option<u32>,
    pub title: Option<String>,
}

/// Position of the episode number in a file name (char indices).
struct Marker {
    start: usize,
    end: usize,
    season: u32,
    episode: u32,
    last: Option<u32>,
}

/// Reads 1..=`max_len` digits at `at`; a longer number is no match.
fn read_number(chars: &[char], at: usize, max_len: usize) -> Option<(u32, usize)> {
    let len = chars.get(at..)?.iter().take_while(|c| c.is_ascii_digit()).count();
    if len == 0 || len > max_len {
        return None;
    }
    let n = chars[at..at + len].iter().collect::<String>().parse().ok()?;
    Some((n, at + len))
}

fn word_ends(chars: &[char], at: usize) -> bool {
    chars.get(at).is_none_or(|c| !c.is_alphanumeric())
}

/// "S01E02", "s1e2", "S01 E02", "S01E01E02", "S01E01-02".
fn season_episode(lower: &[char], start: usize) -> Option<Marker> {
    if lower[start] != 's' {
        return None;
    }
    let (season, mut i) = read_number(lower, start + 1, 2)?;
    if matches!(lower.get(i), Some(' ' | '-')) {
        i += 1;
    }
    if lower.get(i) != Some(&'e') {
        return None;
    }
    let (episode, mut end) = read_number(lower, i + 1, 3)?;
    let mut last = None;
    loop {
        let mut j = end;
        if lower.get(j) == Some(&'-') {
            j += 1;
        }
        if lower.get(j) == Some(&'e') {
            j += 1;
        } else if j == end {
            break;
        }
        match read_number(lower, j, 3) {
            Some((n, e)) if n > last.unwrap_or(episode) => {
                last = Some(n);
                end = e;
            }
            _ => break,
        }
    }
    Some(Marker { start, end, season, episode, last })
}

/// "1x02" (but not "2x4" or "1920x1080").
fn cross(lower: &[char], start: usize) -> Option<Marker> {
    let (season, i) = read_number(lower, start, 2)?;
    if lower.get(i) != Some(&'x') {
        return None;
    }
    let (episode, end) = read_number(lower, i + 1, 3)?;
    (end - i > 2).then_some(Marker { start, end, season, episode, last: None })
}

/// "E05", "Ep 05", "Epizoda 5", "Díl 5" or a leading number ("05 - Pilot"); the season comes
/// from the folder.
fn episode_number(lower: &[char], start: usize) -> Option<Marker> {
    let leading = lower[..start].iter().all(|c| c.is_whitespace());
    let number_at = if leading && lower[start].is_ascii_digit() {
        Some(start)
    } else if lower[start] == 'e' && lower.get(start + 1).is_some_and(|c| c.is_ascii_digit()) {
        Some(start + 1)
    } else {
        EPISODE_WORDS.iter().find_map(|word| {
            let word: Vec<char> = word.chars().collect();
            let after = start + word.len();
            if lower.get(start..after) != Some(&word[..]) {
                return None;
            }
            Some(if lower.get(after) == Some(&' ') { after + 1 } else { after })
        })
    };
    let (episode, end) = read_number(lower, number_at?, 3)?;
    Some(Marker { start, end, season: 0, episode, last: None })
}

fn find_marker(lower: &[char], folder_season: Option<u32>) -> Option<Marker> {
    let starts = || (0..lower.len()).filter(|&i| i == 0 || !lower[i - 1].is_alphanumeric());
    let found = starts()
        .find_map(|i| season_episode(lower, i).or_else(|| cross(lower, i)).filter(|m| word_ends(lower, m.end)));
    found.or_else(|| {
        let season = folder_season?;
        starts()
            .find_map(|i| episode_number(lower, i).filter(|m| word_ends(lower, m.end)))
            .map(|m| Marker { season, ..m })
    })
}

/// "Season 1", "Sezóna 01", "Série 2", "2. řada", "S01", "Specials" -> season number.
pub fn season_folder(name: &str) -> Option<u32> {
    let lower = name.to_lowercase().replace(['.', '_', '-'], " ");
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let number = |t: &str| {
        (t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit()))
            .then(|| t.parse().ok())
            .flatten()
    };
    match tokens.as_slice() {
        [word, n, ..] if SEASON_WORDS.contains(word) => number(n),
        [n, word, ..] if SEASON_WORDS.contains(word) => number(n),
        [word, ..] if SPECIALS_WORDS.contains(word) => Some(0),
        [token, ..] => token.strip_prefix('s').and_then(number),
        [] => None,
    }
}

/// Name used to group the episodes of one show: "The.Office.(US)" equals "the office us".
pub fn show_key(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '’'))
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// " - Pilot [720p]" -> "Pilot"
fn episode_title(text: &str) -> Option<String> {
    let words: Vec<&str> = text
        .split_whitespace()
        .take_while(|t| !is_junk(t) && as_year(t).is_none())
        .collect();
    let title = words
        .join(" ")
        .trim_matches(|c: char| c == '-' || c.is_whitespace())
        .to_string();
    (!title.is_empty()).then_some(title)
}

/// Recognises an episode from its path relative to the library folder:
/// "Show.Name.S01E02.Title.mkv", "Show - 1x02.avi" or "Show/Season 1/02 - Title.mkv".
/// The show name comes from the file name, or from the folder when the name has none.
pub fn parse_episode(rel: &str) -> Option<EpisodeName> {
    let mut dirs: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    let file = dirs.pop()?;
    let stem = Path::new(file).file_stem()?.to_string_lossy().replace(['.', '_'], " ");
    let chars: Vec<char> = stem.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let folder_season = dirs.last().and_then(|d| season_folder(d));
    let marker = find_marker(&lower, folder_season)?;

    let folder = dirs
        .iter()
        .rev()
        .find(|d| season_folder(d).is_none())
        .map(|d| parse_title(d));
    let prefix: String = chars[..marker.start].iter().collect();
    let prefix = prefix.trim_matches(|c: char| c == '-' || c.is_whitespace());
    let (show, year) = match (prefix, folder) {
        ("", Some(folder)) => folder,
        ("", None) => ("Neznámý seriál".to_string(), None),
        (prefix, folder) => {
            let parsed = parse_title(prefix);
            // Prefer the folder's spelling ("The Office (US)" over "the office us").
            match folder {
                Some(folder) if show_key(&folder.0) == show_key(&parsed.0) => {
                    (folder.0, parsed.1.or(folder.1))
                }
                _ => parsed,
            }
        }
    };
    let suffix: String = chars[marker.end..].iter().collect();
    Some(EpisodeName {
        show,
        year,
        season: marker.season,
        episode: marker.episode,
        last_episode: marker.last,
        title: episode_title(&suffix),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_episode, parse_title, season_folder, show_key, EpisodeName};

    fn ep(show: &str, season: u32, episode: u32, title: Option<&str>) -> Option<EpisodeName> {
        Some(EpisodeName {
            show: show.into(),
            year: None,
            season,
            episode,
            last_episode: None,
            title: title.map(String::from),
        })
    }

    #[test]
    fn parses_episodes() {
        assert_eq!(
            parse_episode("Breaking.Bad.S01E02.Cat.in.the.Bag.720p.BluRay.x264.mkv"),
            ep("Breaking Bad", 1, 2, Some("Cat in the Bag"))
        );
        assert_eq!(
            parse_episode("Seriály/Přátelé/Season 3/Přátelé - 3x05 - Ten s malířem [CZ].avi"),
            ep("Přátelé", 3, 5, Some("Ten s malířem"))
        );
        assert_eq!(
            parse_episode("The Office (US)/Sezóna 2/the.office.us.s02e01.hdtv.mkv"),
            ep("The Office (US)", 2, 1, None)
        );
        assert_eq!(
            parse_episode("Chernobyl/S01/01 - 1.23.45.mkv"),
            ep("Chernobyl", 1, 1, Some("1 23 45"))
        );
        assert_eq!(parse_episode("Most!/2. řada/Díl 3.mp4"), ep("Most!", 2, 3, None));
        assert_eq!(parse_episode("Simpsonovi/Speciály/E01.mkv"), ep("Simpsonovi", 0, 1, None));

        let double = parse_episode("Lost.S01E01E02.Pilot.mkv").unwrap();
        assert_eq!((double.episode, double.last_episode), (1, Some(2)));
        let with_year = parse_episode("Doctor.Who.2005.S01E01.Rose.mkv").unwrap();
        assert_eq!((with_year.show.as_str(), with_year.year), ("Doctor Who", Some(2005)));

        // Movies stay movies.
        assert_eq!(parse_episode("Filmy/Blade.Runner.2049.(2017).1080p.BluRay.x264.mkv"), None);
        assert_eq!(parse_episode("Filmy/1917.2019.1080p.mkv"), None);
        assert_eq!(parse_episode("Video 1920x1080.mp4"), None);
        assert_eq!(parse_episode("Filmy/12 Angry Men (1957).mkv"), None);
    }

    #[test]
    fn recognises_season_folders() {
        assert_eq!(season_folder("Season 01"), Some(1));
        assert_eq!(season_folder("Sezóna 2"), Some(2));
        assert_eq!(season_folder("3. série"), Some(3));
        assert_eq!(season_folder("S04"), Some(4));
        assert_eq!(season_folder("Specials"), Some(0));
        assert_eq!(season_folder("Sci-fi"), None);
        assert_eq!(season_folder("Seriály"), None);
        assert_eq!(show_key("Grey's.Anatomy"), show_key("greys anatomy"));
    }

    #[test]
    fn parses_titles() {
        assert_eq!(
            parse_title("Blade.Runner.2049.(2017).1080p.BluRay.x264"),
            ("Blade Runner 2049".into(), Some(2017))
        );
        assert_eq!(parse_title("1917.2019.1080p"), ("1917".into(), Some(2019)));
        assert_eq!(
            parse_title("Pelíšky (1999) CZ"),
            ("Pelíšky".into(), Some(1999))
        );
        assert_eq!(parse_title("Matrix"), ("Matrix".into(), None));
        assert_eq!(
            parse_title("The_Big_Lebowski_720p_x264-GRP"),
            ("The Big Lebowski".into(), None)
        );
    }
}
