use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{
    ext::Ext,
    input,
    models::{Episode, Meta, Movie, Stream},
    util::show_image_thumb,
};

pub struct AnimekuCli {
    /// Cached page results together with the provider's total page count.
    movie_cache: HashMap<String, (Vec<Movie>, u64)>,
    episode_cache: HashMap<String, (Vec<Episode>, Meta)>,
    stream_cache: HashMap<String, Vec<Stream>>,
    extractor: Box<dyn Ext>,
}

const SEARCH_SEPARATOR: &str = "────────────────────────";
const EPISODES_PER_PAGE: usize = 25;

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
) -> (Vec<input::MenuEntry>, Vec<EpisodeAction>) {
    let total_pages = total_pages.max(1);
    let page = page.clamp(1, total_pages as usize);
    let mut entries = Vec::with_capacity(EPISODES_PER_PAGE + 5);
    let mut actions = Vec::with_capacity(EPISODES_PER_PAGE + 5);

    entries.push(input::MenuEntry::new("[ Kembali ke Pilih Anime ]"));
    actions.push(EpisodeAction::Back);

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

impl AnimekuCli {
    pub fn get_meta(&self, id: &str) -> Option<crate::models::Meta> {
        self.episode_cache.get(id).map(|(_, m)| m.clone())
    }

    /// Load and cache movie metadata without showing the episode picker.
    /// Used when resuming from history, where only the stored IDs are available.
    pub async fn ensure_meta(&mut self, movie: &Movie) -> Option<Meta> {
        if !self.episode_cache.contains_key(&movie.id) {
            let episodes_and_meta = self.extractor.get_episodes(movie.clone()).await.ok()?;
            self.episode_cache
                .insert(movie.id.clone(), episodes_and_meta);
        }
        self.get_meta(&movie.id)
    }

    pub fn get_episode_list(&self, id: &str) -> Option<Vec<Episode>> {
        self.episode_cache.get(id).map(|(e, _)| e.clone())
    }

    pub fn new(extractor: Box<dyn Ext>) -> Self {
        Self {
            movie_cache: HashMap::new(),
            episode_cache: HashMap::new(),
            stream_cache: HashMap::new(),
            extractor,
        }
    }

    pub async fn search(&mut self, search_title: &str) -> anyhow::Result<Option<Movie>> {
        let mut page = 1usize;
        let mut is_latest = false;

        loop {
            if let std::collections::hash_map::Entry::Vacant(_) =
                self.movie_cache.entry(page.to_string())
            {
                let spinner_message = format!("Mencari '{}'...", search_title);
                let result = crate::util::with_spinner(
                    spinner_message,
                    self.extractor.search(search_title.into(), page),
                )
                .await;
                let (movie_list, total_pages) = match result {
                    Ok(result) => result,
                    Err(error) => {
                        eprintln!(
                            "{} Gagal mencari '{}': {:#}",
                            "■".red(),
                            search_title,
                            error
                        );
                        println!();
                        println!("Periksa koneksi internet lalu coba lagi.");
                        println!("Tekan Enter untuk kembali...");
                        let mut dummy = String::new();
                        let _ = std::io::stdin().read_line(&mut dummy);
                        return Ok(None);
                    }
                };
                if page == 1 && !movie_list.is_empty() {
                    println!(
                        "{} Ditemukan {} judul",
                        "✓".green(),
                        movie_list.len().to_string().green()
                    );
                }
                self.movie_cache
                    .insert(page.to_string(), (movie_list, total_pages.max(1)));
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
                        return Ok(Some(movie));
                    }
                    continue;
                }
            }
        }
    }

    pub async fn extract_episode(&mut self, movie: Movie) -> anyhow::Result<Option<Episode>> {
        let id = movie.id.clone();
        let watch_state = EpisodeWatchState::from_movie(&id);
        let spinner_message = format!("Memuat daftar episode '{}'...", movie.title.trim());

        if !self.episode_cache.contains_key(&id) {
            let result =
                crate::util::with_spinner(spinner_message, self.extractor.get_episodes(movie))
                    .await;
            match result {
                Ok(item) => {
                    self.episode_cache.insert(id.clone(), item);
                }
                Err(error) => {
                    eprintln!("{} Gagal memuat episode: {:#}", "■".red(), error);
                    println!();
                    println!("Tekan Enter untuk kembali ke daftar judul...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                    return Ok(None);
                }
            }
        }

        let (episodes, meta) = self.episode_cache.get(&id).unwrap().to_owned();
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

        println!("{} Daftar episode dimuat", "✓".green());
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
            let (entries, actions) = build_episode_menu(&episodes, page, total_pages, &watch_state);
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

    pub async fn extract_stream_urls(
        &mut self,
        episode: Episode,
    ) -> anyhow::Result<Option<Stream>> {
        let id = episode.id.clone();
        let spinner_message = format!("Memuat server stream '{}'...", episode.title.trim());

        if !self.stream_cache.contains_key(&id) {
            let result =
                crate::util::with_spinner(spinner_message, self.extractor.get_stream_urls(episode))
                    .await;
            match result {
                Ok(streams) => {
                    self.stream_cache.insert(id.clone(), streams);
                }
                Err(error) => {
                    eprintln!("{} Gagal memuat server stream: {:#}", "■".red(), error);
                    println!();
                    println!("Tekan Enter untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                    return Ok(None);
                }
            }
        }

        let streams = self.stream_cache.get(&id).unwrap().to_owned();
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
        println!("{} Server stream dimuat", "✓".green());

        let mut opts = streams;
        opts.push(Stream {
            url: "0".into(),
            title: "[ Kembali ke Pilih Episode ]".into(),
        });
        let Some(selected) = input::choice("Pilih Server / Kualitas:", opts, 0)? else {
            return Ok(None);
        };
        if selected.url == "0" {
            Ok(None)
        } else {
            Ok(Some(selected))
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
        let (_, first_page) = build_episode_menu(&episodes, 1, total_pages, &state);
        assert_eq!(first_page[0], EpisodeAction::Back);
        assert_eq!(first_page[1], EpisodeAction::Search);
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

        let (_, last_page) = build_episode_menu(&episodes, 2, total_pages, &state);
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
        let (_, actions) = build_episode_menu(&episode_list(5), 1, 1, &state);
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
}
