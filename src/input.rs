use std::collections::HashMap;
use std::fs;

use dialoguer::console::{style, truncate_str, Key, Term};

// ─── Search History ───────────────────────────────────────────────────────────

const MAX_SEARCH_HISTORY: usize = 50;

pub fn load_history() -> Vec<String> {
    let mut history_items = vec![];
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".animeku_history");
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() && !history_items.iter().any(|h: &String| h.eq_ignore_ascii_case(&trimmed)) {
                    history_items.push(trimmed);
                }
            }
        }
    }
    history_items.truncate(MAX_SEARCH_HISTORY);
    history_items
}

pub fn save_history(title: &str) {
    let Some(home) = dirs::home_dir() else { return; };
    let path = home.join(".animeku_history");
    let cleaned = title.trim().to_string();
    if cleaned.is_empty() { return; }

    let mut history = load_history();
    history.retain(|h| !h.eq_ignore_ascii_case(&cleaned));
    history.insert(0, cleaned);
    history.truncate(MAX_SEARCH_HISTORY);

    let new_content = if history.is_empty() {
        String::new()
    } else {
        history.join("\n") + "\n"
    };
    let _ = fs::write(path, new_content);
}

pub fn clear_search_history() {
    let Some(home) = dirs::home_dir() else { return; };
    let path = home.join(".animeku_history");
    let _ = fs::write(path, String::new());
    clear_search_providers();
}

// ─── Search provider cache ───────────────────────────────────────────────────
//
// The old search history is a plain newline file. Keeping provider choices in
// a sidecar JSON map lets existing history files keep working while letting a
// selected title reopen with the same provider next time.

const SEARCH_PROVIDER_FILE: &str = ".animeku_search_providers.json";

fn search_providers_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(SEARCH_PROVIDER_FILE))
}

fn load_search_providers() -> HashMap<String, usize> {
    let Some(path) = search_providers_path() else {
        return HashMap::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn write_search_providers(providers: &HashMap<String, usize>) {
    let Some(path) = search_providers_path() else {
        return;
    };
    if let Ok(content) = serde_json::to_string_pretty(providers) {
        let _ = fs::write(path, content);
    }
}

pub fn save_search_provider(title: &str, provider: usize) {
    let mut providers = load_search_providers();
    providers.insert(title.trim().to_lowercase(), provider);
    write_search_providers(&providers);
}

pub fn load_search_provider(title: &str) -> Option<usize> {
    load_search_providers()
        .get(&title.trim().to_lowercase())
        .copied()
}

pub fn delete_search_provider(title: &str) {
    let mut providers = load_search_providers();
    providers.remove(&title.trim().to_lowercase());
    write_search_providers(&providers);
}

pub fn clear_search_providers() {
    let Some(path) = search_providers_path() else {
        return;
    };
    let _ = fs::write(path, "{}");
}

// ─── AniList detection cache ─────────────────────────────────────────────────

const PROVIDER_DETECTION_FILE: &str = ".animeku_provider_detection.json";

fn provider_detection_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(PROVIDER_DETECTION_FILE))
}

fn load_provider_detections() -> HashMap<String, bool> {
    let Some(path) = provider_detection_path() else {
        return HashMap::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn load_provider_detection(title: &str) -> Option<bool> {
    load_provider_detections()
        .get(&title.trim().to_lowercase())
        .copied()
}

pub fn save_provider_detection(title: &str, is_anime: bool) {
    let mut detections = load_provider_detections();
    detections.insert(title.trim().to_lowercase(), is_anime);
    let Some(path) = provider_detection_path() else {
        return;
    };
    if let Ok(content) = serde_json::to_string_pretty(&detections) {
        let _ = fs::write(path, content);
    }
}

pub fn clear_provider_detections() {
    let Some(path) = provider_detection_path() else {
        return;
    };
    let _ = fs::write(path, "{}");
}

/// Force the provider menu, bypassing config/cache. Used by the history
/// screen's "Ganti Provider" action.
pub async fn choose_provider_manually() -> anyhow::Result<Option<usize>> {
    let labels = ["Idlix (Anime & Movies)", "Otakudesu (Anime)", "Kembali"];
    let entries = labels
        .iter()
        .map(|label| MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let Some(choice) = select_menu_simple("Pilih Provider:", &entries, 0)? else {
        return Ok(None);
    };
    match choice {
        0 => Ok(Some(0)),
        1 => Ok(Some(1)),
        _ => Ok(None),
    }
}

/// Remove a single title from the search history file.
pub fn delete_search_history_entry(title: &str) {
    let Some(home) = dirs::home_dir() else { return; };
    let path = home.join(".animeku_history");
    let mut history = load_history();
    let target = title.trim();
    history.retain(|h| !h.eq_ignore_ascii_case(target));
    let new_content = if history.is_empty() {
        String::new()
    } else {
        history.join("\n") + "\n"
    };
    let _ = fs::write(path, new_content);
    delete_search_provider(title);
}

// ─── Watch History ────────────────────────────────────────────────────────────

use crate::models::{Input, Movie, WatchEntry, WatchlistEntry};

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

pub fn clear_watch_history() {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(WATCH_HISTORY_FILE);
    let _ = fs::write(path, "[]");
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

// ─── Watchlist ───────────────────────────────────────────────────────────────

const WATCHLIST_FILE: &str = ".animeku_watchlist.json";

pub fn load_watchlist() -> Vec<WatchlistEntry> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let path = home.join(WATCHLIST_FILE);
    let Ok(content) = fs::read_to_string(path) else {
        return vec![];
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn write_watchlist(watchlist: &[WatchlistEntry]) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(WATCHLIST_FILE);
    if let Ok(content) = serde_json::to_string_pretty(watchlist) {
        let _ = fs::write(path, content);
    }
}

fn now_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn push_watchlist_entry(
    watchlist: &mut Vec<WatchlistEntry>,
    movie: &Movie,
    provider_type: usize,
    added_at: u64,
) {
    let known_episode_count = watchlist
        .iter()
        .find(|entry| entry.provider_type == provider_type && entry.movie.id == movie.id)
        .and_then(|entry| entry.known_episode_count);
    watchlist.retain(|entry| {
        !(entry.provider_type == provider_type && entry.movie.id == movie.id)
    });
    watchlist.insert(
        0,
        WatchlistEntry {
            movie: movie.clone(),
            provider_type,
            added_at,
            known_episode_count,
        },
    );
}

/// Add a title to the watchlist, or move it to the front if it already exists.
pub fn add_watchlist_entry(movie: &Movie, provider_type: usize) {
    let mut watchlist = load_watchlist();
    push_watchlist_entry(&mut watchlist, movie, provider_type, now_seconds());
    write_watchlist(&watchlist);
}

pub fn is_in_watchlist(provider_type: usize, movie_id: &str) -> bool {
    load_watchlist().iter().any(|entry| {
        entry.provider_type == provider_type && entry.movie.id == movie_id
    })
}

/// Remember how many episodes the provider listed when the watchlist was last
/// checked. Used by the "Cek Episode Baru" notification flow.
pub fn update_watchlist_episode_count(provider_type: usize, movie_id: &str, count: usize) {
    let mut watchlist = load_watchlist();
    if let Some(entry) = watchlist
        .iter_mut()
        .find(|entry| entry.provider_type == provider_type && entry.movie.id == movie_id)
    {
        if entry.known_episode_count != Some(count) {
            entry.known_episode_count = Some(count);
            write_watchlist(&watchlist);
        }
    }
}

pub fn delete_watchlist_entry(provider_type: usize, movie_id: &str) {
    let mut watchlist = load_watchlist();
    let original_len = watchlist.len();
    watchlist.retain(|entry| {
        !(entry.provider_type == provider_type && entry.movie.id == movie_id)
    });
    if watchlist.len() != original_len {
        write_watchlist(&watchlist);
    }
}

pub fn clear_watchlist() {
    write_watchlist(&[]);
}

// ─── Legacy wrappers (keep resume feature working) ───────────────────────────


// ─── Provider picker ─────────────────────────────────────────────────────────

/// Tanya jenis konten → tentukan provider.
/// Movie / Series → Idlix otomatis.
/// Anime → pilih Idlix atau Otakudesu.
pub async fn pick_provider(
    title: &str,
    is_from_suggestion: bool,
) -> anyhow::Result<Option<usize>> {
    use crossterm::style::Stylize;
    use std::io::Write;

    if let Some(provider) = crate::provider_override() {
        return Ok(Some(provider));
    }

    match crate::config::Config::load().provider {
        crate::config::ProviderPreference::Idlix => return Ok(Some(0)),
        crate::config::ProviderPreference::Otakudesu => return Ok(Some(1)),
        crate::config::ProviderPreference::Auto => {}
    }

    print!("{} Mendeteksi jenis konten... ", "◆".blue());
    std::io::stdout().flush().unwrap();

    let cached_detection = load_provider_detection(title);
    let mut is_anime = is_from_suggestion || cached_detection.unwrap_or(false);
    let mut detection_completed = false;

    if !is_anime && cached_detection.is_none() {
        let query_payload = serde_json::json!({
            "query": "query ($search: String) { Media(search: $search, type: ANIME, sort: POPULARITY_DESC) { title { romaji english } } }",
            "variables": { "search": title }
        });

        if let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(3)).build() {
            if let Ok(res) = client.post("https://graphql.anilist.co").json(&query_payload).send().await {
                if res.status().is_success() {
                    if let Ok(json) = res.json::<serde_json::Value>().await {
                        if let Some(media) = json.pointer("/data/Media") {
                            detection_completed = true;
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

    if detection_completed {
        save_provider_detection(title, is_anime);
    }

    if is_anime {
        println!(
            "\r{} Terdeteksi sebagai: {}                     ",
            "✓".green(),
            "Anime".cyan()
        );
        let labels = ["Idlix (Anime & Movies)", "Otakudesu (Anime)", "Kembali"];
        let entries = labels
            .iter()
            .map(|label| MenuEntry::new(*label))
            .collect::<Vec<_>>();
        let Some(choice) = select_menu_simple("Pilih Provider:", &entries, 0)? else {
            return Ok(None);
        };
        match choice {
            0 => Ok(Some(0)),
            1 => Ok(Some(1)),
            _ => Ok(None),
        }
    } else {
        if !detection_completed && cached_detection.is_none() && !is_from_suggestion {
            println!(
                "\r{} Deteksi tidak tersedia; memakai {} ...",
                "⚠".yellow(),
                "Idlix".cyan()
            );
        } else {
            println!(
                "\r{} Terdeteksi sebagai: {}                     ",
                "✓".green(),
                "Movie/Series (Non-Anime)".yellow()
            );
        }
        // Default to Idlix for movies/series
        Ok(Some(0))
    }
}

// ─── User input with history menu ────────────────────────────────────────────

const NEW_SEARCH_LABEL: &str = "[ Cari judul baru... ]";

/// Prompt untuk input judul baru (tanpa history dropdown).
/// History disimpan di "Watch History" terpisah.
pub async fn get_user_input() -> anyhow::Result<Option<Input>> {
    let history = load_history();

    loop {
        // Build items: first option is always "new search", rest are history.
        // A custom menu is used here so q + Enter can return instead of being
        // swallowed by a filter input.
        let mut entries = Vec::with_capacity(history.len() + 1);
        entries.push(MenuEntry::new(NEW_SEARCH_LABEL));
        entries.extend(history.iter().cloned().map(MenuEntry::new));

        let selection = select_menu_q_back(
            "Cari anime / film (q+Enter kembali, ketik untuk filter riwayat):",
            &entries,
            0,
        )?;

        let (title, is_sugg) = match selection {
            // User pressed Escape / q: back to the previous screen.
            None => return Ok(None),
            Some(0) => {
                // "New search" selected → use custom live async autocomplete!
                match crate::live_input::get_live_input("Masukkan judul anime/movie").await? {
                    Some((raw, is_s))
                        if !raw.trim().is_empty() && !raw.trim().eq_ignore_ascii_case("q") =>
                    {
                        (raw.trim().to_string(), is_s)
                    }
                    // Esc / q / empty input returns to the history menu.
                    _ => continue,
                }
            }
            Some(idx) => {
                // History item chosen directly
                (history[idx - 1].clone(), false)
            }
        };

        if title.eq_ignore_ascii_case("q") {
            return Ok(None);
        }

        // CLI/config provider choices take precedence over the per-title cache.
        let forced_provider = crate::provider_override().or_else(|| {
            match crate::config::Config::load().provider {
                crate::config::ProviderPreference::Idlix => Some(0),
                crate::config::ProviderPreference::Otakudesu => Some(1),
                crate::config::ProviderPreference::Auto => None,
            }
        });
        if let Some(tipe) = forced_provider.or_else(|| load_search_provider(&title)) {
            save_search_provider(&title, tipe);
            save_history(&title);
            return Ok(Some(Input { title, tipe }));
        }

        let Some(tipe) = pick_provider(&title, is_sugg).await? else {
            // Provider picker cancelled: return to the title menu instead of
            // dropping the user all the way back to the main menu.
            continue;
        };

        save_search_provider(&title, tipe);
        save_history(&title);

        return Ok(Some(Input { title, tipe }));
    }
}

// ─── Generic choice helper ───────────────────────────────────────────────────

pub fn choice<T: std::fmt::Display + Clone>(
    prompt: &str,
    options: Vec<T>,
    default: usize,
) -> anyhow::Result<Option<T>> {
    let default = default.min(options.len().saturating_sub(1));
    let entries = options
        .iter()
        .map(|option| MenuEntry::new(option.to_string()))
        .collect::<Vec<_>>();
    let config = MenuConfig {
        q_back: true,
        hint: Some("↑↓ pilih • Enter buka • ketik filter • q+Enter/Esc kembali"),
        ..Default::default()
    };
    match select_menu_custom(prompt, &entries, default, &config)? {
        MenuResult::Selected(index) => Ok(options.get(index).cloned()),
        _ => Ok(None),
    }
}

// ─── Menu with non-selectable separators ─────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MenuEntry {
    label: String,
    selectable: bool,
}

impl MenuEntry {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            selectable: true,
        }
    }

    pub fn separator(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            selectable: false,
        }
    }
}

fn first_selectable(entries: &[MenuEntry], visible: &[usize]) -> Option<usize> {
    visible
        .iter()
        .copied()
        .find(|&index| entries[index].selectable)
}

fn step_selectable(
    entries: &[MenuEntry],
    visible: &[usize],
    current: usize,
    forward: bool,
) -> usize {
    if visible.is_empty() {
        return current;
    }

    let position = visible
        .iter()
        .position(|&index| index == current)
        .unwrap_or(0);
    for offset in 1..=visible.len() {
        let next = if forward {
            (position + offset) % visible.len()
        } else {
            (position + visible.len() - (offset % visible.len())) % visible.len()
        };
        let index = visible[next];
        if entries[index].selectable {
            return index;
        }
    }
    current
}

/// Move the selection by up to `steps` selectable entries without wrapping.
/// Page keys should stop at the first/last item instead of cycling the list.
fn jump_selectable(
    entries: &[MenuEntry],
    visible: &[usize],
    current: usize,
    forward: bool,
    steps: usize,
) -> usize {
    let selectable = visible
        .iter()
        .copied()
        .filter(|&index| entries[index].selectable)
        .collect::<Vec<_>>();
    if selectable.is_empty() {
        return current;
    }

    let position = selectable
        .iter()
        .position(|&index| index == current)
        .unwrap_or(0);
    let target = if forward {
        (position + steps).min(selectable.len() - 1)
    } else {
        position.saturating_sub(steps)
    };
    selectable[target]
}

fn fit_line(text: &str, width: usize) -> String {
    truncate_str(text, width.max(1), "…").into_owned()
}

fn is_q_back(filter: &str, q_back: bool) -> bool {
    q_back && filter.trim().eq_ignore_ascii_case("q")
}

fn visible_entries(entries: &[MenuEntry], filter: &str) -> Vec<usize> {
    let query = filter.trim().to_lowercase();
    if query.is_empty() {
        (0..entries.len()).collect()
    } else {
        entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.selectable && entry.label.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }
}

/// Result of a menu that supports horizontal page navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuResult {
    Selected(usize),
    PreviousPage,
    NextPage,
    /// The user started typing in a paginated picker. The caller can open a
    /// global search with this text instead of filtering only the current page.
    SearchFilter(String),
    Cancelled,
}

/// Shared menu options. The defaults match the normal searchable picker:
/// typing filters entries and Escape cancels.
#[derive(Debug, Clone)]
pub struct MenuConfig<'a> {
    pub page_keys: bool,
    pub q_back: bool,
    pub allow_filter: bool,
    /// Return the first typed character as `SearchFilter` instead of filtering
    /// the entries that are currently visible.
    pub filter_returns_search: bool,
    pub initial_filter: String,
    pub hint: Option<&'a str>,
}

impl Default for MenuConfig<'_> {
    fn default() -> Self {
        Self {
            page_keys: false,
            q_back: false,
            allow_filter: true,
            filter_returns_search: false,
            initial_filter: String::new(),
            hint: None,
        }
    }
}

/// History/stream picker variant where typing `q` and pressing Enter cancels.
pub fn select_menu_q_back(
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
) -> anyhow::Result<Option<usize>> {
    let config = MenuConfig {
        q_back: true,
        hint: Some("↑↓ pilih • Enter buka • ketik filter • q+Enter/Esc kembali"),
        ..Default::default()
    };
    match run_menu(prompt, entries, default, &config)? {
        MenuResult::Selected(index) => Ok(Some(index)),
        _ => Ok(None),
    }
}

/// Simple arrow-key menu without text filtering. Used for main menus and
/// action menus where typing should not accidentally filter the choices.
pub fn select_menu_simple(
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
) -> anyhow::Result<Option<usize>> {
    let config = MenuConfig {
        q_back: true,
        allow_filter: false,
        hint: Some("↑↓ pilih • Enter buka • q/Esc kembali"),
        ..Default::default()
    };
    match run_menu(prompt, entries, default, &config)? {
        MenuResult::Selected(index) => Ok(Some(index)),
        _ => Ok(None),
    }
}

/// Like `select_menu`, but Left/Right arrows return page-navigation results
/// instead of being ignored. Used by the paginated episode picker.
pub fn select_menu_paged(
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
) -> anyhow::Result<MenuResult> {
    let config = MenuConfig {
        page_keys: true,
        filter_returns_search: true,
        hint: Some("↑↓ pilih • ←/→ halaman • ketik cari semua • Esc kembali"),
        ..Default::default()
    };
    run_menu(prompt, entries, default, &config)
}

pub fn select_menu_custom(
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
    config: &MenuConfig<'_>,
) -> anyhow::Result<MenuResult> {
    run_menu(prompt, entries, default, config)
}

fn run_menu(
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
    config: &MenuConfig<'_>,
) -> anyhow::Result<MenuResult> {
    if entries.iter().all(|entry| !entry.selectable) {
        anyhow::bail!("menu tidak memiliki pilihan");
    }

    let term = Term::stderr();
    term.hide_cursor()?;
    let result = run_select_menu(&term, prompt, entries, default, config);
    term.show_cursor()?;
    result
}

fn run_select_menu(
    term: &Term,
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
    config: &MenuConfig<'_>,
) -> anyhow::Result<MenuResult> {
    let mut filter = config.initial_filter.clone();
    let mut selected = {
        let visible = visible_entries(entries, &filter);
        let start = default.min(entries.len());
        visible
            .iter()
            .copied()
            .find(|&index| index >= start && entries[index].selectable)
            .or_else(|| first_selectable(entries, &visible))
    };
    let mut drawn_lines = 0usize;

    loop {
        if drawn_lines > 0 {
            term.clear_last_lines(drawn_lines)?;
        }

        let visible = visible_entries(entries, &filter);
        if selected.is_none_or(|index| {
            !visible.contains(&index) || !entries[index].selectable
        }) {
            selected = first_selectable(entries, &visible);
        }

        let (rows, columns) = term.size();
        let columns = columns as usize;
        let prompt_text = if filter.is_empty() {
            prompt.to_string()
        } else {
            format!("{} {}", prompt, filter)
        };
        let prompt_text = fit_line(&prompt_text, columns.saturating_sub(2));
        term.write_line(&format!("{} {}", style("?").cyan(), prompt_text))?;
        let mut lines = 1usize;

        if visible.is_empty() {
            let empty_text = if is_q_back(&filter, config.q_back) {
                "Tekan Enter untuk kembali"
            } else {
                "(tidak ada hasil)"
            };
            let empty = fit_line(empty_text, columns.saturating_sub(2));
            term.write_line(&format!("  {}", style(&empty).dim()))?;
            lines += 1;
        } else {
            let max_rows = (rows as usize)
                .saturating_sub(4)
                .clamp(1, 10)
                .min(visible.len());
            let item_width = columns.saturating_sub(2).max(1);
            let selected_position = selected
                .and_then(|index| visible.iter().position(|&item| item == index))
                .unwrap_or(0);
            let start = selected_position.saturating_sub(max_rows.saturating_sub(1));
            let end = (start + max_rows).min(visible.len());

            for &index in &visible[start..end] {
                let entry = &entries[index];
                let label = fit_line(&entry.label, item_width);
                if !entry.selectable {
                    term.write_line(&format!("  {}", style(&label).dim()))?;
                } else if Some(index) == selected {
                    term.write_line(&format!(
                        "{} {}",
                        style("❯").cyan(),
                        style(&label).cyan()
                    ))?;
                } else {
                    term.write_line(&format!("  {}", label))?;
                }
                lines += 1;
            }
        }

        if let Some(hint) = config.hint {
            let hint = fit_line(hint, columns.saturating_sub(2));
            term.write_line(&format!("  {}", style(&hint).dim()))?;
            lines += 1;
        }

        drawn_lines = lines;
        term.flush()?;

        match term.read_key()? {
            Key::ArrowUp | Key::BackTab => {
                if let Some(current) = selected {
                    selected = Some(step_selectable(entries, &visible, current, false));
                }
            }
            Key::ArrowDown | Key::Tab => {
                if let Some(current) = selected {
                    selected = Some(step_selectable(entries, &visible, current, true));
                }
            }
            Key::PageUp => {
                if let Some(current) = selected {
                    selected = Some(jump_selectable(entries, &visible, current, false, 10));
                }
            }
            Key::PageDown => {
                if let Some(current) = selected {
                    selected = Some(jump_selectable(entries, &visible, current, true, 10));
                }
            }
            Key::Home => {
                selected = first_selectable(entries, &visible);
            }
            Key::End => {
                selected = visible
                    .iter()
                    .rev()
                    .copied()
                    .find(|&index| entries[index].selectable);
            }
            Key::Backspace => {
                if config.allow_filter && !filter.is_empty() {
                    filter.pop();
                    let visible = visible_entries(entries, &filter);
                    selected = first_selectable(entries, &visible);
                }
            }
            Key::Char('q') | Key::Char('Q') if config.q_back && !config.allow_filter => {
                term.clear_last_lines(drawn_lines)?;
                term.flush()?;
                return Ok(MenuResult::Cancelled);
            }
            Key::Char(ch) if config.allow_filter && !ch.is_control() => {
                filter.push(ch);
                if config.filter_returns_search {
                    term.clear_last_lines(drawn_lines)?;
                    term.flush()?;
                    return Ok(MenuResult::SearchFilter(filter));
                }
                let visible = visible_entries(entries, &filter);
                selected = first_selectable(entries, &visible);
            }
            Key::CtrlC => {
                term.clear_last_lines(drawn_lines)?;
                term.flush()?;
                return Ok(MenuResult::Cancelled);
            }
            Key::Enter => {
                if is_q_back(&filter, config.q_back) {
                    term.clear_last_lines(drawn_lines)?;
                    term.flush()?;
                    return Ok(MenuResult::Cancelled);
                }
                if let Some(index) = selected {
                    term.clear_last_lines(drawn_lines)?;
                    let final_line = format!("? {} {}", prompt, entries[index].label);
                    let final_line = fit_line(&final_line, term.size().1 as usize);
                    term.write_line(&format!("{}", style(&final_line).cyan()))?;
                    term.flush()?;
                    return Ok(MenuResult::Selected(index));
                }
            }
            Key::ArrowLeft if config.page_keys => {
                term.clear_last_lines(drawn_lines)?;
                term.flush()?;
                return Ok(MenuResult::PreviousPage);
            }
            Key::ArrowRight if config.page_keys => {
                term.clear_last_lines(drawn_lines)?;
                term.flush()?;
                return Ok(MenuResult::NextPage);
            }
            Key::Escape => {
                term.clear_last_lines(drawn_lines)?;
                term.flush()?;
                return Ok(MenuResult::Cancelled);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<MenuEntry> {
        vec![
            MenuEntry::new("[ Kembali ke Pencarian ]"),
            MenuEntry::separator("────────────────────────"),
            MenuEntry::new("1. Takopii no Genzai"),
            MenuEntry::separator("────────────────────────"),
            MenuEntry::new("Selanjutnya (Halaman 2)"),
        ]
    }

    #[test]
    fn navigation_skips_separators_in_both_directions() {
        let entries = entries();
        let visible = visible_entries(&entries, "");

        assert_eq!(step_selectable(&entries, &visible, 0, true), 2);
        assert_eq!(step_selectable(&entries, &visible, 2, true), 4);
        assert_eq!(step_selectable(&entries, &visible, 4, true), 0);
        assert_eq!(step_selectable(&entries, &visible, 4, false), 2);
        assert_eq!(step_selectable(&entries, &visible, 2, false), 0);
    }

    #[test]
    fn filter_only_returns_selectable_matches() {
        let entries = entries();
        assert_eq!(visible_entries(&entries, "takopii"), vec![2]);
        assert_eq!(visible_entries(&entries, "halaman 2"), vec![4]);
        assert!(visible_entries(&entries, "tidak-ada").is_empty());
    }

    #[test]
    fn page_jumps_clamp_without_wrapping() {
        let entries = entries();
        let visible = visible_entries(&entries, "");

        assert_eq!(jump_selectable(&entries, &visible, 0, true, 10), 4);
        assert_eq!(jump_selectable(&entries, &visible, 4, false, 10), 0);
        assert_eq!(jump_selectable(&entries, &visible, 2, true, 1), 4);
        assert_eq!(jump_selectable(&entries, &visible, 2, false, 1), 0);
    }

    #[test]
    fn first_selectable_skips_a_leading_separator() {
        let entries = vec![
            MenuEntry::separator("────────────────────────"),
            MenuEntry::new("1. Takopii no Genzai"),
        ];
        let visible = visible_entries(&entries, "");
        assert_eq!(first_selectable(&entries, &visible), Some(1));
    }

    #[test]
    fn q_back_only_triggers_on_exact_q() {
        assert!(is_q_back("q", true));
        assert!(is_q_back(" Q ", true));
        assert!(!is_q_back("qu", true));
        assert!(!is_q_back("", true));
        assert!(!is_q_back("q", false));
    }

    #[test]
    fn watchlist_upsert_moves_existing_title_to_front() {
        let mut watchlist = Vec::new();
        let first = Movie {
            id: "first".into(),
            title: "First".into(),
            total_episodes: None,
        };
        let second = Movie {
            id: "second".into(),
            title: "Second".into(),
            total_episodes: None,
        };

        push_watchlist_entry(&mut watchlist, &first, 0, 1);
        push_watchlist_entry(&mut watchlist, &second, 0, 2);
        push_watchlist_entry(&mut watchlist, &first, 0, 3);

        assert_eq!(watchlist.len(), 2);
        assert_eq!(watchlist[0].movie.id, "first");
        assert_eq!(watchlist[0].added_at, 3);
        assert_eq!(watchlist[1].movie.id, "second");
    }

    #[test]
    fn long_labels_are_truncated_to_one_terminal_line() {
        let fitted = fit_line(
            "[ Lanjutkan: [Series] Takopi's Original Sin - S1E01 - To You in 2016 | 00:43 | Idlix ]",
            60,
        );
        assert!(dialoguer::console::measure_text_width(&fitted) <= 60);
        assert!(fitted.ends_with('…'));
    }
}
