use std::fs;

pub fn load_history() -> Vec<String> {
    let mut history_items = vec![];
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".animeku_history");
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() && !history_items.contains(&trimmed) {
                    history_items.push(trimmed);
                }
            }
        }
    }
    history_items
}

pub fn save_history(title: &str) {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".animeku_history");
        let history = load_history();
        let cleaned = title.trim().to_string();
        if !history.contains(&cleaned) {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "{}", cleaned);
            }
        }
    }
}

use crate::models::{Input, LastWatch};

const LAST_WATCH_FILE: &str = ".animeku_last_watch.json";

pub fn load_last_watch() -> Option<LastWatch> {
    let home = dirs::home_dir()?;
    let path = home.join(LAST_WATCH_FILE);
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_last_watch(last_watch: &LastWatch) {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(LAST_WATCH_FILE);
        if let Ok(content) = serde_json::to_string_pretty(last_watch) {
            let _ = fs::write(path, content);
        }
    }
}

/// Tanya jenis konten → tentukan provider.
/// Movie / Series → Idlix otomatis.
/// Anime → pilih Idlix atau Otakudesu.
pub fn pick_provider() -> anyhow::Result<usize> {
    let content_type = dialoguer::Select::with_theme(&crate::util::custom_theme())
        .with_prompt("Jenis konten")
        .items(&[
            "Movie",
            "Series (Live Action / Drama)",
            "Anime",
        ])
        .default(2)
        .interact()?;

    match content_type {
        // Movie atau Series → Idlix saja
        0 | 1 => Ok(0), // PROVIDER_IDLIX
        // Anime → tawarkan Idlix atau Otakudesu
        _ => {
            let provider = dialoguer::Select::with_theme(&crate::util::custom_theme())
                .with_prompt("Pilih Provider")
                .items(&["Idlix (Anime & Movies)", "Otakudesu (Anime)"])
                .default(0)
                .interact()?;
            Ok(provider)
        }
    }
}

/// Prompt untuk input judul baru (tanpa history dropdown).
/// History disimpan di "Watch History" terpisah.
pub fn get_user_input() -> anyhow::Result<Option<Input>> {
    let title: String = dialoguer::Input::with_theme(&crate::util::custom_theme())
        .with_prompt("Masukan judul anime/movie (q untuk kembali)")
        .interact()?;

    if title.trim().eq_ignore_ascii_case("q") {
        return Ok(None);
    }

    save_history(title.trim());

    let tipe = pick_provider()?;

    Ok(Some(Input { title: title.trim().to_string(), tipe }))
}

pub fn choice<T: std::fmt::Display + Clone>(options: Vec<T>, _fuzzy: bool) -> anyhow::Result<T> {
    let selected = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih")
        .default(0)
        .max_length(8)
        .items(&options)
        .interact()?;
    Ok(options[selected].clone())
}
