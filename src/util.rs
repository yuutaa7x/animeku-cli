use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, console::Style};

pub fn custom_theme() -> ColorfulTheme {
    let mut theme = ColorfulTheme::default();
    theme.hint_style = Style::new().cyan();
    theme
}

use reqwest::Client;

#[macro_export]
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

pub async fn show_image_thumb(url: String) {
    let client = Client::new();
    if let Ok(resp) = client.get(url).send().await {
        if let Ok(bytes) = resp.bytes().await {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let conf = viuer::Config {
                    transparent: true,
                    width: Some(50),
                    height: Some(30),
                    y: 8,
                    x: 2,
                    ..Default::default()
                };
                if viuer::print(&img, &conf).is_ok() {
                    println!();
                }
            }
        }
    }
}

pub fn clearscreen_and_show_banner() -> anyhow::Result<()> {
    clearscreen::clear()?;

    eprintln!(
        "{}
{}
",
        r#"
               ▄▀█ █▄░█ █ █▀▄▀█ █▀▀ █▄▀ █░█ ▄▄ █▀▀ █░░ █
               █▀█ █░▀█ █ █░▀░█ ██▄ █░█ █▄█ ░░ █▄▄ █▄▄ █ "#
            .bright_green(),
        format!(
            "                     v{} {} val - Forked by Yuutaa7x",
            env!("CARGO_PKG_VERSION"),
            "©".cyan()
        )
    );
    Ok(())
}
