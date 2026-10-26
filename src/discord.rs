use crate::episode::clean_title;
use crate::episode::format_episode;
use crate::{DEFAULT_DISCORD_LOGO, DISCORD_IMAGE_MAX_LEN};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

pub(crate) fn is_otakudesu_image(url: &str) -> bool {
    url.to_ascii_lowercase().contains("otakudesu")
}

/// Fill a square canvas with the poster. Discord's RPC image box is square,
/// so `fit=cover` zooms instead of adding letterbox bars. `a=attention` asks
/// wsrv.nl to keep faces and visually busy parts in frame while cropping.
pub(crate) fn square_discord_poster(image_url: &str) -> String {
    // wsrv.nl refuses to fetch AniList directly, so route it through
    // DuckDuckGo's public image proxy first.
    let duckduckgo_proxy = format!(
        "https://external-content.duckduckgo.com/iu/?u={}",
        urlencoding::encode(image_url)
    );
    format!(
        "https://wsrv.nl/?url={}&w=512&h=512&fit=cover&a=attention&output=jpg",
        urlencoding::encode(&duckduckgo_proxy)
    )
}

/// Resolve a Discord-compatible poster URL. Otakudesu's own image host is
/// blocked when Discord tries to fetch it, so use AniList's cover CDN instead.
pub(crate) async fn resolve_discord_poster(thumb_url: Option<String>, movie_title: &str) -> String {
    let Some(url) = thumb_url else {
        return DEFAULT_DISCORD_LOGO.to_string();
    };
    if !is_otakudesu_image(&url) {
        return url;
    }

    let lookup_title = crate::models::clean_movie_title(movie_title);
    let lookup_title = if lookup_title.is_empty() {
        movie_title
    } else {
        lookup_title.as_str()
    };

    let Some(cover) = fetch_anilist_cover(lookup_title).await else {
        return DEFAULT_DISCORD_LOGO.to_string();
    };
    let square = square_discord_poster(&cover);
    if square.len() <= DISCORD_IMAGE_MAX_LEN {
        square
    } else {
        // Discord rejects image URLs longer than 300 characters.
        cover
    }
}

pub(crate) async fn fetch_anilist_cover(title: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let query = r#"
        query ($search: String) {
            Media(search: $search, type: ANIME) {
                coverImage { extraLarge large }
            }
        }
    "#;
    let response = client
        .post("https://graphql.anilist.co")
        .json(&serde_json::json!({
            "query": query,
            "variables": { "search": title }
        }))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }

    let value: serde_json::Value = response.json().await.ok()?;
    value["data"]["Media"]["coverImage"]["extraLarge"]
        .as_str()
        .or_else(|| value["data"]["Media"]["coverImage"]["large"].as_str())
        .map(str::to_string)
}

/// Everything the Discord card needs. Passed as one struct because the card
/// grew past the point where a long argument list stays readable.
pub(crate) struct PlaybackStatus<'a> {
    pub movie_id: &'a str,
    pub movie_title: &'a str,
    pub episode_title: &'a str,
    pub episode_id: &'a str,
    pub is_series: bool,
    pub season_num: u32,
    pub thumb_url: Option<String>,
    pub meta: Vec<(String, String)>,
    pub provider_type: usize,
    pub start_seconds: u64,
    pub duration_seconds: Option<u64>,
}

/// "2026 • 118 menit" for a film, "Episode 5 • 24 menit" for a series. Fields
/// Discord (or ffprobe) did not give us are simply left out.
fn status_line(status: &PlaybackStatus<'_>) -> String {
    let minutes = status
        .duration_seconds
        .filter(|seconds| *seconds > 0)
        .map(|seconds| format!("{} menit", (seconds / 60).max(1)));

    if status.is_series {
        let episode = format_episode(&clean_title(status.episode_title), true, status.season_num);
        return match minutes {
            Some(minutes) => format!("{episode} • {minutes}"),
            None => episode,
        };
    }

    let year = meta_release_year(&status.meta);
    let parts = [year, minutes]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "Full Movie".to_string()
    } else {
        parts.join(" • ")
    }
}

/// Providers spell the release date differently ("Rilis", "Tanggal Rilis",
/// "Tahun"); the first four-digit year in those values is what we want.
fn meta_release_year(meta: &[(String, String)]) -> Option<String> {
    let key = meta.iter().find(|(key, _)| {
        let key = key.to_lowercase();
        key.contains("rilis") || key.contains("tahun") || key.contains("release")
    })?;
    let value = key.1.trim();
    for (index, _) in value.char_indices() {
        let rest = &value[index..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 4 {
            let year = &rest[..4];
            if year.starts_with("19") || year.starts_with("20") {
                return Some(year.to_string());
            }
        }
    }
    None
}

/// Deep link to the provider page when the episode id is already a URL
/// (Otakudesu). Idlix ids are `type|slug`, which map to its public pages.
fn watch_url(status: &PlaybackStatus<'_>) -> Option<String> {
    if status.episode_id.starts_with("http://") || status.episode_id.starts_with("https://") {
        return Some(status.episode_id.to_string());
    }

    if status.provider_type == crate::PROVIDER_IDLIX {
        let (content_type, slug) = status.movie_id.split_once('|')?;
        let section = if content_type == "movie" {
            "movie"
        } else {
            "series"
        };
        let base = crate::ext::idlix::idlix_base_url();
        return Some(format!("{}/{section}/{slug}", base.trim_end_matches('/')));
    }

    None
}

fn watch_button(status: &PlaybackStatus<'_>) -> Option<activity::Button<'static>> {
    let label = if status.is_series {
        "Tonton Episode"
    } else {
        "Tonton Film"
    };
    watch_url(status).map(|url| activity::Button::new(label, url))
}

pub(crate) async fn update_discord_status(
    discord: &mut Option<DiscordIpcClient>,
    status: &PlaybackStatus<'_>,
) {
    let drpc = match discord {
        Some(ref mut client) => client,
        None => return,
    };

    let large_image = resolve_discord_poster(status.thumb_url.clone(), status.movie_title).await;

    let clean_movie = clean_title(status.movie_title);

    let mut assets = activity::Assets::new()
        .large_image(&large_image)
        .large_text(&clean_movie);

    if large_image != DEFAULT_DISCORD_LOGO {
        assets = assets.small_image(DEFAULT_DISCORD_LOGO).small_text("Animeku-CLI");
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // Start is shifted back by the resume point so the bar reflects the real
    // position; Discord animates it locally once both ends are known.
    let start = now - status.start_seconds as i64;
    let mut timestamps = activity::Timestamps::new().start(start);
    if let Some(duration) = status.duration_seconds.filter(|seconds| *seconds > 0) {
        timestamps = timestamps.end(start + duration as i64);
    }

    let mut card = activity::Activity::new()
        .details(&clean_movie)
        .state(status_line(status))
        .assets(assets)
        .timestamps(timestamps)
        .activity_type(activity::ActivityType::Watching)
        // Puts the show title in "Watching ..." instead of the app name.
        .status_display_type(activity::StatusDisplayType::Details);

    if let Some(button) = watch_button(status) {
        card = card.buttons(vec![button]);
    }

    let _ = drpc.set_activity(card);
}

pub(crate) fn reset_discord_status(discord: &mut Option<DiscordIpcClient>) {
    if let Some(ref mut drpc) = discord {
        let _ = drpc.set_activity(
            activity::Activity::new()
                .state("Memilih Anime...")
                .details("Animeku-CLI")
                .activity_type(activity::ActivityType::Watching),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(is_series: bool, meta: Vec<(String, String)>) -> PlaybackStatus<'static> {
        PlaybackStatus {
            movie_id: "tv_series|khadam-2026",
            movie_title: "[Series] Khadam",
            episode_title: "S1E06 - To All of You",
            episode_id: "episode|abc",
            is_series,
            season_num: 1,
            thumb_url: None,
            meta,
            provider_type: crate::PROVIDER_IDLIX,
            start_seconds: 0,
            duration_seconds: Some(7080),
        }
    }

    #[test]
    fn film_status_line_matches_the_card_layout() {
        let meta = vec![("Rilis".to_string(), "2026-01-15".to_string())];
        assert_eq!(status_line(&status(false, meta)), "2026 • 118 menit");
    }

    #[test]
    fn series_status_line_keeps_the_episode() {
        let meta = vec![("Rilis".to_string(), "2026-01-15".to_string())];
        assert_eq!(status_line(&status(true, meta)), "Episode 6 • 118 menit");
    }

    #[test]
    fn release_year_is_read_from_any_date_format() {
        let meta = vec![
            ("Skor".to_string(), "8.91".to_string()),
            ("Tanggal Rilis".to_string(), "15 Jan 2024".to_string()),
        ];
        assert_eq!(meta_release_year(&meta).as_deref(), Some("2024"));
        assert_eq!(meta_release_year(&[]), None);
    }

    #[test]
    fn idlix_button_opens_the_series_page() {
        let url = watch_url(&status(true, Vec::new())).expect("series url");
        assert!(url.ends_with("/series/khadam-2026"), "unexpected url {url}");
    }

    #[test]
    fn idlix_movies_use_the_movie_section() {
        let mut info = status(false, Vec::new());
        info.movie_id = "movie|khadam";
        let url = watch_url(&info).expect("movie url");
        assert!(url.ends_with("/movie/khadam"), "unexpected url {url}");
    }

    #[test]
    fn http_episode_ids_become_the_button_url() {
        let mut info = status(true, Vec::new());
        info.episode_id = "https://otakudesu.blog/episode/judul-ep-1/";
        info.provider_type = crate::PROVIDER_OTAKUDESU;
        assert_eq!(
            watch_url(&info).as_deref(),
            Some("https://otakudesu.blog/episode/judul-ep-1/")
        );
    }

    #[test]
    fn unknown_providers_without_a_url_get_no_button() {
        let mut info = status(true, Vec::new());
        info.provider_type = 99;
        assert!(watch_button(&info).is_none());
    }
}
