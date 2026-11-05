use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;

use crate::{APP_NAME, config::ProfileConfig};

#[derive(Debug, Clone)]
pub struct ProfilePaths {
    pub name: String,
    pub config: PathBuf,
    pub ruffle_config: PathBuf,
    pub saves: PathBuf,
    pub cache: PathBuf,
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
        Self {
            name: name.to_owned(),
            config: config_root.join("config.toml"),
            ruffle_config: config_root.join("ruffle"),
            saves: data.join("profiles").join(name).join("saves"),
            cache: cache.join("profiles").join(name),
        }
    }

    pub fn create(&self) -> Result<()> {
        if self.config.exists() {
            bail!("profile '{}' already exists", self.name);
        }
        for directory in [&self.ruffle_config, &self.saves, &self.cache] {
            fs::create_dir_all(directory)
                .with_context(|| format!("cannot create {}", directory.display()))?;
        }
        ProfileConfig::default().write_new(&self.config)
    }

    pub fn require_config(&self) -> Result<ProfileConfig> {
        if !self.config.is_file() {
            bail!(
                "profile '{}' does not exist; run `kof-wing profile create {}`",
                self.name,
                self.name
            );
        }
        ProfileConfig::read(&self.config)
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
    use super::validate_name;

    #[test]
    fn profile_names_are_safe_path_components() {
        for valid in ["default", "player-2", "arcade_2026"] {
            assert!(validate_name(valid).is_ok());
        }
        for invalid in ["", "../escape", "two words", "é"] {
            assert!(validate_name(invalid).is_err());
        }
    }
}
