use serde::{Deserialize, Serialize};

pub struct Input {
    pub title: String,
    pub tipe: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Movie {
    pub id: String,
    pub title: String,
    pub total_episodes: Option<String>,
}

impl std::fmt::Display for Movie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.title.trim())?;
        if let Some(total_episodes) = &self.total_episodes {
            write!(f, " ({} eps)", total_episodes)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub title: String,
    pub is_series: bool,
}

impl std::fmt::Display for Episode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.title.trim())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatchEntry {
    pub movie: Movie,
    pub episode: Episode,
    pub provider_type: usize,
    pub position_seconds: u64,
    /// Unix timestamp (seconds) of when this entry was last updated.
    pub updated_at: u64,
}

impl WatchEntry {
    pub fn format_duration(&self) -> String {
        format_seconds(self.position_seconds)
    }
}

/// Backward-compatibility alias.

/// Ordered watch history (most recent first), capped at 30 entries.

pub fn format_seconds(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stream {
    pub url: String,
    pub title: String,
}

impl std::fmt::Display for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.title.trim())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub thumb_url: Option<String>,
    pub data: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_seconds() {
        assert_eq!(format_seconds(0), "00:00");
        assert_eq!(format_seconds(59), "00:59");
        assert_eq!(format_seconds(60), "01:00");
        assert_eq!(format_seconds(125), "02:05");
        assert_eq!(format_seconds(3600), "01:00:00");
        assert_eq!(format_seconds(3665), "01:01:05");
    }

    #[test]
    fn test_last_watch_serde() {
        let lw = LastWatch {
            movie: Movie {
                id: "naruto-shippuden".into(),
                title: "Naruto Shippuden".into(),
                total_episodes: Some("500".into()),
            },
            episode: Episode {
                id: "naruto-ep-1".into(),
                title: "Episode 1".into(),
                is_series: true,
            },
            provider_type: 1,
            position_seconds: 754,
            updated_at: 0,
        };
        assert_eq!(lw.format_duration(), "12:34");

        let json = serde_json::to_string(&lw).unwrap();
        let deserialized: LastWatch = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.movie.title, "Naruto Shippuden");
        assert_eq!(deserialized.episode.title, "Episode 1");
        assert_eq!(deserialized.position_seconds, 754);
    }
}
