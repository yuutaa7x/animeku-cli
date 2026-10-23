use crate::util::clearscreen_and_show_banner;
use crate::health::check_command_exists;
use crate::REPO_URL;
use crate::{ update };
use colored::Colorize;
use std::process::Command;

/// Mark onboarding as handled when the user skips it, so the guide does not
/// reappear on every launch. Defaults already in the config remain unchanged.
/// First-run guide. It exists to remove two recurring prompts: provider
/// selection for every title and player selection for every episode.
/// Return the source checkout only when the current directory is actually this
/// project. Checking for a bare `.git` in the working directory is dangerous:
/// running the installed binary from any other repository would otherwise run
/// `git pull` and `cargo install --path .` inside that unrelated project.
pub(crate) fn local_update_repo() -> Option<std::path::PathBuf> {
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

pub(crate) fn pause_after_update(wait_for_enter: bool) {
    if !wait_for_enter {
        return;
    }
    println!("\nTekan Enter untuk kembali ke menu...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
}

pub(crate) async fn handle_check_update() -> anyhow::Result<()> {
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

pub(crate) async fn handle_self_update(wait_for_enter: bool) -> anyhow::Result<()> {
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

pub(crate) fn handle_source_update(wait_for_enter: bool) -> anyhow::Result<()> {
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
