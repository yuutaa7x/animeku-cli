use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::io::{stdout, Write};
use std::process::Command;
use animeku::AnimekuCli;
use colored::Colorize;
use ext::Ext;
use tokio::runtime;

use crate::{input::{get_user_input, load_history, save_history}, util::clearscreen_and_show_banner};

mod animeku;
mod ext;
mod input;
mod models;
mod util;

const DISCORD_APP_ID: &str = "1549897147577667784";
const DEFAULT_DISCORD_LOGO: &str = "logoanimekucli";
fn idlix_audio_path() -> String {
    crate::util::temp_file("animeku_idlix_audio.m3u8")
}

fn idlix_subtitle_path() -> String {
    crate::util::temp_file("animeku_idlix.vtt")
}

fn mpv_track_script_path() -> String {
    crate::util::temp_file("animeku_track.lua")
}

fn mpv_last_pos_file_path() -> String {
    crate::util::temp_file("animeku_last_pos.txt")
}

const PROVIDER_IDLIX: usize = 0;
const PROVIDER_OTAKUDESU: usize = 1;

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

    if cfg!(target_os = "windows") {
        if check_windows_known_paths(cmd) {
            return true;
        }
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
                .args(&["apt-get", "install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "pacman" => {
            let _ = Command::new("sudo")
                .args(&["pacman", "-S", "--noconfirm", "mpv", "yt-dlp"])
                .status();
        },
        "dnf" => {
            let _ = Command::new("sudo")
                .args(&["dnf", "install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "zypper" => {
            let _ = Command::new("sudo")
                .args(&["zypper", "install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "apk" => {
            let _ = Command::new("sudo")
                .args(&["apk", "add", "mpv", "yt-dlp"])
                .status();
        },
        "pkg" => {
            // Termux (Android) — tidak butuh sudo
            println!("  {} Menjalankan: pkg install mpv yt-dlp ...", "ℹ".yellow());
            let _ = Command::new("pkg")
                .args(&["install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "brew" => {
            let _ = Command::new("brew")
                .args(&["install", "mpv", "yt-dlp"])
                .status();
        },
        "winget" => {
            let winget_bin = resolve_command_path("winget");
            println!("  {} Menjalankan: winget install --id shinchiro.mpv --source winget ...", "ℹ".yellow());
            let status1 = Command::new(&winget_bin)
                .args(&["install", "--id", "shinchiro.mpv", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
                .status();

            let mpv_ok = match status1 {
                Ok(s) if s.success() => true,
                _ => {
                    println!("  {} Mencoba alternatif: winget install --id mpv.net --source winget ...", "ℹ".yellow());
                    let s2 = Command::new(&winget_bin)
                        .args(&["install", "--id", "mpv.net", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
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
                .args(&["install", "--id", "yt-dlp.yt-dlp", "-e", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements"])
                .status();
        },
        "choco" => {
            let choco_bin = resolve_command_path("choco");
            println!("  {} Menjalankan: choco install -y mpv yt-dlp ...", "ℹ".yellow());
            let _ = Command::new(&choco_bin)
                .args(&["install", "-y", "mpv", "yt-dlp"])
                .status();
        },
        "scoop" => {
            let scoop_bin = resolve_command_path("scoop");
            println!("  {} Menjalankan: scoop install mpv yt-dlp ...", "ℹ".yellow());
            let _ = Command::new(&scoop_bin)
                .args(&["install", "mpv", "yt-dlp"])
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

fn execute_player(stream_url: &str, provider_type: usize, start_seconds: u64) -> anyhow::Result<u64> {
    print!("{} Membuka tautan diaplikasi eksternal .. \n", "◆".blue());
    stdout().flush()?;

    let player_choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
        .with_prompt("Pilih Aksi:")
        .default(0)
        .items(&["Putar dengan MPV", "Putar dengan VLC", "Buka di Browser", "Kembali"])
        .interact()?;

    let mut final_pos = start_seconds;

    match player_choice {
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

            if let Ok(saved) = std::fs::read_to_string(&last_pos_file) {
                if let Ok(pos) = saved.trim().parse::<u64>() {
                    final_pos = pos;
                }
            }
            let _ = std::fs::remove_file(&last_pos_file);
            let _ = std::fs::remove_file(&track_script);
        },
        1 => {
            println!("Sedang memutar video di VLC... (Tutup VLC untuk kembali ke menu)");
            let vlc_bin = resolve_command_path("vlc");
            let mut cmd = Command::new(&vlc_bin);
            if start_seconds > 0 {
                cmd.arg(format!("--start-time={}", start_seconds));
            }
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
    Ok(final_pos)
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
            let last_pos = execute_player(&stream.url, provider_type, 0)?;
            input::save_last_watch(&crate::models::LastWatch {
                movie: movie.clone(),
                episode: episode.clone(),
                provider_type,
                position_seconds: last_pos,
            });
        }
        
        reset_discord_status(discord);
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

fn handle_self_update() -> anyhow::Result<()> {
    clearscreen_and_show_banner()?;
    println!("{} Memeriksa dan mengunduh pembaruan...", "◆".blue());
    println!("  Repository: {}\n", REPO_URL.cyan());

    let bin_name = if cfg!(target_os = "windows") { "animeku-cli.exe" } else { "animeku-cli" };
    let is_git_repo = std::path::Path::new(".git").exists();

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

    let status = if is_git_repo {
        if !check_command_exists("git") {
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
        println!("{} Terdeteksi repositori lokal. Menjalankan git pull & cargo install...", "ℹ".yellow());
        let _ = Command::new("git").args(["pull", "origin", "main"]).status();
        Command::new("cargo")
            .args(["install", "--path", ".", "--force"])
            .status()
    } else {
        if !check_command_exists("cargo") {
            println!("\n  {} cargo / Rust toolchain tidak ditemukan di PATH.", "■".red());
            println!("  Install Rust dari https://rustup.rs/ lalu jalankan ulang update ini.");
            println!("\nTekan Enter untuk kembali ke menu...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        }
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
        let input = match get_user_input()? {
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

        let last_watch_opt = input::load_last_watch();
        let history = load_history();

        // Build menu: last watch resume (if any) + all history titles + back
        let mut menu_items: Vec<String> = Vec::new();

        if let Some(ref lw) = last_watch_opt {
            let provider_name = if lw.provider_type == PROVIDER_OTAKUDESU { "Otakudesu" } else { "Idlix" };
            menu_items.push(format!(
                "[ Lanjutkan: {} - {} | {} | {} ]",
                lw.movie.title.trim(),
                lw.episode.title.trim(),
                lw.format_duration(),
                provider_name
            ));
        }

        if history.is_empty() && last_watch_opt.is_none() {
            println!("Belum ada riwayat tontonan.\n");
            println!("Tekan Enter untuk kembali...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
            return Ok(());
        }

        // Separator + Search History section (only show if there's history)
        let separator_index: Option<usize> = if !history.is_empty() {
            let idx = menu_items.len();
            menu_items.push("─── Search History ───────────────────────────".to_string());
            Some(idx)
        } else {
            None
        };

        for title in &history {
            menu_items.push(format!("  {}", title));
        }
        menu_items.push("  Kembali".to_string());

        let choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
            .with_prompt("Watch History:")
            .default(0)
            .items(&menu_items)
            .interact()?;

        // Klik separator = no-op, loop lagi
        if Some(choice) == separator_index {
            continue;
        }

        let last_watch_offset = if last_watch_opt.is_some() { 1 } else { 0 };
        // separator takes 1 slot if shown
        let separator_offset = if separator_index.is_some() { 1 } else { 0 };
        let back_index = menu_items.len() - 1;

        if choice == back_index {
            break;
        }


        // Pilih lanjutkan last watch
        if last_watch_opt.is_some() && choice == 0 {
            let last_watch = last_watch_opt.unwrap();
            clearscreen_and_show_banner()?;

            println!("Riwayat Tontonan Terakhir:");
            println!("  Judul    : {}", last_watch.movie.title.trim());
            println!("  Episode  : {}", last_watch.episode.title.trim());
            println!("  Posisi   : {}", last_watch.format_duration());
            let provider_name = if last_watch.provider_type == PROVIDER_OTAKUDESU { "Otakudesu" } else { "Idlix" };
            println!("  Provider : {}\n", provider_name);

            let actions = [
                format!("1. Lanjutkan Menonton (mulai {})", last_watch.format_duration()),
                "2. Putar Ulang dari Awal (00:00)".to_string(),
                "3. Kembali".to_string(),
            ];

            let action_choice = dialoguer::Select::with_theme(&crate::util::custom_theme())
                .with_prompt("Pilih Aksi:")
                .default(0)
                .items(&actions)
                .interact()?;

            let start_seconds = match action_choice {
                0 => last_watch.position_seconds,
                1 => 0,
                _ => continue,
            };

            clearscreen_and_show_banner()?;

            let season_num = if last_watch.episode.is_series {
                detect_season(&last_watch.movie.title, "", &[]).unwrap_or(1)
            } else {
                1
            };

            update_discord_status(
                discord,
                &last_watch.movie.title,
                &last_watch.episode.title,
                last_watch.episode.is_series,
                season_num,
                None,
            );

            let mut animeku = AnimekuCli::new(get_ext_by_provider(last_watch.provider_type));
            let stream_opt = animeku.extract_stream_urls(last_watch.episode.clone()).await?;
            if let Some(stream) = stream_opt {
                let final_pos = execute_player(&stream.url, last_watch.provider_type, start_seconds)?;
                input::save_last_watch(&crate::models::LastWatch {
                    movie: last_watch.movie.clone(),
                    episode: last_watch.episode.clone(),
                    provider_type: last_watch.provider_type,
                    position_seconds: final_pos,
                });
            }

            reset_discord_status(discord);

        } else {
            // Pilih dari history — langsung search judul tersebut
            let history_index = choice - last_watch_offset - separator_offset;
            let selected_title = history[history_index].clone();

            // Pilih provider
            let tipe = dialoguer::FuzzySelect::with_theme(&crate::util::custom_theme())
                .with_prompt(format!("Provider untuk \"{}\":", selected_title))
                .item("Idlix (Animation & Movies)").item("Otakudesu")
                .default(0)
                .interact()?;

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
                println!("\nSampai jumpa lagi!\n");
                return Ok(());
            }
            _ => break,
        }
    }

    Ok(())
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
}

