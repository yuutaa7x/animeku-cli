use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::io::{stdout, Write};
use std::process::Command;
use std::sync::OnceLock;
use animeku::AnimekuCli;
use colored::Colorize;
use ext::otakudesu::{OTAKUDESU_REFERER, OTAKUDESU_USER_AGENT};
use tokio::runtime;

use crate::{input::{get_user_input, load_history, save_history, pick_provider, load_watch_history, delete_watch_entry, delete_search_history_entry}, util::clearscreen_and_show_banner};

mod anilist;
mod animeku;
mod batch;
mod cache;
mod config;
mod download;
mod episode;
mod ext;
mod input;
mod live_input;
mod models;
mod proxy;
mod update;
mod util;

use episode::{
    clean_title, detect_season, detect_season_from_titles, find_next_episode, format_episode,
};

const DISCORD_APP_ID: &str = "1549897147577667784";
const DEFAULT_DISCORD_LOGO: &str = "logoanimekucli";
const DISCORD_IMAGE_MAX_LEN: usize = 300;
fn idlix_audio_path() -> String {
    crate::util::temp_file("animeku_idlix_audio.m3u8")
}

fn idlix_subtitle_path() -> String {
    crate::util::get_subtitle_path()
}

fn mpv_track_script_path() -> String {
    crate::util::temp_file("animeku_track.lua")
}

fn mpv_last_pos_file_path() -> String {
    crate::util::temp_file("animeku_last_pos.txt")
}

fn mpv_next_file_path() -> String {
    crate::util::temp_file("animeku_next_requested.txt")
}

const PROVIDER_IDLIX: usize = ext::PROVIDER_IDLIX;
pub const PROVIDER_OTAKUDESU: usize = ext::PROVIDER_OTAKUDESU;

const REPO_URL: &str = "https://github.com/yuutaa7x/animeku-cli";

#[derive(Debug, Default)]
struct CliArgs {
    search: Option<String>,
    provider: Option<usize>,
    player: Option<config::PlayerChoice>,
    resume: bool,
    no_rpc: bool,
    update: bool,
    check_update: bool,
}

static CLI_ARGS: OnceLock<CliArgs> = OnceLock::new();

pub(crate) fn provider_override() -> Option<usize> {
    CLI_ARGS.get().and_then(|args| args.provider)
}

pub(crate) fn player_override() -> Option<config::PlayerChoice> {
    CLI_ARGS.get().and_then(|args| args.player)
}

pub(crate) fn rpc_disabled() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.no_rpc)
}

fn cli_search_title() -> Option<String> {
    CLI_ARGS.get().and_then(|args| args.search.clone())
}

fn cli_resume() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.resume)
}

fn cli_update() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.update)
}

fn cli_check_update() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.check_update)
}

fn print_help() {
    println!(
        "Animeku-CLI v{}\n\nUsage:\n  animeku-cli [OPTIONS] [JUDUL]\n\nOptions:\n  --search <JUDUL>       Langsung cari judul tanpa membuka menu utama\n  --provider <PROVIDER>  auto, idlix, atau otakudesu\n  --player <PLAYER>      ask, mpv, vlc, atau browser\n  --resume               Lanjutkan tontonan terakhir\n  --update               Unduh dan pasang pembaruan terbaru\n  --check-update         Cek versi terbaru tanpa memasang\n  --no-rpc               Matikan Discord Rich Presence untuk sesi ini\n  -h, --help             Tampilkan bantuan\n  -V, --version          Tampilkan versi",
        env!("CARGO_PKG_VERSION")
    );
}

fn parse_args() -> anyhow::Result<CliArgs> {
    let mut args = CliArgs::default();
    let mut iter = std::env::args().skip(1);
    let mut positional = Vec::new();

    while let Some(arg) = iter.next() {
        if arg == "--" {
            positional.extend(iter.by_ref());
            break;
        }
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("animeku-cli {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--resume" => args.resume = true,
            "--update" => args.update = true,
            "--check-update" => args.check_update = true,
            "--no-rpc" => args.no_rpc = true,
            "--search" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--search membutuhkan judul");
                };
                args.search = Some(value);
            }
            "--provider" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--provider membutuhkan nilai: auto, idlix, atau otakudesu");
                };
                args.provider = match value.to_lowercase().as_str() {
                    "auto" => None,
                    "idlix" | "0" => Some(PROVIDER_IDLIX),
                    "otakudesu" | "1" => Some(PROVIDER_OTAKUDESU),
                    _ => anyhow::bail!(
                        "provider tidak dikenal: {value}. Gunakan auto, idlix, atau otakudesu."
                    ),
                };
            }
            "--player" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--player membutuhkan nilai: ask, mpv, vlc, atau browser");
                };
                args.player = Some(match value.to_lowercase().as_str() {
                    "ask" => config::PlayerChoice::Ask,
                    "mpv" => config::PlayerChoice::Mpv,
                    "vlc" => config::PlayerChoice::Vlc,
                    "browser" => config::PlayerChoice::Browser,
                    _ => anyhow::bail!(
                        "player tidak dikenal: {value}. Gunakan ask, mpv, vlc, atau browser."
                    ),
                });
            }
            _ if arg.starts_with("--search=") => {
                args.search = Some(arg["--search=".len()..].to_string());
            }
            _ if arg.starts_with("--provider=") => {
                let value = &arg["--provider=".len()..];
                args.provider = match value.to_lowercase().as_str() {
                    "auto" => None,
                    "idlix" | "0" => Some(PROVIDER_IDLIX),
                    "otakudesu" | "1" => Some(PROVIDER_OTAKUDESU),
                    _ => anyhow::bail!(
                        "provider tidak dikenal: {value}. Gunakan auto, idlix, atau otakudesu."
                    ),
                };
            }
            _ if arg.starts_with("--player=") => {
                let value = &arg["--player=".len()..];
                args.player = Some(match value.to_lowercase().as_str() {
                    "ask" => config::PlayerChoice::Ask,
                    "mpv" => config::PlayerChoice::Mpv,
                    "vlc" => config::PlayerChoice::Vlc,
                    "browser" => config::PlayerChoice::Browser,
                    _ => anyhow::bail!(
                        "player tidak dikenal: {value}. Gunakan ask, mpv, vlc, atau browser."
                    ),
                });
            }
            _ if arg.starts_with('-') => {
                anyhow::bail!("opsi tidak dikenal: {arg}. Gunakan --help untuk bantuan.");
            }
            _ => positional.push(arg),
        }
    }

    if args.search.is_none() && !positional.is_empty() {
        args.search = Some(positional.join(" "));
    }

    Ok(args)
}

fn check_command_exists(cmd: &str) -> bool {
    let p = std::path::Path::new(cmd);
    if p.is_file() {
        return true;
    }

    if let Some(paths) = std::env::var_os("PATH") {
        let extensions: Vec<String> = if cfg!(target_os = "windows") {
            let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string());
            pathext.split(';').map(|s| s.to_string()).collect()
        } else {
            vec!["".to_string()]
        };

        for dir in std::env::split_paths(&paths) {
            let direct = dir.join(cmd);
            if direct.is_file() {
                return true;
            }
            if cfg!(target_os = "windows") {
                for ext in &extensions {
                    let with_ext = dir.join(format!("{}{}", cmd, ext));
                    if with_ext.is_file() {
                        return true;
                    }
                }
            }
        }
    }

    let checker = if cfg!(target_os = "windows") { "where" } else { "which" };
    if let Ok(status) = Command::new(checker)
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        if status.success() {
            return true;
        }
    }

    if cfg!(target_os = "windows") && check_windows_known_paths(cmd) {
        return true;
    }

    false
}

fn check_windows_known_paths(cmd: &str) -> bool {
    let cmd_exe = if cmd.ends_with(".exe") {
        cmd.to_string()
    } else {
        format!("{}.exe", cmd)
    };

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let p = std::path::Path::new(&local_app_data).join("Microsoft").join("WindowsApps").join(&cmd_exe);
        if p.exists() {
            return true;
        }
    }

    if let Ok(prog_files) = std::env::var("ProgramFiles") {
        let paths = [
            std::path::Path::new(&prog_files).join(cmd).join(&cmd_exe),
            std::path::Path::new(&prog_files).join("mpv").join("mpv.exe"),
            std::path::Path::new(&prog_files).join("MPV Player").join("mpv.exe"),
            std::path::Path::new(&prog_files).join("mpv.net").join("mpvnet.exe"),
            std::path::Path::new(&prog_files).join("VideoLAN").join("VLC").join(&cmd_exe),
        ];
        for p in &paths {
            if p.exists() {
                return true;
            }
        }
    }

    if let Ok(prog_files_x86) = std::env::var("ProgramFiles(x86)") {
        let paths = [
            std::path::Path::new(&prog_files_x86).join(cmd).join(&cmd_exe),
            std::path::Path::new(&prog_files_x86).join("mpv").join("mpv.exe"),
            std::path::Path::new(&prog_files_x86).join("VideoLAN").join("VLC").join(&cmd_exe),
        ];
        for p in &paths {
            if p.exists() {
                return true;
            }
        }
    }

    if let Ok(prog_data) = std::env::var("ProgramData") {
        let p = std::path::Path::new(&prog_data).join("chocolatey").join("bin").join(&cmd_exe);
        if p.exists() {
            return true;
        }
    }

    if let Some(home) = dirs::home_dir() {
        let paths = [
            home.join("scoop").join("shims").join(&cmd_exe),
            home.join("scoop").join("shims").join(format!("{}.cmd", cmd)),
            home.join("scoop").join("shims").join(format!("{}.ps1", cmd)),
            home.join("scoop").join("apps").join(cmd).join("current").join(&cmd_exe),
            std::path::PathBuf::from(r"C:\mpv\mpv.exe"),
        ];
        for p in &paths {
            if p.exists() {
                return true;
            }
        }
    }

    false
}

fn resolve_command_path(cmd: &str) -> String {
    if !cfg!(target_os = "windows") {
        return cmd.to_string();
    }

    let cmd_exe = if cmd.ends_with(".exe") {
        cmd.to_string()
    } else {
        format!("{}.exe", cmd)
    };

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let p = std::path::Path::new(&local_app_data).join("Microsoft").join("WindowsApps").join(&cmd_exe);
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }

    if let Ok(prog_files) = std::env::var("ProgramFiles") {
        let p = std::path::Path::new(&prog_files).join(cmd).join(&cmd_exe);
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
        if cmd == "mpv" {
            let p_mpv = std::path::Path::new(&prog_files).join("mpv").join("mpv.exe");
            if p_mpv.exists() {
                return p_mpv.to_string_lossy().to_string();
            }
            let p_mpv2 = std::path::Path::new(&prog_files).join("MPV Player").join("mpv.exe");
            if p_mpv2.exists() {
                return p_mpv2.to_string_lossy().to_string();
            }
            let p_mpvnet = std::path::Path::new(&prog_files).join("mpv.net").join("mpvnet.exe");
            if p_mpvnet.exists() {
                return p_mpvnet.to_string_lossy().to_string();
            }
        }
        if cmd == "vlc" {
            let pv = std::path::Path::new(&prog_files).join("VideoLAN").join("VLC").join("vlc.exe");
            if pv.exists() {
                return pv.to_string_lossy().to_string();
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        let scoop = home.join("scoop").join("shims").join(&cmd_exe);
        if scoop.exists() {
            return scoop.to_string_lossy().to_string();
        }
        let choco = std::path::PathBuf::from(r"C:\ProgramData\chocolatey\bin").join(&cmd_exe);
        if choco.exists() {
            return choco.to_string_lossy().to_string();
        }
        let c_mpv = std::path::PathBuf::from(r"C:\mpv\mpv.exe");
        if cmd == "mpv" && c_mpv.exists() {
            return c_mpv.to_string_lossy().to_string();
        }
    }

    cmd.to_string()
}

fn install_dependencies(package_manager: &str) {
    println!("\n{} Menginstall dependensi menggunakan {}...", "◆".blue(), package_manager);
    match package_manager {
        "apt-get" => {
            let _ = Command::new("sudo")
                .args(["apt-get", "install", "-y", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "pacman" => {
            let _ = Command::new("sudo")
                .args(["pacman", "-S", "--noconfirm", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "dnf" => {
            let _ = Command::new("sudo")
                .args(["dnf", "install", "-y", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "zypper" => {
            let _ = Command::new("sudo")
                .args(["zypper", "install", "-y", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "apk" => {
            let _ = Command::new("sudo")
                .args(["apk", "add", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "pkg" => {
            // Termux (Android) — tidak butuh sudo
            println!("  {} Menjalankan: pkg install mpv yt-dlp ffmpeg ...", "ℹ".yellow());
            let _ = Command::new("pkg")
                .args(["install", "-y", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "brew" => {
            let _ = Command::new("brew")
                .args(["install", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "winget" => {
            let winget_bin = resolve_command_path("winget");
            println!("  {} Menjalankan: winget install --id shinchiro.mpv --source winget ...", "ℹ".yellow());
            let status1 = Command::new(&winget_bin)
                .args(["install", "--id", "shinchiro.mpv", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
                .status();

            let mpv_ok = match status1 {
                Ok(s) if s.success() => true,
                _ => {
                    println!("  {} Mencoba alternatif: winget install --id mpv.net --source winget ...", "ℹ".yellow());
                    let s2 = Command::new(&winget_bin)
                        .args(["install", "--id", "mpv.net", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
                        .status();
                    s2.map(|s| s.success()).unwrap_or(false)
                }
            };

            if mpv_ok {
                println!("  {} MPV berhasil diinstal!", "✓".green());
            } else {
                eprintln!("  {} Gagal menginstal MPV via winget.", "■".red());
            }

            println!("  {} Menjalankan: winget install --id yt-dlp.yt-dlp --source winget ...", "ℹ".yellow());
            let _ = Command::new(&winget_bin)
                .args(["install", "--id", "yt-dlp.yt-dlp", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
                .status();

            println!("  {} Menjalankan: winget install --id Gyan.FFmpeg --source winget ...", "ℹ".yellow());
            let _ = Command::new(&winget_bin)
                .args(["install", "--id", "Gyan.FFmpeg", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
                .status();
        },
        "choco" => {
            let choco_bin = resolve_command_path("choco");
            println!("  {} Menjalankan: choco install -y mpv yt-dlp ffmpeg ...", "ℹ".yellow());
            let _ = Command::new(&choco_bin)
                .args(["install", "-y", "mpv", "yt-dlp", "ffmpeg"])
                .status();
        },
        "scoop" => {
            let scoop_bin = resolve_command_path("scoop");
            println!("  {} Menjalankan: scoop install mpv yt-dlp ffmpeg ...", "ℹ".yellow());
            let _ = Command::new(&scoop_bin)
                .args(["install", "mpv", "yt-dlp", "ffmpeg"])
                .status();
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
        // Termux (Android) uses pkg; check before apt-get since Termux also ships apt-get
        if check_command_exists("pkg") && std::env::var("TERMUX_VERSION").is_ok() {
            return "pkg";
        }
        if check_command_exists("apt-get") { return "apt-get"; }
        if check_command_exists("pacman") { return "pacman"; }
        if check_command_exists("dnf") { return "dnf"; }
        if check_command_exists("zypper") { return "zypper"; }
        if check_command_exists("apk") { return "apk"; }
    }
    "unknown"
}

async fn show_health_screen() -> anyhow::Result<bool> {
    loop {
        clearscreen_and_show_banner()?;
        
        println!("  {}", "================ ANIMEKU-CLI SETUP & HEALTH ================".bright_magenta());
        println!();
        
        let has_mpv = check_command_exists("mpv");
        if has_mpv {
            println!("  {}  MPV Video Player", "[✓ INSTALLED]".bright_green());
        } else {
            println!("  {}  MPV Video Player", "[✗ NOT FOUND]".bright_red());
            println!("    {} {}", "↳".bright_yellow(), "Terminal playback is a NO GO (Browser playback only)".bright_yellow());
        }
        println!();
        
        let has_ytdlp = check_command_exists("yt-dlp") || check_command_exists("youtube-dl");
        if has_ytdlp {
            println!("  {}  yt-dlp / youtube-dl", "[✓ INSTALLED]".bright_green());
        } else {
            println!("  {}  yt-dlp / youtube-dl", "[- OPTIONAL ]".bright_yellow());
            println!("    {} {}", "↳".bright_black(), "Fallback extraction disabled".bright_black());
        }
        println!();

        let has_ffmpeg = check_command_exists("ffmpeg");
        if has_ffmpeg {
            println!("  {}  ffmpeg (Episode Download)", "[✓ INSTALLED]".bright_green());
        } else {
            println!("  {}  ffmpeg (Episode Download)", "[- OPTIONAL ]".bright_yellow());
            println!(
                "    {} {}",
                "↳".bright_black(),
                "Episode download disabled (ffmpeg missing)".bright_black()
            );
        }
        println!();

        println!("  {}  Native Rust Scraper (Idlix Animation)", "[✓ BUILT-IN ]".bright_cyan());
        println!();
        
        println!("  {}", "------------------------------------------------------------------".bright_white());
        println!("  {}", "Current Limitations:".bright_yellow());
        if !has_mpv {
            println!("  {} {}", "•".bright_yellow(), "Terminal playback is a NO GO (MPV missing)".bright_yellow());
        }
        if !has_ffmpeg {
            println!("  {} {}", "•".bright_yellow(), "Episode download is a NO GO (ffmpeg missing)".bright_yellow());
        }
        if has_mpv && has_ffmpeg {
            println!("  {} {}", "•".bright_green(), "None. All systems nominal.".bright_green());
        }
        println!("  {}", "------------------------------------------------------------------".bright_white());
        println!();
        
        println!("  {} {}", "[ 1 ]".bright_green(), "Auto-install dependencies (MPV + ffmpeg + yt-dlp)".bright_green());
        if has_mpv {
            println!("  {} {}", "[ 2 ]".bright_white(), "Continue to Animeku-CLI (Watch Anime)".bright_white());
        } else {
            println!("  {} {}", "[ 2 ]".bright_black(), "Continue to Animeku-CLI (Watch Anime - Browser Only)".bright_black());
        }
        println!(
            "  {} {}",
            "[ 3 ]".bright_white(),
            "Tes koneksi Idlix & Otakudesu".bright_white()
        );
        println!("  {} {}", "[ q ]".bright_white(), "Kembali".bright_white());
        println!();
        
        print!("  > ");
        stdout().flush()?;
        
        let term = dialoguer::console::Term::stdout();
        let choice = term.read_char()?;
        println!("{}", choice);
        
        if choice == 'q' || choice == 'Q' || choice == '\x1b' || choice == '\x03' {
            return Ok(false);
        } else if choice == '2' || choice == '\n' {
            return Ok(true);
        } else if choice == '3' {
            handle_provider_health().await?;
        } else if choice == '1' {
            let package_manager = detect_package_manager();
            if package_manager == "unknown" {
                if cfg!(target_os = "windows") {
                    println!("\n{} Tidak ditemukan package manager otomatis (winget / choco / scoop).", "■".red());
                    println!("  Silakan pilih salah satu opsi instalasi MPV untuk Windows:");
                    println!("  1. Jalankan di PowerShell / CMD:");
                    println!("     {}", "winget install --id shinchiro.mpv --source winget".cyan());
                    println!("  2. Atau install via Scoop:");
                    println!("     {}", "scoop install mpv".cyan());
                    println!("  3. Atau unduh manual installer MPV di:");
                    println!("     {}", "https://mpv.io/installation/".cyan());
                    println!("  4. Untuk download episode, install ffmpeg:");
                    println!("     {}", "winget install --id Gyan.FFmpeg --source winget".cyan());
                    println!("\n  Ingin membuka halaman unduhan MPV di browser? (y/n)");
                    let mut answer = String::new();
                    if std::io::stdin().read_line(&mut answer).is_ok() && answer.trim().eq_ignore_ascii_case("y") {
                        let _ = open::that("https://mpv.io/installation/");
                    }
                } else {
                    println!("\n{} Package manager tidak terdeteksi! Silakan install mpv, yt-dlp, dan ffmpeg secara manual.", "■".red());
                }
            } else {
                install_dependencies(package_manager);
            }
            
            println!("\n  {} Tekan Enter untuk memeriksa kembali status dependensi.", "✓".green());
            let mut dummy = String::new();
            std::io::stdin().read_line(&mut dummy)?;
        }
    }
}

async fn handle_provider_health() -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    println!("  {}", "================ KESEHATAN PROVIDER ================".bright_magenta());
    println!();
    for provider in [PROVIDER_IDLIX, PROVIDER_OTAKUDESU] {
        print!("  {} ... ", ext::provider_name(provider).bright_white());
        stdout().flush()?;
        match crate::ext::check_provider_health(provider).await {
            Ok(health) => println!(
                "{} siap • {}ms • {} hasil uji",
                "✓".bright_green(),
                health.latency.as_millis(),
                health.results
            ),
            Err(error) => {
                println!("{} tidak dapat dihubungi", "■".bright_red());
                println!("    {} {:#}", "↳".bright_yellow(), error);
            }
        }
    }
    println!();
    println!(
        "  Jika satu provider gagal, animeku-cli tetap mencoba provider lainnya secara otomatis."
    );
    println!("  Tekan Enter untuk kembali...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
    Ok(())
}

fn is_otakudesu_image(url: &str) -> bool {
    url.to_ascii_lowercase().contains("otakudesu")
}

/// Fill a square canvas with the poster. Discord's RPC image box is square,
/// so `fit=cover` zooms instead of adding letterbox bars. `a=attention` asks
/// wsrv.nl to keep faces and visually busy parts in frame while cropping.
fn square_discord_poster(image_url: &str) -> String {
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
async fn resolve_discord_poster(thumb_url: Option<String>, movie_title: &str) -> String {
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

async fn fetch_anilist_cover(title: &str) -> Option<String> {
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

async fn update_discord_status(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackOutcome {
    Played {
        position: u64,
        offer_next: bool,
        completed: bool,
        /// MPV's `n` key was pressed. The caller can skip the usual prompt.
        next_requested: bool,
    },
    Cancelled,
}

async fn execute_player(
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
fn detect_season_for_episode(
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

enum EpisodePlayback {
    Played {
        offer_next: bool,
        next_requested: bool,
    },
    PlayerCancelled,
    StreamCancelled,
}

fn confirm_next_episode(next_ep: &crate::models::Episode) -> anyhow::Result<bool> {
    match config::Config::load().auto_next {
        config::AutoNext::Always => Ok(true),
        config::AutoNext::Never => Ok(false),
        config::AutoNext::Ask => Ok(dialoguer::Confirm::with_theme(&crate::util::custom_theme())
            .with_prompt(format!(
                "Lanjut ke {}? (Esc = tidak)",
                next_ep.title.trim()
            ))
            .default(true)
            // Sembunyikan hint "yes" bawaan dialoguer; Enter tetap berarti ya.
            .show_default(false)
            .interact_opt()?
            .unwrap_or(false)),
    }
}

/// Play one episode and persist its progress. Returning a distinct result for
/// "player cancelled" and "stream picker cancelled" lets the caller reopen the
/// right screen instead of treating a cancelled playback as finished.
async fn play_episode(
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
    let thumb_url = animeku.get_meta(&movie.id).and_then(|m| m.thumb_url);

    update_discord_status(
        discord,
        &movie.title,
        &episode.title,
        episode.is_series,
        season_num,
        thumb_url,
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

fn sync_active_playback(
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

async fn handle_movie_episodes(
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

async fn resume_watch_entry(
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
fn next_watch_entry(
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

async fn handle_resume_latest(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
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

fn discovery_label(info: &crate::anilist::MediaInfo) -> String {
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

async fn show_discovery_list<F>(
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

async fn handle_explore(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
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

async fn handle_batch_manager() -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let Some(mut queue) = crate::batch::BatchQueue::load() else {
            println!("Tidak ada antrean batch yang tersimpan.\n");
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        };

        if queue.is_done() {
            crate::batch::BatchQueue::clear();
            println!("Antrean batch terakhir sudah selesai.\n");
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        }

        let (completed, skipped, failed, pending) = queue.counts();
        println!("Kelola Download:");
        println!(
            "  Judul    : {}",
            crate::models::clean_movie_title(&queue.movie.title)
        );
        println!("  Provider : {}", ext::provider_name(queue.provider_type));
        println!("  Kualitas : {}", queue.target_quality);
        println!("  Folder   : {}", queue.directory);
        println!(
            "  Progres  : {} berhasil • {} dilewati • {} gagal • {} tertunda",
            completed, skipped, failed, pending
        );
        println!();

        for (index, episode) in queue.episodes.iter().enumerate() {
            let status = queue.status_at(index);
            println!(
                "  {} {:>3}. {} — {}",
                status.icon(),
                index + 1,
                crate::models::clean_episode_title(&episode.title),
                status.label()
            );
        }
        println!();

        let mut labels = vec!["▶ Lanjutkan / Mulai Antrean".to_string()];
        let mut actions = vec![BatchManagerAction::Run];
        if failed > 0 {
            labels.push(format!("↻ Retry {} Episode Gagal", failed));
            actions.push(BatchManagerAction::RetryFailed);
        }
        labels.push("📂 Buka Folder Download".to_string());
        actions.push(BatchManagerAction::OpenFolder);
        labels.push("🗑 Hapus Antrean".to_string());
        actions.push(BatchManagerAction::Clear);
        labels.push("Kembali".to_string());
        actions.push(BatchManagerAction::Back);

        let entries = labels
            .iter()
            .cloned()
            .map(input::MenuEntry::new)
            .collect::<Vec<_>>();
        let Some(choice) = input::select_menu_simple("Pilih Tindakan Download:", &entries, 0)?
        else {
            break;
        };
        let action = actions
            .get(choice)
            .copied()
            .unwrap_or(BatchManagerAction::Back);

        match action {
            BatchManagerAction::Run => {
                let mut animeku = AnimekuCli::for_provider(queue.provider_type);
                if let Err(error) = animeku.run_batch(&mut queue).await {
                    eprintln!("{} Batch download gagal: {:#}", "■".red(), error);
                    println!();
                    println!("Tekan Enter untuk kembali...");
                    let mut dummy = String::new();
                    let _ = std::io::stdin().read_line(&mut dummy);
                }
            }
            BatchManagerAction::RetryFailed => {
                let retried = queue.retry_failed();
                queue.save()?;
                println!(
                    "{} {} episode gagal dipindahkan kembali ke antrean.",
                    "✓".green(),
                    retried
                );
                println!("\nTekan Enter untuk kembali...");
                let mut dummy = String::new();
                let _ = std::io::stdin().read_line(&mut dummy);
            }
            BatchManagerAction::OpenFolder => {
                let directory = std::path::PathBuf::from(&queue.directory);
                let _ = std::fs::create_dir_all(&directory);
                match open::that(&directory) {
                    Ok(()) => println!("{} Membuka '{}'.", "✓".green(), directory.display()),
                    Err(error) => eprintln!(
                        "{} Gagal membuka folder '{}': {}",
                        "■".red(),
                        directory.display(),
                        error
                    ),
                }
                println!("\nTekan Enter untuk kembali...");
                let mut dummy = String::new();
                let _ = std::io::stdin().read_line(&mut dummy);
            }
            BatchManagerAction::Clear => {
                if confirm_action(
                    "Hapus antrean download? File yang sudah selesai tidak dihapus.",
                )? {
                    crate::batch::BatchQueue::clear();
                    return Ok(());
                }
            }
            BatchManagerAction::Back => break,
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchManagerAction {
    Run,
    RetryFailed,
    OpenFolder,
    Clear,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainAction {
    Continue,
    Batch,
    Search,
    Explore,
    Watchlist,
    History,
    Settings,
    Health,
    Update,
    Exit,
}

fn show_main_menu() -> anyhow::Result<Option<MainAction>> {
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
    for (label, action) in [
        ("Cari anime / film".to_string(), MainAction::Search),
        ("Jelajahi anime (AniList)".to_string(), MainAction::Explore),
        (watchlist_label, MainAction::Watchlist),
        ("Riwayat".to_string(), MainAction::History),
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

fn confirm_action(prompt: &str) -> anyhow::Result<bool> {
    Ok(dialoguer::Confirm::with_theme(&crate::util::custom_theme())
        .with_prompt(prompt)
        .default(false)
        .interact_opt()?
        .unwrap_or(false))
}

/// Mark onboarding as handled when the user skips it, so the guide does not
/// reappear on every launch. Defaults already in the config remain unchanged.
fn skip_onboarding(config: &mut config::Config) -> anyhow::Result<()> {
    config.onboarding_done = true;
    config.save();
    Ok(())
}

/// First-run guide. It exists to remove two recurring prompts: provider
/// selection for every title and player selection for every episode.
fn run_onboarding(config: &mut config::Config) -> anyhow::Result<()> {
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

fn choose_player_setting(config: &mut config::Config) -> anyhow::Result<()> {
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

fn choose_auto_next_setting(config: &mut config::Config) -> anyhow::Result<()> {
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

fn choose_download_dir_setting(config: &mut config::Config) -> anyhow::Result<()> {
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

fn choose_provider_setting(config: &mut config::Config) -> anyhow::Result<()> {
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

async fn handle_settings_menu(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
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

/// Return the source checkout only when the current directory is actually this
/// project. Checking for a bare `.git` in the working directory is dangerous:
/// running the installed binary from any other repository would otherwise run
/// `git pull` and `cargo install --path .` inside that unrelated project.
fn local_update_repo() -> Option<std::path::PathBuf> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .ok()?;
    let cwd = std::env::current_dir().ok()?.canonicalize().ok()?;

    if !cwd.starts_with(&manifest) || !manifest.join(".git").exists() {
        return None;
    }

    let cargo_toml = std::fs::read_to_string(manifest.join("Cargo.toml")).ok()?;
    let is_this_package = cargo_toml
        .lines()
        .any(|line| line.trim() == "name = \"animeku-cli\"");
    if !is_this_package {
        return None;
    }

    Some(manifest)
}

fn pause_after_update(wait_for_enter: bool) {
    if !wait_for_enter {
        return;
    }
    println!("\nTekan Enter untuk kembali ke menu...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
}

async fn handle_check_update() -> anyhow::Result<()> {
    println!("Versi terpasang: {}", update::current_version());
    match update::check().await {
        Ok(Some(latest)) => {
            println!("Versi terbaru  : v{latest}");
            println!("Jalankan `animeku-cli --update` untuk memasangnya.");
        }
        Ok(None) => println!("Kamu sudah memakai versi terbaru."),
        Err(error) => println!("Gagal memeriksa pembaruan: {error:#}"),
    }
    Ok(())
}

async fn handle_self_update(wait_for_enter: bool) -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    println!("{} Memeriksa dan mengunduh pembaruan...", "◆".blue());
    println!("  Repository: {}\n", REPO_URL.cyan());

    // Developers running from a checkout expect `git pull` + a local build.
    // Installed binaries use the prebuilt release asset so Rust is optional.
    if local_update_repo().is_some() {
        return handle_source_update(wait_for_enter);
    }

    match update::install_latest().await {
        Ok(update::UpdateOutcome::Updated { from, to, path }) => {
            println!(
                "\n  {} Pembaruan berhasil: v{} -> v{}",
                "✓".green(),
                from,
                to
            );
            println!("  Binary: {}", path.display());
            println!("  Silakan jalankan ulang aplikasi.");
        }
        Ok(update::UpdateOutcome::UpToDate { current, latest }) => {
            println!(
                "\n  {} Sudah versi terbaru (v{}; rilis terbaru v{}).",
                "✓".green(),
                current,
                latest
            );
        }
        Ok(update::UpdateOutcome::Unavailable(reason)) => {
            println!("\n  {} {}.", "ℹ".yellow(), reason);
            println!("  Fallback ke build dari source...");
            return handle_source_update(wait_for_enter);
        }
        Err(error) => {
            println!("\n  {} Update prebuilt gagal: {:#}", "■".red(), error);
            println!("  Fallback ke build dari source...");
            return handle_source_update(wait_for_enter);
        }
    }

    pause_after_update(wait_for_enter);
    Ok(())
}

fn handle_source_update(wait_for_enter: bool) -> anyhow::Result<()> {
    let bin_name = if cfg!(target_os = "windows") {
        "animeku-cli.exe"
    } else {
        "animeku-cli"
    };
    let local_repo = local_update_repo();

    // Check requirements before touching the executable, otherwise an early
    // return on Windows would leave the binary renamed to `.exe.old`.
    if local_repo.is_some() && !check_command_exists("git") {
        println!("\n  {} git tidak ditemukan di PATH.", "■".red());
        println!("  Silakan install git terlebih dahulu, lalu jalankan ulang update ini.");
        pause_after_update(wait_for_enter);
        return Ok(());
    }
    if !check_command_exists("cargo") {
        println!(
            "\n  {} cargo / Rust toolchain tidak ditemukan di PATH.",
            "■".red()
        );
        println!("  Install Rust dari https://rustup.rs/ lalu jalankan ulang update ini.");
        pause_after_update(wait_for_enter);
        return Ok(());
    }

    // On Windows, rename running executable before cargo install to avoid file locking.
    let mut renamed_old: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
    if cfg!(target_os = "windows") {
        if let Ok(current_exe) = std::env::current_exe() {
            let old_exe = current_exe.with_extension("exe.old");
            let _ = std::fs::remove_file(&old_exe);
            if std::fs::rename(&current_exe, &old_exe).is_ok() {
                renamed_old = Some((current_exe, old_exe));
            }
        }
    }

    let status = if let Some(repo) = local_repo.as_ref() {
        println!(
            "{} Terdeteksi source checkout lokal di {}. Menjalankan git pull & cargo install...",
            "ℹ".yellow(),
            repo.display()
        );
        let _ = Command::new("git")
            .args(["pull", "origin", "main"])
            .current_dir(repo)
            .status();
        Command::new("cargo")
            .args(["install", "--path", ".", "--locked", "--force"])
            .current_dir(repo)
            .status()
    } else {
        println!("{} Mengunduh dan mengompilasi dari GitHub...", "◆".blue());
        Command::new("cargo")
            .args(["install", "--git", REPO_URL, "--locked", "--force"])
            .status()
    };

    match status {
        Ok(s) if s.success() => {
            println!("\n  {} Pembaruan berhasil diinstal!", "✓".green());
            if let Some(home) = dirs::home_dir() {
                let local_bin = home.join(".local/bin").join(bin_name);
                let candidate_bins = [
                    home.join(".cargo/bin").join(bin_name),
                    home.join(".local/share/rust-cargo/bin").join(bin_name),
                    std::path::PathBuf::from(format!("target/release/{}", bin_name)),
                    std::path::PathBuf::from(format!("target/debug/{}", bin_name)),
                ];
                for candidate in candidate_bins {
                    if candidate.exists() && local_bin.exists() {
                        let _ = std::fs::copy(&candidate, &local_bin);
                        break;
                    }
                }
            }
            if let Some((_, old_exe)) = renamed_old {
                let _ = std::fs::remove_file(old_exe);
            }
            println!("  Silakan jalankan ulang aplikasi.");
        }
        _ => {
            if let Some((current_exe, old_exe)) = renamed_old {
                let _ = std::fs::rename(old_exe, current_exe);
            }
            println!("\n  {} Gagal memperbarui secara otomatis.", "■".red());
            println!("  Silakan jalankan secara manual:");
            println!("  cargo install --git {} --locked --force", REPO_URL);
        }
    }

    pause_after_update(wait_for_enter);
    Ok(())
}

async fn run_cli_search(
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

async fn handle_search_and_watch(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
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

async fn handle_resume_last_watch(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchlistAction {
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
fn episodes_after_last(
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

async fn handle_watchlist_download_new(
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

    if !confirm_action(&format!(
        "Download {} episode baru ke '{}'?",
        new_episodes.len(),
        directory.display()
    ))? {
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

async fn handle_check_new_episodes(
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

async fn handle_watchlist(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
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

