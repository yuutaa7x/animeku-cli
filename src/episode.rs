use regex::Regex;

use crate::models::Episode;

pub fn clean_title(title: &str) -> String {
    let mut cleaned = title.to_string();
    cleaned = cleaned.replace("[Movie]", "");
    cleaned = cleaned.replace("[Series]", "");
    cleaned = cleaned.replace("Subtitle Indonesia", "");
    cleaned = cleaned.replace("Sub Indo", "");
    cleaned = cleaned.replace("subtitle indonesia", "");
    cleaned = cleaned.replace("sub indo", "");

    let re_season = Regex::new(r"(?i)\bSeason\s*\d+\b").unwrap();
    cleaned = re_season.replace_all(&cleaned, "").to_string();

    let re_s = Regex::new(r"(?i)\bS\d+\b").unwrap();
    cleaned = re_s.replace_all(&cleaned, "").to_string();

    let re_ep_range = Regex::new(r"(?i)\(\s*episode\s*.*?\)").unwrap();
    cleaned = re_ep_range.replace_all(&cleaned, "").to_string();

    let re_batch = Regex::new(r"(?i)\(\s*batch\s*\)").unwrap();
    cleaned = re_batch.replace_all(&cleaned, "").to_string();

    let re_spaces = Regex::new(r"\s+").unwrap();
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();

    let re_hyphen = Regex::new(r"(?i)\s*-\s*$").unwrap();
    cleaned = re_hyphen.replace_all(&cleaned, "").to_string();

    let re_brackets = Regex::new(r"\(\s*\)").unwrap();
    cleaned = re_brackets.replace_all(&cleaned, "").to_string();

    cleaned.trim().to_string()
}

pub fn detect_season(title: &str, img_url: &str, meta_data: &[(String, String)]) -> Option<u32> {
    let re_season = Regex::new(r"(?i)(?:season|s)\s*-?\s*(\d+)").unwrap();
    let re_ordinal = Regex::new(r"(?i)(\d+)(?:st|nd|rd|th)\s+season").unwrap();
    let re_roman = Regex::new(r"(?i)\b(II|III|IV|V|VI|VII|VIII|IX|X)\b").unwrap();

    let parse_text = |text: &str| -> Option<u32> {
        if let Some(caps) = re_season.captures(text) {
            if let Ok(num) = caps[1].parse::<u32>() {
                return Some(num);
            }
        }
        if let Some(caps) = re_ordinal.captures(text) {
            if let Ok(num) = caps[1].parse::<u32>() {
                return Some(num);
            }
        }
        if let Some(caps) = re_roman.captures(text) {
            let roman = caps[1].to_uppercase();
            let num = match roman.as_str() {
                "II" => 2,
                "III" => 3,
                "IV" => 4,
                "V" => 5,
                "VI" => 6,
                "VII" => 7,
                "VIII" => 8,
                "IX" => 9,
                "X" => 10,
                _ => 1,
            };
            return Some(num);
        }
        None
    };

    if let Some(s) = parse_text(title) {
        return Some(s);
    }
    if let Some(s) = parse_text(img_url) {
        return Some(s);
    }

    for (_meta_key, meta_value) in meta_data {
        if let Some(s) = parse_text(meta_value) {
            return Some(s);
        }
    }

    None
}

pub fn format_episode(title: &str, is_series: bool, season_num: u32) -> String {
    let trimmed = title.trim();
    if !is_series {
        return "Full Movie".to_string();
    }

    let re_episode = Regex::new(r"(?i)episode\s*(\d+)").unwrap();
    if let Some(caps) = re_episode.captures(trimmed) {
        if let Some(num) = caps.get(1) {
            let ep_num: u32 = num.as_str().parse().unwrap_or(1);
            return if season_num > 1 {
                format!("Season {} • Episode {}", season_num, ep_num)
            } else {
                format!("Episode {}", ep_num)
            };
        }
    }

    let re_season_ep = Regex::new(r"(?i)s(\d+)e(\d+)").unwrap();
    if let Some(caps) = re_season_ep.captures(trimmed) {
        let season: u32 = caps[1].parse().unwrap_or(season_num);
        let ep: u32 = caps[2].parse().unwrap_or(1);
        return if season > 1 {
            format!("Season {} • Episode {}", season, ep)
        } else {
            format!("Episode {}", ep)
        };
    }

    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return if season_num > 1 {
            format!("Season {} • Episode {}", season_num, trimmed)
        } else {
            format!("Episode {}", trimmed)
        };
    }

    if season_num > 1 {
        format!("Season {} • {}", season_num, trimmed)
    } else {
        trimmed.to_string()
    }
}

fn extract_ep_num(title: &str) -> u32 {
    // Extract the last continuous sequence of digits from the title as the episode number.
    // "Episode 10" -> 10, "S1E02" -> 2
    let mut num_str = String::new();
    let mut in_num = false;
    let mut last_num = 0;
    for c in title.chars() {
        if c.is_ascii_digit() {
            num_str.push(c);
            in_num = true;
        } else if in_num {
            last_num = num_str.parse().unwrap_or(0);
            num_str.clear();
            in_num = false;
        }
    }
    if in_num {
        last_num = num_str.parse().unwrap_or(0);
    }
    last_num
}

/// Parse season and episode numbers from common provider title formats:
/// `S1E01`, `S01E01`, `Season 2 Episode 5`, `Season 2 Ep 5`.
fn parse_season_episode(title: &str) -> Option<(u32, u32)> {
    let sxe = Regex::new(r"(?i)\bS(\d{1,2})\s*E(\d{1,4})\b").unwrap();
    if let Some(caps) = sxe.captures(title) {
        if let (Ok(season), Ok(episode)) = (caps[1].parse(), caps[2].parse()) {
            return Some((season, episode));
        }
    }

    let long =
        Regex::new(r"(?i)\bSeason\s*(\d{1,2})\s*(?:[-:•]\s*)?(?:Episode|Ep|E)\s*(\d{1,4})\b")
            .unwrap();
    if let Some(caps) = long.captures(title) {
        if let (Ok(season), Ok(episode)) = (caps[1].parse(), caps[2].parse()) {
            return Some((season, episode));
        }
    }

    None
}

/// Detect a season number using only titles. Returns `None` when the titles
/// carry no season information at all.
pub fn detect_season_from_titles(movie_title: &str, episode_title: &str) -> Option<u32> {
    parse_season_episode(episode_title)
        .map(|(season, _)| season)
        .or_else(|| detect_season(episode_title, "", &[]))
        .or_else(|| detect_season(movie_title, "", &[]))
}

/// Pick the next episode from the provider's ordered list. List order handles
/// season boundaries correctly (`S1E12` -> `S2E01`), unlike a global
/// "episode number + 1" search which can jump back to an earlier season.
pub fn find_next_episode<'a>(episodes: &'a [Episode], current: &Episode) -> Option<&'a Episode> {
    if let Some(index) = episodes.iter().position(|e| e.id == current.id) {
        if let Some(next) = episodes.get(index + 1) {
            return Some(next);
        }
    }

    if let Some((season, episode)) = parse_season_episode(&current.title) {
        return episodes.iter().find(|e| {
            matches!(
                parse_season_episode(&e.title),
                Some((s, n)) if (s == season && n == episode + 1) || (s == season + 1 && n == 1)
            )
        });
    }

    let current_num = extract_ep_num(&current.title);
    if current_num > 0 {
        return episodes
            .iter()
            .find(|e| extract_ep_num(&e.title) == current_num + 1);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_season_episode() {
        assert_eq!(parse_season_episode("S1E01 - To You in 2016"), Some((1, 1)));
        assert_eq!(parse_season_episode("S02E13 - Crossing"), Some((2, 13)));
        assert_eq!(
            parse_season_episode("Season 3 Episode 7 - Reunion"),
            Some((3, 7))
        );
        assert_eq!(parse_season_episode("Episode 5"), None);
        assert_eq!(parse_season_episode("Takopii no Genzai Episode 1"), None);
    }

    #[test]
    fn test_find_next_episode_across_seasons() {
        let episodes = vec![
            Episode {
                id: "s1e12".to_string(),
                title: "S1E12 - Finale".to_string(),
                is_series: true,
            },
            Episode {
                id: "s2e01".to_string(),
                title: "S2E01 - New Beginning".to_string(),
                is_series: true,
            },
            Episode {
                id: "s2e02".to_string(),
                title: "S2E02 - Next".to_string(),
                is_series: true,
            },
        ];

        assert_eq!(
            find_next_episode(&episodes, &episodes[0]).map(|e| e.id.as_str()),
            Some("s2e01")
        );
        assert_eq!(
            find_next_episode(&episodes, &episodes[1]).map(|e| e.id.as_str()),
            Some("s2e02")
        );
        assert!(find_next_episode(&episodes, &episodes[2]).is_none());
    }

    #[test]
    fn test_detect_season_from_titles() {
        assert_eq!(
            detect_season_from_titles("Takopii no Genzai", "S1E01 - To You in 2016"),
            Some(1)
        );
        assert_eq!(
            detect_season_from_titles("Jujutsu Kaisen Season 2", "Episode 5"),
            Some(2)
        );
        assert_eq!(detect_season_from_titles("Full Movie", "Full Movie"), None);
    }

    #[test]
    fn test_clean_title_removes_provider_noise() {
        assert_eq!(
            clean_title("[Movie] Grave of the Fireflies"),
            "Grave of the Fireflies"
        );
        assert_eq!(
            clean_title("Takopii no Genzai (Episode 1 – 6) Subtitle Indonesia"),
            "Takopii no Genzai"
        );
        assert_eq!(
            clean_title("[Series] Takopi's Original Sin"),
            "Takopi's Original Sin"
        );
    }

    #[test]
    fn test_format_episode() {
        assert_eq!(format_episode("Episode 5", true, 1), "Episode 5");
        assert_eq!(format_episode("Episode 5", true, 2), "Season 2 • Episode 5");
        assert_eq!(format_episode("S1E01 - Start", true, 1), "Episode 1");
        assert_eq!(
            format_episode("Putar Film (Full Movie)", false, 1),
            "Full Movie"
        );
    }
}
