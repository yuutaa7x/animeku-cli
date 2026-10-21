use async_trait::async_trait;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::models::{Episode, Meta, Movie, Stream};

pub mod idlix;
pub mod otakudesu;

pub const PROVIDER_IDLIX: usize = 0;
pub const PROVIDER_OTAKUDESU: usize = 1;

/// Keep provider traffic polite. The value can be overridden for testing or
/// for users on slower connections with `ANIMEKU_RATE_LIMIT_MS`.
fn rate_limit_interval() -> Duration {
    std::env::var("ANIMEKU_RATE_LIMIT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(350))
}

struct AsyncRateLimiter {
    interval: Duration,
    next: tokio::sync::Mutex<Instant>,
}

impl AsyncRateLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: tokio::sync::Mutex::new(Instant::now()),
        }
    }

    async fn wait(&self) {
        let mut next = self.next.lock().await;
        let now = Instant::now();
        if *next > now {
            tokio::time::sleep(*next - now).await;
        }
        *next = Instant::now() + self.interval;
    }
}

struct BlockingRateLimiter {
    interval: Duration,
    next: std::sync::Mutex<Instant>,
}

impl BlockingRateLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: std::sync::Mutex::new(Instant::now()),
        }
    }

    fn wait(&self) {
        let mut next = self
            .next
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        if *next > now {
            std::thread::sleep(*next - now);
        }
        *next = Instant::now() + self.interval;
    }
}

static OTAKUDESU_LIMITER: OnceLock<AsyncRateLimiter> = OnceLock::new();
static IDLIX_LIMITER: OnceLock<BlockingRateLimiter> = OnceLock::new();

/// Rate limit an async provider request (Otakudesu).
pub async fn wait_for_provider(provider_type: usize) {
    if provider_type != PROVIDER_OTAKUDESU {
        return;
    }
    OTAKUDESU_LIMITER
        .get_or_init(|| AsyncRateLimiter::new(rate_limit_interval()))
        .wait()
        .await;
}

/// Rate limit a blocking provider request (Idlix uses curl).
pub fn wait_for_provider_blocking(provider_type: usize) {
    if provider_type != PROVIDER_IDLIX {
        return;
    }
    IDLIX_LIMITER
        .get_or_init(|| BlockingRateLimiter::new(rate_limit_interval()))
        .wait();
}

/// Shared desktop browser fingerprint used for Idlix requests and the local
/// proxy. Keeping it in one place avoids subtle Cloudflare mismatches.
pub const CHROME_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub fn provider_name(provider_type: usize) -> &'static str {
    match provider_type {
        PROVIDER_OTAKUDESU => "Otakudesu",
        _ => "Idlix",
    }
}

pub fn make_extractor(provider_type: usize) -> Box<dyn Ext> {
    match provider_type {
        PROVIDER_OTAKUDESU => Box::new(otakudesu::Otakudesu::new()),
        _ => Box::new(idlix::Idlix::new()),
    }
}

#[async_trait]
#[allow(clippy::double_must_use)] // false positive from the async_trait expansion
pub trait Ext {
    async fn search(&mut self, title: String, page: usize) -> anyhow::Result<(Vec<Movie>, u64)>;
    async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)>;
    async fn get_stream_urls(&self, episode: Episode) -> anyhow::Result<Vec<Stream>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_limiter_spaces_requests() {
        let limiter = BlockingRateLimiter::new(Duration::from_millis(30));
        let start = Instant::now();
        limiter.wait();
        limiter.wait();
        assert!(start.elapsed() >= Duration::from_millis(20));
    }

    #[tokio::test]
    async fn async_limiter_spaces_requests() {
        let limiter = AsyncRateLimiter::new(Duration::from_millis(30));
        let start = Instant::now();
        limiter.wait().await;
        limiter.wait().await;
        assert!(start.elapsed() >= Duration::from_millis(20));
    }
}
