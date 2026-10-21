use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Preferred external player. `Ask` keeps the picker visible and remembers the
/// last choice as the default cursor position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerChoice {
    #[default]
    Ask,
    Mpv,
    Vlc,
    Browser,
}

impl PlayerChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Tanya setiap kali",
            Self::Mpv => "MPV",
            Self::Vlc => "VLC",
            Self::Browser => "Browser",
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Mpv,
            2 => Self::Vlc,
            3 => Self::Browser,
            _ => Self::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoNext {
    #[default]
    Ask,
    Always,
    Never,
}

impl AutoNext {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Tanya",
            Self::Always => "Selalu lanjut",
            Self::Never => "Jangan pernah",
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Always,
            2 => Self::Never,
            _ => Self::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderPreference {
    #[default]
    Auto,
    Idlix,
    Otakudesu,
}

impl ProviderPreference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Otomatis",
            Self::Idlix => "Idlix",
            Self::Otakudesu => "Otakudesu",
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Idlix,
            2 => Self::Otakudesu,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `Ask` shows the player picker; any other value bypasses it.
    pub player: PlayerChoice,
    /// Last player chosen from the picker, used as the default cursor.
    pub last_player: Option<PlayerChoice>,
    pub auto_next: AutoNext,
    pub provider: ProviderPreference,
    pub rpc: bool,
    pub compact_banner: bool,
    pub show_posters: bool,
    /// Embed provider subtitles into the downloaded container instead of
    /// writing a `.vtt` sidecar file.
    pub embed_subtitles: bool,
    /// Last folder used for episode downloads. `None` means use the platform
    /// Downloads directory with an `Animeku-CLI` subfolder.
    pub download_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player: PlayerChoice::Ask,
            last_player: None,
            auto_next: AutoNext::Ask,
            provider: ProviderPreference::Auto,
            rpc: true,
            compact_banner: false,
            show_posters: true,
            embed_subtitles: false,
            download_dir: None,
        }
    }
}

impl Config {
    pub fn effective_player(&self) -> PlayerChoice {
        match self.player {
            PlayerChoice::Ask => self.last_player.unwrap_or(PlayerChoice::Mpv),
            player => player,
        }
    }

    pub fn path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        });
        base.join("animeku-cli").join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, content);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_ask_mode() {
        let config = Config::default();
        assert_eq!(config.player, PlayerChoice::Ask);
        assert_eq!(config.effective_player(), PlayerChoice::Mpv);
        assert_eq!(config.auto_next, AutoNext::Ask);
        assert_eq!(config.provider, ProviderPreference::Auto);
    }

    #[test]
    fn config_roundtrip_keeps_choices() {
        let config = Config {
            player: PlayerChoice::Vlc,
            last_player: Some(PlayerChoice::Vlc),
            auto_next: AutoNext::Always,
            provider: ProviderPreference::Otakudesu,
            rpc: false,
            compact_banner: true,
            show_posters: false,
            embed_subtitles: true,
            download_dir: Some("/tmp/animeku".to_string()),
        };
        let encoded = serde_json::to_string(&config).unwrap();
        let decoded: Config = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.player, PlayerChoice::Vlc);
        assert_eq!(decoded.effective_player(), PlayerChoice::Vlc);
        assert_eq!(decoded.auto_next, AutoNext::Always);
        assert_eq!(decoded.provider, ProviderPreference::Otakudesu);
        assert!(!decoded.rpc);
        assert!(decoded.compact_banner);
        assert!(!decoded.show_posters);
        assert!(decoded.embed_subtitles);
        assert_eq!(decoded.download_dir.as_deref(), Some("/tmp/animeku"));
    }
}
