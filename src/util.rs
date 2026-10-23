use std::collections::HashMap;
use std::future::Future;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use colored::Colorize;
use dialoguer::{
    console::{truncate_str, Style, Term},
    theme::ColorfulTheme,
};

/// Print a single loading line before waiting for `future`.
///
/// This deliberately avoids an animated, carriage-return-based spinner. On
/// narrow terminals and some emulators every spinner frame still wrapped to a
/// new line, leaving a trail of duplicate loading messages. A plain line cannot
/// do that, and the caller prints the result once the future completes.
pub async fn with_spinner<F, T>(message: impl Into<String>, future: F) -> T
where
    F: Future<Output = T>,
{
    if std::io::stderr().is_terminal() {
        eprintln!("{}", message.into());
    }
    future.await
}

static QUIET: AtomicBool = AtomicBool::new(false);

/// Suppresses provider chatter (gateway handshake, subtitle notices) while the
/// batch screen owns the terminal.
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn custom_theme() -> ColorfulTheme {
    ColorfulTheme {
        hint_style: Style::new().cyan(),
        ..Default::default()
    }
}

/// Keep dialoguer prompts on a single terminal line. `dialoguer` clears only
/// the current line after a prompt, so a wrapped prompt leaves fragments behind
/// (for example a stray character before the final `✔` line).
pub fn fit_prompt(prompt: &str) -> String {
    let (_rows, columns) = Term::stderr().size();
    fit_prompt_to_width(prompt, columns as usize)
}

fn fit_prompt_to_width(prompt: &str, columns: usize) -> String {
    // Leave room for the `? ` prefix and the `· yes/no` suffix added by the
    // ColorfulTheme.
    let width = columns.saturating_sub(10).max(12);
    truncate_str(prompt, width, "…").into_owned()
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
                .user_agent(concat!("animeku-cli/", env!("CARGO_PKG_VERSION")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use dialoguer::console::measure_text_width;

    #[test]
    fn long_prompts_are_truncated_to_one_terminal_line() {
        let prompt = "Download 6 episode (S1E01 - To You in 2016 s/d S1E06 - To All of You in 2016) ke '/home/yuu/dow/Animeku-CLI/Takopi'?";
        let fitted = fit_prompt_to_width(prompt, 40);
        assert!(measure_text_width(&fitted) <= 30);
        assert!(fitted.ends_with('…'));
    }

    #[test]
    fn short_prompts_are_left_unchanged() {
        assert_eq!(fit_prompt_to_width("Timpa file?", 80), "Timpa file?");
    }

    #[test]
    fn quiet_flag_round_trips() {
        assert!(!is_quiet());
        set_quiet(true);
        assert!(is_quiet());
        set_quiet(false);
        assert!(!is_quiet());
    }
}
