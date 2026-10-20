use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, console::Style};

pub fn custom_theme() -> ColorfulTheme {
    let mut theme = ColorfulTheme::default();
    theme.hint_style = Style::new().cyan();
    theme
}

pub fn temp_file(name: &str) -> String {
    std::env::temp_dir()
        .join(name)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn is_android() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

pub fn get_subtitle_path() -> String {
    if is_android() {
        let download_dir = std::path::Path::new("/sdcard/Download");
        if download_dir.is_dir() {
            let sdcard = "/sdcard/Download/animeku_idlix.vtt";
            if std::fs::OpenOptions::new().create(true).write(true).open(sdcard).is_ok() {
                return sdcard.to_string();
            }
        }
    }
    temp_file("animeku_idlix.vtt")
}

pub fn cleanup_temp_files() {
    let temp_dir = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(temp_dir) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with("animeku_idlix_video_")
                    || name == "animeku_idlix_audio.m3u8"
                    || name == "animeku_track.lua"
                    || name == "animeku_last_pos.txt"
                    || name == "animeku_proxy_debug.txt"
                {
                    let _ = std::fs::remove_file(entry.path());
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

    eprintln!(
        "{}
{}
",
        r#"
               ▄▀█ █▄░█ █ █▀▄▀█ █▀▀ █▄▀ █░█ ▄▄ █▀▀ █░░ █
               █▀█ █░▀█ █ █░▀░█ ██▄ █░█ █▄█ ░░ █▄▄ █▄▄ █ "#
            .bright_green(),
        format!(
            "                     v{} {} val - Forked by Yuutaa7x",
            env!("CARGO_PKG_VERSION"),
            "©".cyan()
        )
    );
    Ok(())
}
