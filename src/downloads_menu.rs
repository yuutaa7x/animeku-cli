use crate::util::clearscreen_and_show_banner;
use crate::animeku::AnimekuCli;
use crate::health::{check_command_exists, resolve_command_path};
use crate::menu::confirm_action;
use crate::{ config, ext, input };
use colored::Colorize;
use std::process::Command;

pub(crate) async fn handle_batch_manager() -> anyhow::Result<()> {
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

pub(crate) fn downloaded_file_label(file: &crate::download::DownloadedFile) -> String {
    let name = file
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("(tanpa nama)");
    let age = if file.modified > 0 {
        crate::models::format_relative_time(file.modified)
    } else {
        "waktu tidak diketahui".to_string()
    };
    format!(
        "{}  •  {}  •  {}",
        name,
        crate::download::format_bytes(file.size),
        age
    )
}

pub(crate) fn open_download_folder(directory: &std::path::Path) {
    let _ = std::fs::create_dir_all(directory);
    match open::that(directory) {
        Ok(()) => println!("{} Membuka '{}'.", "✓".green(), directory.display()),
        Err(error) => eprintln!(
            "{} Gagal membuka folder '{}': {}",
            "■".red(),
            directory.display(),
            error
        ),
    }
}

pub(crate) fn pause_for_enter() {
    println!("\nTekan Enter untuk kembali...");
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
}

pub(crate) fn handle_downloaded_menu() -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let directory = crate::download::default_download_dir();
        let files = crate::download::downloaded_files_in(&directory);

        println!("Terunduh:");
        println!("  Folder: {}\n", directory.display());

        if files.is_empty() {
            println!("Belum ada file video yang diunduh.\n");
            let entries = vec![
                input::MenuEntry::new("📂 Buka Folder Download"),
                input::MenuEntry::new("Kembali"),
            ];
            let Some(choice) = input::select_menu_simple("Pilih Tindakan:", &entries, 0)? else {
                break;
            };
            if choice == 0 {
                open_download_folder(&directory);
                pause_for_enter();
                continue;
            }
            break;
        }

        let total_size = files.iter().map(|file| file.size).sum::<u64>();
        println!(
            "  {} file • {}\n",
            files.len(),
            crate::download::format_bytes(total_size)
        );

        let mut entries = files
            .iter()
            .map(|file| input::MenuEntry::new(downloaded_file_label(file)))
            .collect::<Vec<_>>();
        entries.push(input::MenuEntry::new("📂 Buka Folder Download"));
        entries.push(input::MenuEntry::new("Kembali"));

        let Some(choice) = input::select_menu_simple("Pilih file yang diunduh:", &entries, 0)?
        else {
            break;
        };
        if choice < files.len() {
            handle_downloaded_file_action(&files[choice])?;
            continue;
        }
        if choice == files.len() {
            open_download_folder(&directory);
            pause_for_enter();
            continue;
        }
        break;
    }
    Ok(())
}

pub(crate) fn handle_downloaded_file_action(file: &crate::download::DownloadedFile) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        let name = file
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("(tanpa nama)");
        println!("File Terunduh:");
        println!("  Nama   : {}", name);
        println!(
            "  Folder : {}",
            file.path
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_else(|| "-".to_string())
        );
        println!("  Ukuran : {}", crate::download::format_bytes(file.size));
        if file.modified > 0 {
            println!(
                "  Diubah : {}",
                crate::models::format_relative_time(file.modified)
            );
        }
        println!();

        let actions = [
            "▶ Putar",
            "📂 Buka Folder",
            "🗑 Hapus File",
            "Kembali",
        ];
        let entries = actions
            .iter()
            .map(|label| input::MenuEntry::new(*label))
            .collect::<Vec<_>>();
        let Some(choice) = input::select_menu_simple("Pilih Tindakan:", &entries, 0)? else {
            return Ok(());
        };

        match choice {
            0 => {
                play_downloaded_file(&file.path)?;
                pause_for_enter();
            }
            1 => {
                let directory = file
                    .path
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(crate::download::default_download_dir);
                open_download_folder(&directory);
                pause_for_enter();
            }
            2 => {
                if confirm_action(&format!("Hapus file '{}'?", name))? {
                    if let Err(error) = std::fs::remove_file(&file.path) {
                        eprintln!(
                            "{} Gagal menghapus '{}': {}",
                            "■".red(),
                            file.path.display(),
                            error
                        );
                        pause_for_enter();
                    } else {
                        let sidecar = file.path.with_extension("vtt");
                        if sidecar.is_file() {
                            let _ = std::fs::remove_file(sidecar);
                        }
                        return Ok(());
                    }
                }
            }
            _ => return Ok(()),
        }
    }
}

pub(crate) fn play_downloaded_file(path: &std::path::Path) -> anyhow::Result<()> {
    use config::PlayerChoice;

    let config = config::Config::load();
    let mut selected = config.effective_player();
    if config.player == PlayerChoice::Ask {
        let labels = ["MPV", "VLC", "Aplikasi default", "Kembali"];
        let entries = labels
            .iter()
            .map(|label| input::MenuEntry::new(*label))
            .collect::<Vec<_>>();
        let Some(choice) = input::select_menu_simple("Putar dengan:", &entries, 0)? else {
            return Ok(());
        };
        selected = match choice {
            0 => PlayerChoice::Mpv,
            1 => PlayerChoice::Vlc,
            2 => PlayerChoice::Browser,
            _ => return Ok(()),
        };
    }

    match selected {
        PlayerChoice::Mpv => {
            if !check_command_exists("mpv") {
                eprintln!("{} MPV tidak ditemukan.", "■".red());
                return Ok(());
            }
            let status = Command::new(resolve_command_path("mpv"))
                .arg(path)
                .status();
            match status {
                Ok(status) if status.success() => {}
                Ok(_) => eprintln!("{} MPV keluar dengan status tidak normal.", "■".red()),
                Err(error) => eprintln!("{} Gagal menjalankan MPV: {}", "■".red(), error),
            }
        }
        PlayerChoice::Vlc => {
            if !check_command_exists("vlc") {
                eprintln!("{} VLC tidak ditemukan.", "■".red());
                return Ok(());
            }
            let status = Command::new(resolve_command_path("vlc"))
                .arg(path)
                .status();
            match status {
                Ok(status) if status.success() => {}
                Ok(_) => eprintln!("{} VLC keluar dengan status tidak normal.", "■".red()),
                Err(error) => eprintln!("{} Gagal menjalankan VLC: {}", "■".red(), error),
            }
        }
        _ => match open::that(path) {
            Ok(()) => println!(
                "{} Membuka '{}' dengan aplikasi default.",
                "✓".green(),
                path.display()
            ),
            Err(error) => eprintln!("{} Gagal membuka file: {}", "■".red(), error),
        },
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchManagerAction {
    Run,
    RetryFailed,
    OpenFolder,
    Clear,
    Back,
}
