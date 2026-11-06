use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;

use crate::{
    APP_NAME,
    config::ProfileConfig,
    input::{Binding, DeviceKind, FieldUpdate, InputConfig, InputSource},
};

#[derive(Debug, Clone)]
pub struct ProfilePaths {
    pub name: String,
    pub config_home: PathBuf,
    pub data_home: PathBuf,
    pub cache_home: PathBuf,
    pub config: PathBuf,
    pub input: PathBuf,
    pub catalogs: PathBuf,
    pub ruffle_config: PathBuf,
    pub saves: PathBuf,
    pub cache: PathBuf,
    pub sunshine_config: PathBuf,
    pub sunshine_apps: PathBuf,
    pub sunshine_state: PathBuf,
    pub sunshine_log: PathBuf,
    pub moonlight_config_home: PathBuf,
    pub moonlight_data_home: PathBuf,
    pub moonlight_cache_home: PathBuf,
    pub transport_tools: PathBuf,
}

impl ProfilePaths {
    pub fn discover(name: &str) -> Result<Self> {
        validate_name(name)?;
        let dirs = ProjectDirs::from("com", "KpihX Labs", APP_NAME)
            .context("platform does not expose application data directories")?;
        Ok(Self::from_roots(
            name,
            dirs.config_dir().to_path_buf(),
            dirs.data_dir().to_path_buf(),
            dirs.cache_dir().to_path_buf(),
        ))
    }

    pub fn from_roots(name: &str, config: PathBuf, data: PathBuf, cache: PathBuf) -> Self {
        let config_root = config.join("profiles").join(name);
        let data_root = data.join("profiles").join(name);
        let cache_root = cache.join("profiles").join(name);
        let sunshine_config_root = config_root.join("sunshine");
        let sunshine_data_root = data_root.join("sunshine");
        Self {
            name: name.to_owned(),
            config_home: config,
            data_home: data,
            cache_home: cache,
            config: config_root.join("config.toml"),
            input: config_root.join("input.toml"),
            catalogs: config_root.join("catalogs"),
            ruffle_config: config_root.join("ruffle"),
            saves: data_root.join("saves"),
            cache: cache_root.clone(),
            sunshine_config: sunshine_config_root.join("sunshine.conf"),
            sunshine_apps: sunshine_config_root.join("apps.json"),
            sunshine_state: sunshine_data_root.join("sunshine_state.json"),
            sunshine_log: cache_root.join("sunshine/sunshine.log"),
            moonlight_config_home: config_root.join("moonlight"),
            moonlight_data_home: data_root.join("moonlight"),
            moonlight_cache_home: cache_root.join("moonlight"),
            transport_tools: data_root.join("transport-tools"),
        }
    }

    pub fn create(&self) -> Result<()> {
        if self.config.exists() || self.input.exists() {
            bail!("profile '{}' already exists", self.name);
        }
        for directory in [&self.ruffle_config, &self.saves, &self.cache] {
            fs::create_dir_all(directory)
                .with_context(|| format!("cannot create {}", directory.display()))?;
        }
        InputConfig::default().write_new(&self.input)?;
        if let Err(error) = ProfileConfig::default().write_new(&self.config) {
            fs::remove_file(&self.input).with_context(|| {
                format!(
                    "cannot roll back {} after profile creation failed: {error:#}",
                    self.input.display()
                )
            })?;
            return Err(error);
        }
        Ok(())
    }

    pub fn require_config(&self) -> Result<ProfileConfig> {
        if !self.config.is_file() {
            bail!(
                "profile '{}' does not exist; run `kof-wing profile create {}`",
                self.name,
                self.name
            );
        }
        let mut config = ProfileConfig::read(&self.config)?;
        if !config.controls.gamepad.is_empty() {
            let mut input = self.require_input()?;
            for (button, target) in &config.controls.gamepad {
                input.set_binding(
                    "p1",
                    "legacy-gamepad-p1",
                    DeviceKind::Gamepad,
                    FieldUpdate::Keep,
                    FieldUpdate::Keep,
                    Binding {
                        source: InputSource::Button {
                            button: button.clone(),
                        },
                        target: target.clone(),
                        chord: Vec::new(),
                    },
                )?;
            }
            input.validate()?;
            input.write_atomic(&self.input)?;
        }
        if !config.controls.gamepad.is_empty()
            || fs::read_to_string(&self.config)?.contains("[controls.gamepad]")
        {
            config.controls.gamepad.clear();
            config.write_atomic(&self.config)?;
        }
        Ok(config)
    }

    pub fn require_input(&self) -> Result<InputConfig> {
        if !self.input.is_file() {
            bail!(
                "input config for profile '{}' does not exist; run `kof-wing --profile {} input reset`",
                self.name,
                self.name
            );
        }
        InputConfig::read(&self.input)
    }
}

pub fn list() -> Result<Vec<String>> {
    let paths = ProfilePaths::discover("placeholder")?;
    let root = paths
        .config
        .parent()
        .and_then(std::path::Path::parent)
        .context("invalid profile configuration path")?;
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("cannot list {}", root.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && entry.path().join("config.toml").is_file()
            && let Some(name) = entry.file_name().to_str()
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

pub fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !valid {
        bail!("profile names must be 1-64 ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{ProfilePaths, validate_name};

    #[test]
    fn profile_names_are_safe_path_components() {
        for valid in ["default", "player-2", "arcade_2026"] {
            assert!(validate_name(valid).is_ok());
        }
        for invalid in ["", "../escape", "two words", "é"] {
            assert!(validate_name(invalid).is_err());
        }
    }

    #[test]
    fn profile_creation_does_not_overwrite_or_expose_partial_input() {
        let temp = tempdir().unwrap();
        let profile = ProfilePaths::from_roots(
            "arcade",
            temp.path().join("config"),
            temp.path().join("data"),
            temp.path().join("cache"),
        );
        fs::create_dir_all(profile.input.parent().unwrap()).unwrap();
        fs::write(&profile.input, b"keep me").unwrap();

        assert!(profile.create().is_err());
        assert_eq!(fs::read(&profile.input).unwrap(), b"keep me");
        assert!(!profile.config.exists());
    }
}
