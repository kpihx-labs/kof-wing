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
        Command::Lan {
            command: LanCommand::Doctor,
        } => {
            lan::doctor(&work);
            Ok(())
        }
        Command::Doctor => doctor(cli.source.as_deref(), &cli.profile, &work),
        Command::Play(options) => play(cli.source.as_deref(), &cli.profile, &work, &options),
    }
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
