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

pub(crate) async fn update_discord_status(
    discord: &mut Option<DiscordIpcClient>,
    movie_title: &str,
    episode_title: &str,
    is_series: bool,
    season_num: u32,
    thumb_url: Option<String>,
) {
    let drpc = match discord {
        Some(ref mut client) => client,
        None => return,
    };

    let large_image = resolve_discord_poster(thumb_url, movie_title).await;

    let clean_movie = clean_title(movie_title);
    let clean_ep = clean_title(episode_title);

    let mut assets = activity::Assets::new()
        .large_image(&large_image)
        .large_text(&clean_movie);

    if large_image != DEFAULT_DISCORD_LOGO {
        assets = assets.small_image(DEFAULT_DISCORD_LOGO).small_text("Animeku-CLI");
    }

    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let _ = drpc.set_activity(
        activity::Activity::new()
            .details(&clean_movie)
            .state(format_episode(&clean_ep, is_series, season_num))
            .assets(assets)
            .timestamps(activity::Timestamps::new().start(current_timestamp))
            .activity_type(activity::ActivityType::Watching),
    );
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
