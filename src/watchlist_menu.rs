use crate::input::load_watch_history;
use crate::util::clearscreen_and_show_banner;
use crate::animeku::AnimekuCli;
use crate::player::{handle_movie_episodes, resume_watch_entry};
use crate::menu::confirm_action;
use crate::{ download, ext, input };
use colored::Colorize;
use discord_rich_presence::DiscordIpcClient;
use std::io::{ stdout, Write };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchlistAction {
    OpenEpisodes,
    Resume,
    DownloadNewEpisodes,
    InfoAniList,
    Remove,
    Back,
}

/// Providers may return episodes newest-first or oldest-first. Compare the
/// first and last episode numbers to detect the direction, then return only
/// episodes newer than the last watched one in chronological order.
pub(crate) fn episodes_after_last(
    episodes: &[crate::models::Episode],
    last: Option<&crate::models::WatchEntry>,
) -> Vec<crate::models::Episode> {
    let Some(last) = last else {
        return episodes.to_vec();
    };
    let Some(index) = episodes
        .iter()
        .position(|episode| episode.id == last.episode.id)
    else {
        return episodes.to_vec();
    };

    let episode_key = |title: &str| {
        crate::episode::parse_season_episode(title)
            .or_else(|| crate::episode::episode_number(title).map(|number| (1, number)))
    };
    let first_key = episodes
        .first()
        .and_then(|episode| episode_key(&episode.title));
    let last_key = episodes
        .last()
        .and_then(|episode| episode_key(&episode.title));
    let newest_first = matches!(
        (first_key, last_key),
        (Some(first), Some(last)) if first > last
    );

    if newest_first {
        let mut new_episodes = episodes[..index].to_vec();
        new_episodes.reverse();
        new_episodes
    } else {
        episodes[index + 1..].to_vec()
    }
}

pub(crate) async fn handle_watchlist_download_new(
    entry: &crate::models::WatchlistEntry,
    last: Option<&crate::models::WatchEntry>,
) -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    let mut animeku = AnimekuCli::for_provider(entry.provider_type);
    if animeku.ensure_meta(&entry.movie).await.is_none() {
        anyhow::bail!("gagal memuat daftar episode dari provider");
    }

    let movie = animeku
        .current_movie()
        .cloned()
        .unwrap_or_else(|| entry.movie.clone());
    let episodes = animeku.get_episode_list(&movie.id).unwrap_or_default();
    if episodes.is_empty() {
        anyhow::bail!("daftar episode kosong");
    }

    let new_episodes = episodes_after_last(&episodes, last);

    if new_episodes.is_empty() {
        println!(
            "{} Tidak ada episode baru untuk '{}'.",
            "✓".green(),
            crate::models::clean_movie_title(&movie.title)
        );
        println!("\nTekan Enter untuk kembali...");
        let mut dummy = String::new();
        let _ = std::io::stdin().read_line(&mut dummy);
        return Ok(());
    }
    println!(
        "{} {} episode baru ditemukan ({} s/d {}).",
        "◆".blue(),
        new_episodes.len(),
        crate::models::clean_episode_title(&new_episodes[0].title),
        crate::models::clean_episode_title(&new_episodes[new_episodes.len() - 1].title)
    );

    let streams = animeku
        .resolve_streams_for_episode(&new_episodes[0])
        .await?;
    let Some(stream) = input::choice("Pilih Kualitas untuk Batch:", streams, 0)? else {
        return Ok(());
    };
    let Some(directory) = download::prompt_batch_directory()? else {
        return Ok(());
    };
    let Some(existing) = download::prompt_existing_policy()? else {
        return Ok(());
    };

    println!();
    println!("{} Ringkasan download episode baru:", "◆".blue());
    println!("  Jumlah  : {} episode", new_episodes.len());
    println!("  Kualitas: {}", stream.title.cyan());
    println!("  Folder  : {}", directory.display());
    if !confirm_action("Mulai download episode baru?")? {
        return Ok(());
    }

    let provider_type = animeku.provider_type();
    let episode_count = episodes.len();
    let mut queue = crate::batch::BatchQueue::new(
        provider_type,
        movie,
        new_episodes,
        stream.title,
        directory,
        existing,
    );
    queue.save()?;
    animeku.run_batch(&mut queue).await?;
    if queue.is_done() {
        input::update_watchlist_episode_count(entry.provider_type, &entry.movie.id, episode_count);
    }
    Ok(())
}

pub(crate) async fn handle_check_new_episodes(
    watchlist: &[crate::models::WatchlistEntry],
) -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    println!(
        "Memeriksa episode baru untuk {} judul watchlist...\n",
        watchlist.len()
    );

    let mut found = 0usize;
    for entry in watchlist {
        let title = crate::models::clean_movie_title(&entry.movie.title);
        print!("  {} ... ", title);
        stdout().flush().unwrap_or(());

        let mut animeku = AnimekuCli::for_provider(entry.provider_type);
        if animeku.ensure_meta(&entry.movie).await.is_none() {
            println!("gagal memuat daftar episode");
            continue;
        }

        let movie = animeku
            .current_movie()
            .cloned()
            .unwrap_or_else(|| entry.movie.clone());
        let episodes = animeku.get_episode_list(&movie.id).unwrap_or_default();
        if episodes.is_empty() {
            println!("daftar episode kosong");
            continue;
        }

        let count = episodes.len();
        match entry.known_episode_count {
            Some(previous) if count > previous => {
                println!(
                    "{} episode baru!",
                    (count - previous).to_string().green()
                );
                found += 1;
            }
            Some(previous) if count < previous => {
                println!("daftar berubah ({} -> {} episode)", previous, count);
            }
            Some(_) => println!("tidak ada episode baru"),
            None => println!("{} episode (baseline disimpan)", count),
        }
        input::update_watchlist_episode_count(entry.provider_type, &entry.movie.id, count);
    }

    println!("\nSelesai. {} judul punya episode baru.", found);
    println!("\nTekan Enter untuk kembali...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
    Ok(())
}

pub(crate) async fn handle_watchlist(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let watchlist = input::load_watchlist();
        if watchlist.is_empty() {
            println!("Watchlist masih kosong.\n");
            println!(
                "Tambahkan judul lewat menu [ ☆ Tambah ke Watchlist ] saat memilih episode/server.\n"
            );
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        }

        let history = load_watch_history();
        let mut entries = Vec::with_capacity(watchlist.len() + 3);
        for entry in &watchlist {
            let title = crate::models::clean_movie_title(&entry.movie.title);
            let last = history.iter().find(|item| {
                item.provider_type == entry.provider_type && item.movie.id == entry.movie.id
            });
            let status = last
                .map(|item| {
                    format!(
                        "{} • {}",
                        crate::models::clean_episode_title(&item.episode.title),
                        item.status_label()
                    )
                })
                .unwrap_or_else(|| "belum ditonton".to_string());
            entries.push(input::MenuEntry::new(format!(
                "[ {} • {} • {} ]",
                title,
                status,
                ext::provider_name(entry.provider_type)
            )));
        }

        let separator_index = entries.len();
        entries.push(input::MenuEntry::separator(
            "─── Aksi ─────────────────────────────────────",
        ));
        entries.push(input::MenuEntry::new("Cek Episode Baru"));
        entries.push(input::MenuEntry::new("Hapus Semua Watchlist"));
        entries.push(input::MenuEntry::new("Kembali"));

        let Some(choice) = input::select_menu_q_back("Watchlist:", &entries, 0)? else {
            break;
        };
        if choice == separator_index + 1 {
            handle_check_new_episodes(&watchlist).await?;
            continue;
        }
        if choice == separator_index + 2 {
            if confirm_action("Hapus semua watchlist?")? {
                input::clear_watchlist();
            }
            continue;
        }
        if choice == separator_index + 3 || choice >= watchlist.len() {
            break;
        }

        let entry = watchlist[choice].clone();
        let last = history
            .iter()
            .find(|item| {
                item.provider_type == entry.provider_type && item.movie.id == entry.movie.id
            })
            .cloned();

        clearscreen_and_show_banner()?;
        println!("Watchlist:");
        println!(
            "  Judul    : {}",
            crate::models::clean_movie_title(&entry.movie.title)
        );
        println!("  Provider : {}", ext::provider_name(entry.provider_type));
        println!("  Ditambah : {}", entry.format_added());
        if let Some(last) = &last {
            println!(
                "  Terakhir : {} • {} • {}",
                crate::models::clean_episode_title(&last.episode.title),
                last.status_label(),
                last.format_updated()
            );
        }
        println!();

        let mut labels = vec!["Pilih Episode / Putar".to_string()];
        let mut actions = vec![WatchlistAction::OpenEpisodes];
        if let Some(last) = &last {
            labels.push(if last.completed {
                "Putar Ulang Episode Terakhir".to_string()
            } else {
                "Lanjutkan Episode Terakhir".to_string()
            });
            actions.push(WatchlistAction::Resume);
        }
        labels.push("Download Episode Baru".to_string());
        actions.push(WatchlistAction::DownloadNewEpisodes);
        labels.push("Info AniList".to_string());
        actions.push(WatchlistAction::InfoAniList);
        labels.push("Hapus dari Watchlist".to_string());
        actions.push(WatchlistAction::Remove);
        labels.push("Kembali".to_string());
        actions.push(WatchlistAction::Back);

        let menu_entries = labels
            .iter()
            .cloned()
            .map(input::MenuEntry::new)
            .collect::<Vec<_>>();
        let Some(action_index) =
            input::select_menu_simple("Pilih Tindakan:", &menu_entries, 0)?
        else {
            continue;
        };

        match actions
            .get(action_index)
            .copied()
            .unwrap_or(WatchlistAction::Back)
        {
            WatchlistAction::OpenEpisodes => {
                let mut animeku = AnimekuCli::for_provider(entry.provider_type);
                handle_movie_episodes(&mut animeku, &entry.movie, discord).await?;
                let movie = animeku
                    .current_movie()
                    .cloned()
                    .unwrap_or_else(|| entry.movie.clone());
                if let Some(episodes) = animeku.get_episode_list(&movie.id) {
                    input::update_watchlist_episode_count(
                        entry.provider_type,
                        &entry.movie.id,
                        episodes.len(),
                    );
                }
            }
            WatchlistAction::Resume => {
                if let Some(last) = last {
                    let start_seconds = if last.completed {
                        0
                    } else {
                        last.position_seconds
                    };
                    resume_watch_entry(discord, &last, start_seconds).await?;
                }
            }
            WatchlistAction::DownloadNewEpisodes => {
                if let Err(error) = handle_watchlist_download_new(&entry, last.as_ref()).await {
                    eprintln!("{} {:#}", "■".red(), error);
                    println!();
                    println!("Tekan Enter untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                }
            }
            WatchlistAction::InfoAniList => {
                clearscreen_and_show_banner()?;
                let title = crate::models::clean_movie_title(&entry.movie.title);
                match crate::anilist::search(&title).await {
                    Ok(Some(info)) => {
                        if let Some(cover) = &info.cover_url {
                            crate::util::show_image_thumb(cover.clone()).await;
                        }
                        println!("{}", crate::anilist::format_info(&info));
                    }
                    Ok(None) => println!("Tidak ditemukan di AniList."),
                    Err(error) => {
                        eprintln!("{} Gagal memuat info AniList: {:#}", "■".red(), error)
                    }
                }
                println!("\nTekan Enter untuk kembali...");
                let mut dummy = String::new();
                let _ = std::io::stdin().read_line(&mut dummy);
            }
            WatchlistAction::Remove => {
                let title = crate::models::clean_movie_title(&entry.movie.title);
                if confirm_action(&format!("Hapus '{}' dari watchlist?", title))? {
                    input::delete_watchlist_entry(
                        entry.provider_type,
                        &entry.movie.id,
                    );
                }
            }
            WatchlistAction::Back => {}
        }
    }
    Ok(())
}
