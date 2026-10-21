use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::models::{Episode, Meta, Movie};

/// Bump this when the on-disk layout changes so old entries are discarded
/// instead of being interpreted with the wrong fields.
const CACHE_VERSION: u32 = 1;
const CACHE_FILE_NAME: &str = "metadata-cache.json";

/// Search results change more often than an episode list (new episodes and
/// listings appear), so they use the shorter TTL.
const SEARCH_TTL_SECONDS: u64 = 6 * 60 * 60;
const EPISODE_TTL_SECONDS: u64 = 12 * 60 * 60;

/// Keep the cache bounded even for users who search every day. Expired entries
/// are removed on save, and the oldest remaining entries are evicted when the
/// limit is exceeded.
const MAX_SEARCH_ENTRIES: usize = 300;
const MAX_EPISODE_ENTRIES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSearch {
    movies: Vec<Movie>,
    total_pages: u64,
    fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedEpisodes {
    episodes: Vec<Episode>,
    meta: Meta,
    fetched_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct CacheFile {
    version: u32,
    searches: HashMap<String, CachedSearch>,
    episodes: HashMap<String, CachedEpisodes>,
}

impl Default for CacheFile {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            searches: HashMap::new(),
            episodes: HashMap::new(),
        }
    }
}

/// Provider metadata cache stored outside the session/temp directory so it can
/// survive restarts. Stream URLs are intentionally not cached: they expire
/// quickly and must always be fetched from the provider.
pub struct MetadataCache {
    path: PathBuf,
    data: CacheFile,
}

impl MetadataCache {
    pub fn load() -> Self {
        Self::from_path(Self::default_path())
    }

    fn default_path() -> PathBuf {
        let base = dirs::cache_dir()
            .unwrap_or_else(|| dirs::data_local_dir().unwrap_or_else(std::env::temp_dir));
        base.join("animeku-cli").join(CACHE_FILE_NAME)
    }

    fn from_path(path: PathBuf) -> Self {
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<CacheFile>(&content).ok())
            .filter(|cache| cache.version == CACHE_VERSION)
            .unwrap_or_default();
        Self { path, data }
    }

    pub fn get_search(
        &self,
        provider: usize,
        title: &str,
        page: usize,
    ) -> Option<(Vec<Movie>, u64)> {
        let entry = self.data.searches.get(&search_key(provider, title, page))?;
        if !is_fresh(entry.fetched_at, SEARCH_TTL_SECONDS) {
            return None;
        }
        Some((entry.movies.clone(), entry.total_pages.max(1)))
    }

    pub fn put_search(
        &mut self,
        provider: usize,
        title: &str,
        page: usize,
        movies: Vec<Movie>,
        total_pages: u64,
    ) {
        // Empty results are not persisted: they are often caused by a transient
        // provider problem, and caching them would block the fallback path.
        if movies.is_empty() {
            return;
        }
        self.data.searches.insert(
            search_key(provider, title, page),
            CachedSearch {
                movies,
                total_pages: total_pages.max(1),
                fetched_at: now_seconds(),
            },
        );
        self.save();
    }

    pub fn get_episodes(&self, provider: usize, movie_id: &str) -> Option<(Vec<Episode>, Meta)> {
        let entry = self.data.episodes.get(&episode_key(provider, movie_id))?;
        if !is_fresh(entry.fetched_at, EPISODE_TTL_SECONDS) {
            return None;
        }
        Some((entry.episodes.clone(), entry.meta.clone()))
    }

    pub fn put_episodes(
        &mut self,
        provider: usize,
        movie_id: &str,
        episodes: Vec<Episode>,
        meta: Meta,
    ) {
        if episodes.is_empty() {
            return;
        }
        self.data.episodes.insert(
            episode_key(provider, movie_id),
            CachedEpisodes {
                episodes,
                meta,
                fetched_at: now_seconds(),
            },
        );
        self.save();
    }

    fn save(&mut self) {
        self.prune();

        let Some(parent) = self.path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(content) = serde_json::to_string(&self.data) else {
            return;
        };

        // Write through a private temp file and rename so an interrupted write
        // cannot leave a truncated cache behind.
        let temp_path = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        if fs::write(&temp_path, content).is_err() {
            return;
        }
        #[cfg(windows)]
        {
            // Windows' rename does not replace an existing destination.
            let _ = fs::remove_file(&self.path);
        }
        if fs::rename(&temp_path, &self.path).is_err() {
            let _ = fs::remove_file(&temp_path);
        }
    }

    fn prune(&mut self) {
        let now = now_seconds();
        self.data
            .searches
            .retain(|_, entry| is_fresh_at(entry.fetched_at, SEARCH_TTL_SECONDS, now));
        self.data
            .episodes
            .retain(|_, entry| is_fresh_at(entry.fetched_at, EPISODE_TTL_SECONDS, now));
        trim_oldest(&mut self.data.searches, MAX_SEARCH_ENTRIES, |entry| {
            entry.fetched_at
        });
        trim_oldest(&mut self.data.episodes, MAX_EPISODE_ENTRIES, |entry| {
            entry.fetched_at
        });
    }
}

/// Remove every persisted metadata entry. Used by the settings menu.
pub fn clear() {
    let path = MetadataCache::default_path();
    let _ = fs::remove_file(path);
}

fn search_key(provider: usize, title: &str, page: usize) -> String {
    format!("{}\u{1f}{}\u{1f}{}", provider, normalize_title(title), page)
}

fn episode_key(provider: usize, movie_id: &str) -> String {
    format!("{}\u{1f}{}", provider, movie_id.trim())
}

fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_fresh(fetched_at: u64, ttl_seconds: u64) -> bool {
    is_fresh_at(fetched_at, ttl_seconds, now_seconds())
}

fn is_fresh_at(fetched_at: u64, ttl_seconds: u64, now: u64) -> bool {
    now.saturating_sub(fetched_at) <= ttl_seconds
}

fn trim_oldest<T>(
    entries: &mut HashMap<String, T>,
    max_entries: usize,
    fetched_at: impl Fn(&T) -> u64,
) {
    if entries.len() <= max_entries {
        return;
    }

    let mut oldest = entries
        .iter()
        .map(|(key, entry)| (key.clone(), fetched_at(entry)))
        .collect::<Vec<_>>();
    oldest.sort_by_key(|(_, timestamp)| *timestamp);

    for (key, _) in oldest.into_iter().take(entries.len() - max_entries) {
        entries.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_cache_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "animeku-cache-test-{}-{}.json",
            std::process::id(),
            sequence
        ))
    }

    fn movie(id: &str) -> Movie {
        Movie {
            id: id.into(),
            title: format!("Movie {}", id),
            total_episodes: Some("12".into()),
        }
    }

    fn episode(id: &str) -> Episode {
        Episode {
            id: id.into(),
            title: format!("Episode {}", id),
            is_series: true,
        }
    }

    #[test]
    fn search_cache_roundtrip_is_provider_and_page_specific() {
        let path = temp_cache_path();
        let mut cache = MetadataCache::from_path(path.clone());
        cache.put_search(0, "Solo  Leveling", 1, vec![movie("a")], 3);

        let reloaded = MetadataCache::from_path(path.clone());
        let (movies, total_pages) = reloaded
            .get_search(0, "solo leveling", 1)
            .expect("fresh search cache");
        assert_eq!(movies.len(), 1);
        assert_eq!(movies[0].id, "a");
        assert_eq!(total_pages, 3);
        assert!(reloaded.get_search(1, "solo leveling", 1).is_none());
        assert!(reloaded.get_search(0, "solo leveling", 2).is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn episode_cache_roundtrip_keeps_meta() {
        let path = temp_cache_path();
        let meta = Meta {
            thumb_url: Some("https://example.com/poster.jpg".into()),
            data: vec![("Genre".into(), "Action".into())],
        };
        let mut cache = MetadataCache::from_path(path.clone());
        cache.put_episodes(1, "movie-1", vec![episode("1")], meta.clone());

        let reloaded = MetadataCache::from_path(path.clone());
        let (episodes, cached_meta) = reloaded
            .get_episodes(1, "movie-1")
            .expect("fresh episode cache");
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].title, "Episode 1");
        assert_eq!(cached_meta.thumb_url, meta.thumb_url);
        assert!(reloaded.get_episodes(0, "movie-1").is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn expired_and_empty_entries_are_not_served() {
        let path = temp_cache_path();
        let mut cache = MetadataCache::from_path(path.clone());
        cache.put_search(0, "empty", 1, Vec::new(), 1);
        assert!(cache.get_search(0, "empty", 1).is_none());

        cache.data.searches.insert(
            search_key(0, "old", 1),
            CachedSearch {
                movies: vec![movie("old")],
                total_pages: 1,
                fetched_at: now_seconds().saturating_sub(SEARCH_TTL_SECONDS + 1),
            },
        );
        assert!(cache.get_search(0, "old", 1).is_none());

        let _ = fs::remove_file(path);
    }
}
