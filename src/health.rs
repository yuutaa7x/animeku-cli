use crate::util::clearscreen_and_show_banner;
use crate::{PROVIDER_IDLIX, PROVIDER_OTAKUDESU};
use crate::{ ext };
use colored::Colorize;
use std::process::Command;
use std::io::{ stdout, Write };

pub(crate) fn check_command_exists(cmd: &str) -> bool {
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

pub(crate) fn check_windows_known_paths(cmd: &str) -> bool {
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

pub(crate) fn resolve_command_path(cmd: &str) -> String {
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

pub(crate) fn install_dependencies(package_manager: &str) {
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

pub(crate) fn detect_package_manager() -> &'static str {
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

pub(crate) async fn show_health_screen() -> anyhow::Result<bool> {
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

pub(crate) async fn handle_provider_health() -> anyhow::Result<()> {
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
