use std::fs;

fn load_history() -> Vec<String> {
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

fn save_history(title: &str) {
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

pub fn get_user_input() -> anyhow::Result<Option<Input>> {
    let mut options = vec!["[ Ketik Judul Pencarian Baru ]".to_string()];
    options.extend(load_history());

    let selection = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih Rekomendasi/Histori, atau ketik baru (q untuk keluar)")
        .items(&options)
        .default(0)
        .interact()?;

    let title = if selection == 0 {
        dialoguer::Input::with_theme(&crate::util::custom_theme())
            .with_prompt("Masukan judul (ketik 'q' untuk keluar)")
            .interact()?
    } else {
        options[selection].clone()
    };
    
    if title.trim().eq_ignore_ascii_case("q") {
        return Ok(None);
    }
    save_history(&title);

    let tipe = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih Provider")
        .item("Idlix (Animation & Movies)").item("Otakudesu")
        .default(0)
        .interact()?;

    Ok(Some(Input { title, tipe }))
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
