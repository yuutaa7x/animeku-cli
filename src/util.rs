use std::collections::HashMap;
use std::future::Future;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};

use colored::Colorize;
use dialoguer::{console::Style, theme::ColorfulTheme};

/// Tiny dependency-free loading spinner. It writes to stderr and disables
/// itself automatically when output is redirected.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: impl Into<String>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        if !std::io::stderr().is_terminal() {
            return Self { stop, handle: None };
        }

        let message = message.into();
        let clear_width = message.chars().count() + 3;
        let stop_flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut index = 0usize;
            while !stop_flag.load(Ordering::Relaxed) {
                eprint!("\r{} {}", frames[index % frames.len()], message);
                let _ = std::io::stderr().flush();
                index += 1;
                std::thread::sleep(std::time::Duration::from_millis(90));
            }
            eprint!("\r{}\r", " ".repeat(clear_width));
            let _ = std::io::stderr().flush();
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    pub fn stop(mut self) {
        self.finish();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.finish();
    }
}

pub async fn with_spinner<F, T>(message: impl Into<String>, future: F) -> T
where
    F: Future<Output = T>,
{
    let spinner = Spinner::start(message);
    let result = future.await;
    spinner.stop();
    result
}

pub fn custom_theme() -> ColorfulTheme {
    ColorfulTheme {
        hint_style: Style::new().cyan(),
        ..Default::default()
    }
}

/// Private per-user directory for cookies, playlists, subtitles and logs.
///
/// Sensitive session data (Idlix cookies, stream tokens) must never live in a
/// world-readable shared temp directory such as `/tmp`.
pub fn app_data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
        let dir = base.join("animeku-cli");
        let _ = std::fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        dir
    })
    .clone()
}

pub fn temp_file(name: &str) -> String {
    app_data_dir()
        .join(name)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Create a file (if missing) and restrict it to the current user.
pub fn ensure_private_file(path: &str) -> std::io::Result<()> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .truncate(false)
        .open(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn is_android() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

pub fn get_subtitle_path() -> String {
    if is_android() {
        let download_dir = std::path::Path::new("/sdcard/Download");
        if download_dir.is_dir() {
            let sdcard = "/sdcard/Download/animeku_idlix.vtt";
            if std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(sdcard)
                .is_ok()
            {
                return sdcard.to_string();
            }
        }
    }
    temp_file("animeku_idlix.vtt")
}

fn is_session_file(name: &str) -> bool {
    name.starts_with("animeku_idlix_video_")
        || name == "animeku_idlix_audio.m3u8"
        || name == "animeku_idlix.vtt"
        || name == "animeku_track.lua"
        || name == "animeku_last_pos.txt"
        || name == "animeku_proxy_debug.txt"
}

/// Remove transient playback artifacts from both the new private directory and
/// the legacy system temp directory. Cookies are intentionally preserved.
pub fn cleanup_temp_files() {
    for dir in [app_data_dir(), std::env::temp_dir()] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if is_session_file(&name) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

use reqwest::Client;

#[macro_export]
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

pub async fn show_image_thumb(url: String) {
    static POSTER_CACHE: OnceLock<Mutex<HashMap<String, Arc<image::DynamicImage>>>> =
        OnceLock::new();

    let cache = POSTER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&url).cloned());

    let image = match cached {
        Some(image) => image,
        None => {
            let client = Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .user_agent("animeku-cli/0.2.0")
                .build()
                .unwrap_or_else(|_| Client::new());
            let Ok(resp) = client.get(&url).send().await else {
                return;
            };
            let Ok(bytes) = resp.bytes().await else {
                return;
            };
            let Ok(image) = image::load_from_memory(&bytes) else {
                return;
            };
            let image = Arc::new(image);
            if let Ok(mut cache) = cache.lock() {
                cache.insert(url, image.clone());
            }
            image
        }
    };

    let (rows, columns) = crossterm::terminal::size().unwrap_or((40, 120));
    let width = ((columns as u32 * 35) / 100).clamp(30, 60);
    let height = ((rows as u32 * 45) / 100).clamp(15, 35);
    let conf = viuer::Config {
        transparent: true,
        width: Some(width),
        height: Some(height),
        y: 8,
        x: 2,
        ..Default::default()
    };
    if viuer::print(&image, &conf).is_ok() {
        println!();
    }
}

pub fn clearscreen_and_show_banner() -> anyhow::Result<()> {
    clearscreen::clear()?;

    let config = crate::config::Config::load();
    let short_terminal = crossterm::terminal::size()
        .map(|(_, rows)| rows < 22)
        .unwrap_or(false);

    if config.compact_banner || short_terminal {
        eprintln!(
            "  {} v{}  •  Fork by Yuutaa7x\n",
            "Animeku-CLI".bright_green().bold(),
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }

    let version = format!(
        "                     v{}  •  Fork by Yuutaa7x",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!(
        "{}
{}
",
        r#"
               ▄▀█ █▄░█ █ █▀▄▀█ █▀▀ █▄▀ █░█ ▄▄ █▀▀ █░░ █
               █▀█ █░▀█ █ █░▀░█ ██▄ █░█ █▄█ ░░ █▄▄ █▄▄ █ "#
            .bright_green(),
        version
    );
    Ok(())
}
