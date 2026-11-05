use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileConfig {
    pub stage: StageOverlay,
    pub music: MusicOverlay,
    pub controls: Controls,
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

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    /// Ruffle gamepad button to Flash key name.
    pub gamepad: BTreeMap<String, String>,
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
        fs::write(path, text)
            .with_context(|| format!("cannot write profile config {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        self.controls.validate()
    }
}

impl Controls {
    pub fn validate(&self) -> Result<()> {
        for (button, key) in &self.gamepad {
            if !is_gamepad_button(button) {
                bail!(
                    "invalid gamepad button '{button}'; expected a Ruffle button such as south, d-pad-up, left-trigger2, or start"
                );
            }
            if !is_flash_key(key) {
                bail!(
                    "invalid Flash key '{key}' for gamepad button '{button}'; use a-z, key0-key9, up/down/left/right, return, space, comma, or semicolon"
                );
            }
        }
        Ok(())
    }
}

fn is_gamepad_button(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "north"
            | "east"
            | "south"
            | "west"
            | "d-pad-up"
            | "d-pad-down"
            | "d-pad-left"
            | "d-pad-right"
            | "left-trigger"
            | "left-trigger2"
            | "right-trigger"
            | "right-trigger2"
            | "select"
            | "start"
    )
}

fn is_flash_key(value: &str) -> bool {
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
    fn parses_typed_overlay_and_controls() {
        let config: ProfileConfig = toml::from_str(
            r#"
                [stage.files]
                temple = "mods/temple.swf"
                [stage.assignments]
                kyo = "temple"
                [music.tracks]
                temple = "mods/temple.mp3"
                [controls.gamepad]
                south = "j"
            "#,
        )
        .expect("valid config");
        assert_eq!(config.stage.assignments["kyo"], "temple");
        assert_eq!(config.controls.gamepad["south"], "j");
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(toml::from_str::<ProfileConfig>("mystery = true").is_err());
    }

    #[test]
    fn rejects_invalid_gamepad_button_and_key_before_runtime() {
        let invalid_button: ProfileConfig =
            toml::from_str("[controls.gamepad]\nlaunch-missiles = 'j'")
                .expect("syntactically valid config");
        assert!(invalid_button.validate().is_err());

        let invalid_key: ProfileConfig =
            toml::from_str("[controls.gamepad]\nsouth = 'not-a-flash-key'")
                .expect("syntactically valid config");
        assert!(invalid_key.validate().is_err());
    }
}
