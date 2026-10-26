use crate::input::delete_search_history_entry;
use crate::input::delete_watch_entry;
use crate::input::load_history;
use crate::input::load_watch_history;
use crate::input::pick_provider;
use crate::input::save_history;
use crate::util::clearscreen_and_show_banner;
use crate::episode::detect_season;
use crate::episode::detect_season_from_titles;
use crate::episode::find_next_episode;
use crate::animeku::AnimekuCli;
use crate::ext::otakudesu::OTAKUDESU_REFERER;
use crate::ext::otakudesu::OTAKUDESU_USER_AGENT;
use crate::cli::{player_override, provider_override};
use crate::health::resolve_command_path;
use crate::discord::{reset_discord_status, update_discord_status};
use crate::menu::confirm_action;
use crate::{PROVIDER_IDLIX, PROVIDER_OTAKUDESU};
use crate::{ config, ext, input };
use colored::Colorize;
use std::process::Command;
use discord_rich_presence::DiscordIpcClient;
use std::io::{ stdout, Write };

pub(crate) fn idlix_audio_path() -> String {
    crate::util::temp_file("animeku_idlix_audio.m3u8")
}

pub(crate) fn idlix_subtitle_path() -> String {
    crate::util::get_subtitle_path()
}

pub(crate) fn mpv_track_script_path() -> String {
    crate::util::temp_file("animeku_track.lua")
}

pub(crate) fn mpv_last_pos_file_path() -> String {
    crate::util::temp_file("animeku_last_pos.txt")
}

pub(crate) fn mpv_next_file_path() -> String {
    crate::util::temp_file("animeku_next_requested.txt")
}

pub(crate) enum PlaybackOutcome {
    Played {
        position: u64,
        offer_next: bool,
        completed: bool,
        /// MPV's `n` key was pressed. The caller can skip the usual prompt.
        next_requested: bool,
    },
    Cancelled,
}

pub(crate) async fn execute_player(
    stream_url: &str,
    provider_type: usize,
    start_seconds: u64,
) -> anyhow::Result<PlaybackOutcome> {
    println!("{} Menyiapkan pemutar video .. ", "◆".blue());
    stdout().flush()?;

    let config = config::Config::load();
    let is_android = std::env::var("TERMUX_VERSION").is_ok();
    let override_player = player_override();
    let configured_player = override_player.unwrap_or_else(|| config.effective_player());
    let use_picker = override_player
        .map(|player| player == config::PlayerChoice::Ask)
        .unwrap_or(config.player == config::PlayerChoice::Ask);

    let selected_player = if use_picker {
        let labels = if is_android {
            vec!["Putar di VLC / Pemutar Android", "Buka di Browser", "Kembali"]
        } else {
            vec!["Putar dengan MPV", "Putar dengan VLC", "Buka di Browser", "Kembali"]
        };
        let entries = labels
            .iter()
            .map(|label| input::MenuEntry::new(*label))
            .collect::<Vec<_>>();
        let last = configured_player;
        let default = if is_android {
            match last {
                config::PlayerChoice::Browser => 1,
                _ => 0,
            }
        } else {
            match last {
                config::PlayerChoice::Mpv => 0,
                config::PlayerChoice::Vlc => 1,
                config::PlayerChoice::Browser => 2,
                config::PlayerChoice::Ask => 0,
            }
        };
        let Some(index) = input::select_menu_simple("Pilih Pemutar:", &entries, default)? else {
            return Ok(PlaybackOutcome::Cancelled);
        };
        let selected = if is_android {
            match index {
                0 => config::PlayerChoice::Vlc,
                1 => config::PlayerChoice::Browser,
                _ => return Ok(PlaybackOutcome::Cancelled),
            }
        } else {
            match index {
                0 => config::PlayerChoice::Mpv,
                1 => config::PlayerChoice::Vlc,
                2 => config::PlayerChoice::Browser,
                _ => return Ok(PlaybackOutcome::Cancelled),
            }
        };
        let mut updated = config.clone();
        updated.last_player = Some(selected);
        updated.save();
        selected
    } else if is_android {
        match configured_player {
            config::PlayerChoice::Browser => config::PlayerChoice::Browser,
            _ => config::PlayerChoice::Vlc,
        }
    } else {
        configured_player
    };

    println!("Memutar dengan {}...", selected_player.label());
    let choice_normalized: usize = match selected_player {
        config::PlayerChoice::Mpv => 0,
        config::PlayerChoice::Vlc => 1,
        config::PlayerChoice::Browser => 2,
        config::PlayerChoice::Ask => 0,
    };

    let mut final_pos = start_seconds;
    let mut played = false;
    let mut offer_next = true;
    let mut completed = false;
    let mut next_requested = false;

    match choice_normalized {
        0 => {
            println!("Sedang memutar video di MPV... (Tutup MPV untuk kembali ke menu)");
            println!(
                "  {} Tekan n di MPV untuk langsung lanjut ke episode berikutnya.",
                "•".dimmed()
            );

            let track_script = mpv_track_script_path();
            let last_pos_file = mpv_last_pos_file_path();
            let next_file = mpv_next_file_path();

            let lua_script = format!(
                r#"
local last_pos = {}
mp.observe_property("time-pos", "number", function(name, val)
    if val then last_pos = val end
end)
local function save()
    local f = io.open("{}", "w")
    if f then
        f:write(tostring(math.floor(last_pos)))
        f:close()
    end
end
mp.add_periodic_timer(2, save)
mp.register_event("shutdown", save)
mp.add_key_binding("n", "animeku-next", function()
    save()
    local f = io.open("{}", "w")
    if f then
        f:write("1")
        f:close()
    end
    mp.commandv("quit")
end)
"#,
                start_seconds, last_pos_file, next_file
            );
            let _ = std::fs::write(&track_script, lua_script);
            let _ = std::fs::remove_file(&last_pos_file);
            let _ = std::fs::remove_file(&next_file);

            let mpv_bin = resolve_command_path("mpv");
            let mut cmd = Command::new(&mpv_bin);

            if std::env::var("TERMUX_VERSION").is_ok() {
                // Cek apakah termux-x11 sudah berjalan
                if let Ok(out) = Command::new("pgrep").arg("termux-x11").output() {
                    if out.stdout.is_empty() {
                        println!("  {} Menjalankan Termux-X11 server otomatis...", "ℹ".yellow());
                        let _ = Command::new("termux-x11").arg(":0").spawn();
                        std::thread::sleep(std::time::Duration::from_millis(2000));
                    }
                }

                cmd.env("DISPLAY", ":0");
                cmd.arg("--vo=x11"); // Pastikan pakai x11 untuk mencegah error gpu-context
            }

            cmd.arg("--hwdec=auto-safe");
            cmd.arg("--cache=yes");
            cmd.arg(format!("--script={}", track_script));

            if start_seconds > 0 {
                cmd.arg(format!("--start={}", start_seconds));
            }

            if provider_type == PROVIDER_IDLIX {
                cmd.arg("--demuxer=lavf");
                cmd.arg("--demuxer-lavf-o-append=protocol_whitelist=file,http,https,tcp,tls,crypto,data");
                cmd.arg("--demuxer-lavf-o-append=http_persistent=0");
                cmd.arg("--demuxer-lavf-o-append=reconnect=1");
                cmd.arg("--demuxer-lavf-o-append=reconnect_streamed=1");
                cmd.arg("--demuxer-lavf-o-append=reconnect_on_network_error=1");
                cmd.arg("--demuxer-lavf-o-append=reconnect_delay_max=5");
                cmd.arg("--demuxer-max-bytes=150M");
                cmd.arg("--demuxer-readahead-secs=30");
                cmd.arg("--ytdl=no");
                cmd.arg("--msg-level=ffmpeg/demuxer=error");
                let audio_file = idlix_audio_path();
                if std::path::Path::new(&audio_file).exists() {
                    cmd.arg(format!("--audio-file={}", audio_file));
                }
                let sub_file = idlix_subtitle_path();
                if config.subtitles && std::path::Path::new(&sub_file).exists() {
                    cmd.arg(format!("--sub-file={}", sub_file));
                } else if !config.subtitles {
                    cmd.arg("--sub-auto=no");
                }
            } else if provider_type == PROVIDER_OTAKUDESU {
                cmd.arg(format!("--http-header-fields=Referer: {}", OTAKUDESU_REFERER));
                cmd.arg(format!("--user-agent={}", OTAKUDESU_USER_AGENT));
                // Otakudesu gives direct MP4/Google Video URLs. Let mpv handle
                // them natively instead of invoking yt-dlp's generic extractor,
                // which fails with 403 on these signed URLs.
                cmd.arg("--ytdl=no");
            }
            match cmd.arg(stream_url).status() {
                Ok(status) => {
                    played = true;
                    if !status.success() {
                        eprintln!(
                            "{} MPV keluar dengan status tidak normal. Tekan Enter untuk kembali...",
                            "■".red()
                        );
                        let mut dummy = String::new();
                        let _ = std::io::stdin().read_line(&mut dummy);
                    }
                }
                Err(_) => {
                    eprintln!("{} MPV tidak ditemukan!", "■".red());
                }
            }

            if let Ok(saved) = std::fs::read_to_string(&last_pos_file) {
                if let Ok(pos) = saved.trim().parse::<u64>() {
                    final_pos = pos;
                    // The tracking script wrote a position, so playback really
                    // started even if mpv later reported a non-zero exit code.
                    played = true;
                }
            }
            if std::fs::read_to_string(&next_file).is_ok() {
                next_requested = true;
                offer_next = true;
            }
            let _ = std::fs::remove_file(&last_pos_file);
            let _ = std::fs::remove_file(&next_file);
            let _ = std::fs::remove_file(&track_script);
        },
        1 => {
            if is_android {
                println!("Membuka aplikasi video player bawaan Android (VLC)...");
                
                // Simpan subtitle ke folder Download agar user bisa nge-load manual
                let sub_path = crate::util::get_subtitle_path();
                if std::path::Path::new(&sub_path).exists() && sub_path.contains("Download") {
                    println!("{} Subtitle disimpan di folder Download (animeku_idlix.vtt). Silakan load manual di VLC.", "ℹ".yellow());
                }

                // Kita nyalakan Local HTTP Proxy untuk memanipulasi Header!
                // Handle ini menjaga listener tetap hidup selama playback dan
                // otomatis menghentikannya saat keluar dari blok ini.
                let proxy = match crate::proxy::start_proxy(provider_type).await {
                    Ok(handle) => handle,
                    Err(e) => {
                        eprintln!("{} Gagal menjalankan proxy lokal: {}", "■".red(), e);
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        return Ok(PlaybackOutcome::Cancelled);
                    }
                };
                let proxy_port = proxy.port;
                let encoded_url = urlencoding::encode(stream_url);
                let proxy_url = format!("http://127.0.0.1:{}/?url={}", proxy_port, encoded_url);

                // Buka menggunakan termux-open dengan MIME video/* 
                // Karena kita pakai http://127.0.0.1, VLC tidak butuh Header lagi, Proxy yang urus!
                let status = Command::new("termux-open")
                    .args(["--content-type", "video/*", "--chooser", &proxy_url])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                
                if status.is_err() || !status.unwrap().success() {
                    eprintln!("{} Gagal memanggil termux-open. Pastikan VLC sudah di-install.", "■".red());
                    std::thread::sleep(std::time::Duration::from_secs(3));
                } else {
                    played = true;
                    println!("Proxy berjalan di port {}. Silakan tonton di aplikasi pilihanmu!", proxy_port);
                    println!("\n{} Jangan tutup layar ini selama menonton agar proxy tetap hidup.", "ℹ".yellow());
                    println!("Jika sudah selesai menonton, tekan Enter untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                    
                    print!("Simpan posisi terakhir tontonan (dalam menit)? (Kosongkan jika sudah tamat): ");
                    stdout().flush().unwrap_or(());
                    let mut pos_input = String::new();
                    let _ = std::io::stdin().read_line(&mut pos_input);
                    if let Ok(m) = pos_input.trim().parse::<u64>() {
                        final_pos = m * 60;
                    } else if pos_input.trim().is_empty() {
                        final_pos = 0;
                        completed = true;
                    }
                }
            } else {
                println!("Sedang memutar video di VLC... (Tutup VLC untuk kembali ke menu)");
                let vlc_bin = resolve_command_path("vlc");
                let mut cmd = Command::new(&vlc_bin);
                if start_seconds > 0 {
                    cmd.arg(format!("--start-time={}", start_seconds));
                }
                if provider_type == PROVIDER_OTAKUDESU {
                    cmd.arg(format!("--http-referrer={}", OTAKUDESU_REFERER));
                    cmd.arg(format!("--http-user-agent={}", OTAKUDESU_USER_AGENT));
                }
                match cmd.arg(stream_url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() {
                    Ok(status) => {
                        played = true;
                        if !status.success() {
                            eprintln!("{} VLC keluar dengan status tidak normal.", "■".red());
                        }
                    }
                    Err(_) => eprintln!("{} VLC tidak ditemukan!", "■".red()),
                }

                if played {
                    println!();
                    print!("Simpan posisi terakhir tontonan (dalam menit)? (Kosongkan jika sudah tamat): ");
                    stdout().flush().unwrap_or(());
                    let mut pos_input = String::new();
                    let _ = std::io::stdin().read_line(&mut pos_input);
                    if let Ok(m) = pos_input.trim().parse::<u64>() {
                        final_pos = m * 60;
                    } else if pos_input.trim().is_empty() {
                        final_pos = 0;
                        completed = true;
                    }
                }
            }
        },
        2 => {
            match open::that(stream_url) {
                Ok(()) => {
                    played = true;
                    // Browser playback is external and untracked, so don't
                    // push the user into the auto-next prompt afterwards.
                    offer_next = false;
                    println!("\n{} Tautan dibuka di browser.", "✓".green());
                    println!(
                        "  {} Browser adalah fallback: posisi tontonan dan auto-next tidak dilacak.",
                        "ℹ".yellow()
                    );
                    println!("Tekan Enter setelah selesai menonton untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                }
                Err(e) => {
                    eprintln!("{} Gagal membuka browser: {}", "■".red(), e);
                }
            }
        },
        _ => {},
    }

    if played {
        Ok(PlaybackOutcome::Played {
            position: final_pos,
            offer_next,
            completed,
            next_requested,
        })
    } else {
        Ok(PlaybackOutcome::Cancelled)
    }
}

/// Best-effort season detection for a concrete episode. Provider titles like
/// `S2E05` are authoritative; provider metadata is only a fallback. Missing
/// information defaults to season 1 instead of interrupting the user.
pub(crate) fn detect_season_for_episode(
    animeku: &AnimekuCli,
    movie: &crate::models::Movie,
    episode: &crate::models::Episode,
) -> u32 {
    if !episode.is_series {
        return 1;
    }

    if let Some(season) = detect_season_from_titles(&movie.title, &episode.title) {
        return season;
    }

    let meta_opt = animeku.get_meta(&movie.id);
    let thumb = meta_opt
        .as_ref()
        .and_then(|m| m.thumb_url.clone())
        .unwrap_or_default();
    let meta_data = meta_opt.map(|m| m.data).unwrap_or_default();
    detect_season(&movie.title, &thumb, &meta_data).unwrap_or(1)
}

pub(crate) enum EpisodePlayback {
    Played {
        offer_next: bool,
        next_requested: bool,
    },
    PlayerCancelled,
    StreamCancelled,
}

pub(crate) fn confirm_next_episode(next_ep: &crate::models::Episode) -> anyhow::Result<bool> {
    match config::Config::load().auto_next {
        config::AutoNext::Always => Ok(true),
        config::AutoNext::Never => Ok(false),
        config::AutoNext::Ask => {
            println!(
                "{} Berikutnya: {}",
                "▸".cyan(),
                next_ep.title.trim()
            );
            Ok(dialoguer::Confirm::with_theme(&crate::util::custom_theme())
                .with_prompt("Lanjut ke episode berikutnya? (Esc = tidak)")
                .default(true)
                // Sembunyikan hint "yes" bawaan dialoguer; Enter tetap berarti ya.
                .show_default(false)
                .interact_opt()?
                .unwrap_or(false))
        }
    }
}

/// Play one episode and persist its progress. Returning a distinct result for
/// "player cancelled" and "stream picker cancelled" lets the caller reopen the
/// right screen instead of treating a cancelled playback as finished.
pub(crate) async fn play_episode(
    animeku: &mut AnimekuCli,
    movie: &crate::models::Movie,
    episode: &crate::models::Episode,
    start_seconds: u64,
    discord: &mut Option<DiscordIpcClient>,
) -> anyhow::Result<EpisodePlayback> {
    clearscreen_and_show_banner()?;

    let stream_opt = animeku.extract_stream_urls(episode.clone()).await?;
    let Some(stream) = stream_opt else {
        reset_discord_status(discord);
        return Ok(EpisodePlayback::StreamCancelled);
    };

    // Provider fallback may replace both the movie and episode with the other
    // site's equivalents. Use the active values for RPC, playback, and history
    // so stale IDs are never persisted.
    let movie = animeku
        .current_movie()
        .cloned()
        .unwrap_or_else(|| movie.clone());
    let episode = animeku
        .current_episode()
        .cloned()
        .unwrap_or_else(|| episode.clone());
    let provider_type = animeku.provider_type();
    let season_num = detect_season_for_episode(animeku, &movie, &episode);
    let meta = animeku.get_meta(&movie.id);
    let thumb_url = meta.as_ref().and_then(|meta| meta.thumb_url.clone());
    let meta_data = meta.map(|meta| meta.data).unwrap_or_default();
    // Drives the Discord progress bar; without it the card still shows the
    // elapsed time, just no total. Skip the probe entirely when RPC is off.
    let duration_seconds = if discord.is_some() {
        crate::download::probe_duration(provider_type, &stream.url).map(|seconds| seconds as u64)
    } else {
        None
    };

    update_discord_status(
        discord,
        &crate::discord::PlaybackStatus {
            movie_id: &movie.id,
            movie_title: &movie.title,
            episode_title: &episode.title,
            episode_id: &episode.id,
            is_series: episode.is_series,
            season_num,
            thumb_url,
            meta: meta_data,
            provider_type,
            start_seconds,
            duration_seconds,
        },
    )
    .await;

    let outcome = execute_player(&stream.url, provider_type, start_seconds).await?;
    reset_discord_status(discord);

    match outcome {
        PlaybackOutcome::Cancelled => Ok(EpisodePlayback::PlayerCancelled),
        PlaybackOutcome::Played {
            position,
            offer_next,
            completed,
            next_requested,
        } => {
            input::save_watch_entry(&crate::models::WatchEntry {
                movie,
                episode,
                provider_type,
                position_seconds: position,
                completed,
                updated_at: 0,
            });
            Ok(EpisodePlayback::Played {
                offer_next,
                next_requested,
            })
        }
    }
}

pub(crate) fn sync_active_playback(
    animeku: &AnimekuCli,
    movie: &mut crate::models::Movie,
    episode: &mut crate::models::Episode,
) {
    if let Some(active_movie) = animeku.current_movie() {
        *movie = active_movie.clone();
    }
    if let Some(active_episode) = animeku.current_episode() {
        *episode = active_episode.clone();
    }
}

pub(crate) async fn handle_movie_episodes(
    animeku: &mut AnimekuCli,
    movie: &crate::models::Movie,
    discord: &mut Option<DiscordIpcClient>,
) -> anyhow::Result<()> {
    let mut active_movie = movie.clone();
    'movie_list: loop {
        clearscreen_and_show_banner()?;
        let Some(mut episode) = animeku.extract_episode(active_movie.clone()).await? else {
            break; // Back to movie search
        };
        if let Some(current_movie) = animeku.current_movie() {
            active_movie = current_movie.clone();
        }
        // Full movies skip the episode picker, so cancelling the stream picker
        // should return to search instead of looping back to the same movie.
        loop {
            match play_episode(animeku, &active_movie, &episode, 0, discord).await? {
                EpisodePlayback::PlayerCancelled => {
                    sync_active_playback(animeku, &mut active_movie, &mut episode);
                    continue;
                }
                EpisodePlayback::StreamCancelled => {
                    sync_active_playback(animeku, &mut active_movie, &mut episode);
                    if !episode.is_series {
                        break 'movie_list;
                    }
                    break;
                }
                EpisodePlayback::Played {
                    offer_next,
                    next_requested,
                } => {
                    sync_active_playback(animeku, &mut active_movie, &mut episode);
                    let single_movie = !episode.is_series;
                    if offer_next {
                        // Auto-next logic: follow the provider's ordered episode
                        // list so the last episode of a season advances to the
                        // next season.
                        let next_ep_opt = animeku
                            .get_episode_list(&active_movie.id)
                            .and_then(|episodes| find_next_episode(&episodes, &episode).cloned());

                        if let Some(next_ep) = next_ep_opt {
                            if next_requested || confirm_next_episode(&next_ep)? {
                                episode = next_ep;
                                continue;
                            }
                        }
                    }

                    // A finished full movie has no episode list to return to.
                    if single_movie {
                        break 'movie_list;
                    }

                    // No next episode / user declined / browser playback.
                    break;
                }
            }
        }
    }
    Ok(())
}

pub(crate) async fn resume_watch_entry(
    discord: &mut Option<DiscordIpcClient>,
    entry: &crate::models::WatchEntry,
    start_seconds: u64,
) -> anyhow::Result<()> {
    let mut animeku = AnimekuCli::for_provider(entry.provider_type);
    // Populate the episode cache so auto-next can follow the provider's
    // ordered list instead of stopping after the resumed episode.
    let _ = animeku.ensure_meta(&entry.movie).await;

    let mut movie = entry.movie.clone();
    let mut episode = entry.episode.clone();
    let mut start_seconds = start_seconds;

    loop {
        match play_episode(
            &mut animeku,
            &movie,
            &episode,
            start_seconds,
            discord,
        )
        .await?
        {
            EpisodePlayback::PlayerCancelled => {
                sync_active_playback(&animeku, &mut movie, &mut episode);
                continue;
            }
            EpisodePlayback::StreamCancelled => break,
            EpisodePlayback::Played {
                offer_next,
                next_requested,
            } => {
                sync_active_playback(&animeku, &mut movie, &mut episode);
                if offer_next {
                    let next_ep_opt = animeku
                        .get_episode_list(&movie.id)
                        .and_then(|episodes| find_next_episode(&episodes, &episode).cloned());

                    if let Some(next_ep) = next_ep_opt {
                        if next_requested || confirm_next_episode(&next_ep)? {
                            episode = next_ep;
                            start_seconds = 0;
                            continue;
                        }
                    }
                }
                break;
            }
        }
    }

    Ok(())
}

/// Build the next history entry for a completed episode. A completed item
/// should continue the story, not replay the same episode from the beginning.
pub(crate) fn next_watch_entry(
    entry: &crate::models::WatchEntry,
    episodes: &[crate::models::Episode],
) -> Option<crate::models::WatchEntry> {
    if !entry.completed {
        return None;
    }
    let next_episode = find_next_episode(episodes, &entry.episode)?.clone();
    Some(crate::models::WatchEntry {
        movie: entry.movie.clone(),
        episode: next_episode,
        provider_type: entry.provider_type,
        position_seconds: 0,
        completed: false,
        updated_at: 0,
    })
}

pub(crate) async fn handle_resume_latest(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    let Some(entry) = load_watch_history().into_iter().next() else {
        clearscreen_and_show_banner()?;
        println!("Belum ada riwayat tontonan.\n");
        println!("Tekan Enter untuk kembali...");
        let mut dummy = String::new();
        let _ = std::io::stdin().read_line(&mut dummy);
        return Ok(());
    };

    if entry.completed {
        let mut animeku = AnimekuCli::for_provider(entry.provider_type);
        let _ = animeku.ensure_meta(&entry.movie).await;
        let episodes = animeku
            .get_episode_list(&entry.movie.id)
            .unwrap_or_default();
        if let Some(mut next_entry) = next_watch_entry(&entry, &episodes) {
            if let Some(active_movie) = animeku.current_movie() {
                next_entry.movie = active_movie.clone();
            }
            return resume_watch_entry(discord, &next_entry, 0).await;
        }

        clearscreen_and_show_banner()?;
        println!(
            "{} '{}' sudah tamat.",
            "✓".green(),
            crate::models::clean_movie_title(&entry.movie.title)
        );
        println!();
        if confirm_action("Putar ulang episode terakhir dari awal?")? {
            return resume_watch_entry(discord, &entry, 0).await;
        }
        return Ok(());
    }

    resume_watch_entry(discord, &entry, entry.position_seconds).await
}

pub(crate) async fn handle_resume_last_watch(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;

        let watch_history = load_watch_history();
        let search_history = load_history();

        let mut menu_items: Vec<input::MenuEntry> = Vec::new();

        // 1. Add Watch History entries
        for lw in &watch_history {
            let provider_name = ext::provider_name(lw.provider_type);
            menu_items.push(input::MenuEntry::new(format!(
                "[ {} - {} | {} | {} | {} ]",
                crate::models::clean_movie_title(&lw.movie.title),
                crate::models::clean_episode_title(&lw.episode.title),
                lw.status_label(),
                lw.format_updated(),
                provider_name
            )));
        }

        if watch_history.is_empty() && search_history.is_empty() {
            println!("Belum ada riwayat tontonan/pencarian.\n");
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        }

        // 2. Separator + Search History
        let separator_index: Option<usize> = if !search_history.is_empty() {
            let idx = menu_items.len();
            menu_items.push(input::MenuEntry::separator(
                "─── Riwayat Pencarian ────────────────────────",
            ));
            Some(idx)
        } else {
            None
        };

        for title in &search_history {
            menu_items.push(input::MenuEntry::new(format!("  {}", title)));
        }

        menu_items.push(input::MenuEntry::separator(
            "─── Aksi ─────────────────────────────────────",
        ));
        menu_items.push(input::MenuEntry::new("Hapus Semua Riwayat"));
        menu_items.push(input::MenuEntry::new("Kembali"));

        let Some(choice) =
            input::select_menu_q_back("Riwayat Tontonan & Pencarian:", &menu_items, 0)?
        else {
            break;
        };

        let back_index = menu_items.len() - 1;
        let clear_index = menu_items.len() - 2;
        if choice == back_index {
            break;
        }
        if choice == clear_index {
            if confirm_action("Hapus semua riwayat tontonan dan pencarian?")? {
                input::clear_watch_history();
                input::clear_search_history();
            }
            continue;
        }

        let watch_count = watch_history.len();

        if choice < watch_count {
            // Picked from Watch History
            let selected_watch = watch_history[choice].clone();
            
            clearscreen_and_show_banner()?;
            let display_title = crate::models::clean_movie_title(&selected_watch.movie.title);
            let display_episode = crate::models::clean_episode_title(&selected_watch.episode.title);
            println!("Riwayat Tontonan:");
            println!("  Judul    : {}", display_title);
            println!("  Episode  : {}", display_episode);
            println!("  Status   : {}", selected_watch.status_label());
            println!("  Terakhir : {}", selected_watch.format_updated());
            println!(
                "  Provider : {}\n",
                ext::provider_name(selected_watch.provider_type)
            );

            let actions = [
                format!(
                    "Lanjutkan Menonton (mulai {})",
                    selected_watch.format_duration()
                ),
                "Putar Ulang dari Awal (00:00)".to_string(),
                "Pilih Episode Lain".to_string(),
                "Hapus dari Histori".to_string(),
                "Kembali".to_string(),
            ];
            let entries = actions
                .iter()
                .cloned()
                .map(input::MenuEntry::new)
                .collect::<Vec<_>>();

            let Some(action_choice) = input::select_menu_simple("Pilih Tindakan:", &entries, 0)? else {
                continue;
            };

            if action_choice == 3 {
                if confirm_action(&format!("Hapus {} dari histori?", display_title))? {
                    delete_watch_entry(choice);
                }
                continue;
            } else if action_choice == 4 {
                continue;
            } else if action_choice == 2 {
                let mut animeku =
                    AnimekuCli::for_provider(selected_watch.provider_type);
                handle_movie_episodes(&mut animeku, &selected_watch.movie, discord).await?;
                continue;
            }

            let start_seconds = if action_choice == 0 {
                selected_watch.position_seconds
            } else {
                0
            };

            resume_watch_entry(discord, &selected_watch, start_seconds).await?;
        } else {
            // Picked from Search History
            let separator_offset = if separator_index.is_some() { 1 } else { 0 };
            let history_index = choice - watch_count - separator_offset;
            let selected_title = search_history[history_index].clone();

            let actions = [
                "Cari Judul Ini".to_string(),
                "Ganti Provider & Cari".to_string(),
                "Hapus dari Histori".to_string(),
                "Kembali".to_string(),
            ];
            let entries = actions
                .iter()
                .cloned()
                .map(input::MenuEntry::new)
                .collect::<Vec<_>>();

            clearscreen_and_show_banner()?;
            println!("Judul Pencarian: {}\n", selected_title);

            let Some(action_choice) = input::select_menu_simple("Pilih Tindakan:", &entries, 0)? else {
                continue;
            };

            let mut tipe_override = None;
            if action_choice == 1 {
                let Some(provider) = input::choose_provider_manually().await? else {
                    continue;
                };
                input::save_search_provider(&selected_title, provider);
                tipe_override = Some(provider);
            } else if action_choice == 2 {
                if confirm_action(&format!("Hapus '{}' dari histori pencarian?", selected_title))? {
                    delete_search_history_entry(&selected_title);
                    input::delete_search_provider(&selected_title);
                }
                continue;
            } else if action_choice == 3 {
                continue;
            }

            let forced_provider = provider_override().or_else(|| {
                match config::Config::load().provider {
                    config::ProviderPreference::Idlix => Some(PROVIDER_IDLIX),
                    config::ProviderPreference::Otakudesu => Some(PROVIDER_OTAKUDESU),
                    config::ProviderPreference::Auto => None,
                }
            });
            let tipe = if let Some(provider) = tipe_override.or(forced_provider) {
                provider
            } else if let Some(cached) = input::load_search_provider(&selected_title) {
                cached
            } else {
                let Some(selected) = pick_provider(&selected_title, false).await? else {
                    continue;
                };
                input::save_search_provider(&selected_title, selected);
                selected
            };
            save_history(&selected_title);

            let mut animeku = AnimekuCli::for_provider(tipe);

            loop {
                clearscreen_and_show_banner()?;
                let movie = animeku.search(&selected_title).await?;
                if animeku.provider_type() != tipe {
                    input::save_search_provider(&selected_title, animeku.provider_type());
                }
                let Some(movie) = movie else {
                    break;
                };
                handle_movie_episodes(&mut animeku, &movie, discord).await?;
                if animeku.provider_type() != tipe {
                    input::save_search_provider(&selected_title, animeku.provider_type());
                }
            }
        }
    }
    Ok(())
}
