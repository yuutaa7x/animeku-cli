use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::io::{stdout, Write};
use std::process::Command;
use animeku::AnimekuCli;
use colored::Colorize;
use ext::Ext;
use tokio::runtime;

use crate::{input::get_user_input, util::clearscreen_and_show_banner};

mod animeku;
mod ext;
mod input;
mod models;
mod util;

const DISCORD_APP_ID: &str = "1549897147577667784";
const DEFAULT_DISCORD_LOGO: &str = "logoanimekucli";
const IDLIX_AUDIO_PATH: &str = "/tmp/animeku_idlix_audio.m3u8";
const IDLIX_SUBTITLE_PATH: &str = "/tmp/animeku_idlix.vtt";

const PROVIDER_IDLIX: usize = 0;
const PROVIDER_OTAKUDESU: usize = 1;

fn get_ext(input: &crate::models::Input) -> Box<dyn Ext> {
    match input.tipe {
        PROVIDER_IDLIX => Box::new(ext::idlix::Idlix::new()),
        PROVIDER_OTAKUDESU => Box::new(ext::otakudesu::Otakudesu::new()),
        _ => Box::new(ext::idlix::Idlix::new()),
    }
}

fn check_command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn install_dependencies(package_manager: &str) {
    let run_command_silent = |mut cmd: Command| {
        let _ = cmd.stdout(std::process::Stdio::null())
                   .stderr(std::process::Stdio::null())
                   .status();
    };

    println!("\n{} Menginstall dependensi menggunakan {}...", "◆".blue(), package_manager);
    match package_manager {
        "apt-get" => {
            let mut cmd = Command::new("sudo");
            cmd.args(&["apt-get", "install", "-y", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "pacman" => {
            let mut cmd = Command::new("sudo");
            cmd.args(&["pacman", "-S", "--noconfirm", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "dnf" => {
            let mut cmd = Command::new("sudo");
            cmd.args(&["dnf", "install", "-y", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "zypper" => {
            let mut cmd = Command::new("sudo");
            cmd.args(&["zypper", "install", "-y", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "apk" => {
            let mut cmd = Command::new("sudo");
            cmd.args(&["apk", "add", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "brew" => {
            let mut cmd = Command::new("brew");
            cmd.args(&["install", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "winget" => {
            let mut cmd1 = Command::new("winget");
            cmd1.args(&["install", "-e", "--id", "mpv.net.mpv.net", "--accept-source-agreements", "--accept-package-agreements"]);
            run_command_silent(cmd1);
            let mut cmd2 = Command::new("winget");
            cmd2.args(&["install", "-e", "--id", "yt-dlp.yt-dlp", "--accept-source-agreements", "--accept-package-agreements"]);
            run_command_silent(cmd2);
        },
        "choco" => {
            let mut cmd = Command::new("choco");
            cmd.args(&["install", "-y", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        "scoop" => {
            let mut cmd = Command::new("scoop");
            cmd.args(&["install", "mpv", "yt-dlp"]);
            run_command_silent(cmd);
        },
        _ => {},
    }
}

fn detect_package_manager() -> &'static str {
    if cfg!(target_os = "windows") {
        if check_command_exists("winget") { return "winget"; }
        if check_command_exists("choco") { return "choco"; }
        if check_command_exists("scoop") { return "scoop"; }
    } else if cfg!(target_os = "macos") {
        if check_command_exists("brew") { return "brew"; }
    } else {
        if check_command_exists("apt-get") { return "apt-get"; }
        if check_command_exists("pacman") { return "pacman"; }
        if check_command_exists("dnf") { return "dnf"; }
        if check_command_exists("zypper") { return "zypper"; }
        if check_command_exists("apk") { return "apk"; }
    }
    "unknown"
}

fn show_health_screen() -> anyhow::Result<bool> {
    loop {
        clearscreen_and_show_banner()?;
        
        println!("  {}", "================ ANIMEKU-CLI SETUP & HEALTH ================".bright_magenta());
        println!();
        
        let has_mpv = check_command_exists("mpv");
        if has_mpv {
            println!("  {}  {}", "[✓ INSTALLED]".bright_green(), "MPV Video Player");
        } else {
            println!("  {}  {}", "[✗ NOT FOUND]".bright_red(), "MPV Video Player");
            println!("    {} {}", "↳".bright_yellow(), "Terminal playback is a NO GO (Browser playback only)".bright_yellow());
        }
        println!();
        
        let has_ytdlp = check_command_exists("yt-dlp") || check_command_exists("youtube-dl");
        if has_ytdlp {
            println!("  {}  {}", "[✓ INSTALLED]".bright_green(), "yt-dlp / youtube-dl");
        } else {
            println!("  {}  {}", "[- OPTIONAL ]".bright_yellow(), "yt-dlp / youtube-dl");
            println!("    {} {}", "↳".bright_black(), "Fallback extraction disabled".bright_black());
        }
        println!();
        
        println!("  {}  {}", "[✓ BUILT-IN ]".bright_cyan(), "Native Rust Scraper (Idlix Animation)");
        println!();
        
        println!("  {}", "------------------------------------------------------------------".bright_white());
        println!("  {}", "Current Limitations:".bright_yellow());
        if !has_mpv {
            println!("  {} {}", "•".bright_yellow(), "Terminal playback is a NO GO (MPV missing)".bright_yellow());
        } else {
            println!("  {} {}", "•".bright_green(), "None. All systems nominal.".bright_green());
        }
        println!("  {}", "------------------------------------------------------------------".bright_white());
        println!();
        
        println!("  {} {}", "[ 1 ]".bright_green(), "Auto-install dependencies (MPV + yt-dlp)".bright_green());
        if has_mpv {
            println!("  {} {}", "[ 2 ]".bright_white(), "Continue to Animeku-CLI (Watch Anime)".bright_white());
        } else {
            println!("  {} {}", "[ 2 ]".bright_black(), "Continue to Animeku-CLI (Watch Anime - Browser Only)".bright_black());
        }
        println!("  {} {}", "[ q ]".bright_white(), "Quit".bright_white());
        println!();
        
        print!("  > ");
        stdout().flush()?;
        
        let term = dialoguer::console::Term::stdout();
        let choice = term.read_char()?;
        println!("{}", choice);
        
        if choice == 'q' || choice == 'Q' {
            std::process::exit(0);
        } else if choice == '2' || choice == '\n' {
            return Ok(true);
        } else if choice == '1' {
            let package_manager = detect_package_manager();
            if package_manager == "unknown" {
                println!("\n{} Package manager tidak terdeteksi! Silahkan install mpv dan yt-dlp secara manual.", "■".red());
            } else {
                install_dependencies(package_manager);
            }
            
            println!("  {} Selesai! Tekan Enter untuk melanjutkan.", "✓".green());
            let mut dummy = String::new();
            std::io::stdin().read_line(&mut dummy)?;
        }
    }
}

fn clean_title(title: &str) -> String {
    let mut cleaned = title.to_string();
    cleaned = cleaned.replace("[Movie]", "");
    cleaned = cleaned.replace("[Series]", "");
    cleaned = cleaned.replace("Subtitle Indonesia", "");
    cleaned = cleaned.replace("Sub Indo", "");
    cleaned = cleaned.replace("subtitle indonesia", "");
    cleaned = cleaned.replace("sub indo", "");
    
    let re_season = regex::Regex::new(r"(?i)\bSeason\s*\d+\b").unwrap();
    cleaned = re_season.replace_all(&cleaned, "").to_string();
    
    let re_s = regex::Regex::new(r"(?i)\bS\d+\b").unwrap();
    cleaned = re_s.replace_all(&cleaned, "").to_string();
    
    let re_ep_range = regex::Regex::new(r"(?i)\(\s*episode\s*.*?\)").unwrap();
    cleaned = re_ep_range.replace_all(&cleaned, "").to_string();
    
    let re_batch = regex::Regex::new(r"(?i)\(\s*batch\s*\)").unwrap();
    cleaned = re_batch.replace_all(&cleaned, "").to_string();

    let re_spaces = regex::Regex::new(r"\s+").unwrap();
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();
    
    let re_hyphen = regex::Regex::new(r"(?i)\s*-\s*$").unwrap();
    cleaned = re_hyphen.replace_all(&cleaned, "").to_string();
    
    let re_brackets = regex::Regex::new(r"\(\s*\)").unwrap();
    cleaned = re_brackets.replace_all(&cleaned, "").to_string();
    
    cleaned.trim().to_string()
}

fn detect_season(title: &str, img_url: &str, meta_data: &[(String, String)]) -> Option<u32> {
    let re_season = regex::Regex::new(r"(?i)(?:season|s)\s*-?\s*(\d+)").unwrap();
    let re_ordinal = regex::Regex::new(r"(?i)(\d+)(?:st|nd|rd|th)\s+season").unwrap();
    let re_roman = regex::Regex::new(r"(?i)\b(II|III|IV|V|VI|VII|VIII|IX|X)\b").unwrap();

    let parse_text = |text: &str| -> Option<u32> {
        if let Some(caps) = re_season.captures(text) {
            if let Ok(num) = caps[1].parse::<u32>() { return Some(num); }
        }
        if let Some(caps) = re_ordinal.captures(text) {
            if let Ok(num) = caps[1].parse::<u32>() { return Some(num); }
        }
        if let Some(caps) = re_roman.captures(text) {
            let roman = caps[1].to_uppercase();
            let num = match roman.as_str() {
                "II" => 2, "III" => 3, "IV" => 4, "V" => 5, "VI" => 6,
                "VII" => 7, "VIII" => 8, "IX" => 9, "X" => 10, _ => 1,
            };
            return Some(num);
        }
        None
    };

    if let Some(s) = parse_text(title) { return Some(s); }
    if let Some(s) = parse_text(img_url) { return Some(s); }
    
    for (_meta_key, meta_value) in meta_data {
        if let Some(s) = parse_text(meta_value) { return Some(s); }
    }

    None
}

fn format_episode(title: &str, is_series: bool, season_num: u32) -> String {
    let trimmed = title.trim();
    if !is_series {
        return "Full Movie".to_string();
    }
    
    let re_episode = regex::Regex::new(r"(?i)episode\s*(\d+)").unwrap();
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

    let re_season_ep = regex::Regex::new(r"(?i)s(\d+)e(\d+)").unwrap();
    if let Some(caps) = re_season_ep.captures(trimmed) {
        let season: u32 = caps[1].parse().unwrap_or(season_num);
        let ep: u32 = caps[2].parse().unwrap_or(1);
        return if season > 1 {
            format!("Season {} • Episode {}", season, ep)
        } else {
            format!("Episode {}", ep)
        };
    }
    
    if trimmed.chars().all(|c| c.is_digit(10)) {
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

fn update_discord_status(
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

    let mut large_image = thumb_url.unwrap_or_else(|| DEFAULT_DISCORD_LOGO.to_string());
    if large_image.contains("otakudesu") {
        large_image = DEFAULT_DISCORD_LOGO.to_string();
    }

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
            .state(&format_episode(&clean_ep, is_series, season_num))
            .assets(assets)
            .timestamps(activity::Timestamps::new().start(current_timestamp))
            .activity_type(activity::ActivityType::Watching),
    );
}

fn reset_discord_status(discord: &mut Option<DiscordIpcClient>) {
    if let Some(ref mut drpc) = discord {
        let _ = drpc.set_activity(
            activity::Activity::new()
                .state("Memilih Anime...")
                .details("Animeku-CLI")
                .activity_type(activity::ActivityType::Watching),
        );
    }
}

fn execute_player(stream_url: &str, provider_type: usize) -> anyhow::Result<()> {
    print!("{} Membuka tautan diaplikasi eksternal .. \n", "◆".blue());
    stdout().flush()?;

    let player_choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih Aksi:")
        .default(0)
        .items(&["🎬 Putar dengan MPV", "🎬 Putar dengan VLC", "🌐 Buka di Browser", "🔙 Kembali"])
        .interact()?;

    match player_choice {
        0 => {
            println!("🎬 Sedang memutar video di MPV... (Tutup MPV untuk kembali ke menu)");
            let mut cmd = Command::new("mpv");
            cmd.arg("--hwdec=auto-safe");
            cmd.arg("--cache=yes");
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
                if std::path::Path::new(IDLIX_AUDIO_PATH).exists() {
                    cmd.arg(format!("--audio-file={}", IDLIX_AUDIO_PATH));
                }
                if std::path::Path::new(IDLIX_SUBTITLE_PATH).exists() {
                    cmd.arg(format!("--sub-file={}", IDLIX_SUBTITLE_PATH));
                }
            } else if provider_type == PROVIDER_OTAKUDESU {
                cmd.arg("--http-header-fields=Referer: https://desustream.net/");
                cmd.arg("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36");
            }
            if let Ok(status) = cmd.arg(stream_url).status() {
                if !status.success() {
                    eprintln!("{} Gagal memutar video di MPV. Tekan Enter untuk kembali...", "■".red());
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                }
            } else {
                eprintln!("{} MPV tidak ditemukan!", "■".red());
            }
        },
        1 => {
            println!("🎬 Sedang memutar video di VLC... (Tutup VLC untuk kembali ke menu)");
            let mut cmd = Command::new("vlc");
            if provider_type == PROVIDER_OTAKUDESU {
                cmd.arg("--http-referrer=https://desustream.net/");
                cmd.arg("--http-user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36");
            }
            if let Ok(status) = cmd.arg(stream_url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() {
                if !status.success() { eprintln!("{} Gagal memutar video di VLC", "■".red()); }
            } else { eprintln!("{} VLC tidak ditemukan!", "■".red()); }
        },
        2 => {
            if open::that(stream_url).is_ok() {
                if dialoguer::Confirm::with_theme(&crate::util::custom_theme())
                    .with_prompt("Tutup aplikasi?")
                    .interact()?
                {
                    std::process::exit(0);
                }
            } else {
                println!("gagal membuka browser");
            }
        },
        _ => {},
    }
    Ok(())
}

async fn handle_movie_episodes(
    animeku: &mut AnimekuCli,
    movie: &crate::models::Movie,
    provider_type: usize,
    discord: &mut Option<DiscordIpcClient>,
) -> anyhow::Result<()> {
    let mut cached_season: Option<u32> = None;

    loop {
        clearscreen_and_show_banner()?;
        let episode = match animeku.extract_episode(movie.clone()).await? {
            Some(ep) => ep,
            None => break, // Back to movie search
        };

        let season_num = match cached_season {
            Some(s) => s,
            None => {
                let detected = if episode.is_series {
                    let meta_opt = animeku.get_meta(&movie.id);
                    let thumb = meta_opt.clone().and_then(|m| m.thumb_url).unwrap_or_default();
                    let meta_data = meta_opt.map(|m| m.data).unwrap_or_default();
                    if let Some(s) = detect_season(&movie.title, &thumb, &meta_data) {
                        s
                    } else {
                        let answer: String = dialoguer::Input::with_theme(&crate::util::custom_theme())
                            .with_prompt("Sistem gagal mendeteksi Season. Ini Season berapa? (Kosongkan jika S1)")
                            .allow_empty(true)
                            .interact_text()
                            .unwrap_or_default();
                        answer.trim().parse::<u32>().unwrap_or(1)
                    }
                } else {
                    1
                };
                cached_season = Some(detected);
                detected
            }
        };

        // Stream selection & playback
        clearscreen_and_show_banner()?;
        let meta_opt = animeku.get_meta(&movie.id);
        let thumb_url = meta_opt.and_then(|m| m.thumb_url);
        
        update_discord_status(discord, &movie.title, &episode.title, episode.is_series, season_num, thumb_url);

        let stream_opt = animeku.extract_stream_urls(episode.clone()).await?;
        if let Some(stream) = stream_opt {
            execute_player(&stream.url, provider_type)?;
        }
        
        reset_discord_status(discord);
    }
    Ok(())
}

async fn run_app() -> anyhow::Result<()> {
    let mut discord = Some(DiscordIpcClient::new(DISCORD_APP_ID));
    if let Some(ref mut drpc) = discord {
        if drpc.connect().is_ok() {
            reset_discord_status(&mut discord);
        } else {
            discord = None;
        }
    }

    if !check_command_exists("mpv") {
        show_health_screen()?;
    }

    loop {
        clearscreen_and_show_banner()?;
        let input = match get_user_input()? {
            Some(user_input) => user_input,
            None => return Ok(()),
        };

        let extractor = get_ext(&input);
        let mut animeku = AnimekuCli::new(extractor);

        loop {
            clearscreen_and_show_banner()?;
            let movie = match animeku.search(&input.title).await? {
                Some(selected_movie) => selected_movie,
                None => break, // Back to input prompt
            };

            handle_movie_episodes(&mut animeku, &movie, input.tipe, &mut discord).await?;
        }
    }
}

fn main() -> anyhow::Result<()> {
    let rt = runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async {
        if let Err(e) = run_app().await {
            println!(" {} {}\n", "■".red(), format!("{:#?}", e).yellow());
        }
    });
    Ok(())
}
