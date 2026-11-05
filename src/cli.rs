use std::{net::IpAddr, net::SocketAddr, path::PathBuf};

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
    /// Discover, negotiate, and opt into pinned same-subnet LAN transport.
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
    /// Diagnose LAN addressing, UDP binds, compatibility hashes, and capabilities.
    Doctor(LanNetworkArgs),
    /// Verify pinned Sunshine and generate an isolated host configuration for this profile.
    SetupHost,
    /// Discover compatible host sessions on the directly connected subnet.
    Discover(LanDiscoveryArgs),
    /// Advertise and negotiate an authoritative-host session.
    Host(LanHostArgs),
    /// Negotiate with a host and optionally start pinned Moonlight transport.
    Join(LanJoinArgs),
    /// Negotiate a host and explicitly start one-time Moonlight PIN pairing.
    Pair(LanPairArgs),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum HostTransport {
    /// Keep the existing negotiation-only behavior.
    #[default]
    Negotiation,
    /// Start the pinned Sunshine host before serving negotiation.
    Sunshine,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum JoinTransport {
    /// Keep the existing negotiation-only behavior.
    #[default]
    Negotiation,
    /// Stream the negotiated KOF application with pinned Moonlight.
    Moonlight,
}

#[derive(Clone, Debug, Args)]
pub struct LanNetworkArgs {
    /// Local address used for UDP sockets. Unspecified means all local interfaces.
    #[arg(long, default_value = "0.0.0.0")]
    pub bind: IpAddr,

    /// Concrete local IPv4 address to advertise, bind clients to, and subnet-check.
    #[arg(long)]
    pub interface: Option<IpAddr>,

    /// Assert the selected interface's IPv4 prefix (normally detected from the OS).
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=32))]
    pub prefix_len: Option<u8>,

    /// UDP port for same-subnet discovery probes and announcements.
    #[arg(long, default_value_t = 27_880)]
    pub discovery_port: u16,

    /// UDP port for strict session negotiation.
    #[arg(long, default_value_t = 27_881)]
    pub session_port: u16,

    /// Permit non-private/non-local interface and peer addresses.
    #[arg(long)]
    pub allow_public: bool,
}

#[derive(Clone, Debug, Args)]
pub struct LanDiscoveryArgs {
    #[command(flatten)]
    pub network: LanNetworkArgs,

    /// Time to wait for host announcements.
    #[arg(long, default_value_t = 1_500, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout_ms: u64,

    /// Reject announcements older than this many milliseconds.
    #[arg(long, default_value_t = 5_000, value_parser = clap::value_parser!(u64).range(1..))]
    pub stale_after_ms: u64,
}

#[derive(Clone, Debug, Args)]
pub struct LanHostArgs {
    #[command(flatten)]
    pub network: LanNetworkArgs,

    /// Human-readable session name included in discovery announcements.
    #[arg(long, default_value = "KOF Wing LAN")]
    pub name: String,

    /// Negotiation-only input metadata port (unused by Sunshine transport).
    #[arg(long, default_value_t = 27_882)]
    pub input_port: u16,

    /// Negotiation-only media metadata port (unused by Sunshine transport).
    #[arg(long, default_value_t = 27_883)]
    pub media_port: u16,

    /// Exit after answering one join request (useful for bounded diagnostics).
    #[arg(long)]
    pub once: bool,

    /// Transport to start after compatibility checks.
    #[arg(long, value_enum, default_value_t)]
    pub transport: HostTransport,
}

#[derive(Clone, Debug, Args)]
pub struct LanJoinArgs {
    #[command(flatten)]
    pub discovery: LanDiscoveryArgs,

    /// Direct negotiation endpoint. Without it, exactly one compatible discovery result is required.
    #[arg(long)]
    pub host: Option<SocketAddr>,

    /// Transport to start after successful negotiation.
    #[arg(long, value_enum, default_value_t)]
    pub transport: JoinTransport,
}

#[derive(Clone, Debug, Args)]
pub struct LanPairArgs {
    #[command(flatten)]
    pub discovery: LanDiscoveryArgs,

    /// Direct negotiation endpoint. Without it, exactly one compatible host is required.
    #[arg(long)]
    pub host: Option<SocketAddr>,
}
