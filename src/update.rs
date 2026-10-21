use anyhow::{Context, Result};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_LATEST: &str = "https://api.github.com/repos/yuutaa7x/animeku-cli/releases/latest";
const USER_AGENT: &str = concat!("animeku-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug)]
pub enum UpdateOutcome {
    UpToDate {
        current: Version,
        latest: Version,
    },
    Updated {
        from: Version,
        to: Version,
        path: PathBuf,
    },
    Unavailable(String),
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

pub fn asset_name() -> Option<&'static str> {
    // The release assets are glibc builds. Alpine/musl users must keep using
    // the cargo/source fallback instead of replacing their binary with a
    // glibc executable that cannot start.
    if cfg!(all(target_os = "linux", target_env = "musl")) {
        return None;
    }

    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("animeku-cli-windows-x86_64.exe"),
        ("linux", "x86_64") => Some("animeku-cli-linux-x86_64"),
        ("linux", "aarch64") => Some("animeku-cli-linux-aarch64"),
        ("macos", "x86_64") => Some("animeku-cli-macos-x86_64"),
        ("macos", "aarch64") => Some("animeku-cli-macos-aarch64"),
        _ => None,
    }
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .context("gagal membuat HTTP client untuk update")
}

fn parse_tag(tag: &str) -> Result<Version> {
    Version::parse(tag.trim().trim_start_matches('v'))
        .with_context(|| format!("tag rilis tidak valid: {tag}"))
}

async fn latest_release() -> Result<Release> {
    let response = client()?
        .get(API_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("gagal menghubungi GitHub API")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub API mengembalikan status {}", response.status());
    }

    response
        .json::<Release>()
        .await
        .context("gagal membaca respons rilis GitHub")
}

pub async fn check() -> Result<Option<Version>> {
    let release = latest_release().await?;
    let latest = parse_tag(&release.tag_name)?;
    let current = current_version();
    Ok((latest > current).then_some(latest))
}

pub async fn install_latest() -> Result<UpdateOutcome> {
    let current = current_version();
    let release = latest_release().await?;
    let latest = parse_tag(&release.tag_name)?;
    if latest <= current {
        return Ok(UpdateOutcome::UpToDate { current, latest });
    }

    let Some(name) = asset_name() else {
        return Ok(UpdateOutcome::Unavailable(format!(
            "belum ada binary rilis untuk {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    };

    let Some(asset) = release.assets.iter().find(|asset| asset.name == name) else {
        return Ok(UpdateOutcome::Unavailable(format!(
            "asset '{name}' tidak ditemukan di rilis {}",
            release.tag_name
        )));
    };

    let checksum_name = format!("{name}.sha256");
    let Some(checksum_asset) = release
        .assets
        .iter()
        .find(|asset| asset.name == checksum_name)
    else {
        return Ok(UpdateOutcome::Unavailable(format!(
            "checksum '{checksum_name}' tidak ditemukan di rilis {}",
            release.tag_name
        )));
    };

    let http = client()?;
    let bytes = http
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("gagal mengunduh binary rilis")?
        .error_for_status()
        .context("unduhan binary rilis gagal")?
        .bytes()
        .await
        .context("gagal membaca binary rilis")?;

    let checksum_text = http
        .get(&checksum_asset.browser_download_url)
        .send()
        .await
        .context("gagal mengunduh checksum rilis")?
        .error_for_status()
        .context("unduhan checksum rilis gagal")?
        .text()
        .await
        .context("gagal membaca checksum rilis")?;

    verify_checksum(&bytes, &checksum_text)?;
    let path = install_binary(&bytes)?;

    Ok(UpdateOutcome::Updated {
        from: current,
        to: latest,
        path,
    })
}

fn verify_checksum(bytes: &[u8], checksum_text: &str) -> Result<()> {
    let expected = checksum_text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if expected.len() != 64 {
        anyhow::bail!("checksum rilis tidak valid");
    }
    let actual = hex::encode(Sha256::digest(bytes));
    if expected != actual {
        anyhow::bail!("checksum binary rilis tidak cocok");
    }
    Ok(())
}

fn binary_runs(path: &Path) -> Result<()> {
    let status = std::process::Command::new(path)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("gagal menjalankan binary baru '{}'", path.display()))?;
    if !status.success() {
        anyhow::bail!("binary baru keluar dengan status {status}");
    }
    Ok(())
}

fn install_binary(bytes: &[u8]) -> Result<PathBuf> {
    let current = std::env::current_exe().context("tidak bisa menemukan path executable")?;
    replace_binary(&current, bytes)?;
    Ok(current)
}

fn replace_binary(current: &Path, bytes: &[u8]) -> Result<()> {
    let parent = current
        .parent()
        .context("executable tidak punya parent directory")?;
    let temp = parent.join(format!(
        ".animeku-cli-update-{}{}",
        std::process::id(),
        if cfg!(windows) { ".exe" } else { "" }
    ));
    let backup = current.with_extension(if cfg!(windows) { "exe.old" } else { "old" });

    fs::write(&temp, bytes)
        .with_context(|| format!("gagal menulis binary baru ke '{}'", temp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(0o755));
    }

    // Check compatibility before touching the working installation. This
    // catches glibc/architecture mismatches while the old binary is intact.
    if let Err(error) = binary_runs(&temp) {
        let _ = fs::remove_file(&temp);
        return Err(error).context("binary rilis tidak bisa dijalankan di sistem ini");
    }

    #[cfg(windows)]
    {
        let _ = fs::remove_file(&backup);
        if let Err(error) = fs::rename(current, &backup) {
            let _ = fs::remove_file(&temp);
            return Err(error).with_context(|| format!("gagal mengganti '{}'", current.display()));
        }
        if let Err(error) = fs::rename(&temp, current) {
            let _ = fs::rename(&backup, current);
            let _ = fs::remove_file(&temp);
            return Err(error)
                .with_context(|| format!("gagal memasang binary baru ke '{}'", current.display()));
        }
    }

    #[cfg(not(windows))]
    {
        if let Err(error) = fs::copy(current, &backup) {
            let _ = fs::remove_file(&temp);
            return Err(error).with_context(|| {
                format!(
                    "gagal membuat backup '{}' sebelum memperbarui",
                    backup.display()
                )
            });
        }
        if let Err(error) = fs::rename(&temp, current) {
            let _ = fs::remove_file(&backup);
            let _ = fs::remove_file(&temp);
            return Err(error).with_context(|| {
                format!(
                    "gagal memasang binary baru ke '{}'. Coba jalankan dengan permission yang cukup.",
                    current.display()
                )
            });
        }
    }

    // Verify the installed path as well: permissions or a broken dynamic
    // linker can differ from the temporary file. Roll back on failure.
    if let Err(error) = binary_runs(current) {
        #[cfg(windows)]
        {
            let _ = fs::remove_file(current);
        }
        let restored = fs::rename(&backup, current);
        let detail = if restored.is_ok() {
            "binary lama sudah dipulihkan".to_string()
        } else {
            format!(
                "rollback gagal; binary lama tersedia di '{}'",
                backup.display()
            )
        };
        return Err(error).context(format!("binary baru gagal dijalankan; {detail}"));
    }

    #[cfg(not(windows))]
    {
        let _ = fs::remove_file(&backup);
    }

    update_alias(current, bytes);
    Ok(())
}

fn update_alias(current: &Path, bytes: &[u8]) {
    let Some(parent) = current.parent() else {
        return;
    };
    let Some(stem) = current.file_stem().and_then(|stem| stem.to_str()) else {
        return;
    };
    let alias_stem = if stem == "animeku" {
        "animeku-cli"
    } else if stem == "animeku-cli" {
        "animeku"
    } else {
        return;
    };

    let alias = parent.join(format!(
        "{}{}",
        alias_stem,
        if cfg!(windows) { ".exe" } else { "" }
    ));
    if !alias.exists() {
        return;
    }
    if fs::write(&alias, bytes).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&alias, fs::Permissions::from_mode(0o755));
        }
    }
}

/// Windows keeps the old executable locked until the process exits. Remove it
/// on the next launch, when it is no longer running.
pub fn cleanup_old_binary() {
    #[cfg(windows)]
    {
        let Ok(current) = std::env::current_exe() else {
            return;
        };
        let Some(parent) = current.parent() else {
            return;
        };
        for name in ["animeku-cli.exe.old", "animeku.exe.old"] {
            let _ = fs::remove_file(parent.join(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_tags() {
        assert_eq!(parse_tag("v0.3.8").unwrap(), Version::new(0, 3, 8));
        assert_eq!(parse_tag("0.3.8").unwrap(), Version::new(0, 3, 8));
        assert!(parse_tag("not-a-version").is_err());
    }

    #[test]
    fn checksum_validation_rejects_mismatches() {
        let bytes = b"animeku";
        let good = hex::encode(Sha256::digest(bytes));
        assert!(verify_checksum(bytes, &format!("{good}  animeku-cli")).is_ok());
        assert!(verify_checksum(bytes, "deadbeef  animeku-cli").is_err());
    }

    #[cfg(unix)]
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "animeku-update-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn write_script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_keeps_old_binary_when_new_one_cannot_run() {
        let dir = test_dir("precheck");
        let current = dir.join("animeku-cli");
        write_script(&current, "exit 0");
        let original = fs::read(&current).unwrap();

        assert!(replace_binary(&current, b"not a runnable binary").is_err());
        assert_eq!(fs::read(&current).unwrap(), original);
        assert!(!current.with_extension("old").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_rolls_back_when_installed_binary_fails() {
        let dir = test_dir("rollback");
        let current = dir.join("animeku-cli");
        write_script(&current, "exit 0");
        let original = fs::read(&current).unwrap();

        let broken_after_install =
            b"#!/bin/sh\ncase \"$0\" in */.animeku-cli-update-*) exit 0 ;; *) exit 1 ;; esac\n";
        let error = replace_binary(&current, broken_after_install).unwrap_err();

        assert!(format!("{error:#}").contains("dipulihkan"));
        assert_eq!(fs::read(&current).unwrap(), original);
        assert!(!current.with_extension("old").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_installs_working_binary() {
        let dir = test_dir("success");
        let current = dir.join("animeku-cli");
        write_script(&current, "exit 0");

        let replacement = b"#!/bin/sh\nexit 0\n";
        replace_binary(&current, replacement).unwrap();

        assert_eq!(fs::read(&current).unwrap(), replacement);
        assert!(!current.with_extension("old").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
