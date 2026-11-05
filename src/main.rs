use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use kof_wing::{
    cli::{Cli, Command, LanCommand, ProfileCommand},
    config::ProfileConfig,
    default_work_dir, lan,
    profile::{self, ProfilePaths},
    require_source, runtime, source,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let work = default_work_dir()?;
    match cli.command {
        Command::Profile { command } => profile_command(command),
        Command::Lan { command } => {
            lan_command(command, cli.source.as_deref(), &cli.profile, &work)
        }
        Command::Doctor => doctor(cli.source.as_deref(), &cli.profile, &work),
        Command::Play(options) => play(cli.source.as_deref(), &cli.profile, &work, &options),
    }
}

fn lan_command(
    command: LanCommand,
    source_path: Option<&Path>,
    profile_name: &str,
    work: &Path,
) -> Result<()> {
    match command {
        LanCommand::Doctor(network) => lan::doctor(work, profile_name, &network),
        LanCommand::SetupHost => {
            let source = require_source(source_path)?;
            let launcher = std::env::current_exe().context("cannot locate kof-wing executable")?;
            let paths = kof_wing::transport::setup_host(work, profile_name, &source, &launcher)?;
            println!(
                "generated isolated Sunshine config: {}",
                paths.config.display()
            );
            println!("generated KOF-only Sunshine apps: {}", paths.apps.display());
            println!("Desktop is not exposed; UPnP is disabled; remote web UI access is denied");
        }
        LanCommand::Discover(args) => {
            let sessions = lan::discover(profile_name, &args)?;
            if sessions.is_empty() {
                println!("no LAN sessions discovered");
            }
            for result in sessions {
                let status = if result.compatible {
                    "compatible".to_owned()
                } else {
                    result
                        .incompatibility
                        .unwrap_or_else(|| "incompatible".to_owned())
                };
                println!(
                    "{}\t{}\t{}\t{}",
                    result.session.session_id,
                    result.session.name,
                    result.session.negotiation_endpoint,
                    status
                );
            }
        }
        LanCommand::Host(args) => return lan::host(work, profile_name, &args),
        LanCommand::Join(args) => {
            let session = lan::join(profile_name, &args)?;
            println!("session negotiation accepted: {}", session.session_id);
            println!("authoritative host: {}", session.negotiation_endpoint);
            println!(
                "remote input endpoint: {} ({:?})",
                session.remote_input.endpoint, session.remote_input.status
            );
            println!(
                "media endpoint: {} ({:?})",
                session.media.endpoint, session.media.status
            );
            if args.transport == kof_wing::cli::JoinTransport::Moonlight {
                kof_wing::transport::stream_client(
                    work,
                    profile_name,
                    session.negotiation_endpoint.ip(),
                )?;
            } else {
                println!("LAN gameplay was not started");
            }
        }
        LanCommand::Pair(args) => {
            let join = kof_wing::cli::LanJoinArgs {
                discovery: args.discovery,
                host: args.host,
                transport: kof_wing::cli::JoinTransport::Moonlight,
            };
            let session = lan::join(profile_name, &join)?;
            println!("one-time pairing requires a person at the Sunshine host terminal");
            println!("enter the Moonlight PIN there; pairing is never automatic");
            kof_wing::transport::pair_client(
                work,
                profile_name,
                session.negotiation_endpoint.ip(),
            )?;
        }
    }
    Ok(())
}

fn profile_command(command: ProfileCommand) -> Result<()> {
    match command {
        ProfileCommand::List => {
            for name in profile::list()? {
                println!("{name}");
            }
        }
        ProfileCommand::Create { name } => {
            let profile = ProfilePaths::discover(&name)?;
            profile.create()?;
            println!("created profile '{name}' at {}", profile.config.display());
        }
    }
    Ok(())
}

fn doctor(source_path: Option<&Path>, profile_name: &str, work: &Path) -> Result<()> {
    let source_path = require_source(source_path)?;
    let verified = source::verify_source(&source_path)?;
    println!("[ok] original game layout: {}", verified.root.display());
    println!(
        "[info] projector SHA-256: {} (payload identity is the preservation gate)",
        verified.projector_hash
    );
    let swf = source::prepare_swf(&verified, work)?;
    println!("[ok] known SWF SHA-256: {}", source::SWF_SHA256);
    println!("[ok] extracted payload: {}", swf.display());
    runtime::verify_archives(work)?;
    println!("[ok] pinned Linux and Windows Ruffle archives");
    let platform = runtime::Platform::current().map_err(anyhow::Error::msg)?;
    let executable = runtime::locate(work, platform)?;
    println!("[ok] current Ruffle executable: {}", executable.display());
    let profile = ProfilePaths::discover(profile_name)?;
    let config = profile.require_config()?;
    let overlay = source::generate_overlay(&verified, work, profile_name, &config)?;
    println!("[ok] profile config: {}", profile.config.display());
    println!("[ok] generated overlay: {}", overlay.display());
    Ok(())
}

fn play(
    source_path: Option<&Path>,
    profile_name: &str,
    work: &Path,
    options: &kof_wing::cli::PlayArgs,
) -> Result<()> {
    let source_path = require_source(source_path)?;
    let verified = source::verify_source(&source_path)?;
    runtime::verify_archives(work)?;
    let profile = ProfilePaths::discover(profile_name)?;
    let config: ProfileConfig = profile.require_config()?;
    let swf = source::prepare_swf(&verified, work)?;
    let overlay = source::generate_overlay(&verified, work, profile_name, &config)?;
    let platform = runtime::Platform::current().map_err(anyhow::Error::msg)?;
    let executable = runtime::locate(work, platform)?;
    runtime::launch(
        &executable,
        &swf,
        &overlay,
        &profile,
        &config.controls,
        options,
    )
    .context("game launch failed")
}
