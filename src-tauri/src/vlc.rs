//! Launching VLC and tracking the playback position.
//!
//! VLC is started with its HTTP interface bound to localhost on a random port with a random
//! password. A background thread polls `/requests/status.json` every second, stores the
//! position periodically and once more when VLC exits.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use tauri::AppHandle;

use crate::db::Settings;
use crate::playback;
use crate::tracks::Resolved;

const DEFAULT_PATHS: &[&str] = &[
    r"C:\Program Files\VideoLAN\VLC\vlc.exe",
    r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
];

const SAVE_INTERVAL: Duration = Duration::from_secs(5);

struct Status {
    time: f64,
    length: f64,
    state: String,
}

pub fn detect() -> Option<PathBuf> {
    DEFAULT_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .or_else(registry_lookup)
}

/// Custom path from settings if it still exists, otherwise auto-detection.
pub fn resolve(custom: Option<&str>) -> Option<PathBuf> {
    custom
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(detect)
}

fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn registry_lookup() -> Option<PathBuf> {
    for key in [
        r"HKLM\SOFTWARE\VideoLAN\VLC",
        r"HKLM\SOFTWARE\WOW6432Node\VideoLAN\VLC",
    ] {
        for args in [&["/ve"][..], &["/v", "InstallDir"][..]] {
            let mut cmd = Command::new("reg");
            cmd.arg("query").arg(key).args(args);
            hide_console(&mut cmd);
            let Ok(output) = cmd.output() else {
                continue;
            };
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some((_, value)) = line.split_once("REG_SZ") {
                    let mut path = PathBuf::from(value.trim());
                    if path.is_dir() {
                        path.push("vlc.exe");
                    }
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

/// Player shown in the settings: the VLC executable in use.
pub fn detected(_app: &AppHandle) -> Option<String> {
    detect().map(|p| p.to_string_lossy().to_string())
}

/// Plays `movie` from `start` seconds with the chosen tracks and tracks the position in the
/// background.
pub fn play(
    app: AppHandle,
    settings: &Settings,
    id: u64,
    movie: &Path,
    start: f64,
    _title: &str,
    tracks: &Resolved,
) -> Result<(), String> {
    let vlc = resolve(settings.vlc_path.as_deref())
        .ok_or_else(|| tr!(
            "VLC nebyl nalezen. Nainstalujte VLC nebo nastavte cestu k vlc.exe v Nastavení.",
            "VLC was not found. Install VLC or set the path to vlc.exe in the Settings."
        ))?;
    launch(app, &vlc, id, movie, start, tracks)
}

fn launch(app: AppHandle, vlc: &Path, id: u64, movie: &Path, start: f64, tracks: &Resolved) -> Result<(), String> {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| tr!("Nelze najít volný port: {e}", "No free port found: {e}"))?
        .port();
    let password = format!("{:016x}", RandomState::new().build_hasher().finish());
    let auth = base64::engine::general_purpose::STANDARD.encode(format!(":{password}"));

    let mut cmd = Command::new(vlc);
    cmd.args([
        "--no-one-instance",
        "--play-and-exit",
        "--qt-continue=0",
        "--extraintf=http",
        "--http-host=127.0.0.1",
    ])
    .arg(format!("--http-port={port}"))
    .arg(format!("--http-password={password}"));
    if start > 0.0 {
        cmd.arg(format!("--start-time={start:.0}"));
    }
    cmd.args(tracks.vlc_args());
    cmd.arg(movie);

    let child = cmd
        .spawn()
        .map_err(|e| tr!("Nepodařilo se spustit VLC: {e}", "VLC could not be started: {e}"))?;
    thread::spawn(move || monitor(app, child, id, port, auth));
    Ok(())
}

fn monitor(app: AppHandle, mut child: Child, id: u64, port: u16, auth: String) {
    let mut last_time: Option<f64> = None;
    let mut length = 0.0;
    let mut last_save = Instant::now();
    let mut dirty = false;

    loop {
        thread::sleep(Duration::from_secs(1));
        if !matches!(child.try_wait(), Ok(None)) {
            break;
        }
        let Some(status) = query_status(port, &auth) else {
            continue;
        };
        if status.length > 0.0 {
            length = status.length;
        }
        // After "stop" VLC reports time 0 – keep the last real position instead.
        if (status.state == "playing" || status.state == "paused")
            && status.time > 0.0
            && last_time != Some(status.time)
        {
            last_time = Some(status.time);
            dirty = true;
        }
        if dirty && last_save.elapsed() >= SAVE_INTERVAL {
            playback::record(&app, id, last_time, length, false, false);
            last_save = Instant::now();
            dirty = false;
        }
    }
    playback::record(&app, id, last_time, length, true, false);
}

/// Creates thumbnails (frames from the video) with a hidden VLC instance.
pub struct Thumbnailer(PathBuf);

impl Thumbnailer {
    /// None when VLC is not available.
    pub fn new(_app: &AppHandle, settings: &Settings) -> Option<Thumbnailer> {
        resolve(settings.vlc_path.as_deref()).map(Thumbnailer)
    }

    /// Saves a frame of the movie as JPEG `dir/name`.
    pub fn snapshot(&self, movie: &Path, dir: &Path, name: &str) -> bool {
        snapshot(&self.0, movie, dir, name)
    }
}

/// Saves a frame of the movie as JPEG `dir/name` using a hidden VLC instance.
/// Tries a few positions because short videos end before the first one.
fn snapshot(vlc: &Path, movie: &Path, dir: &Path, name: &str) -> bool {
    let target = dir.join(name);
    let prefix = name.trim_end_matches(".jpg");
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    for start in [420, 90, 5] {
        let mut cmd = Command::new(vlc);
        cmd.args([
            "--intf=dummy",
            "--dummy-quiet",
            "--vout=dummy",
            "--no-audio",
            "--no-one-instance",
            "--play-and-exit",
            "--video-filter=scene",
            "--scene-format=jpg",
            "--scene-width=640",
            "--scene-ratio=10",
            "--scene-replace",
            "--run-time=1",
        ])
        .arg(format!("--scene-prefix={prefix}"))
        .arg(format!("--scene-path={}", dir.display()))
        .arg(format!("--start-time={start}"))
        .arg(movie)
        .arg("vlc://quit");
        hide_console(&mut cmd);
        let Ok(mut child) = cmd.spawn() else {
            return false;
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match child.try_wait() {
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
                Ok(Some(_)) => break,
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
        if target.is_file() {
            return true;
        }
    }
    false
}

fn query_status(port: u16, auth: &str) -> Option<Status> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let request = format!(
        "GET /requests/status.json HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\
         Authorization: Basic {auth}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    // A timeout still leaves whatever was received in `raw`.
    let _ = stream.read_to_end(&mut raw);

    let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
    if !head.lines().next()?.contains(" 200") {
        return None;
    }
    let body = &raw[header_end + 4..];
    let body = if head.contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        body.to_vec()
    };

    let json: serde_json::Value = serde_json::from_slice(&body).ok()?;
    Some(Status {
        time: number(&json["time"]),
        length: number(&json["length"]),
        state: json["state"].as_str().unwrap_or_default().to_string(),
    })
}

fn number(value: &serde_json::Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(line_end) = body.windows(2).position(|w| w == b"\r\n") {
        let size_line = String::from_utf8_lossy(&body[..line_end]);
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        let data = &body[line_end + 2..];
        if size == 0 || data.len() < size {
            break;
        }
        out.extend_from_slice(&data[..size]);
        body = data[size..].strip_prefix(b"\r\n").unwrap_or(&data[size..]);
    }
    out
}
