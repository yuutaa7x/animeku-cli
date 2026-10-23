use std::collections::{HashMap, HashSet};
use std::io::{stdout, IsTerminal, Write};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use colored::Colorize;
use crossterm::{
    cursor::MoveToPreviousLine,
    execute,
    terminal::{Clear, ClearType},
};

use crate::{
    batch::{BatchItemStatus, BatchQueue},
    download::{DownloadProgress, DownloadProgressCallback},
    ext::{self, Ext},
    input,
    models::{Episode, Meta, Movie, Stream},
    util::show_image_thumb,
};

pub struct AnimekuCli {
    /// Cached page results together with the provider's total page count.
    movie_cache: HashMap<String, (Vec<Movie>, u64)>,
    episode_cache: HashMap<String, (Vec<Episode>, Meta)>,
    stream_cache: HashMap<String, Vec<Stream>>,
    metadata_cache: Option<crate::cache::MetadataCache>,
    extractor: Box<dyn Ext>,
    provider_type: usize,
    fallback_enabled: bool,
    current_movie: Option<Movie>,
    current_episode: Option<Episode>,
}

const SEARCH_SEPARATOR: &str = "────────────────────────";
const EPISODES_PER_PAGE: usize = 25;
const PROVIDER_ATTEMPTS: usize = 3;
const BATCH_DOWNLOAD_ATTEMPTS: usize = 3;

/// Retry transient provider failures before giving up or switching providers.
/// A short backoff keeps the UI responsive while still smoothing over momentary
/// network hiccups.
async fn retry_search(
    extractor: &mut dyn Ext,
    title: &str,
    page: usize,
) -> anyhow::Result<(Vec<Movie>, u64)> {
    let mut last_error = None;
    for attempt in 0..PROVIDER_ATTEMPTS {
        match extractor.search(title.to_string(), page).await {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < PROVIDER_ATTEMPTS {
                    let delay = 250u64 * (1u64 << attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("permintaan provider gagal")))
}

async fn retry_episodes(
    extractor: &dyn Ext,
    movie: &Movie,
) -> anyhow::Result<(Vec<Episode>, Meta)> {
    let mut last_error = None;
    for attempt in 0..PROVIDER_ATTEMPTS {
        match extractor.get_episodes(movie.clone()).await {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < PROVIDER_ATTEMPTS {
                    let delay = 250u64 * (1u64 << attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("permintaan provider gagal")))
}

async fn retry_streams(
    extractor: &dyn Ext,
    episode: &Episode,
) -> anyhow::Result<Vec<Stream>> {
    let mut last_error = None;
    for attempt in 0..PROVIDER_ATTEMPTS {
        match extractor.get_stream_urls(episode.clone()).await {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < PROVIDER_ATTEMPTS {
                    let delay = 250u64 * (1u64 << attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("permintaan provider gagal")))
}

fn alternate_provider(provider_type: usize) -> usize {
    match provider_type {
        ext::PROVIDER_OTAKUDESU => ext::PROVIDER_IDLIX,
        _ => ext::PROVIDER_OTAKUDESU,
    }
}

/// Idlix prefixes titles with `[Movie]`/`[Series]`; Otakudesu adds episode
/// ranges and "Subtitle Indonesia". Normalise both before comparing results
/// across providers.
fn fallback_movie_title(movie: &Movie) -> String {
    let raw = movie.title.trim();
    let without_tag = raw
        .strip_prefix("[Movie]")
        .or_else(|| raw.strip_prefix("[Series]"))
        .unwrap_or(raw)
        .trim_start();
    let cleaned = crate::models::clean_movie_title(without_tag);
    if cleaned.is_empty() {
        without_tag.to_string()
    } else {
        cleaned
    }
}

fn normalize_title_text(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn best_movie_match(target: &Movie, candidates: &[Movie]) -> Option<Movie> {
    let target_norm = normalize_title_text(&fallback_movie_title(target));
    if target_norm.is_empty() {
        return None;
    }

    if let Some(movie) = candidates
        .iter()
        .find(|movie| normalize_title_text(&fallback_movie_title(movie)) == target_norm)
    {
        return Some(movie.clone());
    }

    let mut partial = candidates
        .iter()
        .filter(|movie| {
            let candidate = normalize_title_text(&fallback_movie_title(movie));
            !candidate.is_empty()
                && (candidate.contains(&target_norm) || target_norm.contains(&candidate))
        })
        .collect::<Vec<_>>();
    partial.sort_by_key(|movie| {
        normalize_title_text(&fallback_movie_title(movie))
            .len()
            .abs_diff(target_norm.len())
    });
    partial.first().map(|movie| (*movie).clone())
}

fn normalized_episode_title(title: &str) -> String {
    normalize_title_text(&crate::models::clean_episode_title(title))
}

fn best_episode_match(target: &Episode, candidates: &[Episode]) -> Option<Episode> {
    let target_norm = normalized_episode_title(&target.title);
    if !target_norm.is_empty() {
        if let Some(episode) = candidates
            .iter()
            .find(|episode| normalized_episode_title(&episode.title) == target_norm)
        {
            return Some(episode.clone());
        }
    }

    if let Some((season, number)) = crate::episode::parse_season_episode(&target.title) {
        if let Some(episode) = candidates.iter().find(|episode| {
            matches!(
                crate::episode::parse_season_episode(&episode.title),
                Some((candidate_season, candidate_number))
                    if candidate_season == season && candidate_number == number
            )
        }) {
            return Some(episode.clone());
        }
    }

    if let Some(number) = crate::episode::episode_number(&target.title) {
        if let Some(episode) = candidates
            .iter()
            .find(|episode| crate::episode::episode_number(&episode.title) == Some(number))
        {
            return Some(episode.clone());
        }
    }

    if !target.is_series {
        return candidates
            .iter()
            .find(|episode| !episode.is_series)
            .cloned();
    }

    if candidates.len() == 1 {
        return candidates.first().cloned();
    }

    None
}

fn notify_fallback(from: usize, to: usize, reason: &str) {
    eprintln!(
        "{} {} bermasalah: {}",
        "⚠".yellow(),
        ext::provider_name(from),
        reason.trim()
    );
    eprintln!(
        "  {} Ini bukan error fatal. animeku-cli mencoba {} sebagai cadangan dan mencocokkan judul/episode.",
        "ℹ".blue(),
        ext::provider_name(to).cyan()
    );
}

fn notify_fallback_success(to: usize) {
    eprintln!(
        "{} Beralih ke {}. ID judul/episode sudah disesuaikan.",
        "✓".green(),
        ext::provider_name(to).green()
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchAction {
    Back,
    Previous,
    Next,
    Separator,
    Select,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EpisodeAction {
    Back,
    ToggleWatchlist,
    Search,
    Previous,
    Next,
    Separator,
    Select(usize),
}

#[derive(Clone)]
struct SearchMenuItem {
    label: String,
    action: SearchAction,
    movie: Option<Movie>,
}

impl std::fmt::Display for SearchMenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// Build the search menu in a stable order: back navigation first, numbered
/// results in the middle, and page navigation at the bottom. The next-page
/// entry is only shown when the provider reports that another page exists.
fn build_search_menu(
    movies: Vec<Movie>,
    page: usize,
    total_pages: u64,
    is_latest: bool,
) -> Vec<SearchMenuItem> {
    let mut items = Vec::with_capacity(movies.len() + 5);
    items.push(SearchMenuItem {
        label: "[ Kembali ke Pencarian ]".into(),
        action: SearchAction::Back,
        movie: None,
    });
    items.push(SearchMenuItem {
        label: SEARCH_SEPARATOR.into(),
        action: SearchAction::Separator,
        movie: None,
    });

    for (index, movie) in movies.into_iter().enumerate() {
        items.push(SearchMenuItem {
            label: format!("{}. {}", index + 1, movie),
            action: SearchAction::Select,
            movie: Some(movie),
        });
    }

    let has_previous = page > 1;
    let has_next = !is_latest && (page as u64) < total_pages;
    if has_previous || has_next {
        items.push(SearchMenuItem {
            label: SEARCH_SEPARATOR.into(),
            action: SearchAction::Separator,
            movie: None,
        });
    }
    if has_previous {
        items.push(SearchMenuItem {
            label: format!("Sebelumnya (Halaman {})", page - 1),
            action: SearchAction::Previous,
            movie: None,
        });
    }
    if has_next {
        items.push(SearchMenuItem {
            label: format!("Selanjutnya (Halaman {})", page + 1),
            action: SearchAction::Next,
            movie: None,
        });
    }

    items
}

/// Per-title playback state used to add ✔/▶ markers to the episode picker.
#[derive(Debug, Default)]
struct EpisodeWatchState {
    watched: HashSet<String>,
    last: Option<String>,
}

impl EpisodeWatchState {
    fn from_movie(movie_id: &str) -> Self {
        let history = input::load_watch_history();
        let mut state = Self::default();
        for entry in history {
            if entry.movie.id != movie_id {
                continue;
            }
            if state.last.is_none() {
                state.last = Some(entry.episode.id.clone());
            }
            state.watched.insert(entry.episode.id);
        }
        state
    }

    fn label(&self, episode: &Episode) -> String {
        let prefix = if self.last.as_deref() == Some(episode.id.as_str()) {
            "▶ "
        } else if self.watched.contains(&episode.id) {
            "✓ "
        } else {
            "  "
        };
        format!("{}{}", prefix, episode.title.trim())
    }
}

/// Build one page of the episode picker. Search and page-navigation entries
/// are selectable so keyboard navigation can reach them without special cases.
fn build_episode_menu(
    episodes: &[Episode],
    page: usize,
    total_pages: u64,
    watch_state: &EpisodeWatchState,
    watchlisted: bool,
) -> (Vec<input::MenuEntry>, Vec<EpisodeAction>) {
    let total_pages = total_pages.max(1);
    let page = page.clamp(1, total_pages as usize);
    let mut entries = Vec::with_capacity(EPISODES_PER_PAGE + 6);
    let mut actions = Vec::with_capacity(EPISODES_PER_PAGE + 6);

    entries.push(input::MenuEntry::new("[ Kembali ke Pilih Anime ]"));
    actions.push(EpisodeAction::Back);

    entries.push(input::MenuEntry::new(if watchlisted {
        "[ ★ Hapus dari Watchlist ]"
    } else {
        "[ ☆ Tambah ke Watchlist ]"
    }));
    actions.push(EpisodeAction::ToggleWatchlist);

    if total_pages > 1 {
        entries.push(input::MenuEntry::new("[ Cari Episode (semua halaman) ]"));
        actions.push(EpisodeAction::Search);
    }

    entries.push(input::MenuEntry::separator(SEARCH_SEPARATOR));
    actions.push(EpisodeAction::Separator);

    // Keep page navigation above the episode list. With 25 episodes per page
    // it would otherwise be buried below several screens of entries.
    let has_previous = page > 1;
    let has_next = (page as u64) < total_pages;
    if has_previous {
        entries.push(input::MenuEntry::new(format!(
            "Sebelumnya (Halaman {})",
            page - 1
        )));
        actions.push(EpisodeAction::Previous);
    }
    if has_next {
        entries.push(input::MenuEntry::new(format!(
            "Selanjutnya (Halaman {})",
            page + 1
        )));
        actions.push(EpisodeAction::Next);
    }
    if has_previous || has_next {
        entries.push(input::MenuEntry::separator(SEARCH_SEPARATOR));
        actions.push(EpisodeAction::Separator);
    }

    let start = (page - 1) * EPISODES_PER_PAGE;
    let end = (start + EPISODES_PER_PAGE).min(episodes.len());
    for (index, episode) in episodes.iter().enumerate().take(end).skip(start) {
        entries.push(input::MenuEntry::new(watch_state.label(episode)));
        actions.push(EpisodeAction::Select(index));
    }

    (entries, actions)
}

/// Search across every episode, not just the current page. `select_menu`'s
/// built-in typing filter makes this the quickest way to jump to episode 900+.
fn build_episode_search_menu(
    episodes: &[Episode],
    watch_state: &EpisodeWatchState,
) -> (Vec<input::MenuEntry>, Vec<EpisodeAction>) {
    let mut entries = Vec::with_capacity(episodes.len() + 2);
    let mut actions = Vec::with_capacity(episodes.len() + 2);

    entries.push(input::MenuEntry::new("[ Kembali ke Daftar Episode ]"));
    actions.push(EpisodeAction::Back);
    entries.push(input::MenuEntry::separator(SEARCH_SEPARATOR));
    actions.push(EpisodeAction::Separator);

    for (index, episode) in episodes.iter().enumerate() {
        entries.push(input::MenuEntry::new(watch_state.label(episode)));
        actions.push(EpisodeAction::Select(index));
    }

    (entries, actions)
}

fn select_episode_globally(
    episodes: &[Episode],
    watch_state: &EpisodeWatchState,
    initial_filter: String,
) -> anyhow::Result<Option<Episode>> {
    let (entries, actions) = build_episode_search_menu(episodes, watch_state);
    let default_index = actions
        .iter()
        .position(|action| matches!(action, EpisodeAction::Select(_)))
        .unwrap_or(0);
    let config = input::MenuConfig {
        q_back: true,
        initial_filter,
        hint: Some("↑↓ pilih • ketik filter • Backspace hapus • q+Enter/Esc kembali"),
        ..Default::default()
    };
    match input::select_menu_custom("Cari Episode:", &entries, default_index, &config)? {
        input::MenuResult::Selected(index) => match actions[index] {
            EpisodeAction::Select(index) => Ok(Some(episodes[index].clone())),
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn choose_batch_episode(
    prompt: &str,
    episodes: &[Episode],
    default: usize,
    number_offset: usize,
) -> anyhow::Result<Option<usize>> {
    if episodes.is_empty() {
        return Ok(None);
    }

    let entries = episodes
        .iter()
        .enumerate()
        .map(|(index, episode)| {
            input::MenuEntry::new(format!(
                "{:>3}. {}",
                number_offset + index + 1,
                episode.title.trim()
            ))
        })
        .collect::<Vec<_>>();
    let config = input::MenuConfig {
        q_back: true,
        hint: Some("↑↓ pilih • ketik filter • q+Enter/Esc kembali"),
        ..Default::default()
    };
    match input::select_menu_custom(prompt, &entries, default.min(entries.len() - 1), &config)? {
        input::MenuResult::Selected(index) => Ok(Some(index)),
        _ => Ok(None),
    }
}

fn batch_quality_key(title: &str) -> String {
    let trimmed = title.trim();
    let Some(hash) = trimmed.rfind(" #") else {
        return trimmed.to_string();
    };
    let after = &trimmed[hash + 2..];
    let Some(end) = after.find(']') else {
        return trimmed.to_string();
    };

    let mut key = String::with_capacity(trimmed.len());
    key.push_str(&trimmed[..hash]);
    key.push_str(&after[end..]);
    key
}

fn choose_batch_stream<'a>(streams: &'a [Stream], target_title: &str) -> Option<&'a Stream> {
    let target = batch_quality_key(target_title);
    streams
        .iter()
        .find(|stream| batch_quality_key(&stream.title) == target)
        .or_else(|| streams.first())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BatchItemOutcome {
    Completed,
    Skipped,
    Failed(String),
    Cancelled,
}

fn batch_eta(done: usize, total: usize, elapsed: Duration) -> String {
    if done == 0 {
        "—".to_string()
    } else if done >= total {
        "00:00".to_string()
    } else {
        let per_item = elapsed.as_secs_f64() / done as f64;
        let remaining = per_item * (total - done) as f64;
        crate::models::format_seconds(remaining.round() as u64)
    }
}

fn format_batch_status(
    done: usize,
    total: usize,
    completed: usize,
    skipped: usize,
    failed: usize,
    elapsed: Duration,
) -> String {
    format!(
        "Batch {}/{} • {} berhasil • {} dilewati • {} gagal • sisa ~{}",
        done,
        total,
        completed,
        skipped,
        failed,
        batch_eta(done, total, elapsed)
    )
}

fn print_batch_status(
    done: usize,
    total: usize,
    completed: usize,
    skipped: usize,
    failed: usize,
    started: Instant,
) {
    println!(
        "  {} {}",
        "•".dimmed(),
        format_batch_status(done, total, completed, skipped, failed, started.elapsed())
    );
}

const BATCH_PROGRESS_BAR_WIDTH: usize = 10;
const BATCH_MAX_VISIBLE_ROWS: usize = 12;

fn batch_episode_label(index: usize, episode: &Episode) -> String {
    if let Some((season, number)) = crate::episode::parse_season_episode(&episode.title) {
        return if season > 1 {
            format!("S{}E{:02}", season, number)
        } else {
            format!("E{:02}", number)
        };
    }
    if let Some(number) = crate::episode::episode_number(&episode.title) {
        return format!("E{:02}", number);
    }
    format!("E{:02}", index + 1)
}

fn batch_progress_bar(ratio: f64) -> String {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * BATCH_PROGRESS_BAR_WIDTH as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled.min(BATCH_PROGRESS_BAR_WIDTH)),
        "░".repeat(BATCH_PROGRESS_BAR_WIDTH.saturating_sub(filled))
    )
}

/// Live list used while a sequential batch runs. Only one episode is active at
/// a time; the rest stay visible as `pending`, `selesai`, `dilewati`, or
/// `gagal`, which makes the queue much easier to scan than a single status
/// line printed after every episode.
struct BatchListUi {
    labels: Vec<String>,
    statuses: Vec<BatchItemStatus>,
    active: Option<usize>,
    progress: DownloadProgress,
    drawn_lines: usize,
    enabled: bool,
    cancelled: bool,
    last_size: u64,
    last_rate_at: Instant,
    bytes_per_sec: f64,
}

impl BatchListUi {
    fn new(
        episodes: &[Episode],
        statuses: &[BatchItemStatus],
        active: Option<usize>,
        enabled: bool,
    ) -> Self {
        Self {
            labels: episodes
                .iter()
                .enumerate()
                .map(|(index, episode)| batch_episode_label(index, episode))
                .collect(),
            statuses: statuses.to_vec(),
            active,
            progress: DownloadProgress::default(),
            drawn_lines: 0,
            enabled,
            cancelled: false,
            last_size: 0,
            last_rate_at: Instant::now(),
            bytes_per_sec: 0.0,
        }
    }

    fn set_active(&mut self, index: usize) {
        self.active = Some(index);
        self.progress = DownloadProgress::default();
        self.cancelled = false;
        self.last_size = 0;
        self.last_rate_at = Instant::now();
        self.bytes_per_sec = 0.0;
        self.draw();
    }

    fn set_progress(&mut self, progress: DownloadProgress) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_rate_at).as_secs_f64();
        if elapsed >= 0.20 {
            if progress.total_size >= self.last_size {
                let instant = (progress.total_size - self.last_size) as f64 / elapsed;
                if instant > 0.0 {
                    self.bytes_per_sec = if self.bytes_per_sec <= 0.0 {
                        instant
                    } else {
                        self.bytes_per_sec * 0.65 + instant * 0.35
                    };
                }
            } else {
                self.bytes_per_sec = 0.0;
            }
            self.last_size = progress.total_size;
            self.last_rate_at = now;
        }
        self.progress = progress;
        self.draw();
    }

    fn set_status(&mut self, index: usize, status: BatchItemStatus) {
        if let Some(slot) = self.statuses.get_mut(index) {
            *slot = status;
        }
        if self.active == Some(index) {
            self.active = None;
        }
        self.draw();
    }

    fn mark_cancelled(&mut self) {
        self.cancelled = true;
        self.draw();
    }

    fn log_line(&mut self, message: &str) {
        if !self.enabled {
            println!("{message}");
            return;
        }
        self.clear_block();
        println!("{message}");
        self.drawn_lines = 0;
        self.draw();
    }

    fn finish(&mut self) {
        self.draw();
        self.drawn_lines = 0;
    }

    fn clear_block(&mut self) {
        if !self.enabled || self.drawn_lines == 0 {
            return;
        }
        let mut out = stdout();
        execute!(out, MoveToPreviousLine(self.drawn_lines as u16)).ok();
        for index in 0..self.drawn_lines {
            execute!(out, Clear(ClearType::CurrentLine)).ok();
            if index + 1 < self.drawn_lines {
                let _ = writeln!(out);
            }
        }
        if self.drawn_lines > 1 {
            execute!(out, MoveToPreviousLine((self.drawn_lines - 1) as u16)).ok();
        }
        let _ = out.flush();
        self.drawn_lines = 0;
    }

    fn draw(&mut self) {
        if !self.enabled || self.labels.is_empty() {
            return;
        }
        let (start, end) = self.visible_range();
        let width = crossterm::terminal::size()
            .map(|(columns, _)| columns as usize)
            .unwrap_or(80)
            .saturating_sub(1)
            .max(20);
        let lines = (start..end)
            .map(|index| {
                let line = self.row(index);
                dialoguer::console::truncate_str(&line, width, "…").into_owned()
            })
            .collect::<Vec<_>>();

        let mut out = stdout();
        let old_lines = self.drawn_lines;
        if old_lines > 0 {
            execute!(out, MoveToPreviousLine(old_lines as u16)).ok();
        }
        for line in &lines {
            execute!(out, Clear(ClearType::CurrentLine)).ok();
            let _ = writeln!(out, "{line}");
        }
        if lines.len() < old_lines {
            for index in lines.len()..old_lines {
                execute!(out, Clear(ClearType::CurrentLine)).ok();
                if index + 1 < old_lines {
                    let _ = writeln!(out);
                }
            }
            let move_up = old_lines.saturating_sub(lines.len() + 1);
            if move_up > 0 {
                execute!(out, MoveToPreviousLine(move_up as u16)).ok();
            }
        }
        let _ = out.flush();
        self.drawn_lines = lines.len();
    }

    fn visible_range(&self) -> (usize, usize) {
        let total = self.labels.len();
        if total <= BATCH_MAX_VISIBLE_ROWS {
            return (0, total);
        }
        let active = self.active.unwrap_or(0);
        let half = BATCH_MAX_VISIBLE_ROWS / 2;
        let start = active
            .saturating_sub(half)
            .min(total - BATCH_MAX_VISIBLE_ROWS);
        (start, start + BATCH_MAX_VISIBLE_ROWS)
    }

    fn row(&self, index: usize) -> String {
        let prefix = format!(
            "[{}/{}] {}",
            index + 1,
            self.labels.len(),
            self.labels[index]
        );
        if self.active == Some(index) {
            if self.cancelled {
                return format!("{prefix}  ■ dibatalkan");
            }
            let progress = &self.progress;
            let speed = self.format_rate();
            if progress.paused {
                return format!("{prefix}  ‖ dijeda");
            }
            if let Some(duration) = progress.duration.filter(|duration| *duration > 0.0) {
                let ratio = if progress.finished {
                    1.0
                } else {
                    (progress.out_seconds / duration).clamp(0.0, 1.0)
                };
                return format!(
                    "{prefix}  {} {:3.0}%  {}",
                    batch_progress_bar(ratio),
                    ratio * 100.0,
                    speed
                );
            }
            return format!(
                "{prefix}  ⬇ {:>6}  {:>8}  {}",
                crate::models::format_seconds(progress.out_seconds as u64),
                crate::download::format_bytes(progress.total_size),
                speed
            );
        }

        match self.statuses.get(index).copied().unwrap_or_default() {
            BatchItemStatus::Pending => format!("{prefix}  pending      -        -"),
            BatchItemStatus::Completed => format!("{prefix}  ✓ selesai"),
            BatchItemStatus::Skipped => format!("{prefix}  ↷ dilewati"),
            BatchItemStatus::Failed => format!("{prefix}  ■ gagal"),
        }
    }

    fn format_rate(&self) -> String {
        if self.bytes_per_sec >= 1.0 {
            format!(
                "{}/s",
                crate::download::format_bytes(self.bytes_per_sec as u64)
            )
        } else if !self.progress.speed.is_empty() {
            self.progress.speed.clone()
        } else {
            "—".to_string()
        }
    }
}

#[derive(Clone)]
struct BatchDownloadControls {
    pause: Arc<AtomicBool>,
    ui: Option<Arc<Mutex<BatchListUi>>>,
}

fn batch_log(ui: &Option<Arc<Mutex<BatchListUi>>>, message: impl Into<String>) {
    let message = message.into();
    match ui {
        Some(ui) => {
            if let Ok(mut ui) = ui.lock() {
                ui.log_line(&message);
            }
        }
        None => println!("{message}"),
    }
}

impl AnimekuCli {
    fn cached_search(
        &self,
        provider: usize,
        title: &str,
        page: usize,
    ) -> Option<(Vec<Movie>, u64)> {
        self.metadata_cache
            .as_ref()?
            .get_search(provider, title, page)
    }

    fn remember_search(
        &mut self,
        provider: usize,
        title: &str,
        page: usize,
        movies: Vec<Movie>,
        total_pages: u64,
    ) {
        if let Some(cache) = self.metadata_cache.as_mut() {
            cache.put_search(provider, title, page, movies, total_pages);
        }
    }

    fn cached_episodes(
        &self,
        provider: usize,
        movie_id: &str,
    ) -> Option<(Vec<Episode>, Meta)> {
        self.metadata_cache
            .as_ref()?
            .get_episodes(provider, movie_id)
    }

    fn remember_episodes(
        &mut self,
        provider: usize,
        movie_id: &str,
        episodes: Vec<Episode>,
        meta: Meta,
    ) {
        if let Some(cache) = self.metadata_cache.as_mut() {
            cache.put_episodes(provider, movie_id, episodes, meta);
        }
    }

    pub fn get_meta(&self, id: &str) -> Option<crate::models::Meta> {
        self.episode_cache.get(id).map(|(_, m)| m.clone())
    }

    /// Load and cache movie metadata without showing the episode picker.
    /// Used when resuming from history, where only the stored IDs are available.
    pub async fn ensure_meta(&mut self, movie: &Movie) -> Option<Meta> {
        self.current_movie = Some(movie.clone());
        self.current_episode = None;
        if !self.episode_cache.contains_key(&movie.id) {
            let provider = self.provider_type;
            if let Some(cached) = self.cached_episodes(provider, &movie.id) {
                self.episode_cache.insert(movie.id.clone(), cached);
            } else {
                let message = format!("Memuat metadata '{}'...", movie.title.trim());
                let episodes_and_meta = crate::util::with_spinner(
                    message,
                    retry_episodes(self.extractor.as_ref(), movie),
                )
                .await
                .ok()?;
                if !episodes_and_meta.0.is_empty() {
                    self.remember_episodes(
                        provider,
                        &movie.id,
                        episodes_and_meta.0.clone(),
                        episodes_and_meta.1.clone(),
                    );
                }
                self.episode_cache
                    .insert(movie.id.clone(), episodes_and_meta);
            }
        }
        self.get_meta(&movie.id)
    }

    pub fn get_episode_list(&self, id: &str) -> Option<Vec<Episode>> {
        self.episode_cache.get(id).map(|(e, _)| e.clone())
    }

    /// Constructor for tests and other callers that supply their own extractor.
    /// Automatic provider fallback is disabled here so an empty stub result
    /// cannot accidentally hit the network.
    #[cfg(test)]
    pub fn new(extractor: Box<dyn Ext>) -> Self {
        Self::build(extractor, ext::PROVIDER_IDLIX, false)
    }

    /// Production constructor: the provider is known, and if it fails or returns
    /// no data the matching provider on the other side is tried automatically.
    pub fn for_provider(provider_type: usize) -> Self {
        Self::build(
            ext::make_extractor(provider_type),
            provider_type,
            true,
        )
    }

    fn build(extractor: Box<dyn Ext>, provider_type: usize, fallback_enabled: bool) -> Self {
        let provider_type = if provider_type == ext::PROVIDER_OTAKUDESU {
            ext::PROVIDER_OTAKUDESU
        } else {
            ext::PROVIDER_IDLIX
        };
        Self {
            movie_cache: HashMap::new(),
            episode_cache: HashMap::new(),
            stream_cache: HashMap::new(),
            // Tests use the fallback-disabled constructor and must not read or
            // write the real user cache.
            metadata_cache: fallback_enabled.then(crate::cache::MetadataCache::load),
            extractor,
            provider_type,
            fallback_enabled,
            current_movie: None,
            current_episode: None,
        }
    }

    pub fn provider_type(&self) -> usize {
        self.provider_type
    }

    pub fn current_movie(&self) -> Option<&Movie> {
        self.current_movie.as_ref()
    }

    pub fn current_episode(&self) -> Option<&Episode> {
        self.current_episode.as_ref()
    }

    fn switch_provider(&mut self, provider_type: usize) {
        self.provider_type = provider_type;
        self.extractor = ext::make_extractor(provider_type);
        // IDs and cached pages are provider-specific. Keeping them would make
        // an episode from one site look valid on the other.
        self.movie_cache.clear();
        self.episode_cache.clear();
        self.stream_cache.clear();
        self.current_movie = None;
        self.current_episode = None;
    }

    async fn try_search_fallback(
        &mut self,
        title: &str,
        reason: &str,
    ) -> Option<(Vec<Movie>, u64)> {
        if !self.fallback_enabled {
            return None;
        }
        let from = self.provider_type;
        let to = alternate_provider(from);
        notify_fallback(from, to, reason);

        if let Some((movies, total_pages)) = self.cached_search(to, title, 1) {
            self.switch_provider(to);
            notify_fallback_success(to);
            return Some((movies, total_pages));
        }

        let mut extractor = ext::make_extractor(to);
        match retry_search(extractor.as_mut(), title, 1).await {
            Ok((movies, total_pages)) if !movies.is_empty() => {
                self.switch_provider(to);
                notify_fallback_success(to);
                Some((movies, total_pages))
            }
            Ok(_) => {
                eprintln!(
                    "{} {} juga tidak menemukan hasil.",
                    "■".red(),
                    ext::provider_name(to)
                );
                None
            }
            Err(error) => {
                eprintln!(
                    "{} {} juga gagal: {:#}",
                    "■".red(),
                    ext::provider_name(to),
                    error
                );
                None
            }
        }
    }

    async fn try_fallback_episodes(
        &mut self,
        movie: &Movie,
        reason: &str,
    ) -> Option<(Vec<Episode>, Meta)> {
        if !self.fallback_enabled {
            return None;
        }
        let from = self.provider_type;
        let to = alternate_provider(from);
        notify_fallback(from, to, reason);

        let title = fallback_movie_title(movie);
        if title.is_empty() {
            return None;
        }

        let mut extractor = ext::make_extractor(to);
        let candidates = if let Some((movies, _)) = self.cached_search(to, &title, 1) {
            movies
        } else {
            match retry_search(extractor.as_mut(), &title, 1).await {
                Ok((movies, total_pages)) if !movies.is_empty() => {
                    self.remember_search(
                        to,
                        &title,
                        1,
                        movies.clone(),
                        total_pages,
                    );
                    movies
                }
                Ok(_) => {
                    eprintln!(
                        "{} {} juga tidak menemukan judul yang cocok.",
                        "■".red(),
                        ext::provider_name(to)
                    );
                    return None;
                }
                Err(error) => {
                    eprintln!(
                        "{} {} juga gagal mencari judul: {:#}",
                        "■".red(),
                        ext::provider_name(to),
                        error
                    );
                    return None;
                }
            }
        };

        let Some(fallback_movie) = best_movie_match(movie, &candidates) else {
            eprintln!(
                "{} {} tidak menemukan judul yang cocok untuk '{}'.",
                "■".red(),
                ext::provider_name(to),
                title
            );
            return None;
        };

        let (episodes, meta) =
            if let Some(cached) = self.cached_episodes(to, &fallback_movie.id) {
                cached
            } else {
                match retry_episodes(extractor.as_ref(), &fallback_movie).await {
                    Ok(item) if !item.0.is_empty() => {
                        self.remember_episodes(
                            to,
                            &fallback_movie.id,
                            item.0.clone(),
                            item.1.clone(),
                        );
                        item
                    }
                    Ok(_) => {
                        eprintln!(
                            "{} {} juga tidak memiliki daftar episode.",
                            "■".red(),
                            ext::provider_name(to)
                        );
                        return None;
                    }
                    Err(error) => {
                        eprintln!(
                            "{} {} juga gagal memuat episode: {:#}",
                            "■".red(),
                            ext::provider_name(to),
                            error
                        );
                        return None;
                    }
                }
            };

        self.switch_provider(to);
        self.current_movie = Some(fallback_movie.clone());
        self.episode_cache
            .insert(fallback_movie.id.clone(), (episodes.clone(), meta.clone()));
        notify_fallback_success(to);
        Some((episodes, meta))
    }

    async fn try_fallback_streams(
        &mut self,
        episode: &Episode,
        reason: &str,
    ) -> Option<Vec<Stream>> {
        if !self.fallback_enabled {
            return None;
        }
        let movie = self.current_movie.clone()?;
        let from = self.provider_type;
        let to = alternate_provider(from);
        notify_fallback(from, to, reason);

        let title = fallback_movie_title(&movie);
        if title.is_empty() {
            return None;
        }

        let mut extractor = ext::make_extractor(to);
        let candidates = if let Some((movies, _)) = self.cached_search(to, &title, 1) {
            movies
        } else {
            match retry_search(extractor.as_mut(), &title, 1).await {
                Ok((movies, total_pages)) if !movies.is_empty() => {
                    self.remember_search(
                        to,
                        &title,
                        1,
                        movies.clone(),
                        total_pages,
                    );
                    movies
                }
                Ok(_) => {
                    eprintln!(
                        "{} {} juga tidak menemukan judul yang cocok.",
                        "■".red(),
                        ext::provider_name(to)
                    );
                    return None;
                }
                Err(error) => {
                    eprintln!(
                        "{} {} juga gagal mencari judul: {:#}",
                        "■".red(),
                        ext::provider_name(to),
                        error
                    );
                    return None;
                }
            }
        };

        let Some(fallback_movie) = best_movie_match(&movie, &candidates) else {
            eprintln!(
                "{} {} tidak menemukan judul yang cocok untuk '{}'.",
                "■".red(),
                ext::provider_name(to),
                title
            );
            return None;
        };

        let (episodes, meta) =
            if let Some(cached) = self.cached_episodes(to, &fallback_movie.id) {
                cached
            } else {
                match retry_episodes(extractor.as_ref(), &fallback_movie).await {
                    Ok(item) if !item.0.is_empty() => {
                        self.remember_episodes(
                            to,
                            &fallback_movie.id,
                            item.0.clone(),
                            item.1.clone(),
                        );
                        item
                    }
                    Ok(_) => {
                        eprintln!(
                            "{} {} juga tidak memiliki daftar episode.",
                            "■".red(),
                            ext::provider_name(to)
                        );
                        return None;
                    }
                    Err(error) => {
                        eprintln!(
                            "{} {} juga gagal memuat episode: {:#}",
                            "■".red(),
                            ext::provider_name(to),
                            error
                        );
                        return None;
                    }
                }
            };

        let Some(fallback_episode) = best_episode_match(episode, &episodes) else {
            eprintln!(
                "{} {} tidak menemukan episode yang cocok untuk '{}'.",
                "■".red(),
                ext::provider_name(to),
                episode.title.trim()
            );
            return None;
        };

        let streams =
            match retry_streams(extractor.as_ref(), &fallback_episode).await {
                Ok(streams) if !streams.is_empty() => streams,
                Ok(_) => {
                    eprintln!(
                        "{} {} juga tidak memiliki server stream.",
                        "■".red(),
                        ext::provider_name(to)
                    );
                    return None;
                }
                Err(error) => {
                    eprintln!(
                        "{} {} juga gagal memuat stream: {:#}",
                        "■".red(),
                        ext::provider_name(to),
                        error
                    );
                    return None;
                }
            };

        self.switch_provider(to);
        self.current_movie = Some(fallback_movie.clone());
        self.current_episode = Some(fallback_episode.clone());
        self.episode_cache
            .insert(fallback_movie.id.clone(), (episodes, meta));
        if to != crate::PROVIDER_IDLIX {
            self.stream_cache
                .insert(fallback_episode.id.clone(), streams.clone());
        }
        notify_fallback_success(to);
        Some(streams)
    }

    pub async fn search(&mut self, search_title: &str) -> anyhow::Result<Option<Movie>> {
        let mut page = 1usize;
        let mut is_latest = false;

        loop {
            let page_key = page.to_string();
            if !self.movie_cache.contains_key(&page_key) {
                if let Some((movies, total_pages)) =
                    self.cached_search(self.provider_type, search_title, page)
                {
                    if page == 1 && !movies.is_empty() {
                        println!(
                            "{} Ditemukan {} judul di {} (cache)",
                            "✓".green(),
                            movies.len().to_string().green(),
                            ext::provider_name(self.provider_type).cyan()
                        );
                    }
                    self.movie_cache
                        .insert(page_key.clone(), (movies, total_pages));
                }
            }

            if let std::collections::hash_map::Entry::Vacant(_) =
                self.movie_cache.entry(page_key.clone())
            {
                let (movie_list, total_pages) = loop {
                    let spinner_message = format!("Mencari '{}'...", search_title);
                    let result = crate::util::with_spinner(
                        spinner_message,
                        retry_search(self.extractor.as_mut(), search_title, page),
                    )
                    .await;
                    match result {
                        Ok((movies, total)) if !movies.is_empty() || page != 1 => {
                            break (movies, total);
                        }
                        Ok((movies, total)) => {
                            match self
                                .try_search_fallback(search_title, "tidak ada hasil")
                                .await
                            {
                                Some(result) => break result,
                                None => {
                                    if page == 1 {
                                        println!();
                                        println!(
                                            "Provider tidak menemukan hasil. Periksa ejaan judul atau coba provider lain."
                                        );
                                        if input::retry_or_back("Coba pencarian lagi?")? {
                                            continue;
                                        }
                                        return Ok(None);
                                    }
                                    break (movies, total);
                                }
                            }
                        }
                        Err(error) => {
                            if page == 1 {
                                if let Some(result) = self
                                    .try_search_fallback(search_title, &format!("{:#}", error))
                                    .await
                                {
                                    break result;
                                }
                            }
                            eprintln!(
                                "{} Gagal mencari '{}' di {}: {:#}",
                                "■".red(),
                                search_title,
                                ext::provider_name(self.provider_type),
                                error
                            );
                            println!();
                            println!("Periksa koneksi internet lalu coba lagi.");
                            if input::retry_or_back("Coba lagi tanpa keluar aplikasi?")? {
                                continue;
                            }
                            return Ok(None);
                        }
                    }
                };
                if page == 1 && !movie_list.is_empty() {
                    println!(
                        "{} Ditemukan {} judul di {}",
                        "✓".green(),
                        movie_list.len().to_string().green(),
                        ext::provider_name(self.provider_type).cyan()
                    );
                }
                if !movie_list.is_empty() {
                    let provider = self.provider_type;
                    self.remember_search(
                        provider,
                        search_title,
                        page,
                        movie_list.clone(),
                        total_pages,
                    );
                }
                self.movie_cache
                    .insert(page_key, (movie_list, total_pages.max(1)));
            }

            let (page_movies, total_pages) =
                self.movie_cache.get(&page.to_string()).unwrap().clone();

            if page_movies.is_empty() {
                if page == 1 {
                    eprintln!(
                        "tidak ditemukan!\n{} {}",
                        "■".red(),
                        "Silakan periksa kembali kata kunci pencarian\n".yellow()
                    );
                    println!("Tekan Enter untuk kembali ke pencarian...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                    return Ok(None);
                }
                page -= 1;
                is_latest = true;
                continue;
            }

            let result_count = page_movies.len();
            let menu = build_search_menu(page_movies, page, total_pages, is_latest);
            // Keep the cursor on the first actual result; "Kembali" and the
            // separators stay non-default so Enter cannot pick them by mistake.
            let default_index = menu
                .iter()
                .position(|item| item.action == SearchAction::Select)
                .unwrap_or(0);
            let entries = menu
                .iter()
                .map(|item| match item.action {
                    SearchAction::Separator => input::MenuEntry::separator(item.label.clone()),
                    _ => input::MenuEntry::new(item.label.clone()),
                })
                .collect::<Vec<_>>();
            let prompt = if total_pages > 1 {
                format!(
                    "Pilih Judul (Halaman {}/{}, {} hasil):",
                    page, total_pages, result_count
                )
            } else {
                format!("Pilih Judul ({} hasil):", result_count)
            };
            let config = input::MenuConfig {
                page_keys: true,
                q_back: true,
                hint: Some("↑↓ pilih • ←/→ halaman • ketik filter • q+Enter/Esc kembali"),
                ..Default::default()
            };
            let selected =
                match input::select_menu_custom(&prompt, &entries, default_index, &config)? {
                    input::MenuResult::Selected(index) => index,
                    input::MenuResult::PreviousPage => {
                        if page > 1 {
                            page -= 1;
                        }
                        continue;
                    }
                    input::MenuResult::NextPage => {
                        if !is_latest && (page as u64) < total_pages {
                            page += 1;
                        }
                        continue;
                    }
                    input::MenuResult::SearchFilter(_) | input::MenuResult::Cancelled => {
                        return Ok(None);
                    }
                };
            let item = match menu.into_iter().nth(selected) {
                Some(item) => item,
                None => continue,
            };
            match item.action {
                SearchAction::Back => return Ok(None),
                SearchAction::Previous => page -= 1,
                SearchAction::Next => page += 1,
                SearchAction::Separator => continue,
                SearchAction::Select => {
                    if let Some(movie) = item.movie {
                        self.current_movie = Some(movie.clone());
                        self.current_episode = None;
                        return Ok(Some(movie));
                    }
                    continue;
                }
            }
        }
    }

    pub async fn extract_episode(&mut self, movie: Movie) -> anyhow::Result<Option<Episode>> {
        self.current_movie = Some(movie.clone());
        self.current_episode = None;
        let id = movie.id.clone();
        let spinner_message = format!("Memuat daftar episode '{}'...", movie.title.trim());
        let provider = self.provider_type;
        let mut from_cache = false;

        let (episodes, meta) = loop {
            if let Some(cached) = self.episode_cache.get(&id) {
                from_cache = true;
                break cached.clone();
            }
            if let Some(cached) = self.cached_episodes(provider, &id) {
                from_cache = true;
                self.episode_cache.insert(id.clone(), cached.clone());
                break cached;
            }

            let result = crate::util::with_spinner(
                spinner_message.clone(),
                retry_episodes(self.extractor.as_ref(), &movie),
            )
            .await;

            match result {
                Ok(item) if !item.0.is_empty() => {
                    self.remember_episodes(provider, &id, item.0.clone(), item.1.clone());
                    self.episode_cache.insert(id.clone(), item.clone());
                    break item;
                }
                Ok(_) => match self
                    .try_fallback_episodes(&movie, "daftar episode kosong")
                    .await
                {
                    Some(fallback) => break fallback,
                    None => {
                        println!();
                        println!("Provider tidak memiliki daftar episode yang bisa dipakai.");
                        if input::retry_or_back("Coba muat episode lagi?")? {
                            continue;
                        }
                        return Ok(None);
                    }
                },
                Err(error) => match self
                    .try_fallback_episodes(&movie, &format!("{:#}", error))
                    .await
                {
                    Some(fallback) => break fallback,
                    None => {
                        eprintln!(
                            "{} Gagal memuat episode dari {}: {:#}",
                            "■".red(),
                            ext::provider_name(self.provider_type),
                            error
                        );
                        println!();
                        if input::retry_or_back("Coba lagi tanpa keluar aplikasi?")? {
                            continue;
                        }
                        return Ok(None);
                    }
                },
            }
        };

        let active_movie = self.current_movie.clone().unwrap_or_else(|| movie.clone());
        let active_id = active_movie.id.clone();
        let active_provider = self.provider_type;
        let mut watchlisted = input::is_in_watchlist(active_provider, &active_id);
        let watch_state = EpisodeWatchState::from_movie(&active_id);
        if episodes.is_empty() {
            eprintln!(
                "{} {}",
                "■".red(),
                "Daftar episode tidak tersedia untuk judul ini.\n".yellow()
            );
            println!("Tekan Enter untuk kembali ke daftar judul...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(None);
        }

        println!(
            "{} Daftar episode dimuat dari {}{}",
            "✓".green(),
            ext::provider_name(self.provider_type).cyan(),
            if from_cache { " (cache)" } else { "" }
        );
        println!(
            "\n  {}{}\n",
            " ".repeat(45 / 2 - 5),
            " Deskripsi ".black().on_truecolor(252, 136, 3)
        );

        if crate::config::Config::load().show_posters {
            if let Some(ref thumb_url) = meta.thumb_url {
                show_image_thumb(thumb_url.to_string()).await;
            }
        }

        let mut has_episode = false;
        for (k, v) in meta.data {
            if k.to_lowercase().contains("episode") {
                has_episode = true;
            }
            println!("  {} : {}", k.bright_white(), v);
        }
        if !has_episode {
            println!("  {} : {}", "Episodes".bright_white(), episodes.len());
        }
        println!();

        // A full movie has exactly one non-series entry, so an episode picker
        // would only ask the user to confirm the only possible choice.
        if episodes.len() == 1 && !episodes[0].is_series {
            return Ok(Some(episodes[0].clone()));
        }

        let total_pages = episodes.len().div_ceil(EPISODES_PER_PAGE) as u64;
        let mut page = 1usize;

        loop {
            let (entries, actions) =
                build_episode_menu(&episodes, page, total_pages, &watch_state, watchlisted);
            let default_index = actions
                .iter()
                .position(|action| matches!(action, EpisodeAction::Select(_)))
                .unwrap_or(0);
            let prompt = if total_pages > 1 {
                format!("Pilih Episode (Halaman {}/{} • ←/→):", page, total_pages)
            } else {
                "Pilih Episode:".to_string()
            };

            let selected = match input::select_menu_paged(&prompt, &entries, default_index)? {
                input::MenuResult::Selected(index) => index,
                input::MenuResult::PreviousPage => {
                    page = page.saturating_sub(1).max(1);
                    continue;
                }
                input::MenuResult::NextPage => {
                    page = (page + 1).min(total_pages as usize);
                    continue;
                }
                input::MenuResult::SearchFilter(query) => {
                    if let Some(episode) = select_episode_globally(&episodes, &watch_state, query)?
                    {
                        return Ok(Some(episode));
                    }
                    continue;
                }
                input::MenuResult::Cancelled => return Ok(None),
            };

            match actions[selected] {
                EpisodeAction::Back => return Ok(None),
                EpisodeAction::ToggleWatchlist => {
                    if watchlisted {
                        input::delete_watchlist_entry(active_provider, &active_movie.id);
                        println!("{} Dihapus dari watchlist.", "✓".green());
                    } else {
                        input::add_watchlist_entry(&active_movie, active_provider);
                        println!("{} Ditambahkan ke watchlist.", "✓".green());
                    }
                    watchlisted = !watchlisted;
                }
                EpisodeAction::Previous => {
                    page = page.saturating_sub(1).max(1);
                }
                EpisodeAction::Next => {
                    page = (page + 1).min(total_pages as usize);
                }
                EpisodeAction::Search => {
                    if let Some(episode) =
                        select_episode_globally(&episodes, &watch_state, String::new())?
                    {
                        return Ok(Some(episode));
                    }
                }
                EpisodeAction::Separator => {}
                EpisodeAction::Select(index) => return Ok(Some(episodes[index].clone())),
            }
        }
    }

    async fn download_batch_episode(
        &self,
        movie: &Movie,
        episode: &Episode,
        target_title: &str,
        directory: &Path,
        existing: crate::download::ExistingFilePolicy,
        controls: BatchDownloadControls,
    ) -> BatchItemOutcome {
        let streams = match retry_streams(self.extractor.as_ref(), episode).await {
            Ok(streams) if !streams.is_empty() => streams,
            Ok(_) => {
                batch_log(
                    &controls.ui,
                    format!("{} Server stream kosong.", "■".red()),
                );
                return BatchItemOutcome::Failed("server stream kosong".into());
            }
            Err(error) => {
                batch_log(
                    &controls.ui,
                    format!("{} Gagal memuat stream: {:#}", "■".red(), error),
                );
                return BatchItemOutcome::Failed(format!("gagal memuat stream: {error:#}"));
            }
        };

        let Some(stream) = choose_batch_stream(&streams, target_title) else {
            batch_log(
                &controls.ui,
                format!("{} Server stream kosong.", "■".red()),
            );
            return BatchItemOutcome::Failed("server stream kosong".into());
        };
        if batch_quality_key(&stream.title) != batch_quality_key(target_title) {
            batch_log(
                &controls.ui,
                format!(
                    "{} Kualitas '{}' tidak tersedia, memakai '{}'.",
                    "ℹ".blue(),
                    target_title,
                    stream.title
                ),
            );
        }

        let output = crate::download::batch_output_path(
            self.provider_type,
            stream,
            movie,
            episode,
            directory,
        );
        if output.is_dir() {
            batch_log(
                &controls.ui,
                format!(
                    "{} '{}' adalah folder, bukan nama file yang valid.",
                    "■".red(),
                    output.display()
                ),
            );
            return BatchItemOutcome::Failed(format!(
                "'{}' adalah folder, bukan nama file yang valid",
                output.display()
            ));
        }
        if output.exists() {
            match existing {
                crate::download::ExistingFilePolicy::Skip => {
                    batch_log(
                        &controls.ui,
                        format!("{} Dilewati: file sudah ada.", "↷".yellow()),
                    );
                    return BatchItemOutcome::Skipped;
                }
                crate::download::ExistingFilePolicy::Overwrite => {}
            }
        }

        let progress_callback = controls.ui.as_ref().map(|ui| {
            let ui = Arc::clone(ui);
            Arc::new(move |progress: DownloadProgress| {
                if let Ok(mut ui) = ui.lock() {
                    ui.set_progress(progress);
                }
            }) as DownloadProgressCallback
        });

        for attempt in 1..=BATCH_DOWNLOAD_ATTEMPTS {
            let result = match &progress_callback {
                Some(callback) => crate::download::download_to_path_with_progress(
                    self.provider_type,
                    stream,
                    &output,
                    Arc::clone(&controls.pause),
                    Arc::clone(callback),
                ),
                None => crate::download::download_to_path_with_pause(
                    self.provider_type,
                    stream,
                    &output,
                    Arc::clone(&controls.pause),
                ),
            };
            match result {
                Ok(crate::download::DownloadResult::Completed) => {
                    return BatchItemOutcome::Completed;
                }
                Ok(crate::download::DownloadResult::Cancelled) => {
                    return BatchItemOutcome::Cancelled;
                }
                Err(error) if attempt < BATCH_DOWNLOAD_ATTEMPTS => {
                    batch_log(
                        &controls.ui,
                        format!(
                            "{} Download gagal (percobaan {}/{}): {:#}. Mencoba ulang...",
                            "■".red(),
                            attempt,
                            BATCH_DOWNLOAD_ATTEMPTS,
                            error
                        ),
                    );
                    let backoff = Duration::from_secs(1u64 << (attempt - 1));
                    tokio::time::sleep(backoff).await;
                }
                Err(error) => {
                    batch_log(
                        &controls.ui,
                        format!(
                            "{} Download gagal setelah {} percobaan: {:#}",
                            "■".red(),
                            BATCH_DOWNLOAD_ATTEMPTS,
                            error
                        ),
                    );
                    return BatchItemOutcome::Failed(format!("{error:#}"));
                }
            }
        }

        BatchItemOutcome::Failed("download gagal".into())
    }

    /// Run every pending or previously failed item in a persisted queue.
    /// Completed/skipped items are never downloaded again.
    pub async fn run_batch(&mut self, queue: &mut BatchQueue) -> anyhow::Result<()> {
        let total = queue.episodes.len();
        let pending = queue.pending_indices();
        if pending.is_empty() {
            println!("{} Semua episode di antrean sudah selesai.", "✓".green());
            BatchQueue::clear();
            return Ok(());
        }

        let directory = std::path::PathBuf::from(&queue.directory);
        let started = Instant::now();
        let already_done = total.saturating_sub(pending.len());
        let mut processed = 0usize;
        let mut cancelled = false;
        let mut paused = false;
        // Shared with ffmpeg's input listener. `p` pauses on Unix and always
        // stops the queue after the current episode.
        let pause = Arc::new(AtomicBool::new(false));

        let list_enabled = stdout().is_terminal() && crossterm::terminal::size().is_ok();
        let ui = Arc::new(Mutex::new(BatchListUi::new(
            &queue.episodes,
            &queue.statuses,
            None,
            list_enabled,
        )));
        if list_enabled {
            if let Ok(mut ui) = ui.lock() {
                ui.draw();
            }
        }
        let controls = BatchDownloadControls {
            pause: Arc::clone(&pause),
            ui: list_enabled.then(|| Arc::clone(&ui)),
        };

        for index in pending {
            let episode = queue.episodes[index].clone();
            if list_enabled {
                if let Ok(mut ui) = ui.lock() {
                    ui.set_active(index);
                }
            } else {
                println!(
                    "\n{} [{}/{}] {}",
                    "◆".blue(),
                    index + 1,
                    total,
                    episode.title.trim()
                );
            }

            let outcome = self
                .download_batch_episode(
                    &queue.movie,
                    &episode,
                    &queue.target_quality,
                    &directory,
                    queue.existing,
                    controls.clone(),
                )
                .await;

            match outcome {
                BatchItemOutcome::Completed => {
                    queue.mark(index, BatchItemStatus::Completed);
                    if let Ok(mut ui) = ui.lock() {
                        ui.set_status(index, BatchItemStatus::Completed);
                    }
                }
                BatchItemOutcome::Skipped => {
                    queue.mark(index, BatchItemStatus::Skipped);
                    if let Ok(mut ui) = ui.lock() {
                        ui.set_status(index, BatchItemStatus::Skipped);
                    }
                }
                BatchItemOutcome::Failed(error) => {
                    if !list_enabled {
                        eprintln!("{} {}", "■".red(), error);
                    }
                    queue.log_error(&episode, &error);
                    queue.mark(index, BatchItemStatus::Failed);
                    if let Ok(mut ui) = ui.lock() {
                        ui.set_status(index, BatchItemStatus::Failed);
                    }
                }
                BatchItemOutcome::Cancelled => {
                    if let Ok(mut ui) = ui.lock() {
                        ui.mark_cancelled();
                    }
                    cancelled = true;
                    break;
                }
            }

            processed += 1;
            let _ = queue.save();
            let (completed, skipped, failed, _) = queue.counts();
            if !cancelled && !list_enabled {
                print_batch_status(
                    already_done + processed,
                    total,
                    completed,
                    skipped,
                    failed,
                    started,
                );
            }

            if pause.load(Ordering::SeqCst) {
                paused = true;
                break;
            }
        }

        if list_enabled {
            if let Ok(mut ui) = ui.lock() {
                ui.finish();
            }
        }

        let elapsed = started.elapsed().as_secs();
        let (completed, skipped, failed, remaining_count) = queue.counts();
        if cancelled {
            println!(
                "\n{} Batch dihentikan: {} berhasil • {} dilewati • {} gagal • {} belum diproses ({}).",
                "■".yellow(),
                completed,
                skipped,
                failed,
                remaining_count,
                crate::models::format_seconds(elapsed)
            );
            println!(
                "  Antrean disimpan. Pilih menu 'Lanjutkan Batch' untuk melanjutkan nanti."
            );
        } else if paused {
            println!(
                "\n{} Batch dijeda: {} berhasil • {} dilewati • {} gagal • {} tertunda ({}).",
                "■".yellow(),
                completed,
                skipped,
                failed,
                remaining_count,
                crate::models::format_seconds(elapsed)
            );
            println!(
                "  Antrean disimpan. Pilih menu 'Kelola Download' → 'Lanjutkan' untuk melanjutkan."
            );
            let _ = queue.save();
        } else if remaining_count == 0 {
            println!(
                "\n{} Batch selesai: {} berhasil • {} dilewati • {} gagal • total {}.",
                if failed == 0 {
                    "✓".green()
                } else {
                    "■".yellow()
                },
                completed,
                skipped,
                failed,
                crate::models::format_seconds(elapsed)
            );
            if failed > 0 {
                println!(
                    "  Log error: {}",
                    directory.join("animeku-batch.log").display()
                );
            }
            BatchQueue::clear();
        } else {
            println!(
                "\n{} Batch selesai dengan {} episode tertunda.",
                "■".yellow(),
                remaining_count
            );
            println!(
                "  Antrean disimpan. Pilih menu 'Lanjutkan Batch' untuk mencoba lagi."
            );
            if failed > 0 {
                println!(
                    "  Log error: {}",
                    directory.join("animeku-batch.log").display()
                );
            }
            let _ = queue.save();
        }

        Ok(())
    }

    pub async fn resolve_streams_for_episode(
        &mut self,
        episode: &Episode,
    ) -> anyhow::Result<Vec<Stream>> {
        self.current_episode = Some(episode.clone());
        let message = format!("Memuat server stream '{}'...", episode.title.trim());
        let streams = crate::util::with_spinner(
            message,
            retry_streams(self.extractor.as_ref(), episode),
        )
        .await?;
        if streams.is_empty() {
            anyhow::bail!("server stream kosong");
        }
        Ok(streams)
    }

    async fn download_batch(
        &mut self,
        movie: &Movie,
        episodes: &[Episode],
        current_index: usize,
        target_title: &str,
    ) -> anyhow::Result<bool> {
        let Some(start) =
            choose_batch_episode("Mulai batch dari episode:", episodes, current_index, 0)?
        else {
            return Ok(false);
        };

        let remaining = &episodes[start..];
        let Some(end_offset) =
            choose_batch_episode("Sampai episode:", remaining, remaining.len() - 1, start)?
        else {
            return Ok(false);
        };
        let end = start + end_offset;
        let range = &episodes[start..=end];
        let count = range.len();

        let Some(directory) = crate::download::prompt_batch_directory()? else {
            return Ok(false);
        };
        let Some(existing) = crate::download::prompt_existing_policy()? else {
            return Ok(false);
        };

        println!();
        println!("{} Ringkasan batch download:", "◆".blue());
        println!(
            "  Episode  : {} ({} s/d {})",
            count,
            episodes[start].title.trim(),
            episodes[end].title.trim()
        );
        println!("  Kualitas : {}", target_title.cyan());
        println!("  Folder   : {}", directory.display());
        if !crate::confirm_action("Mulai download batch?")? {
            return Ok(false);
        }

        let mut queue = BatchQueue::new(
            self.provider_type,
            movie.clone(),
            range.to_vec(),
            target_title.to_string(),
            directory.clone(),
            existing,
        );
        queue.save()?;

        println!(
            "\n{} Batch download: {} episode • kualitas '{}' • folder '{}'",
            "◆".blue(),
            count,
            target_title.cyan(),
            directory.display()
        );

        self.run_batch(&mut queue).await?;
        Ok(true)
    }

    pub async fn extract_stream_urls(
        &mut self,
        episode: Episode,
    ) -> anyhow::Result<Option<Stream>> {
        self.current_episode = Some(episode.clone());
        let id = episode.id.clone();
        let spinner_message = format!("Memuat server stream '{}'...", episode.title.trim());

        // Idlix writes each ready-to-play stream to a shared local playlist
        // file. A cached entry would point at a file that a later episode
        // overwrites, so always resolve Idlix streams fresh.
        let cached = if self.provider_type == crate::PROVIDER_IDLIX {
            None
        } else {
            self.stream_cache.get(&id).cloned()
        };

        let streams = loop {
            if let Some(cached) = cached.clone() {
                break cached;
            }

            let result = crate::util::with_spinner(
                spinner_message.clone(),
                retry_streams(self.extractor.as_ref(), &episode),
            )
            .await;

            let streams = match result {
                Ok(streams) if !streams.is_empty() => streams,
                Ok(_) => match self
                    .try_fallback_streams(&episode, "server stream kosong")
                    .await
                {
                    Some(fallback) => fallback,
                    None => {
                        println!();
                        println!("Server stream kosong atau sedang offline.");
                        if input::retry_or_back("Coba muat stream lagi?")? {
                            continue;
                        }
                        return Ok(None);
                    }
                },
                Err(error) => match self
                    .try_fallback_streams(&episode, &format!("{:#}", error))
                    .await
                {
                    Some(fallback) => fallback,
                    None => {
                        eprintln!(
                            "{} Gagal memuat server stream dari {}: {:#}",
                            "■".red(),
                            ext::provider_name(self.provider_type),
                            error
                        );
                        println!();
                        if input::retry_or_back("Coba lagi tanpa keluar aplikasi?")? {
                            continue;
                        }
                        return Ok(None);
                    }
                },
            };

            if self.provider_type != crate::PROVIDER_IDLIX
                && !streams.is_empty()
                && self
                    .current_episode
                    .as_ref()
                    .is_some_and(|current| current.id == id)
            {
                self.stream_cache.insert(id.clone(), streams.clone());
            }
            break streams;
        };

        if streams.is_empty() {
            eprintln!(
                "{} {}",
                "■".red(),
                "Server stream tidak ditemukan atau sedang offline.\n".yellow()
            );
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(None);
        }
        println!(
            "{} Server stream dimuat dari {}",
            "✓".green(),
            ext::provider_name(self.provider_type).cyan()
        );

        let active_movie = self.current_movie.clone();
        let active_provider = self.provider_type;
        let mut watchlisted = active_movie.as_ref().is_some_and(|movie| {
            input::is_in_watchlist(active_provider, &movie.id)
        });
        let batch_episodes = active_movie
            .as_ref()
            .and_then(|movie| self.get_episode_list(&movie.id))
            .unwrap_or_default();
        let batch_current_index = self
            .current_episode
            .as_ref()
            .and_then(|episode| batch_episodes.iter().position(|item| item.id == episode.id))
            .unwrap_or(0);
        let can_batch = batch_episodes.len() > 1
            && self
                .current_episode
                .as_ref()
                .is_some_and(|episode| episode.is_series);
        let saved_batch = BatchQueue::load().filter(|queue| {
            queue.provider_type == active_provider
                && active_movie
                    .as_ref()
                    .is_some_and(|movie| movie.id == queue.movie.id)
                && !queue.is_done()
        });

        loop {
            let mut opts = streams.clone();
            opts.push(Stream {
                url: "download".into(),
                title: "[ ⬇ Download Episode Ini ]".into(),
            });
            if can_batch {
                opts.push(Stream {
                    url: "download_batch".into(),
                    title: "[ ⬇ Download Batch Episode... ]".into(),
                });
            }
            if saved_batch.is_some() {
                opts.push(Stream {
                    url: "resume_batch".into(),
                    title: "[ ⏯ Lanjutkan Batch Terakhir ]".into(),
                });
            }
            if active_movie.is_some() {
                opts.push(Stream {
                    url: "watchlist".into(),
                    title: if watchlisted {
                        "[ ★ Hapus dari Watchlist ]"
                    } else {
                        "[ ☆ Tambah ke Watchlist ]"
                    }
                    .into(),
                });
            }
            opts.push(Stream {
                url: "0".into(),
                title: "[ Kembali ke Pilih Episode ]".into(),
            });
            let Some(selected) = input::choice("Pilih Server / Kualitas:", opts, 0)? else {
                return Ok(None);
            };

            if selected.url == "download" {
                if let Err(error) = crate::download::ensure_available() {
                    eprintln!("{} {:#}", "■".red(), error);
                    println!();
                    println!("Tekan Enter untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                    continue;
                }

                let Some(download_stream) =
                    input::choice("Pilih Kualitas untuk Diunduh:", streams.clone(), 0)?
                else {
                    continue;
                };

                let movie = self.current_movie.clone();
                let episode = self.current_episode.clone();
                let provider = self.provider_type;
                if let (Some(movie), Some(episode)) = (movie, episode) {
                    match crate::download::download_stream(
                        provider,
                        &download_stream,
                        &movie,
                        &episode,
                    ) {
                        Ok(crate::download::DownloadOutcome::Completed(_))
                        | Ok(crate::download::DownloadOutcome::Cancelled) => {}
                        Err(error) => {
                            eprintln!("{} Download gagal: {:#}", "■".red(), error);
                            println!();
                            println!("Tekan Enter untuk kembali...");
                            let mut dummy = String::new();
                            let _ = std::io::stdin().read_line(&mut dummy);
                        }
                    }
                }
                continue;
            }

            if selected.url == "download_batch" {
                if let Err(error) = crate::download::ensure_available() {
                    eprintln!("{} {:#}", "■".red(), error);
                    println!();
                    println!("Tekan Enter untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                    continue;
                }

                let Some(download_stream) =
                    input::choice("Pilih Kualitas untuk Batch:", streams.clone(), 0)?
                else {
                    continue;
                };
                let Some(movie) = active_movie.clone() else {
                    continue;
                };

                match self
                    .download_batch(
                        &movie,
                        &batch_episodes,
                        batch_current_index,
                        &download_stream.title,
                    )
                    .await
                {
                    // Idlix's local playlists are overwritten by every batch
                    // item, so the streams still held by this picker are stale.
                    Ok(true) => return Ok(None),
                    Ok(false) => continue,
                    Err(error) => {
                        eprintln!("{} Batch download gagal: {:#}", "■".red(), error);
                        println!();
                        println!("Tekan Enter untuk kembali...");
                        let mut dummy = String::new();
                        let _ = std::io::stdin().read_line(&mut dummy);
                        continue;
                    }
                }
            }

            if selected.url == "resume_batch" {
                if let Some(mut queue) = BatchQueue::load() {
                    if queue.provider_type == self.provider_type
                        && active_movie
                            .as_ref()
                            .is_some_and(|movie| movie.id == queue.movie.id)
                    {
                        if let Err(error) = self.run_batch(&mut queue).await {
                            eprintln!("{} Batch download gagal: {:#}", "■".red(), error);
                            println!();
                            println!("Tekan Enter untuk kembali...");
                            let mut dummy = String::new();
                            let _ = std::io::stdin().read_line(&mut dummy);
                        }
                        return Ok(None);
                    }
                }
                continue;
            }

            if selected.url == "watchlist" {
                if let Some(movie) = &active_movie {
                    if watchlisted {
                        input::delete_watchlist_entry(active_provider, &movie.id);
                        println!("{} Dihapus dari watchlist.", "✓".green());
                    } else {
                        input::add_watchlist_entry(movie, active_provider);
                        println!("{} Ditambahkan ke watchlist.", "✓".green());
                    }
                    watchlisted = !watchlisted;
                }
                continue;
            }
            if selected.url == "0" {
                return Ok(None);
            }
            return Ok(Some(selected));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct StubExt {
        meta_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl crate::ext::Ext for StubExt {
        async fn search(
            &mut self,
            _title: String,
            _page: usize,
        ) -> anyhow::Result<(Vec<Movie>, u64)> {
            Ok((Vec::new(), 1))
        }

        async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
            self.meta_calls.fetch_add(1, Ordering::SeqCst);
            Ok((
                Vec::new(),
                Meta {
                    thumb_url: Some(format!("https://example.com/{}.jpg", movie.id)),
                    data: Vec::new(),
                },
            ))
        }

        async fn get_stream_urls(&self, _episode: Episode) -> anyhow::Result<Vec<Stream>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn ensure_meta_loads_once_and_caches() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut cli = AnimekuCli::new(Box::new(StubExt {
            meta_calls: calls.clone(),
        }));
        let movie = movie("cached");

        let first = cli.ensure_meta(&movie).await;
        let second = cli.ensure_meta(&movie).await;

        let expected = Some("https://example.com/cached.jpg");
        assert_eq!(first.and_then(|meta| meta.thumb_url).as_deref(), expected);
        assert_eq!(second.and_then(|meta| meta.thumb_url).as_deref(), expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    struct SingleMovieExt;

    #[async_trait::async_trait]
    impl crate::ext::Ext for SingleMovieExt {
        async fn search(
            &mut self,
            _title: String,
            _page: usize,
        ) -> anyhow::Result<(Vec<Movie>, u64)> {
            Ok((Vec::new(), 1))
        }

        async fn get_episodes(&self, _movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
            Ok((
                vec![Episode {
                    id: "movie|42".into(),
                    title: "Putar Film (Full Movie)".into(),
                    is_series: false,
                }],
                Meta::default(),
            ))
        }

        async fn get_stream_urls(&self, _episode: Episode) -> anyhow::Result<Vec<Stream>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn full_movie_skips_episode_picker() {
        let mut cli = AnimekuCli::new(Box::new(SingleMovieExt));
        let selected = cli
            .extract_episode(movie("movie|42"))
            .await
            .unwrap()
            .expect("full movie should be selected automatically");

        assert_eq!(selected.id, "movie|42");
        assert!(!selected.is_series);
    }

    fn movie(id: &str) -> Movie {
        Movie {
            id: id.into(),
            title: format!("Movie {}", id),
            total_episodes: None,
        }
    }

    fn labels(items: &[SearchMenuItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    #[test]
    fn fallback_movie_matching_ignores_provider_noise() {
        let target = Movie {
            id: "id|series".into(),
            title: "[Series] Jujutsu Kaisen Season 2".into(),
            total_episodes: None,
        };
        let candidates = vec![
            Movie {
                id: "other".into(),
                title: "Jujutsu Kaisen".into(),
                total_episodes: None,
            },
            Movie {
                id: "match".into(),
                title: "Jujutsu Kaisen Season 2 (Episode 1 - 12) Subtitle Indonesia".into(),
                total_episodes: None,
            },
        ];

        assert_eq!(
            best_movie_match(&target, &candidates).map(|movie| movie.id),
            Some("match".to_string())
        );
        assert_eq!(
            fallback_movie_title(&target),
            "Jujutsu Kaisen Season 2"
        );
    }

    #[test]
    fn fallback_episode_matching_handles_season_and_full_movie_formats() {
        let episodes = vec![
            Episode {
                id: "ep-1".into(),
                title: "Episode 1".into(),
                is_series: true,
            },
            Episode {
                id: "ep-2".into(),
                title: "Episode 2".into(),
                is_series: true,
            },
        ];
        let season_target = Episode {
            id: "s1e1".into(),
            title: "S1E01 - To You in 2016".into(),
            is_series: true,
        };
        assert_eq!(
            best_episode_match(&season_target, &episodes).map(|episode| episode.id),
            Some("ep-1".to_string())
        );

        let full_movie = Episode {
            id: "full".into(),
            title: "Putar Film (Full Movie)".into(),
            is_series: false,
        };
        let full_candidates = vec![
            Episode {
                id: "series".into(),
                title: "Episode 1".into(),
                is_series: true,
            },
            Episode {
                id: "movie".into(),
                title: "Full Movie".into(),
                is_series: false,
            },
        ];
        assert_eq!(
            best_episode_match(&full_movie, &full_candidates).map(|episode| episode.id),
            Some("movie".to_string())
        );
    }

    #[test]
    fn alternate_provider_switches_between_the_two_supported_sites() {
        assert_eq!(
            alternate_provider(ext::PROVIDER_IDLIX),
            ext::PROVIDER_OTAKUDESU
        );
        assert_eq!(
            alternate_provider(ext::PROVIDER_OTAKUDESU),
            ext::PROVIDER_IDLIX
        );
    }

    struct FlakySearchExt {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl crate::ext::Ext for FlakySearchExt {
        async fn search(
            &mut self,
            _title: String,
            _page: usize,
        ) -> anyhow::Result<(Vec<Movie>, u64)> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call < 3 {
                anyhow::bail!("transient failure");
            }
            Ok((vec![movie("recovered")], 1))
        }

        async fn get_episodes(&self, _movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
            Ok((Vec::new(), Meta::default()))
        }

        async fn get_stream_urls(&self, _episode: Episode) -> anyhow::Result<Vec<Stream>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn retry_search_recovers_after_transient_failures() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut extractor = FlakySearchExt {
            calls: calls.clone(),
        };

        let (movies, total) = retry_search(&mut extractor, "test", 1).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(movies.len(), 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn single_page_has_numbered_results_without_next_entry() {
        let menu = build_search_menu(vec![movie("a")], 1, 1, false);
        assert_eq!(
            labels(&menu),
            vec!["[ Kembali ke Pencarian ]", SEARCH_SEPARATOR, "1. Movie a"]
        );
        assert_eq!(menu[0].action, SearchAction::Back);
        assert_eq!(menu[2].action, SearchAction::Select);
        assert_eq!(menu[2].movie.as_ref().unwrap().title, "Movie a");
    }

    #[test]
    fn next_page_is_placed_after_numbered_results() {
        let menu = build_search_menu(vec![movie("a"), movie("b")], 1, 3, false);
        assert_eq!(
            labels(&menu),
            vec![
                "[ Kembali ke Pencarian ]",
                SEARCH_SEPARATOR,
                "1. Movie a",
                "2. Movie b",
                SEARCH_SEPARATOR,
                "Selanjutnya (Halaman 2)",
            ]
        );
        assert_eq!(menu.last().unwrap().action, SearchAction::Next);
    }

    #[test]
    fn middle_and_last_pages_show_previous() {
        let middle = build_search_menu(vec![movie("a")], 2, 3, false);
        assert_eq!(
            labels(&middle),
            vec![
                "[ Kembali ke Pencarian ]",
                SEARCH_SEPARATOR,
                "1. Movie a",
                SEARCH_SEPARATOR,
                "Sebelumnya (Halaman 1)",
                "Selanjutnya (Halaman 3)",
            ]
        );

        let last = build_search_menu(vec![movie("a")], 3, 3, false);
        assert_eq!(
            labels(&last),
            vec![
                "[ Kembali ke Pencarian ]",
                SEARCH_SEPARATOR,
                "1. Movie a",
                SEARCH_SEPARATOR,
                "Sebelumnya (Halaman 2)",
            ]
        );
    }

    #[test]
    fn empty_next_page_is_not_advertised() {
        let menu = build_search_menu(vec![movie("a")], 1, 3, true);
        assert_eq!(
            labels(&menu),
            vec!["[ Kembali ke Pencarian ]", SEARCH_SEPARATOR, "1. Movie a"]
        );
    }

    fn episode_list(count: usize) -> Vec<Episode> {
        (1..=count)
            .map(|number| Episode {
                id: number.to_string(),
                title: format!("Episode {}", number),
                is_series: true,
            })
            .collect()
    }

    #[test]
    fn episode_menu_paginates_and_navigates() {
        let episodes = episode_list(30);
        let total_pages = episodes.len().div_ceil(EPISODES_PER_PAGE) as u64;
        assert_eq!(total_pages, 2);

        let state = EpisodeWatchState::default();
        let (_, first_page) = build_episode_menu(&episodes, 1, total_pages, &state, false);
        assert_eq!(first_page[0], EpisodeAction::Back);
        assert_eq!(first_page[1], EpisodeAction::ToggleWatchlist);
        assert_eq!(first_page[2], EpisodeAction::Search);
        assert!(first_page.contains(&EpisodeAction::Select(0)));
        assert!(first_page.contains(&EpisodeAction::Select(24)));
        assert!(!first_page.contains(&EpisodeAction::Select(25)));
        assert!(first_page.contains(&EpisodeAction::Next));
        assert!(!first_page.contains(&EpisodeAction::Previous));
        assert_eq!(
            first_page
                .iter()
                .filter(|action| matches!(action, EpisodeAction::Select(_)))
                .count(),
            EPISODES_PER_PAGE
        );

        let (_, last_page) = build_episode_menu(&episodes, 2, total_pages, &state, true);
        assert!(last_page.contains(&EpisodeAction::Select(25)));
        assert!(last_page.contains(&EpisodeAction::Select(29)));
        assert!(!last_page.contains(&EpisodeAction::Select(24)));
        assert!(last_page.contains(&EpisodeAction::Previous));
        assert!(!last_page.contains(&EpisodeAction::Next));
        assert_eq!(
            last_page
                .iter()
                .filter(|action| matches!(action, EpisodeAction::Select(_)))
                .count(),
            5
        );
    }

    #[test]
    fn single_page_episode_menu_has_no_search_or_navigation() {
        let state = EpisodeWatchState::default();
        let (_, actions) = build_episode_menu(&episode_list(5), 1, 1, &state, false);
        assert!(actions.contains(&EpisodeAction::ToggleWatchlist));
        assert!(!actions.contains(&EpisodeAction::Search));
        assert!(!actions.contains(&EpisodeAction::Previous));
        assert!(!actions.contains(&EpisodeAction::Next));
        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(action, EpisodeAction::Select(_)))
                .count(),
            5
        );
    }

    #[test]
    fn episode_search_menu_contains_every_episode() {
        let episodes = episode_list(30);
        let state = EpisodeWatchState::default();
        let (_, actions) = build_episode_search_menu(&episodes, &state);
        assert_eq!(actions[0], EpisodeAction::Back);
        assert!(actions.contains(&EpisodeAction::Select(0)));
        assert!(actions.contains(&EpisodeAction::Select(29)));
        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(action, EpisodeAction::Select(_)))
                .count(),
            30
        );
    }

    #[test]
    fn batch_stream_prefers_requested_quality_then_falls_back_to_first() {
        let streams = vec![
            Stream {
                url: "1080".into(),
                title: "Idlix [1080p]".into(),
            },
            Stream {
                url: "720".into(),
                title: "Idlix [720p]".into(),
            },
        ];

        assert_eq!(
            choose_batch_stream(&streams, "Idlix [720p #2]").map(|stream| stream.url.as_str()),
            Some("720")
        );
        assert_eq!(
            choose_batch_stream(&streams, "Idlix [480p]").map(|stream| stream.url.as_str()),
            Some("1080")
        );
        assert!(choose_batch_stream(&[], "Idlix [720p]").is_none());
    }

    #[test]
    fn batch_status_reports_counts_and_eta() {
        let status = format_batch_status(3, 12, 2, 1, 0, Duration::from_secs(90));
        assert_eq!(
            status,
            "Batch 3/12 • 2 berhasil • 1 dilewati • 0 gagal • sisa ~04:30"
        );
        assert_eq!(batch_eta(0, 12, Duration::ZERO), "—");
        assert_eq!(batch_eta(12, 12, Duration::from_secs(90)), "00:00");
    }

    #[test]
    fn batch_list_labels_use_season_and_episode_numbers() {
        let episodes = [
            Episode {
                id: "1".into(),
                title: "S1E03 - Judul".into(),
                is_series: true,
            },
            Episode {
                id: "2".into(),
                title: "Episode 7".into(),
                is_series: true,
            },
            Episode {
                id: "3".into(),
                title: "S2E11 - Judul".into(),
                is_series: true,
            },
        ];

        assert_eq!(batch_episode_label(0, &episodes[0]), "E03");
        assert_eq!(batch_episode_label(1, &episodes[1]), "E07");
        assert_eq!(batch_episode_label(2, &episodes[2]), "S2E11");
    }

    #[test]
    fn batch_list_shows_pending_and_live_progress_rows() {
        let episodes = episode_list(2);
        let statuses = vec![BatchItemStatus::Pending; 2];
        let mut ui = BatchListUi::new(&episodes, &statuses, None, false);

        assert_eq!(
            ui.row(0),
            "[1/2] E01  pending      -        -"
        );

        ui.set_active(0);
        ui.set_progress(DownloadProgress {
            duration: Some(100.0),
            out_seconds: 68.0,
            total_size: 12_000_000,
            speed: "1.2x".into(),
            finished: false,
            paused: false,
        });
        let active = ui.row(0);
        assert!(active.starts_with("[1/2] E01  "));
        assert!(active.contains("68%"));
        assert_eq!(
            ui.row(1),
            "[2/2] E02  pending      -        -"
        );
    }
}
