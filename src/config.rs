use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::input::atomic_write_new;

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileConfig {
    pub stage: StageOverlay,
    pub music: MusicOverlay,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StageOverlay {
    /// Manifest key to source-relative or absolute custom SWF path.
    pub files: BTreeMap<String, String>,
    /// Fighter/form key to stage key.
    pub assignments: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MusicOverlay {
    /// Manifest key to source-relative or absolute custom MP3 path.
    pub tracks: BTreeMap<String, String>,
}

impl ProfileConfig {
    pub fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read profile config {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("invalid profile config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn write_new(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("cannot serialize profile config")?;
        atomic_write_new(path, text.as_bytes())
            .with_context(|| format!("cannot create profile config {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

pub fn is_flash_key(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    matches!(
        value.as_str(),
        "up" | "down" | "left" | "right" | "return" | "space" | "comma" | "semicolon"
    ) || (value.len() == 1 && value.as_bytes()[0].is_ascii_lowercase())
        || (value.len() == 4 && value.starts_with("key") && value.as_bytes()[3].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::ProfileConfig;

    #[test]
    fn parses_typed_overlay() {
        let config: ProfileConfig = toml::from_str(
            r#"
                [stage.files]
                temple = "mods/temple.swf"
                [stage.assignments]
                kyo = "temple"
                [music.tracks]
                temple = "mods/temple.mp3"
            "#,
        )
        .expect("valid config");
        assert_eq!(config.stage.assignments["kyo"], "temple");
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(toml::from_str::<ProfileConfig>("mystery = true").is_err());
    }
}
