use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const ENDPOINT: &str = "https://graphql.anilist.co";

#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub title: String,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub average_score: Option<i64>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub year: Option<i64>,
    pub episodes: Option<i64>,
    pub cover_url: Option<String>,
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("animeku-cli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .context("gagal membuat client AniList")
}

async fn request(query: &str, variables: Value) -> Result<Value> {
    let response = client()?
        .post(ENDPOINT)
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .await
        .context("gagal menghubungi AniList")?;

    if !response.status().is_success() {
        anyhow::bail!("AniList mengembalikan status {}", response.status());
    }

    response
        .json::<Value>()
        .await
        .context("gagal membaca respons AniList")
}

const MEDIA_FIELDS: &str = r#"
    title { romaji english native }
    description(asHtml: false)
    genres
    averageScore
    status
    season
    seasonYear
    episodes
    coverImage { large }
"#;

pub async fn search(title: &str) -> Result<Option<MediaInfo>> {
    let query = format!(
        "query ($search: String) {{ Media(search: $search, type: ANIME, sort: SEARCH_MATCH) {{ {MEDIA_FIELDS} }} }}"
    );
    let json = request(&query, json!({ "search": title })).await?;
    Ok(json.pointer("/data/Media").and_then(parse_media))
}

pub async fn trending(per_page: usize) -> Result<Vec<MediaInfo>> {
    let query = format!(
        "query ($perPage: Int) {{ Page(page: 1, perPage: $perPage) {{ media(type: ANIME, sort: TRENDING_DESC) {{ {MEDIA_FIELDS} }} }} }}"
    );
    let json = request(&query, json!({ "perPage": per_page.clamp(1, 30) })).await?;
    let media = json
        .pointer("/data/Page/media")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_media).collect())
        .unwrap_or_default();
    Ok(media)
}

fn parse_media(value: &Value) -> Option<MediaInfo> {
    let title = ["/title/romaji", "/title/english", "/title/native"]
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }

    Some(MediaInfo {
        title,
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(strip_html)
            .filter(|text| !text.is_empty()),
        genres: value
            .get("genres")
            .and_then(Value::as_array)
            .map(|genres| {
                genres
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        average_score: value.get("averageScore").and_then(Value::as_i64),
        status: value
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string),
        season: value
            .get("season")
            .and_then(Value::as_str)
            .map(str::to_string),
        year: value.get("seasonYear").and_then(Value::as_i64),
        episodes: value.get("episodes").and_then(Value::as_i64),
        cover_url: value
            .pointer("/coverImage/large")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn strip_html(raw: &str) -> String {
    let with_breaks = raw
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n");
    let tags = regex::Regex::new(r"(?is)<[^>]+>").unwrap();
    let without_tags = tags.replace_all(&with_breaks, " ");
    let decoded = html_escape::decode_html_entities(&without_tags);
    decoded
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_info(info: &MediaInfo) -> String {
    let mut lines = Vec::new();
    lines.push(info.title.clone());

    let mut facts = Vec::new();
    if let Some(score) = info.average_score {
        facts.push(format!("Skor {score}/100"));
    }
    if let Some(status) = &info.status {
        facts.push(status.clone());
    }
    let season = match (info.season.as_deref(), info.year) {
        (Some(season), Some(year)) => Some(format!("{season} {year}")),
        (Some(season), None) => Some(season.to_string()),
        (None, Some(year)) => Some(year.to_string()),
        (None, None) => None,
    };
    if let Some(season) = season {
        facts.push(season);
    }
    if let Some(episodes) = info.episodes {
        facts.push(format!("{episodes} episode"));
    }
    if !facts.is_empty() {
        lines.push(format!("  {}", facts.join(" • ")));
    }
    if !info.genres.is_empty() {
        lines.push(format!("  Genre: {}", info.genres.join(", ")));
    }
    if let Some(description) = &info.description {
        lines.push(String::new());
        lines.extend(wrap_text(description, 78));
    }
    lines.join("\n")
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
            } else if line.len() + 1 + word.len() <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(line);
                line = word.to_string();
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_html_and_collapses_whitespace() {
        let text = strip_html("<p>Hello <b>world</b>.<br>Second &amp; line.</p>");
        assert_eq!(text, "Hello world .\nSecond & line.");
    }

    #[test]
    fn parses_media_payload() {
        let json = serde_json::json!({
            "title": { "romaji": "Sousou no Frieren", "english": "Frieren" },
            "description": "<i>Fantasy</i> journey",
            "genres": ["Adventure", "Fantasy"],
            "averageScore": 92,
            "status": "FINISHED",
            "season": "FALL",
            "seasonYear": 2023,
            "episodes": 28,
            "coverImage": { "large": "https://example.com/cover.jpg" }
        });
        let info = parse_media(&json).expect("media should parse");
        assert_eq!(info.title, "Sousou no Frieren");
        assert_eq!(info.average_score, Some(92));
        assert_eq!(info.episodes, Some(28));
        assert_eq!(info.genres, vec!["Adventure", "Fantasy"]);
        assert_eq!(info.description.as_deref(), Some("Fantasy journey"));
    }

    #[test]
    fn formats_facts_and_wraps_description() {
        let info = MediaInfo {
            title: "Test".into(),
            description: Some("one two three four five six seven eight".into()),
            genres: vec!["Action".into()],
            average_score: Some(80),
            status: Some("RELEASING".into()),
            season: Some("WINTER".into()),
            year: Some(2026),
            episodes: Some(12),
            cover_url: None,
        };
        let formatted = format_info(&info);
        assert!(formatted.contains("Skor 80/100"));
        assert!(formatted.contains("WINTER 2026"));
        assert!(formatted.contains("Genre: Action"));
    }
}
