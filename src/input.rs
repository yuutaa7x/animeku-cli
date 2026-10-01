use std::fs;

// ─── Search History ───────────────────────────────────────────────────────────

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

/// Remove a single title from the search history file.
pub fn delete_search_history_entry(title: &str) {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".animeku_history");
        if let Ok(content) = fs::read_to_string(&path) {
            let filtered: Vec<&str> = content
                .lines()
                .filter(|l| l.trim() != title.trim())
                .collect();
            let new_content = filtered.join("\n") + if filtered.is_empty() { "" } else { "\n" };
            let _ = fs::write(path, new_content);
        }
    }
}

// ─── Watch History ────────────────────────────────────────────────────────────

use crate::models::{Input, WatchEntry};

const WATCH_HISTORY_FILE: &str = ".animeku_watch_history.json";
const MAX_WATCH_HISTORY: usize = 30;

pub fn load_watch_history() -> Vec<WatchEntry> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let path = home.join(WATCH_HISTORY_FILE);
    let Ok(content) = fs::read_to_string(path) else {
        return vec![];
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Insert or update an entry in the watch history.
///
/// - If an entry with the same `movie.id` AND `episode.id` already exists,
///   its `position_seconds` and `updated_at` are updated in-place and the
///   entry is moved to the front.
/// - Otherwise the new entry is inserted at the front.
/// - The list is trimmed to `MAX_WATCH_HISTORY` entries.
pub fn save_watch_entry(entry: &WatchEntry) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(WATCH_HISTORY_FILE);

    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut history = load_watch_history();

    // Remove any existing entry for the same movie+episode so we re-insert at front.
    history.retain(|e| !(e.movie.id == entry.movie.id && e.episode.id == entry.episode.id));

    let updated = WatchEntry {
        updated_at: now,
        ..entry.clone()
    };
    history.insert(0, updated);
    history.truncate(MAX_WATCH_HISTORY);

    if let Ok(content) = serde_json::to_string_pretty(&history) {
        let _ = fs::write(path, content);
    }
}

/// Remove the entry at `index` from the watch history.
pub fn delete_watch_entry(index: usize) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(WATCH_HISTORY_FILE);
    let mut history = load_watch_history();
    if index < history.len() {
        history.remove(index);
        if let Ok(content) = serde_json::to_string_pretty(&history) {
            let _ = fs::write(path, content);
        }
    }
}

// ─── Legacy wrappers (keep resume feature working) ───────────────────────────


// ─── Provider picker ─────────────────────────────────────────────────────────

/// Tanya jenis konten → tentukan provider.
/// Movie / Series → Idlix otomatis.
/// Anime → pilih Idlix atau Otakudesu.
pub async fn pick_provider(title: &str, is_from_suggestion: bool) -> anyhow::Result<usize> {
    use std::io::Write;
    use crossterm::style::Stylize;
    
    print!("{} Mendeteksi jenis konten... ", "◆".blue());
    std::io::stdout().flush().unwrap();

    let mut is_anime = is_from_suggestion;

    if !is_anime {
        let query_payload = serde_json::json!({
            "query": "query ($search: String) { Media(search: $search, type: ANIME, sort: POPULARITY_DESC) { title { romaji english } } }",
            "variables": { "search": title }
        });

        if let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(3)).build() {
            if let Ok(res) = client.post("https://graphql.anilist.co").json(&query_payload).send().await {
                if res.status().is_success() {
                    if let Ok(json) = res.json::<serde_json::Value>().await {
                        if let Some(media) = json.pointer("/data/Media") {
                            let romaji = media.pointer("/title/romaji").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
                            let english = media.pointer("/title/english").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
                            let t = title.to_lowercase();
                            
                            if !t.is_empty() && (romaji == t || english == t || romaji.starts_with(&t) || english.starts_with(&t)) {
                                is_anime = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if is_anime {
        println!("\r{} Terdeteksi sebagai: {}                     ", "✓".green(), "Anime".cyan());
        let provider = dialoguer::Select::with_theme(&crate::util::custom_theme())
            .with_prompt("Pilih Provider")
            .items(&["Idlix (Anime & Movies)", "Otakudesu (Anime)"])
            .default(0)
            .interact()?;
        Ok(provider)
    } else {
        println!("\r{} Terdeteksi sebagai: {}                     ", "✓".green(), "Movie/Series (Non-Anime)".yellow());
        // Default to Idlix for movies/series
        Ok(0)
    }
}

// ─── User input with history FuzzySelect ─────────────────────────────────────

const NEW_SEARCH_LABEL: &str = "[ Cari judul baru... ]";

/// Prompt untuk input judul baru (tanpa history dropdown).
/// History disimpan di "Watch History" terpisah.
pub async fn get_user_input() -> anyhow::Result<Option<Input>> {
    let history = load_history();

    // Build items: first option is always "new search", rest are history
    let mut items: Vec<String> = vec![NEW_SEARCH_LABEL.to_string()];
    items.extend(history);

    let selection = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
        .with_prompt("Cari anime/movie (q untuk kembali, ketik untuk filter histori):")
        .default(0)
        .items(&items)
        .interact_opt()?;

    let (title, is_sugg) = match selection {
        // User pressed Escape / closed
        None => return Ok(None),
        Some(0) => {
            // "New search" selected → use custom live async autocomplete!
            let res = crate::live_input::get_live_input("Masukan judul anime/movie").await?;
            let (raw, is_s) = res.unwrap_or_default();
            if raw.trim().eq_ignore_ascii_case("q") || raw.trim().is_empty() {
                return Ok(None);
            }
            (raw.trim().to_string(), is_s)
        }
        Some(idx) => {
            // History item chosen directly
            (items[idx].clone(), false)
        }
    };

    if title.eq_ignore_ascii_case("q") {
        return Ok(None);
    }

    save_history(&title);

    let tipe = pick_provider(&title, is_sugg).await?;

    Ok(Some(Input { title, tipe }))
}

// ─── Generic choice helper ───────────────────────────────────────────────────

pub fn choice<T: std::fmt::Display + Clone>(options: Vec<T>, _fuzzy: bool) -> anyhow::Result<T> {
    let selected = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih")
        .default(0)
        .max_length(8)
        .items(&options)
        .interact()?;
    Ok(options[selected].clone())
}
