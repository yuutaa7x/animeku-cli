use async_trait::async_trait;

use crate::models::{Episode, Meta, Movie, Stream};

pub mod idlix;
pub mod otakudesu;

/// Shared desktop browser fingerprint used for Idlix requests and the local
/// proxy. Keeping it in one place avoids subtle Cloudflare mismatches.
pub const CHROME_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[async_trait]
#[allow(clippy::double_must_use)] // false positive from the async_trait expansion
pub trait Ext {
    async fn search(&mut self, title: String, page: usize) -> anyhow::Result<(Vec<Movie>, u64)>;
    async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)>;
    async fn get_stream_urls(&self, episode: Episode) -> anyhow::Result<Vec<Stream>>;
}
