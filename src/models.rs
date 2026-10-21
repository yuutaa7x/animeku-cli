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
    /// True when the player reported that the episode was watched to the end.
    #[serde(default)]
    pub completed: bool,
    /// Unix timestamp (seconds) of when this entry was last updated.
    pub updated_at: u64,
}

impl WatchEntry {
    pub fn format_duration(&self) -> String {
        format_seconds(self.position_seconds)
    }

    pub fn status_label(&self) -> String {
        if self.completed {
            "✓ Tamat".to_string()
        } else if self.position_seconds == 0 {
            "▶ Belum dimulai".to_string()
        } else {
            format!("▶ {}", self.format_duration())
        }
    }

    pub fn format_updated(&self) -> String {
        format_relative_time(self.updated_at)
    }
}

/// Human-friendly relative timestamp. Kept dependency-free because the only
/// precision needed by the history screen is minutes/hours/days.
pub fn format_relative_time(updated_at: u64) -> String {
    if updated_at == 0 {
        return "waktu tidak diketahui".to_string();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(updated_at);
    match diff {
        0..=59 => "baru saja".to_string(),
        60..=3599 => format!("{} menit lalu", diff / 60),
        3600..=86_399 => format!("{} jam lalu", diff / 3600),
        86_400..=604_799 => format!("{} hari lalu", diff / 86_400),
        _ => format!("{} minggu lalu", diff / 604_800),
    }
}

/// Backward-compatibility alias used by older history payloads/tests.
#[allow(dead_code)]
pub type LastWatch = WatchEntry;

/// Format seconds as `MM:SS`, or `HH:MM:SS` when the duration is at least an hour.
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

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_suffix_ci(text: &str, suffix: &str) -> String {
    if text.to_lowercase().ends_with(&suffix.to_lowercase()) {
        text[..text.len() - suffix.len()].trim_end().to_string()
    } else {
        text.to_string()
    }
}

/// Remove Otakudesu listing noise such as `(Episode 1 – 6) Subtitle Indonesia`
/// so history entries show the actual anime title.
pub fn clean_movie_title(raw: &str) -> String {
    let decoded = html_escape::decode_html_entities(raw);
    let mut title = collapse_whitespace(&decoded);
    title = strip_suffix_ci(&title, " subtitle indonesia");

    let range = regex::Regex::new(
        r"(?i)\s*\(\s*episode\s*\d{1,4}(?:\s*[-–—~]\s*\d{1,4})?\s*\)\s*$",
    )
    .unwrap();
    title = collapse_whitespace(&range.replace(&title, ""));

    if title.is_empty() {
        collapse_whitespace(&decoded)
    } else {
        title
    }
}

/// Convert provider-specific episode labels into a short `Episode N` label.
/// Titles that already use another format (for example Idlix `S1E01`) are
/// returned unchanged.
pub fn clean_episode_title(raw: &str) -> String {
    let decoded = html_escape::decode_html_entities(raw);
    let text = collapse_whitespace(&decoded);

    let episode = regex::Regex::new(r"(?i)\b(?:episode|eps?)\s*\.?\s*(\d{1,4})\b").unwrap();
    if let Some(caps) = episode.captures(&text) {
        if let Ok(number) = caps[1].parse::<u32>() {
            return format!("Episode {}", number);
        }
    }

    let stripped = strip_suffix_ci(&text, " subtitle indonesia");
    if stripped.is_empty() {
        text
    } else {
        stripped
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
    fn test_clean_otakudesu_titles() {
        assert_eq!(
            clean_movie_title("Takopii no Genzai (Episode 1 – 6) Subtitle Indonesia"),
            "Takopii no Genzai"
        );
        assert_eq!(
            clean_movie_title("Jujutsu Kaisen Season 2 (Episode 1 - 12) Subtitle Indonesia"),
            "Jujutsu Kaisen Season 2"
        );
        assert_eq!(
            clean_episode_title("Takopii no Genzai Episode 1 Subtitle Indonesia"),
            "Episode 1"
        );
        assert_eq!(
            clean_episode_title("Takopii no Genzai Eps. 06 Subtitle Indonesia"),
            "Episode 6"
        );
        assert_eq!(
            clean_episode_title("S1E01 - To You in 2016"),
            "S1E01 - To You in 2016"
        );
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
            completed: false,
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
