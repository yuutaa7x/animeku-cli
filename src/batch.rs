use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::download::ExistingFilePolicy;
use crate::models::{Episode, Movie};

const QUEUE_FILE: &str = ".animeku_batch_queue.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    #[default]
    Pending,
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueue {
    pub provider_type: usize,
    pub movie: Movie,
    pub episodes: Vec<Episode>,
    pub target_quality: String,
    pub directory: String,
    pub existing: ExistingFilePolicy,
    #[serde(default)]
    pub statuses: Vec<BatchItemStatus>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl BatchQueue {
    pub fn new(
        provider_type: usize,
        movie: Movie,
        episodes: Vec<Episode>,
        target_quality: String,
        directory: PathBuf,
        existing: ExistingFilePolicy,
    ) -> Self {
        let now = now_seconds();
        Self {
            provider_type,
            movie,
            statuses: vec![BatchItemStatus::Pending; episodes.len()],
            episodes,
            target_quality,
            directory: directory.to_string_lossy().to_string(),
            existing,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn pending_indices(&self) -> Vec<usize> {
        self.statuses
            .iter()
            .enumerate()
            .filter_map(|(index, status)| {
                matches!(status, BatchItemStatus::Pending | BatchItemStatus::Failed)
                    .then_some(index)
            })
            .collect()
    }

    pub fn is_done(&self) -> bool {
        self.pending_indices().is_empty()
    }

    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut completed = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut pending = 0;
        for status in &self.statuses {
            match status {
                BatchItemStatus::Completed => completed += 1,
                BatchItemStatus::Skipped => skipped += 1,
                BatchItemStatus::Failed => failed += 1,
                BatchItemStatus::Pending => pending += 1,
            }
        }
        (completed, skipped, failed, pending)
    }

    pub fn mark(&mut self, index: usize, status: BatchItemStatus) {
        if let Some(slot) = self.statuses.get_mut(index) {
            *slot = status;
            self.updated_at = now_seconds();
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = queue_path().ok_or_else(|| anyhow::anyhow!("home directory tidak ditemukan"))?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn load() -> Option<Self> {
        let path = queue_path()?;
        let content = fs::read_to_string(path).ok()?;
        let mut queue: Self = serde_json::from_str(&content).ok()?;
        queue
            .statuses
            .resize(queue.episodes.len(), BatchItemStatus::Pending);
        queue.statuses.truncate(queue.episodes.len());
        Some(queue)
    }

    pub fn clear() {
        if let Some(path) = queue_path() {
            let _ = fs::remove_file(path);
        }
    }

    pub fn log_error(&self, episode: &Episode, error: &str) {
        let Ok(directory) = fs::metadata(&self.directory) else {
            return;
        };
        if !directory.is_dir() {
            return;
        }
        let path = PathBuf::from(&self.directory).join("animeku-batch.log");
        let line = format!(
            "[{}] {} - {}: {}\n",
            now_seconds(),
            crate::models::clean_movie_title(&self.movie.title),
            crate::models::clean_episode_title(&episode.title),
            error.replace('\n', " ")
        );
        use std::io::Write;
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn queue_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(QUEUE_FILE))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie() -> Movie {
        Movie {
            id: "movie-1".into(),
            title: "Test Anime".into(),
            total_episodes: Some("2".into()),
        }
    }

    fn episode(id: &str) -> Episode {
        Episode {
            id: id.into(),
            title: format!("Episode {id}"),
            is_series: true,
        }
    }

    #[test]
    fn queue_tracks_statuses_and_pending_items() {
        let mut queue = BatchQueue::new(
            0,
            movie(),
            vec![episode("1"), episode("2")],
            "1080p".into(),
            PathBuf::from("/tmp/animeku"),
            ExistingFilePolicy::Skip,
        );
        assert_eq!(queue.pending_indices(), vec![0, 1]);
        assert!(!queue.is_done());

        queue.mark(0, BatchItemStatus::Completed);
        queue.mark(1, BatchItemStatus::Failed);
        assert_eq!(queue.pending_indices(), vec![1]);
        assert_eq!(queue.counts(), (1, 0, 1, 0));

        queue.mark(1, BatchItemStatus::Skipped);
        assert!(queue.is_done());
    }

    #[test]
    fn queue_roundtrip_keeps_episode_metadata() {
        let queue = BatchQueue::new(
            1,
            movie(),
            vec![episode("1")],
            "720p".into(),
            PathBuf::from("/tmp/animeku"),
            ExistingFilePolicy::Overwrite,
        );
        let encoded = serde_json::to_string(&queue).unwrap();
        let decoded: BatchQueue = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.provider_type, 1);
        assert_eq!(decoded.movie.title, "Test Anime");
        assert_eq!(decoded.episodes[0].title, "Episode 1");
        assert_eq!(decoded.existing, ExistingFilePolicy::Overwrite);
    }
}
