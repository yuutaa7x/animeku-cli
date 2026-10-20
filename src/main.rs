use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::io::{stdout, Write};
use std::process::Command;
use animeku::AnimekuCli;
use colored::Colorize;
use ext::otakudesu::{OTAKUDESU_REFERER, OTAKUDESU_USER_AGENT};
use ext::Ext;
use tokio::runtime;

use crate::{input::{get_user_input, load_history, save_history, pick_provider, load_watch_history, delete_watch_entry, delete_search_history_entry}, util::clearscreen_and_show_banner};

mod animeku;
mod episode;
mod ext;
mod input;
mod models;
mod proxy;
mod util;
mod live_input;

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

const PROVIDER_IDLIX: usize = 0;
pub const PROVIDER_OTAKUDESU: usize = 1;

const REPO_URL: &str = "https://github.com/yuutaa7x/animeku-cli";
const MENU_UPDATE: usize = 0;
const MENU_WATCH: usize = 1;
const MENU_HEALTH: usize = 2;
const MENU_EXIT: usize = 3;

fn get_ext_by_provider(provider_type: usize) -> Box<dyn Ext> {
    match provider_type {
        PROVIDER_IDLIX => Box::new(ext::idlix::Idlix::new()),
        PROVIDER_OTAKUDESU => Box::new(ext::otakudesu::Otakudesu::new()),
        _ => Box::new(ext::idlix::Idlix::new()),
    }
}

fn get_ext(input: &crate::models::Input) -> Box<dyn Ext> {
    get_ext_by_provider(input.tipe)
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
                .args(["apt-get", "install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "pacman" => {
            let _ = Command::new("sudo")
                .args(["pacman", "-S", "--noconfirm", "mpv", "yt-dlp"])
                .status();
        },
        "dnf" => {
            let _ = Command::new("sudo")
                .args(["dnf", "install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "zypper" => {
            let _ = Command::new("sudo")
                .args(["zypper", "install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "apk" => {
            let _ = Command::new("sudo")
                .args(["apk", "add", "mpv", "yt-dlp"])
                .status();
        },
        "pkg" => {
            // Termux (Android) — tidak butuh sudo
            println!("  {} Menjalankan: pkg install mpv yt-dlp ...", "ℹ".yellow());
            let _ = Command::new("pkg")
                .args(["install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "brew" => {
            let _ = Command::new("brew")
                .args(["install", "mpv", "yt-dlp"])
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
        },
        "choco" => {
            let choco_bin = resolve_command_path("choco");
            println!("  {} Menjalankan: choco install -y mpv yt-dlp ...", "ℹ".yellow());
            let _ = Command::new(&choco_bin)
                .args(["install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "scoop" => {
            let scoop_bin = resolve_command_path("scoop");
            println!("  {} Menjalankan: scoop install mpv yt-dlp ...", "ℹ".yellow());
            let _ = Command::new(&scoop_bin)
                .args(["install", "mpv", "yt-dlp"])
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

fn show_health_screen() -> anyhow::Result<bool> {
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
        
        println!("  {}  Native Rust Scraper (Idlix Animation)", "[✓ BUILT-IN ]".bright_cyan());
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
                if cfg!(target_os = "windows") {
                    println!("\n{} Tidak ditemukan package manager otomatis (winget / choco / scoop).", "■".red());
                    println!("  Silahkan pilih salah satu opsi instalasi MPV untuk Windows:");
                    println!("  1. Jalankan di PowerShell / CMD:");
                    println!("     {}", "winget install --id shinchiro.mpv --source winget".cyan());
                    println!("  2. Atau install via Scoop:");
                    println!("     {}", "scoop install mpv".cyan());
                    println!("  3. Atau unduh manual installer MPV di:");
                    println!("     {}", "https://mpv.io/installation/".cyan());
                    println!("\n  Ingin membuka halaman unduhan MPV di browser? (y/n)");
                    let mut answer = String::new();
                    if std::io::stdin().read_line(&mut answer).is_ok() && answer.trim().eq_ignore_ascii_case("y") {
                        let _ = open::that("https://mpv.io/installation/");
                    }
                } else {
                    println!("\n{} Package manager tidak terdeteksi! Silahkan install mpv dan yt-dlp secara manual.", "■".red());
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

async fn execute_player(stream_url: &str, provider_type: usize, start_seconds: u64) -> anyhow::Result<u64> {
    println!("{} Membuka tautan diaplikasi eksternal .. ", "◆".blue());
    stdout().flush()?;

    let is_android = std::env::var("TERMUX_VERSION").is_ok();

    let menu_items = if is_android {
        vec!["Putar di VLC / Pemutar Android", "Buka di Browser", "Kembali"]
    } else {
        vec!["Putar dengan MPV", "Putar dengan VLC", "Buka di Browser", "Kembali"]
    };

    let player_choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih Aksi:")
        .default(0)
        .items(&menu_items)
        .interact()?;

    let choice_normalized = if is_android {
        match player_choice {
            0 => 1, // Map to VLC / Android Native
            1 => 2, // Map to Browser
            _ => 3, // Map to Kembali
        }
    } else {
        player_choice
    };

    let mut final_pos = start_seconds;

    match choice_normalized {
        0 => {
            println!("Sedang memutar video di MPV... (Tutup MPV untuk kembali ke menu)");

            let track_script = mpv_track_script_path();
            let last_pos_file = mpv_last_pos_file_path();

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
"#,
                start_seconds, last_pos_file
            );
            let _ = std::fs::write(&track_script, lua_script);
            let _ = std::fs::remove_file(&last_pos_file);

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
                if std::path::Path::new(&sub_file).exists() {
                    cmd.arg(format!("--sub-file={}", sub_file));
                }
            } else if provider_type == PROVIDER_OTAKUDESU {
                cmd.arg(format!("--http-header-fields=Referer: {}", OTAKUDESU_REFERER));
                cmd.arg(format!("--user-agent={}", OTAKUDESU_USER_AGENT));
                // Otakudesu gives direct MP4/Google Video URLs. Let mpv handle
                // them natively instead of invoking yt-dlp's generic extractor,
                // which fails with 403 on these signed URLs.
                cmd.arg("--ytdl=no");
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

            if let Ok(saved) = std::fs::read_to_string(&last_pos_file) {
                if let Ok(pos) = saved.trim().parse::<u64>() {
                    final_pos = pos;
                }
            }
            let _ = std::fs::remove_file(&last_pos_file);
            let _ = std::fs::remove_file(&track_script);
        },
        1 => {
            if is_android {
                println!("Membuka aplikasi video player bawaan Android (VLC)...");
                
                // Simpan subtitle ke folder Download agar user bisa nge-load manual
                let sub_path = crate::util::get_subtitle_path();
                if std::path::Path::new(&sub_path).exists() && sub_path.contains("Download") {
                    println!("{} Subtitle disimpan di folder Download (animeku_idlix.vtt). Silahkan load manual di VLC.", "ℹ".yellow());
                }

                // Kita nyalakan Local HTTP Proxy untuk memanipulasi Header!
                // Handle ini menjaga listener tetap hidup selama playback dan
                // otomatis menghentikannya saat keluar dari blok ini.
                let proxy = match crate::proxy::start_proxy(provider_type).await {
                    Ok(handle) => handle,
                    Err(e) => {
                        eprintln!("{} Gagal menjalankan proxy lokal: {}", "■".red(), e);
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        return Ok(final_pos);
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
                    println!("Proxy berjalan di port {}. Silahkan tonton di aplikasi pilihanmu!", proxy_port);
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
                if let Ok(status) = cmd.arg(stream_url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() {
                    if !status.success() { eprintln!("{} Gagal memutar video di VLC", "■".red()); }
                } else { eprintln!("{} VLC tidak ditemukan!", "■".red()); }
                
                print!("Simpan posisi terakhir tontonan (dalam menit)? (Kosongkan jika sudah tamat): ");
                stdout().flush().unwrap_or(());
                let mut pos_input = String::new();
                let _ = std::io::stdin().read_line(&mut pos_input);
                if let Ok(m) = pos_input.trim().parse::<u64>() {
                    final_pos = m * 60;
                } else if pos_input.trim().is_empty() {
                    final_pos = 0;
                }
            }
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
    Ok(final_pos)
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

async fn handle_movie_episodes(
    animeku: &mut AnimekuCli,
    movie: &crate::models::Movie,
    provider_type: usize,
    discord: &mut Option<DiscordIpcClient>,
) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let mut episode = match animeku.extract_episode(movie.clone()).await? {
            Some(ep) => ep,
            None => break, // Back to movie search
        };

        loop {
            let season_num = detect_season_for_episode(animeku, movie, &episode);

            // Stream selection & playback
            clearscreen_and_show_banner()?;
            let meta_opt = animeku.get_meta(&movie.id);
            let thumb_url = meta_opt.and_then(|m| m.thumb_url);
            
            update_discord_status(
                discord,
                &movie.title,
                &episode.title,
                episode.is_series,
                season_num,
                thumb_url,
            )
            .await;

            let stream_opt = animeku.extract_stream_urls(episode.clone()).await?;
            if let Some(stream) = stream_opt {
                let last_pos = execute_player(&stream.url, provider_type, 0).await?;
                input::save_watch_entry(&crate::models::WatchEntry {
                    movie: movie.clone(),
                    episode: episode.clone(),
                    provider_type,
                    position_seconds: last_pos,
                    updated_at: 0,
                });
            } else {
                reset_discord_status(discord);
                break;
            }
            
            reset_discord_status(discord);

            // Auto-next logic: follow the provider's ordered episode list so
            // the last episode of a season advances to the next season.
            let next_ep_opt = animeku
                .get_episode_list(&movie.id)
                .and_then(|episodes| find_next_episode(&episodes, &episode).cloned());

            if let Some(next_ep) = next_ep_opt {
                let confirm = dialoguer::Confirm::with_theme(&crate::util::custom_theme())
                    .with_prompt(format!("Lanjut ke {}?", next_ep.title.trim()))
                    .default(true)
                    // `default(true)` membuat dialoguer menampilkan kata "yes"
                    // sebagai hint sebelum user menjawab. Sembunyikan hint itu,
                    // tapi Enter tetap berarti "ya".
                    .show_default(false)
                    .interact()?;
                
                if confirm {
                    episode = next_ep;
                    continue; // Loop play again with next episode
                }
            }
            
            // If no next episode, or user declined, break back to episode selection
            break;
        }
    }
    Ok(())
}

fn show_main_menu() -> anyhow::Result<usize> {
    let items = [
        "1. Update Animeku-CLI",
        "2. Go Watch Anime / Movie",
        "3. Check Health",
        "4. Exit",
    ];

    let choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih Menu:")
        .default(MENU_WATCH)
        .items(&items)
        .interact()?;

    Ok(choice)
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

fn handle_self_update() -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    println!("{} Memeriksa dan mengunduh pembaruan...", "◆".blue());
    println!("  Repository: {}\n", REPO_URL.cyan());

    let bin_name = if cfg!(target_os = "windows") { "animeku-cli.exe" } else { "animeku-cli" };
    let local_repo = local_update_repo();

    // Check requirements before touching the executable, otherwise an early
    // return on Windows would leave the binary renamed to `.exe.old`.
    if local_repo.is_some() && !check_command_exists("git") {
        println!("\n  {} git tidak ditemukan di PATH.", "■".red());
        println!("  Silahkan install git terlebih dahulu, lalu jalankan ulang update ini.");
        println!("\nTekan Enter untuk kembali ke menu...");
        let mut dummy = String::new();
        let _ = std::io::stdin().read_line(&mut dummy);
        return Ok(());
    }
    if !check_command_exists("cargo") {
        println!("\n  {} cargo / Rust toolchain tidak ditemukan di PATH.", "■".red());
        println!("  Install Rust dari https://rustup.rs/ lalu jalankan ulang update ini.");
        println!("\nTekan Enter untuk kembali ke menu...");
        let mut dummy = String::new();
        let _ = std::io::stdin().read_line(&mut dummy);
        return Ok(());
    }

    // On Windows, rename running executable before cargo install to avoid file locking
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
        println!("{} Terdeteksi source checkout lokal di {}. Menjalankan git pull & cargo install...", "ℹ".yellow(), repo.display());
        let _ = Command::new("git").args(["pull", "origin", "main"]).current_dir(repo).status();
        Command::new("cargo")
            .args(["install", "--path", ".", "--force"])
            .current_dir(repo)
            .status()
    } else {
        println!("{} Mengunduh dan mengompilasi dari GitHub...", "◆".blue());
        Command::new("cargo")
            .args(["install", "--git", REPO_URL, "--force"])
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
            println!("  Silahkan jalankan ulang aplikasi.");
        }
        _ => {
            if let Some((current_exe, old_exe)) = renamed_old {
                let _ = std::fs::rename(old_exe, current_exe);
            }
            println!("\n  {} Gagal memperbarui secara otomatis.", "■".red());
            println!("  Silahkan jalankan secara manual:");
            println!("  cargo install --git {} --force", REPO_URL);
        }
    }

    println!("\nTekan Enter untuk kembali ke menu...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
    Ok(())
}

async fn handle_search_and_watch(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let input = match get_user_input().await? {
            Some(user_input) => user_input,
            None => break, // Back to watch submenu
        };

        let extractor = get_ext(&input);
        let mut animeku = AnimekuCli::new(extractor);

        loop {
            clearscreen_and_show_banner()?;
            let movie = match animeku.search(&input.title).await? {
                Some(selected_movie) => selected_movie,
                None => break, // Back to input prompt
            };

            handle_movie_episodes(&mut animeku, &movie, input.tipe, discord).await?;
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
            let provider_name = if lw.provider_type == PROVIDER_OTAKUDESU { "Otakudesu" } else { "Idlix" };
            menu_items.push(input::MenuEntry::new(format!(
                "[ Lanjutkan: {} - {} | {} | {} ]",
                crate::models::clean_movie_title(&lw.movie.title),
                crate::models::clean_episode_title(&lw.episode.title),
                lw.format_duration(),
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
                "─── Search History ───────────────────────────",
            ));
            Some(idx)
        } else {
            None
        };

        for title in &search_history {
            menu_items.push(input::MenuEntry::new(format!("  {}", title)));
        }
        menu_items.push(input::MenuEntry::new("  Kembali"));

        let Some(choice) = input::select_menu("Watch & Search History:", &menu_items, 0)? else {
            break;
        };

        let back_index = menu_items.len() - 1;
        if choice == back_index {
            break;
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
            println!("  Posisi   : {}", selected_watch.format_duration());
            let provider_name = if selected_watch.provider_type == PROVIDER_OTAKUDESU { "Otakudesu" } else { "Idlix" };
            println!("  Provider : {}\n", provider_name);

            let actions = [
                format!("1. Lanjutkan Menonton (mulai {})", selected_watch.format_duration()),
                "2. Putar Ulang dari Awal (00:00)".to_string(),
                "3. Hapus dari Histori".to_string(),
                "4. Kembali".to_string(),
            ];

            let action_choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
                .with_prompt("Pilih Aksi:")
                .default(0)
                .items(&actions)
                .interact()?;

            if action_choice == 2 {
                delete_watch_entry(choice);
                continue;
            } else if action_choice == 3 {
                continue;
            }

            let start_seconds = if action_choice == 0 { selected_watch.position_seconds } else { 0 };

            clearscreen_and_show_banner()?;

            let season_num = if selected_watch.episode.is_series {
                detect_season_from_titles(
                    &selected_watch.movie.title,
                    &selected_watch.episode.title,
                )
                .unwrap_or(1)
            } else {
                1
            };

            let mut animeku = AnimekuCli::new(get_ext_by_provider(selected_watch.provider_type));
            let thumb_url = animeku
                .ensure_meta(&selected_watch.movie)
                .await
                .and_then(|meta| meta.thumb_url);

            update_discord_status(
                discord,
                &display_title,
                &display_episode,
                selected_watch.episode.is_series,
                season_num,
                thumb_url,
            )
            .await;

            let stream_opt = animeku.extract_stream_urls(selected_watch.episode.clone()).await?;
            if let Some(stream) = stream_opt {
                let final_pos = execute_player(&stream.url, selected_watch.provider_type, start_seconds).await?;
                input::save_watch_entry(&crate::models::WatchEntry {
                    movie: selected_watch.movie.clone(),
                    episode: selected_watch.episode.clone(),
                    provider_type: selected_watch.provider_type,
                    position_seconds: final_pos,
                    updated_at: 0,
                });
            }
            reset_discord_status(discord);

        } else {
            // Picked from Search History
            let separator_offset = if separator_index.is_some() { 1 } else { 0 };
            let history_index = choice - watch_count - separator_offset;
            let selected_title = search_history[history_index].clone();

            let actions = [
                "1. Cari Judul Ini".to_string(),
                "2. Hapus dari Histori".to_string(),
                "3. Kembali".to_string(),
            ];

            clearscreen_and_show_banner()?;
            println!("Judul Pencarian: {}\n", selected_title);

            let action_choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
                .with_prompt("Pilih Aksi:")
                .default(0)
                .items(&actions)
                .interact()?;

            if action_choice == 1 {
                delete_search_history_entry(&selected_title);
                continue;
            } else if action_choice == 2 {
                continue;
            }

            let tipe = pick_provider(&selected_title, false).await?;
            save_history(&selected_title);

            let input_data = crate::models::Input { title: selected_title, tipe };
            let extractor = get_ext(&input_data);
            let mut animeku = AnimekuCli::new(extractor);

            loop {
                clearscreen_and_show_banner()?;
                let movie = match animeku.search(&input_data.title).await? {
                    Some(m) => m,
                    None => break,
                };
                handle_movie_episodes(&mut animeku, &movie, input_data.tipe, discord).await?;
            }
        }
    }
    Ok(())
}

async fn handle_watch_mode(discord: &mut Option<DiscordIpcClient>) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let menu_items = [
            "1. Watch Any Anime / Movie",
            "2. Watch Last Anime / Movie [Watch & Search History]",
            "3. Kembali ke Menu Utama",
        ];

        let choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
            .with_prompt("Pilih Mode Menonton:")
            .default(0)
            .items(&menu_items)
            .interact()?;

        match choice {
            0 => {
                handle_search_and_watch(discord).await?;
            }
            1 => {
                handle_resume_last_watch(discord).await?;
            }
            _ => break,
        }
    }
    Ok(())
}

async fn run_app() -> anyhow::Result<()> {
    crate::util::cleanup_temp_files();

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
        let menu_choice = show_main_menu()?;

        match menu_choice {
            MENU_UPDATE => {
                handle_self_update()?;
            }
            MENU_WATCH => {
                handle_watch_mode(&mut discord).await?;
            }
            MENU_HEALTH => {
                let _ = show_health_screen()?;
            }
            MENU_EXIT => {
                crate::util::cleanup_temp_files();
                println!("\nSampai jumpa lagi!\n");
                return Ok(());
            }
            _ => break,
        }
    }

    crate::util::cleanup_temp_files();
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let rt = runtime::Builder::new_multi_thread().enable_all().build()?;
    if let Err(e) = rt.block_on(run_app()) {
        eprintln!(" {} {}\n", "■".red(), format!("{:#?}", e).yellow());
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

