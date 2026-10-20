use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use colored::Colorize;
use dialoguer::{console::Style, theme::ColorfulTheme};

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
    let client = Client::new();
    if let Ok(resp) = client.get(url).send().await {
        if let Ok(bytes) = resp.bytes().await {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let conf = viuer::Config {
                    transparent: true,
                    width: Some(50),
                    height: Some(30),
                    y: 8,
                    x: 2,
                    ..Default::default()
                };
                if viuer::print(&img, &conf).is_ok() {
                    println!();
                }
            }
        }
    }
}

pub fn clearscreen_and_show_banner() -> anyhow::Result<()> {
    clearscreen::clear()?;

    let version = format!(
        "                     v{} {} val - Forked by Yuutaa7x",
        env!("CARGO_PKG_VERSION"),
        "©".cyan()
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
