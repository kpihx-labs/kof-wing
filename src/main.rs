use std::{fs, path::Path};

use anyhow::{Context, Result};
use clap::Parser;
use kof_wing::{
    cli::{Cli, Command, InputCommand, InputFormat, LanCommand, ProfileCommand},
    config::ProfileConfig,
    default_work_dir,
    input::{Binding, FieldUpdate, InputConfig, InputSource, atomic_write_new},
    lan,
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
        Command::Input { command } => input_command(command, &cli.profile),
        Command::Lan { command } => {
            lan_command(command, cli.source.as_deref(), &cli.profile, &work)
        }
        Command::Doctor => doctor(cli.source.as_deref(), &cli.profile, &work),
        Command::Play(options) => play(cli.source.as_deref(), &cli.profile, &work, &options),
    }
}

fn input_command(command: InputCommand, profile_name: &str) -> Result<()> {
    let profile = ProfilePaths::discover(profile_name)?;
    match command {
        InputCommand::Show { format } => {
            let config = profile.require_input()?;
            match format {
                InputFormat::Toml => print!("{}", config.to_toml()?),
                InputFormat::Json => print!("{}", config.to_json()?),
            }
        }
        InputCommand::Set(args) => {
            let mut config = profile.require_input()?;
            let mut source = args.control;
            match (&mut source, args.threshold) {
                (InputSource::Axis { threshold, .. }, Some(value)) => *threshold = value,
                (InputSource::Axis { .. }, None) | (_, None) => {}
                (_, Some(_)) => anyhow::bail!("--threshold is valid only for an axis source"),
            }
            config.set_binding(
                &args.player,
                &args.device,
                args.kind.into(),
                match (args.selector, args.clear_selector) {
                    (Some(value), false) => FieldUpdate::Set(value),
                    (None, true) => FieldUpdate::Clear,
                    (None, false) => FieldUpdate::Keep,
                    (Some(_), true) => unreachable!("clap rejects conflicting selector options"),
                },
                match (args.deadzone, args.clear_deadzone) {
                    (Some(value), false) => FieldUpdate::Set(value),
                    (None, true) => FieldUpdate::Clear,
                    (None, false) => FieldUpdate::Keep,
                    (Some(_), true) => unreachable!("clap rejects conflicting deadzone options"),
                },
                Binding {
                    source,
                    target: args.target,
                    chord: args.chord,
                },
            )?;
            config.write_atomic(&profile.input)?;
            println!("updated {}", profile.input.display());
        }
        InputCommand::Reset { preset } => {
            let config = preset
                .map(|value| InputConfig::preset(value.into()))
                .unwrap_or_default();
            config.write_atomic(&profile.input)?;
            println!("reset {}", profile.input.display());
        }
        InputCommand::Import { path } => {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("cannot read import {}", path.display()))?;
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            let config = match extension.as_deref() {
                Some("toml") => InputConfig::from_toml(&text)?,
                Some("json") => InputConfig::from_json(&text)?,
                _ => anyhow::bail!("input import path must end in .toml or .json"),
            };
            config.write_atomic(&profile.input)?;
            println!("imported {}", profile.input.display());
        }
        InputCommand::Export { path } => {
            if path.exists() {
                anyhow::bail!("refusing to overwrite existing export {}", path.display());
            }
            let config = profile.require_input()?;
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            let text = match extension.as_deref() {
                Some("toml") => config.to_toml()?,
                Some("json") => config.to_json()?,
                _ => anyhow::bail!("input export path must end in .toml or .json"),
            };
            atomic_write_new(&path, text.as_bytes())?;
            println!("exported {}", path.display());
        }
        InputCommand::Validate => {
            let config = profile.require_input()?;
            config.validate()?;
            let report = config.backend_report()?;
            println!("[ok] input schema version {}", config.version);
            println!("[ok] no structural or binding conflicts");
            println!(
                "[info] Ruffle --gamepad-button mappings: {}",
                report.ruffle_gamepad_mappings
            );
            if report.limitations.is_empty() {
                println!("[ok] active bindings are exactly representable by Ruffle 0.7.1");
            } else {
                for limitation in report.limitations {
                    println!("[backend limit] {limitation}");
                }
            }
        }
    }
    Ok(())
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
    let input = profile.require_input()?;
    input
        .ruffle_mappings()
        .context("input profile is valid but cannot be applied by the pinned runtime")?;
    let overlay = source::generate_overlay(&verified, work, profile_name, &config)?;
    println!("[ok] profile config: {}", profile.config.display());
    println!("[ok] input config: {}", profile.input.display());
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
    let input = profile.require_input()?;
    let swf = source::prepare_swf(&verified, work)?;
    let overlay = source::generate_overlay(&verified, work, profile_name, &config)?;
    let platform = runtime::Platform::current().map_err(anyhow::Error::msg)?;
    let executable = runtime::locate(work, platform)?;
    if options.dry_run {
        let arguments = runtime::arguments(&swf, &overlay, &profile, &input, options)?;
        println!(
            "{:?}",
            std::process::Command::new(&executable).args(arguments)
        );
        return Ok(());
    }
    runtime::launch(&executable, &swf, &overlay, &profile, &input, options)
        .context("game launch failed")
}
