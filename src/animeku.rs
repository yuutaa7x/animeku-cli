use std::{
    collections::HashMap,
    io::{stdout, Write},
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchAction {
    Back,
    Previous,
    Next,
    Separator,
    Select,
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
            label: format!("{}. {}", index + 1, movie.title.trim()),
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

impl AnimekuCli {
    pub fn get_meta(&self, id: &str) -> Option<crate::models::Meta> {
        self.episode_cache.get(id).map(|(_, m)| m.clone())
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
        print!(
            "{} Proses pencarian '{}'.. ",
            "◆".blue(),
            search_title.green()
        );
        stdout().flush()?;

        let mut page = 1usize;
        let mut is_latest = false;

        loop {
            if let std::collections::hash_map::Entry::Vacant(_) =
                self.movie_cache.entry(page.to_string())
            {
                let (movie_list, total_pages) =
                    self.extractor.search(search_title.into(), page).await?;
                if page == 1 && !movie_list.is_empty() {
                    println!("ditemukan {} judul", movie_list.len().to_string().green());
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
                        "Silahkan periksa kembali kata kunci pencarian\n".yellow()
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
            let Some(selected) = input::select_menu("Pilih", &entries, default_index)? else {
                return Ok(None);
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
        print!(
            "{} Memuat daftar episode '{}' .. ",
            "◆".blue(),
            movie.title.trim().green()
        );
        stdout().flush()?;

        let id = movie.id.clone();

        if !self.episode_cache.contains_key(&id) {
            let item = self.extractor.get_episodes(movie).await?;
            self.episode_cache.insert(id.clone(), item);
        }

        let (episodes, meta) = self.episode_cache.get(&id).unwrap().to_owned();
        if episodes.is_empty() {
            eprintln!(
                "tidak berhasil!\n{} {}",
                "■".red(),
                "Daftar episode tidak tersedia untuk judul ini.\n".yellow()
            );
            println!("Tekan Enter untuk kembali ke daftar judul...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(None);
        }

        println!("berhasil");
        println!(
            "\n  {}{}\n",
            " ".repeat(45 / 2 - 5),
            " Deskripsi ".black().on_truecolor(252, 136, 3)
        );

        if let Some(ref thumb_url) = meta.thumb_url {
            show_image_thumb(thumb_url.to_string()).await;
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

        let mut opts = episodes;
        opts.push(Episode {
            id: "0".into(),
            title: "[ Kembali ke Pilih Anime ]".into(),
            is_series: false,
        });
        let selected = input::choice(opts, true)?;
        if selected.id == "0" {
            Ok(None)
        } else {
            Ok(Some(selected))
        }
    }

    pub async fn extract_stream_urls(&mut self, episode: Episode) -> anyhow::Result<Option<Stream>> {
        print!(
            "{} Memuat server stream '{}' .. ",
            "◆".blue(),
            episode.title.trim().green()
        );
        stdout().flush()?;

        let id = episode.id.clone();
        if !self.stream_cache.contains_key(&id) {
            let streams = self.extractor.get_stream_urls(episode).await?;
            self.stream_cache.insert(id.clone(), streams);
        }

        let streams = self.stream_cache.get(&id).unwrap().to_owned();
        if streams.is_empty() {
            eprintln!(
                "gagal!\n{} {}",
                "■".red(),
                "Server stream tidak ditemukan atau sedang offline.\n".yellow()
            );
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(None);
        }
        println!("berhasil");

        let mut opts = streams;
        opts.push(Stream {
            url: "0".into(),
            title: "[ Kembali ke Pilih Episode ]".into(),
        });
        let selected = input::choice(opts, false)?;
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
}
