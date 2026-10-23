use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use colored::Colorize;
use tokio::runtime;

use crate::util::clearscreen_and_show_banner;

mod anilist;
mod animeku;
mod batch;
mod cache;
mod cli;
mod config;
mod discord;
mod download;
mod downloads_menu;
mod episode;
mod ext;
mod health;
mod input;
mod library;
mod live_input;
mod menu;
mod models;
mod player;
mod proxy;
mod update;
mod update_menu;
mod util;
mod watchlist_menu;

pub(crate) use cli::*;
pub(crate) use discord::*;
pub(crate) use downloads_menu::*;
pub(crate) use health::*;
pub(crate) use library::*;
pub(crate) use menu::*;
pub(crate) use player::*;
pub(crate) use update_menu::*;
pub(crate) use watchlist_menu::*;

const DISCORD_APP_ID: &str = "1549897147577667784";
const DEFAULT_DISCORD_LOGO: &str = "logoanimekucli";
const DISCORD_IMAGE_MAX_LEN: usize = 300;

const PROVIDER_IDLIX: usize = ext::PROVIDER_IDLIX;
pub const PROVIDER_OTAKUDESU: usize = ext::PROVIDER_OTAKUDESU;

const REPO_URL: &str = "https://github.com/yuutaa7x/animeku-cli";

async fn run_app() -> anyhow::Result<()> {
    crate::util::cleanup_temp_files();
    crate::update::cleanup_old_binary();

    if cli_check_update() {
        return handle_check_update().await;
    }
    if cli_update() {
        return handle_self_update(false).await;
    }

    let config = config::Config::load();
    let mut discord = if config.rpc && !rpc_disabled() {
        Some(DiscordIpcClient::new(DISCORD_APP_ID))
    } else {
        None
    };
    if let Some(ref mut drpc) = discord {
        if drpc.connect().is_ok() {
            reset_discord_status(&mut discord);
        } else {
            discord = None;
        }
    }

    if let Some(title) = cli_search_title() {
        let result = run_cli_search(&mut discord, title).await;
        crate::util::cleanup_temp_files();
        return result;
    }

    if cli_resume() {
        let result = handle_resume_latest(&mut discord).await;
        crate::util::cleanup_temp_files();
        return result;
    }

    let mut config = config::Config::load();
    if !config.onboarding_done {
        run_onboarding(&mut config)?;
        config = config::Config::load();
    }

    if config.needs_dependency_check(check_command_exists("mpv")) {
        let proceed = show_health_screen().await?;
        let mut updated = config::Config::load();
        updated.dependency_check_done = true;
        updated.save();
        if !proceed {
            crate::util::cleanup_temp_files();
            return Ok(());
        }
    }

    loop {
        clearscreen_and_show_banner()?;
        let Some(action) = show_main_menu()? else {
            crate::util::cleanup_temp_files();
            println!("\nSampai jumpa lagi!\n");
            return Ok(());
        };

        match action {
            MainAction::Continue => {
                handle_resume_latest(&mut discord).await?;
            }
            MainAction::Batch => {
                handle_batch_manager().await?;
            }
            MainAction::Search => {
                handle_search_and_watch(&mut discord).await?;
            }
            MainAction::Explore => {
                handle_explore(&mut discord).await?;
            }
            MainAction::Watchlist => {
                handle_watchlist(&mut discord).await?;
            }
            MainAction::History => {
                handle_resume_last_watch(&mut discord).await?;
            }
            MainAction::Downloaded => {
                handle_downloaded_menu()?;
            }
            MainAction::Settings => {
                handle_settings_menu(&mut discord).await?;
            }
            MainAction::Health => {
                if show_health_screen().await? {
                    let mut updated = config::Config::load();
                    updated.dependency_check_done = true;
                    updated.save();
                }
            }
            MainAction::Update => {
                handle_self_update(true).await?;
            }
            MainAction::Exit => {
                crate::util::cleanup_temp_files();
                println!("\nSampai jumpa lagi!\n");
                return Ok(());
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let _ = CLI_ARGS.set(args);

    let rt = runtime::Builder::new_multi_thread().enable_all().build()?;
    if let Err(e) = rt.block_on(run_app()) {
        crate::util::cleanup_temp_files();
        eprintln!("\n{} {:#}", "■".red(), e);
        eprintln!(
            "  Coba jalankan lagi, atau buka menu {} untuk memeriksa dependensi.",
            "Cek kesehatan".cyan()
        );
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn test_check_command_exists() {
        assert!(check_command_exists("cargo"));
        assert!(!check_command_exists("nonexistent_random_command_12345"));
    }

    #[test]
    fn watchlist_new_episodes_handles_both_provider_orders() {
        let episode = |id: &str, title: &str| crate::models::Episode {
            id: id.into(),
            title: title.into(),
            is_series: true,
        };
        let last = crate::models::WatchEntry {
            episode: episode("2", "Episode 2"),
            ..Default::default()
        };

        let oldest_first = vec![
            episode("1", "Episode 1"),
            episode("2", "Episode 2"),
            episode("3", "Episode 3"),
        ];
        assert_eq!(
            episodes_after_last(&oldest_first, Some(&last))
                .iter()
                .map(|episode| episode.id.as_str())
                .collect::<Vec<_>>(),
            vec!["3"]
        );

        let newest_first = vec![
            episode("3", "Episode 3"),
            episode("2", "Episode 2"),
            episode("1", "Episode 1"),
        ];
        assert_eq!(
            episodes_after_last(&newest_first, Some(&last))
                .iter()
                .map(|episode| episode.id.as_str())
                .collect::<Vec<_>>(),
            vec!["3"]
        );

        let last_newest = crate::models::WatchEntry {
            episode: episode("3", "Episode 3"),
            ..Default::default()
        };
        assert!(episodes_after_last(&newest_first, Some(&last_newest)).is_empty());

        let last_season_one = crate::models::WatchEntry {
            episode: episode("s1e12", "S1E12 - Finale"),
            ..Default::default()
        };
        let cross_season = vec![
            episode("s2e02", "S2E02 - Next"),
            episode("s2e01", "S2E01 - Start"),
            episode("s1e12", "S1E12 - Finale"),
        ];
        assert_eq!(
            episodes_after_last(&cross_season, Some(&last_season_one))
                .iter()
                .map(|episode| episode.id.as_str())
                .collect::<Vec<_>>(),
            vec!["s2e01", "s2e02"]
        );
    }

    #[test]
    fn completed_watch_entry_continues_with_the_next_episode() {
        let episode = |id: &str, title: &str| crate::models::Episode {
            id: id.into(),
            title: title.into(),
            is_series: true,
        };
        let entry = crate::models::WatchEntry {
            movie: crate::models::Movie {
                id: "series".into(),
                title: "Test Series".into(),
                total_episodes: None,
            },
            episode: episode("2", "Episode 2"),
            provider_type: PROVIDER_IDLIX,
            position_seconds: 0,
            completed: true,
            updated_at: 1,
        };
        let episodes = vec![
            episode("1", "Episode 1"),
            episode("2", "Episode 2"),
            episode("3", "Episode 3"),
        ];

        let next = next_watch_entry(&entry, &episodes).expect("next episode should exist");
        assert_eq!(next.episode.id, "3");
        assert_eq!(next.position_seconds, 0);
        assert!(!next.completed);

        let unfinished = crate::models::WatchEntry {
            completed: false,
            ..entry.clone()
        };
        assert!(next_watch_entry(&unfinished, &episodes).is_none());

        let last = crate::models::WatchEntry {
            episode: episode("3", "Episode 3"),
            ..entry
        };
        assert!(next_watch_entry(&last, &episodes).is_none());
    }

    #[test]
    fn test_temp_file_paths() {
        let p = idlix_audio_path();
        assert!(p.contains("animeku_idlix_audio.m3u8"));
        assert!(!p.contains('\\'));
    }

    #[tokio::test]
    async fn test_discord_poster_resolution() {
        assert_eq!(
            resolve_discord_poster(None, "Whatever").await,
            DEFAULT_DISCORD_LOGO
        );

        let tmdb = "https://image.tmdb.org/t/p/w500/poster.jpg";
        assert_eq!(
            resolve_discord_poster(Some(tmdb.to_string()), "Whatever").await,
            tmdb
        );
        assert!(is_otakudesu_image("https://otakudesu.blog/poster.jpg"));
        assert!(!is_otakudesu_image(tmdb));
    }

    #[test]
    fn test_square_discord_poster() {
        let square = square_discord_poster(
            "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx185407-7uzY4fA3hokP.jpg",
        );
        assert!(square.starts_with("https://wsrv.nl/?url="));
        assert!(square.contains("w=512&h=512&fit=cover&a=attention"));
        assert!(square.len() <= DISCORD_IMAGE_MAX_LEN);
    }

    #[tokio::test]
    #[ignore = "requires network access to AniList and wsrv.nl"]
    async fn test_anilist_cover_lookup() {
        let cover = fetch_anilist_cover("Takopii no Genzai").await;
        assert!(cover.is_some_and(|url| url.contains("anilist")));
    }

    #[tokio::test]
    #[ignore = "requires network access to DuckDuckGo and wsrv.nl"]
    async fn test_square_poster_is_fetchable() {
        let cover = fetch_anilist_cover("Takopii no Genzai")
            .await
            .expect("AniList cover lookup failed");
        let square = square_discord_poster(&cover);
        assert!(square.len() <= DISCORD_IMAGE_MAX_LEN);

        let response = reqwest::get(&square)
            .await
            .expect("square poster fetch failed");
        assert!(response.status().is_success());
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(content_type.starts_with("image/"));
    }
}

