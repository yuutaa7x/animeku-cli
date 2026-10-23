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

struct LocalGroup {
    title: String,
    episodes: Vec<crate::download::DownloadedFile>,
}

/// Batch downloads name files `Judul - Episode N.ext`. Anything else becomes a
/// one-file group so hand-copied videos still show up.
fn split_series_name(stem: &str) -> Option<(&str, &str)> {
    // Split on the first separator: batch names look like
    // `Judul - S1E06 - Judul Episode`, so the episode number is in the middle.
    let (title, rest) = stem.split_once(" - ")?;
    let is_episode = rest.to_ascii_lowercase().starts_with("episode")
        || crate::episode::parse_season_episode(rest).is_some();
    if !is_episode {
        return None;
    }
    Some((title.trim(), rest.trim()))
}

fn file_stem(file: &crate::download::DownloadedFile) -> &str {
    file.path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
}

fn series_title(file: &crate::download::DownloadedFile) -> String {
    if let Some(series) = &file.series {
        return series.clone();
    }
    let stem = file_stem(file);
    split_series_name(stem)
        .map(|(title, _)| title.to_string())
        .unwrap_or_else(|| stem.to_string())
}

fn episode_sort_key(file: &crate::download::DownloadedFile) -> (u32, u32) {
    let stem = file_stem(file);
    let candidate = if file.series.is_some() {
        stem
    } else {
        split_series_name(stem).map(|(_, rest)| rest).unwrap_or(stem)
    };
    if let Some((season, episode)) = crate::episode::parse_season_episode(candidate) {
        return (season, episode);
    }
    (1, crate::episode::episode_number(candidate).unwrap_or(0))
}

fn local_groups(files: &[crate::download::DownloadedFile]) -> Vec<LocalGroup> {
    let mut groups: Vec<LocalGroup> = Vec::new();
    for file in files {
        let title = series_title(file);
        match groups
            .iter_mut()
            .find(|group| group.title.eq_ignore_ascii_case(&title))
        {
            Some(group) => group.episodes.push(file.clone()),
            None => groups.push(LocalGroup {
                title,
                episodes: vec![file.clone()],
            }),
        }
    }
    for group in &mut groups {
        group.episodes.sort_by(|left, right| {
            episode_sort_key(left)
                .cmp(&episode_sort_key(right))
                .then_with(|| left.path.cmp(&right.path))
        });
    }
    groups
}

fn position_path(video: &std::path::Path) -> std::path::PathBuf {
    let name = video
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    video.with_file_name(format!("{name}.pos"))
}

fn read_position(video: &std::path::Path) -> u64 {
    std::fs::read_to_string(position_path(video))
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

fn local_group_label(group: &LocalGroup) -> String {
    let total_size = group.episodes.iter().map(|file| file.size).sum::<u64>();
    let newest = group
        .episodes
        .iter()
        .map(|file| file.modified)
        .max()
        .unwrap_or(0);
    let age = if newest > 0 {
        crate::models::format_relative_time(newest)
    } else {
        "waktu tidak diketahui".to_string()
    };
    if group.episodes.len() > 1 {
        format!(
            "{}  •  {} episode  •  {}  •  {}",
            group.title,
            group.episodes.len(),
            crate::download::format_bytes(total_size),
            age
        )
    } else {
        format!(
            "{}  •  {}  •  {}",
            group.title,
            crate::download::format_bytes(total_size),
            age
        )
    }
}

fn local_episode_name(file: &crate::download::DownloadedFile) -> String {
    let stem = file_stem(file);
    if let Some(series) = &file.series {
        if let Some(rest) = stem.strip_prefix(&format!("{series} - ")) {
            return rest.to_string();
        }
    }
    match split_series_name(stem) {
        Some((_, rest)) => rest.to_string(),
        None if stem.is_empty() => "(tanpa nama)".to_string(),
        None => stem.to_string(),
    }
}

fn local_episode_label(file: &crate::download::DownloadedFile) -> String {
    let position = read_position(&file.path);
    let state = if position > 0 {
        format!("▶ {}", crate::models::format_seconds(position))
    } else {
        "baru".to_string()
    };
    format!(
        "{}  •  {}  •  {}",
        local_episode_name(file),
        crate::download::format_bytes(file.size),
        state
    )
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

        let groups = local_groups(&files);
        let total_size = files.iter().map(|file| file.size).sum::<u64>();
        println!(
            "  {} judul • {} file • {}\n",
            groups.len(),
            files.len(),
            crate::download::format_bytes(total_size)
        );

        let mut entries = groups
            .iter()
            .map(|group| input::MenuEntry::new(local_group_label(group)))
            .collect::<Vec<_>>();
        entries.push(input::MenuEntry::new("📂 Buka Folder Download"));
        entries.push(input::MenuEntry::new("Kembali"));

        let Some(choice) = input::select_menu_simple("Pilih judul yang diunduh:", &entries, 0)?
        else {
            break;
        };
        if choice < groups.len() {
            // Deletion makes `groups` stale, so a reload happens on the next
            // loop iteration.
            handle_local_group(&groups[choice])?;
            continue;
        }
        if choice == groups.len() {
            open_download_folder(&directory);
            pause_for_enter();
            continue;
        }
        break;
    }
    Ok(())
}

fn handle_local_group(group: &LocalGroup) -> anyhow::Result<()> {
    loop {
        clearscreen_and_show_banner()?;
        println!("{}", group.title);
        println!(
            "  {} episode • {}\n",
            group.episodes.len(),
            crate::download::format_bytes(group.episodes.iter().map(|file| file.size).sum())
        );

        let mut entries = group
            .episodes
            .iter()
            .map(|file| input::MenuEntry::new(local_episode_label(file)))
            .collect::<Vec<_>>();
        entries.push(input::MenuEntry::new("📂 Buka Folder"));
        entries.push(input::MenuEntry::new("Kembali"));

        let Some(choice) = input::select_menu_simple("Pilih episode:", &entries, 0)? else {
            return Ok(());
        };
        if choice < group.episodes.len() {
            if handle_local_episode(group, choice)? {
                return Ok(());
            }
            continue;
        }
        if choice == group.episodes.len() {
            let directory = group
                .episodes
                .first()
                .and_then(|file| file.path.parent())
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(crate::download::default_download_dir);
            open_download_folder(&directory);
            pause_for_enter();
            continue;
        }
        return Ok(());
    }
}

/// Returns `true` when the file was deleted, so the caller reloads.
fn handle_local_episode(group: &LocalGroup, index: usize) -> anyhow::Result<bool> {
    let file = &group.episodes[index];
    loop {
        clearscreen_and_show_banner()?;
        let position = read_position(&file.path);
        println!("{} — {}", group.title, local_episode_name(file));
        println!("  Ukuran : {}", crate::download::format_bytes(file.size));
        if position > 0 {
            println!(
                "  Terakhir: {}",
                crate::models::format_seconds(position)
            );
        }
        println!();

        let actions = [
            "▶ Putar",
            "↺ Putar dari awal",
            "📂 Buka Folder",
            "🗑 Hapus File",
            "Kembali",
        ];
        let entries = actions
            .iter()
            .map(|label| input::MenuEntry::new(*label))
            .collect::<Vec<_>>();
        let Some(choice) = input::select_menu_simple("Pilih Tindakan:", &entries, 0)? else {
            return Ok(false);
        };

        match choice {
            0 | 1 => {
                let Some(player) = choose_local_player()? else {
                    continue;
                };
                if choice == 1 {
                    let _ = std::fs::remove_file(position_path(&file.path));
                }
                watch_local_group(group, index, player)?;
                pause_for_enter();
            }
            2 => {
                let directory = file
                    .path
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(crate::download::default_download_dir);
                open_download_folder(&directory);
                pause_for_enter();
            }
            3 => {
                if confirm_action(&format!("Hapus file '{}'?", local_episode_name(file)))? {
                    if let Err(error) = std::fs::remove_file(&file.path) {
                        eprintln!(
                            "{} Gagal menghapus '{}': {}",
                            "■".red(),
                            file.path.display(),
                            error
                        );
                        pause_for_enter();
                    } else {
                        let _ = std::fs::remove_file(position_path(&file.path));
                        let sidecar = file.path.with_extension("vtt");
                        if sidecar.is_file() {
                            let _ = std::fs::remove_file(sidecar);
                        }
                        return Ok(true);
                    }
                }
            }
            _ => return Ok(false),
        }
    }
}

fn choose_local_player() -> anyhow::Result<Option<config::PlayerChoice>> {
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
            return Ok(None);
        };
        selected = match choice {
            0 => PlayerChoice::Mpv,
            1 => PlayerChoice::Vlc,
            2 => PlayerChoice::Browser,
            _ => return Ok(None),
        };
    }
    Ok(Some(selected))
}

/// Plays the picked episode and follows `n`/auto-next within the same folder.
fn watch_local_group(
    group: &LocalGroup,
    start_index: usize,
    player: config::PlayerChoice,
) -> anyhow::Result<()> {
    let auto_next = config::Config::load().auto_next;
    let mut index = start_index;
    loop {
        let file = &group.episodes[index];
        let start = read_position(&file.path);
        if start > 0 {
            println!(
                "  {} Melanjutkan dari {}.",
                "•".dimmed(),
                crate::models::format_seconds(start)
            );
        }
        let next_requested = play_local_episode(&file.path, start, player)?;

        let Some(next) = group.episodes.get(index + 1) else {
            break;
        };
        let advance = next_requested
            || match auto_next {
                config::AutoNext::Always => true,
                config::AutoNext::Never => false,
                config::AutoNext::Ask => confirm_action(&format!(
                    "Putar episode berikutnya '{}'?",
                    local_episode_name(next)
                ))?,
            };
        if !advance {
            break;
        }
        index += 1;
    }
    Ok(())
}

fn play_local_episode(
    path: &std::path::Path,
    start_seconds: u64,
    player: config::PlayerChoice,
) -> anyhow::Result<bool> {
    match player {
        config::PlayerChoice::Mpv => play_local_mpv(path, start_seconds),
        config::PlayerChoice::Vlc => {
            if check_command_exists("vlc") {
                let _ = Command::new(resolve_command_path("vlc")).arg(path).status();
            } else {
                eprintln!("{} VLC tidak ditemukan.", "■".red());
            }
            // shortcut: VLC/browser playback has no resume position, only MPV does.
            Ok(false)
        }
        _ => {
            if let Err(error) = open::that(path) {
                eprintln!("{} Gagal membuka file: {}", "■".red(), error);
            }
            Ok(false)
        }
    }
}

fn play_local_mpv(path: &std::path::Path, start_seconds: u64) -> anyhow::Result<bool> {
    if !check_command_exists("mpv") {
        eprintln!("{} MPV tidak ditemukan.", "■".red());
        return Ok(false);
    }

    let track_script = crate::util::temp_file("animeku_local_track.lua");
    let last_pos_file = crate::util::temp_file("animeku_local_pos.txt");
    let next_file = crate::util::temp_file("animeku_local_next.txt");
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

    let mut cmd = Command::new(resolve_command_path("mpv"));
    cmd.arg("--hwdec=auto-safe")
        .arg(format!("--script={track_script}"));
    if start_seconds > 0 {
        cmd.arg(format!("--start={start_seconds}"));
    }
    if let Err(error) = cmd.arg(path).status() {
        eprintln!("{} Gagal menjalankan MPV: {}", "■".red(), error);
        return Ok(false);
    }

    if let Ok(saved) = std::fs::read_to_string(&last_pos_file) {
        if let Ok(position) = saved.trim().parse::<u64>() {
            let _ = std::fs::write(position_path(path), position.to_string());
        }
    }
    let next_requested = std::fs::read_to_string(&next_file).is_ok();
    let _ = std::fs::remove_file(&last_pos_file);
    let _ = std::fs::remove_file(&next_file);
    let _ = std::fs::remove_file(&track_script);
    Ok(next_requested)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchManagerAction {
    Run,
    RetryFailed,
    OpenFolder,
    Clear,
    Back,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(name: &str) -> crate::download::DownloadedFile {
        crate::download::DownloadedFile {
            path: PathBuf::from("/tmp/animeku").join(name),
            size: 10,
            modified: 0,
            series: None,
        }
    }

    #[test]
    fn local_groups_collect_episodes_by_show_and_sort_them() {
        let files = vec![
            file("Solo Leveling - Episode 3.mp4"),
            file("Solo Leveling - Episode 1.mp4"),
            file("Film Lepas.mkv"),
            file("Solo Leveling - Episode 2.mp4"),
        ];

        let groups = local_groups(&files);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].title, "Solo Leveling");
        let numbers = groups[0]
            .episodes
            .iter()
            .map(local_episode_name)
            .collect::<Vec<_>>();
        assert_eq!(
            numbers,
            vec!["Episode 1", "Episode 2", "Episode 3"]
        );
        assert_eq!(groups[1].title, "Film Lepas");
        assert_eq!(groups[1].episodes.len(), 1);
    }

    #[test]
    fn local_groups_use_series_folders_and_split_episode_titles() {
        let files = vec![
            crate::download::DownloadedFile {
                path: PathBuf::from("/tmp/animeku/Takopi/Takopi - S1E06 - To All of You.mp4"),
                size: 10,
                modified: 0,
                series: Some("Takopi".to_string()),
            },
            crate::download::DownloadedFile {
                path: PathBuf::from("/tmp/animeku/Takopi/Takopi - S1E02 - Kedua.mp4"),
                size: 10,
                modified: 0,
                series: Some("Takopi".to_string()),
            },
        ];
        let groups = local_groups(&files);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].title, "Takopi");
        let order = groups[0]
            .episodes
            .iter()
            .map(local_episode_name)
            .collect::<Vec<_>>();
        assert_eq!(order, vec!["S1E02 - Kedua", "S1E06 - To All of You"]);
    }

    #[test]
    fn loose_files_split_on_the_first_separator() {
        let file = file("Takopi's Original Sin - S1E06 - To All of You.mp4");
        assert_eq!(
            local_episode_name(&file),
            "S1E06 - To All of You"
        );
        let groups = local_groups(&[file]);
        assert_eq!(groups[0].title, "Takopi's Original Sin");
    }

    #[test]
    fn local_groups_keep_season_and_episode_order() {
        let files = vec![
            file("Judul - S2E01.mp4"),
            file("Judul - S1E10.mp4"),
            file("Judul - S1E02.mp4"),
        ];
        let groups = local_groups(&files);
        let order = groups[0]
            .episodes
            .iter()
            .map(local_episode_name)
            .collect::<Vec<_>>();
        assert_eq!(order, vec!["S1E02", "S1E10", "S2E01"]);
    }

    #[test]
    fn position_sidecar_round_trips_next_to_the_video() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("animeku-pos-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("Judul - Episode 1.mp4");

        assert_eq!(read_position(&video), 0);
        std::fs::write(position_path(&video), "754").unwrap();
        assert_eq!(read_position(&video), 754);
        assert!(position_path(&video).ends_with("Judul - Episode 1.mp4.pos"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
