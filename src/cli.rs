use crate::{PROVIDER_IDLIX, PROVIDER_OTAKUDESU};
use crate::{ config };
use std::sync::{ OnceLock };

#[derive(Debug, Default)]
pub(crate) struct CliArgs {
    search: Option<String>,
    provider: Option<usize>,
    player: Option<config::PlayerChoice>,
    resume: bool,
    no_rpc: bool,
    update: bool,
    check_update: bool,
}

pub(crate) static CLI_ARGS: OnceLock<CliArgs> = OnceLock::new();

pub(crate) fn provider_override() -> Option<usize> {
    CLI_ARGS.get().and_then(|args| args.provider)
}

pub(crate) fn player_override() -> Option<config::PlayerChoice> {
    CLI_ARGS.get().and_then(|args| args.player)
}

pub(crate) fn rpc_disabled() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.no_rpc)
}

pub(crate) fn cli_search_title() -> Option<String> {
    CLI_ARGS.get().and_then(|args| args.search.clone())
}

pub(crate) fn cli_resume() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.resume)
}

pub(crate) fn cli_update() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.update)
}

pub(crate) fn cli_check_update() -> bool {
    CLI_ARGS.get().is_some_and(|args| args.check_update)
}

pub(crate) fn print_help() {
    println!(
        "Animeku-CLI v{}\n\nUsage:\n  animeku-cli [OPTIONS] [JUDUL]\n\nOptions:\n  --search <JUDUL>       Langsung cari judul tanpa membuka menu utama\n  --provider <PROVIDER>  auto, idlix, atau otakudesu\n  --player <PLAYER>      ask, mpv, vlc, atau browser\n  --resume               Lanjutkan tontonan terakhir\n  --update               Unduh dan pasang pembaruan terbaru\n  --check-update         Cek versi terbaru tanpa memasang\n  --no-rpc               Matikan Discord Rich Presence untuk sesi ini\n  -h, --help             Tampilkan bantuan\n  -V, --version          Tampilkan versi",
        env!("CARGO_PKG_VERSION")
    );
}

pub(crate) fn parse_args() -> anyhow::Result<CliArgs> {
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
