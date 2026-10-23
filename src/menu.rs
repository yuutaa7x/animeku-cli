use crate::input::load_watch_history;
use crate::util::clearscreen_and_show_banner;
use crate::health::check_command_exists;
use crate::discord::reset_discord_status;
use crate::update_menu::handle_self_update;
use crate::DISCORD_APP_ID;
use crate::{ config, download, input };
use colored::Colorize;
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainAction {
    Continue,
    Batch,
    Search,
    Explore,
    Watchlist,
    History,
    Downloaded,
    Settings,
    Health,
    Update,
    Exit,
}

pub(crate) fn show_main_menu() -> anyhow::Result<Option<MainAction>> {
    let watch_history = load_watch_history();

    let mut entries = Vec::new();
    let mut actions = Vec::new();

    if let Some(entry) = watch_history.first() {
        let status = if entry.completed {
            "lanjut episode berikutnya".to_string()
        } else {
            entry.format_duration()
        };
        entries.push(input::MenuEntry::new(format!(
            "[ Lanjutkan: {} - {} | {} ]",
            crate::models::clean_movie_title(&entry.movie.title),
            crate::models::clean_episode_title(&entry.episode.title),
            status
        )));
        actions.push(MainAction::Continue);
    }

    if let Some(queue) = crate::batch::BatchQueue::load() {
        if queue.is_done() {
            crate::batch::BatchQueue::clear();
        } else {
            let (completed, skipped, failed, pending) = queue.counts();
            entries.push(input::MenuEntry::new(format!(
                "[ Kelola Download: {} | {}/{} selesai, {} gagal, {} tertunda ]",
                crate::models::clean_movie_title(&queue.movie.title),
                completed + skipped,
                queue.episodes.len(),
                failed,
                pending
            )));
            actions.push(MainAction::Batch);
        }
    }

    let watchlist_count = input::load_watchlist().len();
    let watchlist_label = if watchlist_count > 0 {
        format!("Watchlist ({})", watchlist_count)
    } else {
        "Watchlist".to_string()
    };
    let downloaded_count = crate::download::downloaded_files().len();
    let downloaded_label = if downloaded_count > 0 {
        format!("Terunduh ({})", downloaded_count)
    } else {
        "Terunduh".to_string()
    };
    for (label, action) in [
        ("Cari anime / film".to_string(), MainAction::Search),
        ("Jelajahi anime (AniList)".to_string(), MainAction::Explore),
        (watchlist_label, MainAction::Watchlist),
        ("Riwayat".to_string(), MainAction::History),
        (downloaded_label, MainAction::Downloaded),
        ("Pengaturan".to_string(), MainAction::Settings),
        ("Cek kesehatan".to_string(), MainAction::Health),
        ("Perbarui Animeku-CLI".to_string(), MainAction::Update),
        ("Keluar".to_string(), MainAction::Exit),
    ] {
        entries.push(input::MenuEntry::new(label));
        actions.push(action);
    }

    let Some(index) = input::select_menu_simple("Menu Utama:", &entries, 0)? else {
        return Ok(Some(MainAction::Exit));
    };

    Ok(actions.get(index).copied())
}

pub(crate) fn confirm_action(prompt: &str) -> anyhow::Result<bool> {
    // dialoguer clears only one terminal line, so keep the prompt unwrapped.
    let prompt = crate::util::fit_prompt(prompt);
    Ok(dialoguer::Confirm::with_theme(&crate::util::custom_theme())
        .with_prompt(prompt)
        .default(false)
        .interact_opt()?
        .unwrap_or(false))
}

pub(crate) fn skip_onboarding(config: &mut config::Config) -> anyhow::Result<()> {
    config.onboarding_done = true;
    config.save();
    Ok(())
}

pub(crate) fn run_onboarding(config: &mut config::Config) -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    println!("{}", "Selamat datang di Animeku-CLI!".bright_green().bold());
    println!();
    println!("Sebelum mulai, atur default yang paling nyaman untukmu.");
    println!("Semua pilihan ini bisa diubah lagi lewat menu Pengaturan.\n");

    let has_mpv = check_command_exists("mpv");
    let player_labels = [
        "Tanya setiap kali",
        "MPV (disarankan)",
        "VLC",
        "Browser (fallback)",
    ];
    let player_entries = player_labels
        .iter()
        .map(|label| input::MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let player_default = if has_mpv { 1 } else { 3 };
    let Some(player_index) =
        input::select_menu_simple("Player default:", &player_entries, player_default)?
    else {
        return skip_onboarding(config);
    };
    config.player = config::PlayerChoice::from_index(player_index);
    config.last_player = (player_index > 0).then_some(config.player);

    let provider_labels = ["Otomatis (disarankan)", "Idlix", "Otakudesu"];
    let provider_entries = provider_labels
        .iter()
        .map(|label| input::MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let Some(provider_index) =
        input::select_menu_simple("Provider default:", &provider_entries, 0)?
    else {
        return skip_onboarding(config);
    };
    config.provider = config::ProviderPreference::from_index(provider_index);

    let auto_next_labels = ["Tanya", "Selalu lanjut (disarankan)", "Jangan pernah"];
    let auto_next_entries = auto_next_labels
        .iter()
        .map(|label| input::MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let Some(auto_next_index) =
        input::select_menu_simple("Setelah episode selesai:", &auto_next_entries, 1)?
    else {
        return skip_onboarding(config);
    };
    config.auto_next = config::AutoNext::from_index(auto_next_index);

    config.onboarding_done = true;
    config.save();

    clearscreen_and_show_banner()?;
    println!("{} Pengaturan awal disimpan.", "✓".green());
    println!("  Player    : {}", config.player.label());
    println!("  Provider  : {}", config.provider.label());
    println!("  Auto-next : {}", config.auto_next.label());
    println!();
    if !has_mpv && config.effective_player() != config::PlayerChoice::Browser {
        println!(
            "  {} MPV belum terdeteksi. Pemeriksaan dependensi akan dibuka sekali sekarang.",
            "ℹ".yellow()
        );
    }
    println!("\nTekan Enter untuk melanjutkan...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
    Ok(())
}

pub(crate) fn choose_player_setting(config: &mut config::Config) -> anyhow::Result<()> {
    let labels = [
        "Tanya setiap kali",
        "MPV",
        "VLC",
        "Browser",
        "Kembali",
    ];
    let entries = labels
        .iter()
        .map(|label| input::MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let default = match config.player {
        config::PlayerChoice::Ask => 0,
        config::PlayerChoice::Mpv => 1,
        config::PlayerChoice::Vlc => 2,
        config::PlayerChoice::Browser => 3,
    };
    let Some(index) = input::select_menu_simple("Player default:", &entries, default)? else {
        return Ok(());
    };
    if index < 4 {
        config.player = config::PlayerChoice::from_index(index);
        if index > 0 {
            config.last_player = Some(config.player);
        }
        config.save();
    }
    Ok(())
}

pub(crate) fn choose_auto_next_setting(config: &mut config::Config) -> anyhow::Result<()> {
    let labels = ["Tanya", "Selalu lanjut", "Jangan pernah", "Kembali"];
    let entries = labels
        .iter()
        .map(|label| input::MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let default = match config.auto_next {
        config::AutoNext::Ask => 0,
        config::AutoNext::Always => 1,
        config::AutoNext::Never => 2,
    };
    let Some(index) = input::select_menu_simple("Auto-next episode:", &entries, default)? else {
        return Ok(());
    };
    if index < 3 {
        config.auto_next = config::AutoNext::from_index(index);
        config.save();
    }
    Ok(())
}

pub(crate) fn choose_download_dir_setting(config: &mut config::Config) -> anyhow::Result<()> {
    let current = download::default_download_dir();
    let Ok(value) = dialoguer::Input::<String>::with_theme(&crate::util::custom_theme())
        .with_prompt("Folder download (kosongkan untuk default)")
        .with_initial_text(current.display().to_string())
        .allow_empty(true)
        .interact_text()
    else {
        return Ok(());
    };

    let value = value.trim();
    if value.is_empty() {
        config.download_dir = None;
    } else {
        config.download_dir = Some(value.to_string());
    }
    config.save();
    Ok(())
}

pub(crate) fn choose_provider_setting(config: &mut config::Config) -> anyhow::Result<()> {
    let labels = ["Otomatis", "Idlix", "Otakudesu", "Kembali"];
    let entries = labels
        .iter()
        .map(|label| input::MenuEntry::new(*label))
        .collect::<Vec<_>>();
    let default = match config.provider {
        config::ProviderPreference::Auto => 0,
        config::ProviderPreference::Idlix => 1,
        config::ProviderPreference::Otakudesu => 2,
    };
    let Some(index) = input::select_menu_simple("Provider default:", &entries, default)? else {
        return Ok(());
    };
    if index < 3 {
        config.provider = config::ProviderPreference::from_index(index);
        config.save();
    }
    Ok(())
}

pub(crate) async fn handle_settings_menu(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let mut config = config::Config::load();

        let labels = [
            format!("Player default: {}", config.player.label()),
            format!("Auto-next: {}", config.auto_next.label()),
            format!("Provider: {}", config.provider.label()),
            format!(
                "Discord RPC: {}",
                if config.rpc { "Aktif" } else { "Mati" }
            ),
            format!(
                "Banner compact: {}",
                if config.compact_banner { "Aktif" } else { "Mati" }
            ),
            format!(
                "Poster episode: {}",
                if config.show_posters { "Aktif" } else { "Mati" }
            ),
            format!(
                "Embed subtitle: {}",
                if config.embed_subtitles {
                    "Aktif"
                } else {
                    "Mati (sidecar .vtt)"
                }
            ),
            format!(
                "Subtitle saat memutar: {}",
                if config.subtitles { "Aktif" } else { "Mati" }
            ),
            format!(
                "Folder download: {}",
                config
                    .download_dir
                    .as_deref()
                    .unwrap_or("(default)")
            ),
            "Hapus riwayat pencarian".to_string(),
            "Hapus riwayat tontonan".to_string(),
            "Hapus watchlist".to_string(),
            "Hapus cache metadata & provider".to_string(),
            "Panduan awal / onboarding".to_string(),
            "Perbarui Animeku-CLI".to_string(),
            "Kembali".to_string(),
        ];
        let entries = labels
            .iter()
            .cloned()
            .map(input::MenuEntry::new)
            .collect::<Vec<_>>();

        let Some(choice) = input::select_menu_simple("Pengaturan:", &entries, 0)? else {
            break;
        };

        match choice {
            0 => choose_player_setting(&mut config)?,
            1 => choose_auto_next_setting(&mut config)?,
            2 => choose_provider_setting(&mut config)?,
            3 => {
                config.rpc = !config.rpc;
                config.save();
                if config.rpc {
                    let mut client = DiscordIpcClient::new(DISCORD_APP_ID);
                    if client.connect().is_ok() {
                        *discord = Some(client);
                        reset_discord_status(discord);
                    } else {
                        *discord = None;
                    }
                } else {
                    *discord = None;
                }
            }
            4 => {
                config.compact_banner = !config.compact_banner;
                config.save();
            }
            5 => {
                config.show_posters = !config.show_posters;
                config.save();
            }
            6 => {
                config.embed_subtitles = !config.embed_subtitles;
                config.save();
            }
            7 => {
                config.subtitles = !config.subtitles;
                config.save();
            }
            8 => choose_download_dir_setting(&mut config)?,
            9 => {
                if confirm_action("Hapus semua riwayat pencarian?")? {
                    input::clear_search_history();
                }
            }
            10 => {
                if confirm_action("Hapus semua riwayat tontonan?")? {
                    input::clear_watch_history();
                }
            }
            11 => {
                if confirm_action("Hapus semua watchlist?")? {
                    input::clear_watchlist();
                }
            }
            12 => {
                if confirm_action("Hapus cache metadata, deteksi, dan pilihan provider?")? {
                    crate::cache::clear();
                    input::clear_provider_detections();
                    input::clear_search_providers();
                }
            }
            13 => {
                if confirm_action("Jalankan panduan awal lagi?")? {
                    run_onboarding(&mut config)?;
                }
            }
            14 => handle_self_update(true).await?,
            _ => break,
        }
    }
    Ok(())
}
