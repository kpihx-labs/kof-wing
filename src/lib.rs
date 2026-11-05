pub mod cli;
pub mod config;
pub mod lan;
pub mod profile;
pub mod runtime;
pub mod source;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const APP_NAME: &str = "kof-wing";

pub fn default_work_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("KOF_WING_WORK") {
        return Ok(PathBuf::from(path));
    }
    Ok(std::env::current_dir()
        .context("cannot determine the current directory")?
        .join("work"))
}

pub fn require_source(source: Option<&Path>) -> Result<PathBuf> {
    source
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("KOF_SOURCE").map(PathBuf::from))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "original game directory is required; pass --source <DIR> or set KOF_SOURCE"
            )
        })
}
