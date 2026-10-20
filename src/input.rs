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
    choice_with_default(options, 0)
}

pub fn choice_with_default<T: std::fmt::Display + Clone>(
    options: Vec<T>,
    default: usize,
) -> anyhow::Result<T> {
    let default = default.min(options.len().saturating_sub(1));
    let selected = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih")
        .default(default)
        .max_length(8)
        .items(&options)
        .interact()?;
    Ok(options[selected].clone())
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

/// Select from a list where separator rows are visible but can never become
/// the active row. Typing filters the selectable entries; Backspace removes the
/// filter and Escape cancels the selection.
pub fn select_menu(
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
) -> anyhow::Result<Option<usize>> {
    if entries.iter().all(|entry| !entry.selectable) {
        anyhow::bail!("menu tidak memiliki pilihan");
    }

    let term = Term::stderr();
    term.hide_cursor()?;
    let result = run_select_menu(&term, prompt, entries, default);
    term.show_cursor()?;
    result
}

fn run_select_menu(
    term: &Term,
    prompt: &str,
    entries: &[MenuEntry],
    default: usize,
) -> anyhow::Result<Option<usize>> {
    let mut filter = String::new();
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
            let empty = fit_line("(tidak ada hasil)", columns.saturating_sub(2));
            term.write_line(&format!("  {}", style(&empty).dim()))?;
            lines += 1;
        } else {
            let max_rows = (rows as usize)
                .saturating_sub(3)
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
                if !filter.is_empty() {
                    filter.pop();
                    let visible = visible_entries(entries, &filter);
                    selected = first_selectable(entries, &visible);
                }
            }
            Key::Char(ch) if !ch.is_control() => {
                filter.push(ch);
                let visible = visible_entries(entries, &filter);
                selected = first_selectable(entries, &visible);
            }
            Key::Enter => {
                if let Some(index) = selected {
                    term.clear_last_lines(drawn_lines)?;
                    let final_line = format!("? {} {}", prompt, entries[index].label);
                    let final_line = fit_line(&final_line, term.size().1 as usize);
                    term.write_line(&format!("{}", style(&final_line).cyan()))?;
                    term.flush()?;
                    return Ok(Some(index));
                }
            }
            Key::Escape => {
                term.clear_last_lines(drawn_lines)?;
                term.flush()?;
                return Ok(None);
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
    fn long_labels_are_truncated_to_one_terminal_line() {
        let fitted = fit_line(
            "[ Lanjutkan: [Series] Takopi's Original Sin - S1E01 - To You in 2016 | 00:43 | Idlix ]",
            60,
        );
        assert!(dialoguer::console::measure_text_width(&fitted) <= 60);
        assert!(fitted.ends_with('…'));
    }
}
