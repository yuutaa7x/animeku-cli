use crate::input::get_user_input;
use crate::input::save_history;
use crate::util::clearscreen_and_show_banner;
use crate::animeku::AnimekuCli;
use crate::cli::provider_override;
use crate::player::handle_movie_episodes;
use crate::{PROVIDER_IDLIX, PROVIDER_OTAKUDESU};
use crate::{ config, input };
use colored::Colorize;
use discord_rich_presence::DiscordIpcClient;

pub(crate) fn discovery_label(info: &crate::anilist::MediaInfo) -> String {
    let mut facts = Vec::new();
    if let Some(score) = info.average_score {
        facts.push(format!("{score}"));
    }
    if let Some(year) = info.year {
        facts.push(year.to_string());
    }
    if !info.genres.is_empty() {
        facts.push(
            info.genres
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if facts.is_empty() {
        info.title.clone()
    } else {
        format!("{} • {}", info.title, facts.join(" • "))
    }
}

pub(crate) async fn show_discovery_list<F>(
    discord: &mut Option<DiscordIpcClient>,
    heading: &str,
    loading_message: &str,
    fetch: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<Vec<crate::anilist::MediaInfo>>>,
{
    clearscreen_and_show_banner()?;
    let media = match crate::util::with_spinner(loading_message, fetch).await {
        Ok(media) => media,
        Err(error) => {
            eprintln!("{} Gagal memuat {}: {:#}", "■".red(), heading, error);
            println!();
            println!("Koneksi ke AniList bermasalah. Coba lagi dari menu Jelajahi.");
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        }
    };

    if media.is_empty() {
        println!("Tidak ada hasil dari AniList.\n");
        println!("Tekan Enter untuk kembali...");
        let mut dummy = String::new();
        let _ = std::io::stdin().read_line(&mut dummy);
        return Ok(());
    }

    loop {
        clearscreen_and_show_banner()?;
        let mut entries = media
            .iter()
            .map(|info| input::MenuEntry::new(discovery_label(info)))
            .collect::<Vec<_>>();
        entries.push(input::MenuEntry::new("Cari anime lain"));
        entries.push(input::MenuEntry::new("Kembali"));

        let Some(choice) = input::select_menu_simple(heading, &entries, 0)? else {
            break;
        };

        if choice < media.len() {
            run_cli_search(discord, media[choice].title.clone()).await?;
            continue;
        }

        if choice == media.len() {
            let title: String = dialoguer::Input::with_theme(&crate::util::custom_theme())
                .with_prompt("Judul anime")
                .interact_text()?;
            let title = title.trim();
            if title.is_empty() {
                continue;
            }

            clearscreen_and_show_banner()?;
            match crate::anilist::search(title).await {
                Ok(Some(info)) => {
                    if let Some(cover) = &info.cover_url {
                        crate::util::show_image_thumb(cover.clone()).await;
                    }
                    println!("{}", crate::anilist::format_info(&info));
                }
                Ok(None) => println!("Tidak ditemukan di AniList."),
                Err(error) => eprintln!("{} Gagal memuat info AniList: {:#}", "■".red(), error),
            }
            println!("\nTekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            continue;
        }

        break;
    }

    Ok(())
}

pub(crate) async fn handle_explore(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let (season, year) = crate::anilist::current_season();
        let labels = [
            "Trending Anime".to_string(),
            format!("Anime Musim Ini ({} {})", season, year),
            "Jelajahi Genre".to_string(),
            "Cari Info Anime".to_string(),
            "Kembali".to_string(),
        ];
        let entries = labels
            .iter()
            .cloned()
            .map(input::MenuEntry::new)
            .collect::<Vec<_>>();
        let Some(choice) = input::select_menu_simple("Jelajahi Anime (AniList):", &entries, 0)?
        else {
            break;
        };

        match choice {
            0 => {
                show_discovery_list(
                    discord,
                    "Trending Anime (AniList):",
                    "Mengambil trending anime dari AniList...",
                    crate::anilist::trending(20),
                )
                .await?;
            }
            1 => {
                show_discovery_list(
                    discord,
                    &format!("Anime {} {}:", season, year),
                    &format!("Mengambil anime musim {} {}...", season, year),
                    crate::anilist::seasonal(season, year, 20),
                )
                .await?;
            }
            2 => {
                let mut genre_entries = crate::anilist::GENRES
                    .iter()
                    .map(|genre| input::MenuEntry::new(*genre))
                    .collect::<Vec<_>>();
                genre_entries.push(input::MenuEntry::new("Kembali"));
                let Some(genre_index) =
                    input::select_menu_simple("Pilih Genre:", &genre_entries, 0)?
                else {
                    continue;
                };
                let Some(genre) = crate::anilist::GENRES.get(genre_index) else {
                    continue;
                };
                show_discovery_list(
                    discord,
                    &format!("Anime Genre {}:", genre),
                    &format!("Mengambil anime genre {}...", genre),
                    crate::anilist::by_genre(genre, 20),
                )
                .await?;
            }
            3 => {
                show_discovery_list(
                    discord,
                    "Cari Info Anime:",
                    "Menyiapkan pencarian AniList...",
                    async {
                        let title: String =
                            dialoguer::Input::with_theme(&crate::util::custom_theme())
                                .with_prompt("Judul anime")
                                .interact_text()?;
                        let title = title.trim();
                        if title.is_empty() {
                            return Ok(Vec::new());
                        }
                        Ok(crate::anilist::search(title)
                            .await?
                            .into_iter()
                            .collect())
                    },
                )
                .await?;
            }
            _ => break,
        }
    }

    Ok(())
}

pub(crate) async fn run_cli_search(
    discord: &mut Option<DiscordIpcClient>,
    title: String,
) -> anyhow::Result<()> {
    let title = title.trim().to_string();
    if title.is_empty() {
        anyhow::bail!("judul pencarian kosong");
    }

    let mut tipe = provider_override()
        .or_else(|| {
            match config::Config::load().provider {
                config::ProviderPreference::Idlix => Some(PROVIDER_IDLIX),
                config::ProviderPreference::Otakudesu => Some(PROVIDER_OTAKUDESU),
                config::ProviderPreference::Auto => None,
            }
        })
        .unwrap_or(PROVIDER_IDLIX);
    input::save_search_provider(&title, tipe);
    save_history(&title);

    let mut animeku = AnimekuCli::for_provider(tipe);

    loop {
        clearscreen_and_show_banner()?;
        let movie = animeku.search(&title).await?;
        let active_provider = animeku.provider_type();
        if active_provider != tipe {
            tipe = active_provider;
            input::save_search_provider(&title, tipe);
        }
        let Some(movie) = movie else {
            break;
        };
        handle_movie_episodes(&mut animeku, &movie, discord).await?;
        if animeku.provider_type() != tipe {
            tipe = animeku.provider_type();
            input::save_search_provider(&title, tipe);
        }
    }

    Ok(())
}

pub(crate) async fn handle_search_and_watch(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let input = match get_user_input().await? {
            Some(user_input) => user_input,
            None => break, // Back to watch submenu
        };

        let mut animeku = AnimekuCli::for_provider(input.tipe);

        loop {
            clearscreen_and_show_banner()?;
            let movie = animeku.search(&input.title).await?;
            if animeku.provider_type() != input.tipe {
                input::save_search_provider(&input.title, animeku.provider_type());
            }
            let Some(movie) = movie else {
                break; // Back to input prompt
            };

            handle_movie_episodes(&mut animeku, &movie, discord).await?;
            if animeku.provider_type() != input.tipe {
                input::save_search_provider(&input.title, animeku.provider_type());
            }
        }
    }
    Ok(())
}
