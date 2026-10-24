use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, OnceLock,
};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use colored::Colorize;
use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::config::Config;
use crate::ext::{self, idlix, otakudesu, CHROME_USER_AGENT};
use crate::models::{clean_episode_title, clean_movie_title, Episode, Movie, Stream};
use crate::util;

const PROGRESS_BAR_WIDTH: usize = 24;
const MAX_FILENAME_CHARS: usize = 160;
const MAX_FILENAME_BYTES: usize = 200;
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "m4v", "webm", "ts", "mov", "avi"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    Completed(PathBuf),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadResult {
    Completed,
    Cancelled,
}

/// Snapshot emitted while ffmpeg is downloading. `total_size` is the output
/// size reported by ffmpeg; the batch list computes transfer speed from the
/// delta between snapshots.
#[derive(Debug, Clone, Default)]
pub struct DownloadProgress {
    pub duration: Option<f64>,
    pub out_seconds: f64,
    pub total_size: u64,
    pub speed: String,
    pub finished: bool,
    pub paused: bool,
}

pub type DownloadProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingFilePolicy {
    Skip,
    Overwrite,
}

impl std::fmt::Display for ExistingFilePolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skip => write!(formatter, "Lewati file yang sudah ada"),
            Self::Overwrite => write!(formatter, "Timpa file yang sudah ada"),
        }
    }
}

/// Make sure ffmpeg is available before opening the destination prompts.
pub fn ensure_available() -> Result<()> {
    if crate::check_command_exists("ffmpeg") {
        return Ok(());
    }

    anyhow::bail!(
        "ffmpeg tidak ditemukan di sistem.\n\
         Download episode memerlukan ffmpeg. Silakan install:\n\
         - Linux  : sudo apt install ffmpeg  /  sudo pacman -S ffmpeg  /  pkg install ffmpeg (Termux)\n\
         - macOS  : brew install ffmpeg\n\
         - Windows: winget install --id Gyan.FFmpeg --source winget\n\
         Setelah install, jalankan ulang animeku-cli."
    )
}

/// Default destination: the platform Downloads folder with an Animeku-CLI
/// subfolder. The settings menu can override this path.
pub fn default_download_dir() -> PathBuf {
    if let Some(configured) = Config::load().download_dir {
        let path = expand_path(&configured);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    if util::is_android() && Path::new("/sdcard/Download").is_dir() {
        return PathBuf::from("/sdcard/Download/Animeku-CLI");
    }

    let base = dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    base.join("Animeku-CLI")
}

#[derive(Debug, Clone)]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub size: u64,
    /// Unix timestamp from the filesystem, or 0 when unavailable.
    pub modified: u64,
    /// Sub-folder the file lives in, which is the series title for batch
    /// downloads. `None` for loose files directly in the download folder.
    pub series: Option<String>,
}

/// Video files already present in the configured download folder. This is a
/// filesystem view, not a queue: users can inspect and play files even after
/// the batch queue has been cleared.
pub fn downloaded_files() -> Vec<DownloadedFile> {
    downloaded_files_in(&default_download_dir())
}

pub fn downloaded_files_in(directory: &Path) -> Vec<DownloadedFile> {
    let mut files = Vec::new();
    collect_downloaded_files(directory, None, 2, &mut files);
    files.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    files
}

/// Reads the download folder and, one level deep, the per-series sub-folders
/// created by batch downloads.
fn collect_downloaded_files(
    directory: &Path,
    series: Option<&str>,
    depth: usize,
    files: &mut Vec<DownloadedFile>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth == 0 {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            collect_downloaded_files(&path, name.as_deref().or(series), depth - 1, files);
            continue;
        }
        if !path.is_file() || is_partial_file(&path) {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        let metadata = entry.metadata().ok();
        let size = metadata.as_ref().map(|metadata| metadata.len()).unwrap_or(0);
        let modified = metadata
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        files.push(DownloadedFile {
            path,
            size,
            modified,
            series: series.map(str::to_string),
        });
    }
}

fn is_partial_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(".part."))
}

/// Prompt for destination and filename, then download the selected stream.
///
/// The stream has already been resolved by the provider. Idlix desktop streams
/// are local video playlists with a separate audio playlist; Idlix Android
/// streams are master playlists that already contain audio. Otakudesu streams
/// are direct MP4/Google Video URLs and need the provider headers.
pub fn download_stream(
    provider_type: usize,
    stream: &Stream,
    movie: &Movie,
    episode: &Episode,
) -> Result<DownloadOutcome> {
    ensure_available()?;

    let default_dir = default_download_dir();
    let default_name = suggested_filename(provider_type, stream, movie, episode);
    let default_extension = default_extension(provider_type, &stream.url);

    let (directory, output_path) = loop {
        let Some(dir_input) = prompt_text("Folder tujuan", &default_dir.display().to_string())?
        else {
            return Ok(DownloadOutcome::Cancelled);
        };
        let dir_text = if dir_input.trim().is_empty() {
            default_dir.display().to_string()
        } else {
            dir_input.trim().to_string()
        };
        let directory = expand_path(&dir_text);
        if let Err(error) = fs::create_dir_all(&directory) {
            eprintln!(
                "{} Gagal membuat folder '{}': {}",
                "■".red(),
                directory.display(),
                error
            );
            continue;
        }
        if !directory.is_dir() {
            eprintln!(
                "{} '{}' bukan folder yang valid.",
                "■".red(),
                directory.display()
            );
            continue;
        }

        let Some(name_input) = prompt_text("Nama file", &default_name)? else {
            return Ok(DownloadOutcome::Cancelled);
        };
        let name_text = if name_input.trim().is_empty() {
            default_name.clone()
        } else {
            name_input.trim().to_string()
        };
        let filename = ensure_extension(&sanitize_filename(&name_text), default_extension);
        let output_path = directory.join(filename);

        if output_path.is_dir() {
            eprintln!(
                "{} '{}' adalah folder, bukan nama file yang valid.",
                "■".red(),
                output_path.display()
            );
            continue;
        }
        if output_path.exists() {
            println!();
            println!(
                "{} File sudah ada: {}",
                "⚠".yellow(),
                output_path.display()
            );
            if crate::confirm_action("Timpa file yang sudah ada?")? {
                break (directory, output_path);
            }
            continue;
        }
        break (directory, output_path);
    };

    let mut config = Config::load();
    config.download_dir = Some(directory.to_string_lossy().to_string());
    config.save();

    match download_to_path(provider_type, stream, &output_path)? {
        DownloadResult::Completed => Ok(DownloadOutcome::Completed(output_path)),
        DownloadResult::Cancelled => Ok(DownloadOutcome::Cancelled),
    }
}

/// Download one already-resolved stream to an explicit path without prompting.
/// Used by the batch queue after it has chosen the destination folder once.
pub fn download_to_path(
    provider_type: usize,
    stream: &Stream,
    output_path: &Path,
) -> Result<DownloadResult> {
    download_to_path_inner(provider_type, stream, output_path, None, None)
}

/// Batch downloads share a pause flag. Pressing `p` during ffmpeg pauses the
/// process on Unix and always stops the batch after the current episode, so the
/// queue can be resumed later from the download manager.
pub fn download_to_path_with_pause(
    provider_type: usize,
    stream: &Stream,
    output_path: &Path,
    pause: Arc<AtomicBool>,
) -> Result<DownloadResult> {
    download_to_path_inner(provider_type, stream, output_path, Some(pause), None)
}

/// Variant used by the batch list UI. The callback receives live ffmpeg
/// progress, and this function stops writing its own progress line so the
/// caller can render a multi-line status list safely.
pub fn download_to_path_with_progress(
    provider_type: usize,
    stream: &Stream,
    output_path: &Path,
    pause: Arc<AtomicBool>,
    progress: DownloadProgressCallback,
) -> Result<DownloadResult> {
    download_to_path_inner(
        provider_type,
        stream,
        output_path,
        Some(pause),
        Some(progress),
    )
}

fn download_to_path_inner(
    provider_type: usize,
    stream: &Stream,
    output_path: &Path,
    pause: Option<Arc<AtomicBool>>,
    progress: Option<DownloadProgressCallback>,
) -> Result<DownloadResult> {
    ensure_available()?;
    if output_path.is_dir() {
        anyhow::bail!(
            "'{}' adalah folder, bukan nama file yang valid.",
            output_path.display()
        );
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("gagal membuat folder '{}'", parent.display()))?;
    }

    let audio = if provider_type == ext::PROVIDER_IDLIX && !util::is_android() {
        let path = util::temp_file("animeku_idlix_audio.m3u8");
        Path::new(&path).is_file().then_some(path)
    } else {
        None
    };
    let subtitle = if provider_type == ext::PROVIDER_IDLIX {
        let path = util::get_subtitle_path();
        Path::new(&path).is_file().then_some(path)
    } else {
        None
    };

    run_ffmpeg(
        provider_type,
        &stream.url,
        audio.as_deref(),
        subtitle.as_deref(),
        Config::load().embed_subtitles,
        output_path,
        pause,
        progress,
    )
}

/// Ask for the batch destination once, create it, and remember it as the
/// default download folder.
pub fn prompt_batch_directory() -> Result<Option<PathBuf>> {
    let default_dir = default_download_dir();
    loop {
        let Some(dir_input) =
            prompt_text("Folder tujuan batch", &default_dir.display().to_string())?
        else {
            return Ok(None);
        };
        let dir_text = if dir_input.trim().is_empty() {
            default_dir.display().to_string()
        } else {
            dir_input.trim().to_string()
        };
        let directory = expand_path(&dir_text);
        if let Err(error) = fs::create_dir_all(&directory) {
            eprintln!(
                "{} Gagal membuat folder '{}': {}",
                "■".red(),
                directory.display(),
                error
            );
            continue;
        }
        if !directory.is_dir() {
            eprintln!(
                "{} '{}' bukan folder yang valid.",
                "■".red(),
                directory.display()
            );
            continue;
        }

        let mut config = Config::load();
        config.download_dir = Some(directory.to_string_lossy().to_string());
        config.save();
        return Ok(Some(directory));
    }
}

pub fn prompt_existing_policy() -> Result<Option<ExistingFilePolicy>> {
    crate::input::choice(
        "Jika file sudah ada:",
        vec![ExistingFilePolicy::Skip, ExistingFilePolicy::Overwrite],
        0,
    )
}

pub fn batch_output_path(
    provider_type: usize,
    stream: &Stream,
    movie: &Movie,
    episode: &Episode,
    directory: &Path,
) -> PathBuf {
    directory
        .join(sanitize_filename(&crate::models::clean_movie_title(&movie.title)))
        .join(suggested_filename(provider_type, stream, movie, episode))
}

fn prompt_text(prompt: &str, initial: &str) -> Result<Option<String>> {
    match dialoguer::Input::<String>::with_theme(&util::custom_theme())
        .with_prompt(prompt)
        .with_initial_text(initial)
        .allow_empty(true)
        .interact_text()
    {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

fn expand_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn suggested_filename(
    provider_type: usize,
    stream: &Stream,
    movie: &Movie,
    episode: &Episode,
) -> String {
    let movie_title = display_movie_title(&movie.title);
    let base_title = if episode.is_series {
        let episode_title = clean_episode_title(&episode.title);
        if episode_title.is_empty() {
            movie_title
        } else {
            format!("{} - {}", movie_title, episode_title)
        }
    } else {
        movie_title
    };

    format!(
        "{}.{}",
        sanitize_filename(&base_title),
        default_extension(provider_type, &stream.url)
    )
}

fn display_movie_title(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_tag = trimmed
        .strip_prefix("[Movie]")
        .or_else(|| trimmed.strip_prefix("[Series]"))
        .unwrap_or(trimmed)
        .trim_start();
    let cleaned = clean_movie_title(without_tag);
    if cleaned.is_empty() {
        "Animeku".to_string()
    } else {
        cleaned
    }
}

fn default_extension(provider_type: usize, url: &str) -> &'static str {
    if provider_type == ext::PROVIDER_IDLIX {
        // Idlix HLS uses MPEG-TS/fMP4 segments. Matroska is the safest
        // container for stream-copy and accepts any audio/video codec.
        "mkv"
    } else if url.to_ascii_lowercase().contains(".m3u8") {
        "mkv"
    } else {
        "mp4"
    }
}

fn sanitize_filename(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }

    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed
        .trim_matches(|ch: char| ch == '.' || ch.is_whitespace())
        .to_string();
    let mut limited = String::new();
    for ch in trimmed.chars().take(MAX_FILENAME_CHARS) {
        if limited.len() + ch.len_utf8() > MAX_FILENAME_BYTES {
            break;
        }
        limited.push(ch);
    }
    let limited = limited
        .trim_matches(|ch: char| ch == '.' || ch.is_whitespace())
        .to_string();

    if limited.is_empty() {
        "animeku-download".to_string()
    } else {
        limited
    }
}

fn ensure_extension(name: &str, default_extension: &str) -> String {
    let trimmed = name.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return format!("animeku-download.{}", default_extension);
    }

    let has_video_extension = Path::new(trimmed)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        });

    if has_video_extension {
        trimmed.to_string()
    } else {
        format!("{}.{}", trimmed, default_extension)
    }
}

fn partial_path(output: &Path) -> PathBuf {
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4");
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("animeku-download");
    output.with_file_name(format!("{}.part.{}", stem, extension))
}

/// Newer ffmpeg versions split HLS extension validation into a separate
/// `allowed_segment_extensions` option and enable `extension_picky` by default.
/// Idlix disguises media segments as `.webp`, `.jpg`, `.js`, etc., so both
/// checks have to be relaxed. Older ffmpeg versions only have the original
/// `allowed_extensions` option; probing the demuxer help keeps this compatible.
fn hls_extension_options() -> &'static [String] {
    static OPTIONS: OnceLock<Vec<String>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let Ok(output) = Command::new(crate::resolve_command_path("ffmpeg"))
            .args(["-hide_banner", "-h", "demuxer=hls"])
            .output()
        else {
            return Vec::new();
        };
        let help = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let mut options = Vec::new();
        if help.contains("allowed_segment_extensions") {
            options.extend(["-allowed_segment_extensions".to_string(), "ALL".to_string()]);
        }
        if help.contains("extension_picky") {
            options.extend(["-extension_picky".to_string(), "0".to_string()]);
        }
        options
    })
}

fn input_options(provider_type: usize, url: &str) -> Vec<String> {
    let is_hls = url.to_ascii_lowercase().contains(".m3u8");
    let is_remote = url.starts_with("http://") || url.starts_with("https://");
    let mut options = Vec::new();

    if is_hls || provider_type == ext::PROVIDER_IDLIX {
        options.extend([
            "-protocol_whitelist".to_string(),
            "file,http,https,tcp,tls,crypto,data".to_string(),
            "-allowed_extensions".to_string(),
            "ALL".to_string(),
        ]);
        options.extend(hls_extension_options().iter().cloned());
    }

    // HTTP protocol options are rejected when the input itself is a local
    // playlist. Idlix's desktop playlists are local files whose segment URLs
    // are already signed, so no extra headers are needed.
    if is_remote {
        if provider_type == ext::PROVIDER_OTAKUDESU {
            options.extend([
                "-user_agent".to_string(),
                otakudesu::OTAKUDESU_USER_AGENT.to_string(),
                "-headers".to_string(),
                format!("Referer: {}\r\n", otakudesu::OTAKUDESU_REFERER),
            ]);
        } else {
            options.extend([
                "-user_agent".to_string(),
                CHROME_USER_AGENT.to_string(),
                "-headers".to_string(),
                format!("Referer: {}\r\n", idlix::idlix_referer()),
            ]);
        }
    }

    options
}

fn probe_duration(provider_type: usize, url: &str) -> Option<f64> {
    if !crate::check_command_exists("ffprobe") {
        return None;
    }

    let mut command = Command::new(crate::resolve_command_path("ffprobe"));
    command.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
    ]);
    command.args(input_options(provider_type, url));
    command.arg(url);

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<f64>().ok())
        .filter(|duration| *duration > 0.0)
}

fn build_ffmpeg_command(
    provider_type: usize,
    url: &str,
    audio: Option<&str>,
    subtitle: Option<&str>,
    embed_subtitles: bool,
    output: &Path,
) -> Command {
    let mut command = Command::new(crate::resolve_command_path("ffmpeg"));
    command.args([
        "-hide_banner",
        "-nostdin",
        "-y",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
    ]);

    command.args(input_options(provider_type, url));
    command.arg("-i").arg(url);

    let mut next_input = 1;
    let audio_input = if let Some(audio) = audio {
        command.args(input_options(provider_type, audio));
        command.arg("-i").arg(audio);
        let index = next_input;
        next_input += 1;
        Some(index)
    } else {
        None
    };

    let subtitle_input = if embed_subtitles {
        subtitle.map(|subtitle| {
            command.arg("-i").arg(subtitle);
            let index = next_input;
            next_input += 1;
            index
        })
    } else {
        None
    };

    if let Some(audio_input) = audio_input {
        command
            .args(["-map", "0:v:0", "-map"])
            .arg(format!("{audio_input}:a:0"));
    } else {
        command.args(["-map", "0:v:0?", "-map", "0:a:0?"]);
    }
    if let Some(subtitle_input) = subtitle_input {
        command.arg("-map").arg(format!("{subtitle_input}:s:0"));
    }

    command.args(["-c", "copy"]);

    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if subtitle_input.is_some() {
        if extension == "mp4" || extension == "m4v" {
            command.args(["-c:s", "mov_text"]);
        } else {
            command.args(["-c:s", "srt"]);
        }
        command.args(["-metadata:s:s:0", "language=ind"]);
    }
    if extension == "mp4" || extension == "m4v" {
        if provider_type == ext::PROVIDER_IDLIX || url.to_ascii_lowercase().contains(".m3u8") {
            command.args(["-bsf:a", "aac_adtstoasc"]);
        }
        command.args(["-movflags", "+faststart"]);
    }

    command.arg(output);
    command
}

struct DownloadControl {
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl DownloadControl {
    fn start(pause: Arc<AtomicBool>, allow_pause: bool) -> Option<Self> {
        if enable_raw_mode().is_err() {
            return None;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let cancel_thread = Arc::clone(&cancel);
        let pause_thread = Arc::clone(&pause);
        let stop_thread = Arc::clone(&stop);
        let handle = match std::thread::Builder::new()
            .name("animeku-download-input".into())
            .spawn(move || {
                while !stop_thread.load(Ordering::Relaxed) {
                    match event::poll(Duration::from_millis(100)) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(_) => break,
                    }

                    let Ok(event::Event::Key(key)) = event::read() else {
                        continue;
                    };
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }

                    let cancel_key = key.code == KeyCode::Esc
                        || key.code == KeyCode::Char('q')
                        || key.code == KeyCode::Char('Q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL));
                    if cancel_key {
                        cancel_thread.store(true, Ordering::SeqCst);
                    } else if allow_pause
                        && (key.code == KeyCode::Char('p') || key.code == KeyCode::Char('P'))
                    {
                        pause_thread.fetch_xor(true, Ordering::SeqCst);
                    }
                }
            }) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = disable_raw_mode();
                return None;
            }
        };

        Some(Self {
            cancel,
            pause,
            stop,
            handle: Some(handle),
        })
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn paused(&self) -> bool {
        self.pause.load(Ordering::SeqCst)
    }
}

#[cfg(unix)]
fn set_process_paused(pid: u32, paused: bool) {
    let signal = if paused { libc::SIGSTOP } else { libc::SIGCONT };
    // Best effort: the process may have exited between the check and the signal.
    unsafe {
        libc::kill(pid as i32, signal);
    }
}

#[cfg(windows)]
fn set_process_paused(pid: u32, paused: bool) {
    use std::ffi::c_void;
    const PROCESS_SUSPEND_RESUME: u32 = 0x0800;
    // SIGSTOP has no documented Win32 twin. NtSuspendProcess/NtResumeProcess
    // are the calls every task manager wrapper uses; they freeze all threads
    // at once, which is what the raw-mode `p` key needs.
    // SAFETY: the handle comes straight from OpenProcess and is closed after
    // the single ntdll call; a null handle is checked first.
    unsafe {
        let handle = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
        if handle.is_null() {
            return;
        }
        if paused {
            NtSuspendProcess(handle);
        } else {
            NtResumeProcess(handle);
        }
        CloseHandle(handle);
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    fn NtSuspendProcess(process: *mut std::ffi::c_void) -> i32;
    fn NtResumeProcess(process: *mut std::ffi::c_void) -> i32;
}

impl Drop for DownloadControl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = disable_raw_mode();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg(
    provider_type: usize,
    url: &str,
    audio: Option<&str>,
    subtitle: Option<&str>,
    embed_subtitles: bool,
    output: &Path,
    pause: Option<Arc<AtomicBool>>,
    progress_callback: Option<DownloadProgressCallback>,
) -> Result<DownloadResult> {
    let allow_pause = pause.is_some();
    let list_mode = progress_callback.is_some();
    let duration = probe_duration(provider_type, url);
    let partial = partial_path(output);
    if partial.exists() {
        let _ = fs::remove_file(&partial);
    }

    if !list_mode {
        println!("{} Mengunduh dengan ffmpeg...", "◆".blue());
        if allow_pause {
            println!(
                "  {} Tekan Esc/q/Ctrl+C untuk membatalkan • p untuk jeda/lanjut.",
                "•".dimmed()
            );
        } else {
            println!("  {} Tekan Esc/q/Ctrl+C untuk membatalkan.", "•".dimmed());
        }
    }
    if let Some(callback) = &progress_callback {
        callback(DownloadProgress {
            duration,
            finished: false,
            paused: false,
            ..Default::default()
        });
    }
    let mut command =
        build_ffmpeg_command(provider_type, url, audio, subtitle, embed_subtitles, &partial);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| "gagal menjalankan ffmpeg")?;

    let stderr = child.stderr.take().context("gagal membaca log ffmpeg")?;
    let stderr_handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            lines.push(trimmed.to_string());
            if lines.len() > 20 {
                lines.remove(0);
            }
        }
        lines
    });

    let stdout = child
        .stdout
        .take()
        .context("gagal membaca progres ffmpeg")?;
    let (progress_tx, progress_rx) = mpsc::channel();
    let stdout_handle = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if progress_tx.send(line).is_err() {
                break;
            }
        }
    });

    let pause_flag = pause.unwrap_or_default();
    let control = DownloadControl::start(Arc::clone(&pause_flag), allow_pause);
    let mut out_seconds = 0.0_f64;
    let mut out_time_us_seen = false;
    let mut total_size = 0_u64;
    let mut speed = String::new();
    let mut finished = false;
    let mut cancelled = false;
    let mut paused = false;
    let mut last_draw = Instant::now() - Duration::from_secs(1);

    loop {
        match progress_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if let Some(value) = line.strip_prefix("out_time_us=") {
                    if let Ok(microseconds) = value.trim().parse::<f64>() {
                        out_seconds = microseconds / 1_000_000.0;
                        out_time_us_seen = true;
                    }
                } else if let Some(value) = line.strip_prefix("out_time_ms=") {
                    if !out_time_us_seen {
                        if let Ok(microseconds) = value.trim().parse::<f64>() {
                            out_seconds = microseconds / 1_000_000.0;
                        }
                    }
                } else if let Some(value) = line.strip_prefix("out_time=") {
                    if !out_time_us_seen {
                        out_seconds = parse_ffmpeg_time(value.trim()).unwrap_or(out_seconds);
                    }
                } else if let Some(value) = line.strip_prefix("total_size=") {
                    total_size = value.trim().parse::<u64>().unwrap_or(total_size);
                } else if let Some(value) = line.strip_prefix("speed=") {
                    speed = value.trim().to_string();
                } else if line.starts_with("progress=end") {
                    finished = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if control.as_ref().is_some_and(|control| control.cancelled()) {
            cancelled = true;
            if paused {
                set_process_paused(child.id(), false);
            }
            let _ = child.kill();
            break;
        }

        if let Some(control) = &control {
            let want_pause = control.paused();
            if want_pause != paused {
                paused = want_pause;
                set_process_paused(child.id(), paused);
                if !list_mode {
                    clear_progress_line();
                    if paused {
                        println!(
                            "  {} Download dijeda. Tekan p lagi untuk melanjutkan.",
                            "ℹ".blue()
                        );
                    } else {
                        println!("  {} Download dilanjutkan.", "▶".green());
                    }
                }
            }
        }

        if last_draw.elapsed() >= Duration::from_millis(250) {
            if let Some(callback) = &progress_callback {
                callback(DownloadProgress {
                    duration,
                    out_seconds,
                    total_size,
                    speed: speed.clone(),
                    finished,
                    paused,
                });
            } else {
                render_progress(duration, out_seconds, total_size, &speed, finished);
            }
            last_draw = Instant::now();
        }
    }

    if paused {
        set_process_paused(child.id(), false);
    }
    if control.as_ref().is_some_and(|control| control.cancelled()) {
        cancelled = true;
    }
    if cancelled {
        let _ = child.kill();
    }

    let status_result = child.wait().context("gagal menunggu ffmpeg");
    drop(control);
    let error_lines = stderr_handle.join().unwrap_or_default();
    let _ = stdout_handle.join();
    if !list_mode {
        clear_progress_line();
    }
    let status = status_result?;

    if cancelled {
        let _ = fs::remove_file(&partial);
        if !list_mode {
            println!("{} Download dibatalkan.", "■".yellow());
        }
        return Ok(DownloadResult::Cancelled);
    }

    if !status.success() {
        let _ = fs::remove_file(&partial);
        if !list_mode && !error_lines.is_empty() {
            eprintln!("{}", "Log ffmpeg terakhir:".yellow());
            for line in &error_lines {
                eprintln!("  {}", line);
            }
        }
        let detail = if list_mode {
            error_lines
                .last()
                .map(|line| format!(": {line}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        anyhow::bail!("ffmpeg keluar dengan status {}{}", status, detail);
    }

    if !partial.is_file() {
        anyhow::bail!(
            "ffmpeg selesai tetapi file hasil download tidak ditemukan: {}",
            partial.display()
        );
    }
    let size = fs::metadata(&partial)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if size == 0 {
        let _ = fs::remove_file(&partial);
        anyhow::bail!("ffmpeg menghasilkan file kosong; download dibatalkan");
    }

    // On Unix, rename replaces the existing file atomically. Windows needs
    // the destination removed first, which is why the confirmation prompt
    // happens before ffmpeg starts.
    #[cfg(windows)]
    if output.exists() {
        fs::remove_file(output)
            .with_context(|| format!("gagal menimpa file lama '{}'", output.display()))?;
    }
    fs::rename(&partial, output)
        .with_context(|| format!("gagal memindahkan hasil download ke '{}'", output.display()))?;

    if let Some(callback) = &progress_callback {
        callback(DownloadProgress {
            duration,
            out_seconds: duration.unwrap_or(out_seconds),
            total_size: size,
            speed,
            finished: true,
            paused: false,
        });
    }

    if !list_mode {
        println!(
            "{} Download selesai: {} ({})",
            "✓".green(),
            output.display(),
            format_bytes(size)
        );
    }

    if let Some(subtitle) = subtitle {
        if embed_subtitles {
            if !list_mode {
                println!("  {} Subtitle ditanam ke dalam video.", "•".dimmed());
            }
        } else {
            copy_subtitle(Path::new(subtitle), output, list_mode);
        }
    }

    Ok(DownloadResult::Completed)
}

fn parse_ffmpeg_time(value: &str) -> Option<f64> {
    let mut parts = value.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn render_progress(
    duration: Option<f64>,
    out_seconds: f64,
    total_size: u64,
    speed: &str,
    finished: bool,
) {
    let speed = if speed.is_empty() { "—" } else { speed };
    let size = format_bytes(total_size);
    let line = if let Some(duration) = duration {
        let ratio = (out_seconds / duration).clamp(0.0, 1.0);
        let ratio = if finished { 1.0 } else { ratio };
        let filled = (ratio * PROGRESS_BAR_WIDTH as f64).round() as usize;
        let bar = format!(
            "{}{}",
            "█".repeat(filled.min(PROGRESS_BAR_WIDTH)),
            "░".repeat(PROGRESS_BAR_WIDTH.saturating_sub(filled))
        );
        format!(
            "  ⬇ [{}] {:5.1}% • {} • {}",
            bar,
            ratio * 100.0,
            size,
            speed
        )
    } else {
        format!(
            "  ⬇ {} • {} • {}",
            crate::models::format_seconds(out_seconds as u64),
            size,
            speed
        )
    };

    let width = progress_line_width();
    let line = line.chars().take(width).collect::<String>();
    print!("\r{:<width$}", line, width = width);
    let _ = std::io::stdout().flush();
}

fn progress_line_width() -> usize {
    crossterm::terminal::size()
        .map(|(columns, _)| columns as usize)
        .unwrap_or(80)
        .max(1)
        .saturating_sub(1)
}

fn clear_progress_line() {
    let width = progress_line_width();
    print!("\r{}\r", " ".repeat(width));
    let _ = std::io::stdout().flush();
}

/// Free bytes on the filesystem holding `path`, or `None` when the platform
/// call fails. Used before each batch episode so a full disk stops the queue
/// instead of failing halfway through a file.
pub(crate) fn available_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    let space = unix_available_space(path);
    #[cfg(windows)]
    let space = windows_available_space(path);
    #[cfg(not(any(unix, windows)))]
    let space = None;
    space
}

#[cfg(unix)]
fn unix_available_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs only writes into the struct we pass.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
fn windows_available_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and `free` is a valid out pointer.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetDiskFreeSpaceExW(
        directory: *const u16,
        free_bytes_available: *mut u64,
        total_bytes: *mut u64,
        total_free_bytes: *mut u64,
    ) -> i32;
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.2} GB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KB", bytes_f / KIB)
    } else {
        format!("{} B", bytes)
    }
}

fn copy_subtitle(subtitle: &Path, output: &Path, quiet: bool) {
    let sidecar = output.with_extension("vtt");
    if subtitle == sidecar {
        return;
    }
    match fs::copy(subtitle, &sidecar) {
        Ok(_) if !quiet => println!("  Subtitle: {}", sidecar.display()),
        Ok(_) => {}
        Err(error) if !quiet => eprintln!(
            "{} Gagal menyalin subtitle ke '{}': {}",
            "⚠".yellow(),
            sidecar.display(),
            error
        ),
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(title: &str) -> Movie {
        Movie {
            id: "test".into(),
            title: title.into(),
            total_episodes: None,
        }
    }

    fn episode(title: &str, is_series: bool) -> Episode {
        Episode {
            id: "ep".into(),
            title: title.into(),
            is_series,
        }
    }

    fn stream(url: &str) -> Stream {
        Stream {
            url: url.into(),
            title: "Test".into(),
        }
    }

    #[test]
    fn sanitize_filename_removes_path_separators() {
        assert_eq!(
            sanitize_filename("  Movie: The / Best?  "),
            "Movie_ The _ Best_"
        );
        assert_eq!(sanitize_filename("..."), "animeku-download");
    }

    #[test]
    fn ensure_extension_keeps_known_video_extensions() {
        assert_eq!(ensure_extension("Movie.mkv", "mp4"), "Movie.mkv");
        assert_eq!(ensure_extension("Movie.MP4", "mkv"), "Movie.MP4");
        assert_eq!(ensure_extension("Dr. Stone", "mp4"), "Dr. Stone.mp4");
        assert_eq!(ensure_extension("Movie.final", "mkv"), "Movie.final.mkv");
    }

    #[test]
    fn partial_path_keeps_media_extension() {
        let path = partial_path(Path::new("/tmp/Movie - Episode 1.mkv"));
        assert_eq!(path, PathBuf::from("/tmp/Movie - Episode 1.part.mkv"));
    }

    #[test]
    fn suggested_filename_cleans_provider_tags_and_episode_title() {
        let name = suggested_filename(
            ext::PROVIDER_OTAKUDESU,
            &stream("https://example.com/video.mp4"),
            &movie("Solo Leveling (Episode 1 - 12) Subtitle Indonesia"),
            &episode("Solo Leveling Episode 3 Subtitle Indonesia", true),
        );
        assert_eq!(name, "Solo Leveling - Episode 3.mp4");
    }

    #[test]
    fn batch_output_path_uses_episode_filename() {
        let path = batch_output_path(
            ext::PROVIDER_OTAKUDESU,
            &stream("https://example.com/video.mp4"),
            &movie("Solo Leveling (Episode 1 - 12) Subtitle Indonesia"),
            &episode("Solo Leveling Episode 3 Subtitle Indonesia", true),
            Path::new("/tmp/animeku"),
        );
        assert_eq!(
            path,
            PathBuf::from("/tmp/animeku/Solo Leveling/Solo Leveling - Episode 3.mp4")
        );
    }

    #[test]
    fn full_movie_filename_does_not_use_player_placeholder() {
        let name = suggested_filename(
            ext::PROVIDER_IDLIX,
            &stream("/tmp/video.m3u8"),
            &movie("[Movie] A New Dawn"),
            &episode("Putar Film (Full Movie)", false),
        );
        assert_eq!(name, "A New Dawn.mkv");
    }

    #[test]
    fn progress_time_parser_handles_fractional_seconds() {
        assert_eq!(parse_ffmpeg_time("01:02:03.500000"), Some(3723.5));
    }

    #[test]
    fn local_playlists_do_not_receive_http_only_options() {
        let options = input_options(ext::PROVIDER_IDLIX, "/tmp/video.m3u8");
        assert!(options.iter().any(|option| option == "-protocol_whitelist"));
        assert!(!options.iter().any(|option| option == "-user_agent"));
        assert!(!options.iter().any(|option| option == "-headers"));
    }

    #[test]
    fn ffmpeg_command_writes_to_the_partial_file_once() {
        let output = Path::new("/tmp/Movie - Episode 1.part.mkv");
        let command = build_ffmpeg_command(
            ext::PROVIDER_IDLIX,
            "/tmp/video.m3u8",
            Some("/tmp/audio.m3u8"),
            None,
            false,
            output,
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args.iter()
                .filter(|arg| *arg == "/tmp/Movie - Episode 1.part.mkv")
                .count(),
            1
        );
        assert!(args.iter().any(|arg| arg == "-map"));
        assert!(args.iter().any(|arg| arg == "-c"));
    }

    #[test]
    fn embedded_subtitle_is_mapped_and_converted() {
        let output = Path::new("/tmp/Movie.mkv");
        let command = build_ffmpeg_command(
            ext::PROVIDER_IDLIX,
            "/tmp/video.m3u8",
            Some("/tmp/audio.m3u8"),
            Some("/tmp/subtitle.vtt"),
            true,
            output,
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let has_pair = |first: &str, second: &str| {
            args.windows(2)
                .any(|pair| pair[0] == first && pair[1] == second)
        };
        assert!(has_pair("-map", "2:s:0"));
        assert!(has_pair("-c:s", "srt"));
        assert!(has_pair("-metadata:s:s:0", "language=ind"));
    }

    #[test]
    fn remote_otakudesu_urls_receive_referer_and_user_agent() {
        let options = input_options(ext::PROVIDER_OTAKUDESU, "https://example.com/video.mp4");
        assert!(options.iter().any(|option| option == "-user_agent"));
        assert!(options.iter().any(|option| option == "-headers"));
        assert!(options
            .iter()
            .any(|option| option.contains(otakudesu::OTAKUDESU_REFERER)));
    }
}

#[cfg(test)]
mod live_smoke_tests {
    use super::*;
    use crate::ext::Ext;

    fn run_limited(command: Command, seconds: u32) -> std::process::Output {
        let program = command.get_program().to_os_string();
        let args = command
            .get_args()
            .map(|arg| arg.to_os_string())
            .collect::<Vec<_>>();
        let seconds = seconds.to_string();
        let mut limited = Command::new(program);
        for arg in &args[..args.len() - 1] {
            limited.arg(arg);
        }
        limited.args(["-t", &seconds]);
        limited.arg(&args[args.len() - 1]);
        limited.output().expect("ffmpeg should start")
    }

    fn assert_video_and_audio(path: &Path) {
        let probe = Command::new(crate::resolve_command_path("ffprobe"))
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=index,codec_type,codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .output()
            .expect("ffprobe should start");
        assert!(
            probe.status.success(),
            "ffprobe failed: {}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let streams = String::from_utf8_lossy(&probe.stdout);
        println!("{}", streams);
        assert!(streams.lines().any(|line| line.ends_with(",video")));
        assert!(streams.lines().any(|line| line.ends_with(",audio")));
    }

    #[test]
    #[ignore = "live network + ffmpeg smoke test"]
    fn live_idlix_download_smoke() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut provider = idlix::Idlix::new();
            let (movies, _) = provider.search("A New Dawn".to_string(), 1).await.unwrap();
            let movie = movies
                .into_iter()
                .find(|movie| movie.id.contains("a-new-dawn"))
                .expect("A New Dawn should exist");
            let (episodes, _) = provider.get_episodes(movie).await.unwrap();
            let episode = episodes.into_iter().next().expect("episode should exist");
            println!("Idlix episode: {} ({})", episode.title, episode.id);
            let streams = provider.get_stream_urls(episode).await.unwrap();
            for stream in &streams {
                println!("  {} -> {}", stream.title, stream.url);
            }
            let stream = streams.last().expect("stream should exist");
            let dir = std::env::temp_dir().join("animeku-live-test");
            fs::create_dir_all(&dir).unwrap();
            let output = dir.join("idlix-smoke.mkv");
            let audio_path = util::temp_file("animeku_idlix_audio.m3u8");
            let audio = Path::new(&audio_path)
                .is_file()
                .then_some(audio_path.as_str());
            let command =
                build_ffmpeg_command(ext::PROVIDER_IDLIX, &stream.url, audio, None, false, &output);
            let result = run_limited(command, 10);
            assert!(
                result.status.success(),
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_video_and_audio(&output);
            let subtitle = util::get_subtitle_path();
            assert!(Path::new(&subtitle).is_file(), "subtitle should exist");
        });
    }

    #[test]
    #[ignore = "live network + ffmpeg smoke test"]
    fn live_otakudesu_download_smoke() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut provider = otakudesu::Otakudesu::new();
            let (movies, _) = provider
                .search("Jujutsu Kaisen".to_string(), 1)
                .await
                .unwrap();
            let movie = movies
                .iter()
                .find(|movie| movie.title.to_lowercase().contains("jujutsu kaisen"))
                .cloned()
                .or_else(|| movies.first().cloned())
                .expect("Jujutsu Kaisen should exist");
            let (episodes, _) = provider.get_episodes(movie).await.unwrap();
            let episode = episodes.into_iter().next().expect("episode should exist");
            println!("Otakudesu episode: {} ({})", episode.title, episode.id);
            let streams = provider.get_stream_urls(episode).await.unwrap();
            let dir = std::env::temp_dir().join("animeku-live-test");
            fs::create_dir_all(&dir).unwrap();

            for stream in &streams {
                println!("Trying {} -> {}", stream.title, stream.url);
                let output = dir.join("otakudesu-smoke.mp4");
                let command = build_ffmpeg_command(
                    ext::PROVIDER_OTAKUDESU,
                    &stream.url,
                    None,
                    None,
                    false,
                    &output,
                );
                let result = run_limited(command, 10);
                if result.status.success() {
                    println!("OK: {}", stream.title);
                    assert_video_and_audio(&output);
                    return;
                }
                eprintln!(
                    "FAILED {}: {}",
                    stream.title,
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            panic!("all Otakudesu streams failed");
        });
    }

    #[cfg(unix)]
    #[test]
    fn available_space_reports_free_bytes() {
        assert!(
            available_space(Path::new("/tmp")).unwrap_or(0) > 0,
            "statvfs should report free bytes for an existing directory"
        );
    }

    #[test]
    fn downloaded_files_lists_only_final_videos() {        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "animeku-downloaded-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("episode-1.mkv"), b"video").unwrap();
        fs::write(dir.join("episode-2.mp4"), b"video").unwrap();
        fs::write(dir.join("episode-3.part.mp4"), b"partial").unwrap();
        fs::write(dir.join("catatan.txt"), b"not a video").unwrap();
        fs::create_dir_all(dir.join("Judul Seri")).unwrap();
        fs::write(dir.join("Judul Seri").join("Judul Seri - Episode 1.mp4"), b"video").unwrap();

        let files = downloaded_files_in(&dir);
        let mut names = files
            .iter()
            .map(|file| file.path.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            vec![
                "Judul Seri - Episode 1.mp4",
                "episode-1.mkv",
                "episode-2.mp4"
            ]
        );
        let series_file = files
            .iter()
            .find(|file| file.series.is_some())
            .expect("file di sub-folder harus punya nama seri");
        assert_eq!(series_file.series.as_deref(), Some("Judul Seri"));

        let _ = fs::remove_dir_all(&dir);
    }
}
