use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Directory containing the locally owned original game.
    #[arg(long, global = true, env = "KOF_SOURCE")]
    pub source: Option<PathBuf>,

    /// Isolated profile to use.
    #[arg(long, global = true, default_value = "default")]
    pub profile: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Verify source data, pinned tools, profile paths, and configuration.
    Doctor,
    /// Verify, prepare overlays, and launch the game with pinned Ruffle.
    Play(PlayArgs),
    /// Create and list isolated profiles.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Diagnose the future local-network boundary.
    Lan {
        #[command(subcommand)]
        command: LanCommand,
    },
}

#[derive(Debug, Args)]
pub struct PlayArgs {
    /// Start Ruffle in fullscreen mode.
    #[arg(long, conflicts_with = "window")]
    pub fullscreen: bool,

    /// Force a window (the default; useful in explicit scripts).
    #[arg(long)]
    pub window: bool,

    /// Integer scale applied to the native 660x485 stage.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=8))]
    pub scale: u8,

    /// Graphics backend passed to Ruffle.
    #[arg(long, value_enum, default_value_t = Graphics::Vulkan)]
    pub graphics: Graphics,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Graphics {
    Default,
    Vulkan,
    Gl,
    Dx12,
}

impl Graphics {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Vulkan => "vulkan",
            Self::Gl => "gl",
            Self::Dx12 => "dx12",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// List configured profile names.
    List,
    /// Create a named profile with a documented default configuration.
    Create { name: String },
}

#[derive(Debug, Subcommand)]
pub enum LanCommand {
    /// Check local prerequisites; LAN play itself is not implemented.
    Doctor,
}
