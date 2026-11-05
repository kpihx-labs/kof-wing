use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

use crate::{cli::PlayArgs, config::Controls, profile::ProfilePaths, source::verify_hash};

pub const LINUX_RUFFLE_ARCHIVE_SHA256: &str =
    "67950dfdde643f9087ae478eb3637fe52e0deefc79bdb55fe43cd69dab9fdd99";
pub const WINDOWS_RUFFLE_ARCHIVE_SHA256: &str =
    "a5f6df9881771226e330e08a1fa7666f4caff50ddf9f2f993cdd95293edca627";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Linux,
    Windows,
}

impl Platform {
    pub const fn current() -> Result<Self, &'static str> {
        if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "windows") {
            Ok(Self::Windows)
        } else {
            Err("only pinned Linux and Windows Ruffle builds are supported")
        }
    }
}

pub fn verify_archives(work: &Path) -> Result<()> {
    verify_hash(
        &work.join("tools/ruffle-0.7.1-linux-x86_64.tar.gz"),
        LINUX_RUFFLE_ARCHIVE_SHA256,
        "pinned Linux Ruffle archive",
    )?;
    verify_hash(
        &work.join("tools/ruffle-0.7.1-windows-x86_64.zip"),
        WINDOWS_RUFFLE_ARCHIVE_SHA256,
        "pinned Windows Ruffle archive",
    )
}

pub fn locate(work: &Path, platform: Platform) -> Result<PathBuf> {
    let candidates = match platform {
        Platform::Linux => vec![work.join("tools/ruffle-0.7.1/ruffle")],
        Platform::Windows => vec![
            work.join("tools/ruffle-0.7.1-windows-x86_64/ruffle.exe"),
            work.join("tools/ruffle-0.7.1/ruffle.exe"),
        ],
    };
    candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| {
        let contract = match platform {
            Platform::Linux => "work/tools/ruffle-0.7.1/ruffle",
            Platform::Windows => "work/tools/ruffle-0.7.1-windows-x86_64/ruffle.exe",
        };
        anyhow::anyhow!(
            "pinned Ruffle 0.7.1 executable is missing; extract the checked archive to {contract}"
        )
    })
}

pub fn launch(
    executable: &Path,
    swf: &Path,
    base: &Path,
    profile: &ProfilePaths,
    controls: &Controls,
    options: &PlayArgs,
) -> Result<()> {
    for directory in [&profile.ruffle_config, &profile.saves, &profile.cache] {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("cannot create runtime directory {}", directory.display()))?;
    }
    let arguments = arguments(swf, base, profile, controls, options)?;
    let status = Command::new(executable)
        .args(arguments)
        .status()
        .with_context(|| format!("cannot launch pinned Ruffle {}", executable.display()))?;
    if !status.success() {
        bail!("Ruffle exited unsuccessfully with {status}");
    }
    Ok(())
}

pub fn arguments(
    swf: &Path,
    base: &Path,
    profile: &ProfilePaths,
    controls: &Controls,
    options: &PlayArgs,
) -> Result<Vec<OsString>> {
    controls.validate()?;
    let dimension_scale = u32::from(options.scale);
    let mut arguments = vec![
        "--config".into(),
        profile.ruffle_config.clone().into_os_string(),
        "--save-directory".into(),
        profile.saves.clone().into_os_string(),
        "--cache-directory".into(),
        profile.cache.clone().into_os_string(),
        "--graphics".into(),
        options.graphics.as_str().into(),
        "--power".into(),
        "high".into(),
        "--quality".into(),
        "high16x16".into(),
        "--storage".into(),
        "disk".into(),
        "--open-url-mode".into(),
        "deny".into(),
        "--filesystem-access-mode".into(),
        "allow".into(),
        "--base".into(),
        file_url(base)?.into(),
        "--width".into(),
        (660 * dimension_scale).to_string().into(),
        "--height".into(),
        (485 * dimension_scale).to_string().into(),
    ];
    if options.fullscreen {
        arguments.push("--fullscreen".into());
    }
    for (button, key) in &controls.gamepad {
        arguments.push("--gamepad-button".into());
        arguments.push(format!("{button}={key}").into());
    }
    arguments.push(swf.as_os_str().to_owned());
    Ok(arguments)
}

pub fn file_url(directory: &Path) -> Result<String> {
    let directory = directory.canonicalize().with_context(|| {
        format!(
            "cannot resolve Ruffle base directory {}",
            directory.display()
        )
    })?;
    platform_file_url(&directory)
}

#[cfg(unix)]
fn platform_file_url(path: &Path) -> Result<String> {
    use std::os::unix::ffi::OsStrExt;

    if !path.is_absolute() {
        bail!("Ruffle base directory must be absolute: {}", path.display());
    }
    Ok(format!(
        "file://{}/",
        percent_encode_path(path.as_os_str().as_bytes()).trim_end_matches('/')
    ))
}

#[cfg(windows)]
fn platform_file_url(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .with_context(|| format!("Ruffle base path is not valid Unicode: {}", path.display()))?;
    windows_path_to_file_url(text)
}

#[cfg(any(windows, test))]
fn windows_path_to_file_url(path: &str) -> Result<String> {
    let normalized = path.replace('\\', "/");
    if let Some(unc) = normalized.strip_prefix("//") {
        let (host, path) = unc
            .split_once('/')
            .context("Windows UNC base path must include a server and share")?;
        return Ok(format!(
            "file://{}/{}/",
            host,
            percent_encode_path(path.as_bytes()).trim_end_matches('/')
        ));
    }
    let bytes = normalized.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'/' {
        bail!("Windows Ruffle base path must be an absolute drive or UNC path: {path}");
    }
    Ok(format!(
        "file:///{}/",
        percent_encode_path(bytes).trim_end_matches('/')
    ))
}

fn percent_encode_path(path: &[u8]) -> String {
    let mut encoded = String::with_capacity(path.len());
    for &byte in path {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, fs};

    use clap::Parser;
    use tempfile::tempdir;

    use crate::{
        cli::{Cli, Command, Graphics},
        config::Controls,
        profile::ProfilePaths,
    };

    use super::{Platform, arguments, locate, windows_path_to_file_url};

    #[test]
    fn locates_windows_contract_without_running_it() {
        let temp = tempdir().expect("temp dir");
        let executable = temp
            .path()
            .join("tools/ruffle-0.7.1-windows-x86_64/ruffle.exe");
        fs::create_dir_all(executable.parent().expect("parent")).expect("create parent");
        fs::write(&executable, []).expect("create executable");
        assert_eq!(
            locate(temp.path(), Platform::Windows).expect("locate"),
            executable
        );
    }

    #[test]
    fn assembles_exact_ruffle_arguments_from_parsed_cli() {
        let temp = tempdir().expect("temp dir");
        let overlay = temp.path().join("Overlay With # Hash");
        fs::create_dir(&overlay).expect("overlay directory");
        let swf = temp.path().join("game.swf");
        let profile = ProfilePaths::from_roots(
            "arcade",
            temp.path().join("config"),
            temp.path().join("data"),
            temp.path().join("cache"),
        );
        let cli = Cli::try_parse_from([
            "kof-wing",
            "play",
            "--fullscreen",
            "--scale",
            "3",
            "--graphics",
            "gl",
        ])
        .expect("parse play command");
        let Command::Play(options) = cli.command else {
            panic!("expected play command");
        };
        assert!(options.fullscreen);
        assert_eq!(options.scale, 3);
        assert!(matches!(options.graphics, Graphics::Gl));

        let mut controls = Controls::default();
        controls.gamepad.insert("south".into(), "j".into());
        let actual = arguments(&swf, &overlay, &profile, &controls, &options)
            .expect("assemble Ruffle arguments");
        #[cfg(unix)]
        let base_url = format!(
            "file://{}/Overlay%20With%20%23%20Hash/",
            temp.path().to_string_lossy()
        );
        #[cfg(windows)]
        let base_url = windows_path_to_file_url(
            overlay
                .canonicalize()
                .expect("canonical overlay")
                .to_str()
                .expect("Unicode test path"),
        )
        .expect("Windows base URL");
        let expected: Vec<OsString> = vec![
            "--config".into(),
            profile.ruffle_config.clone().into_os_string(),
            "--save-directory".into(),
            profile.saves.clone().into_os_string(),
            "--cache-directory".into(),
            profile.cache.clone().into_os_string(),
            "--graphics".into(),
            "gl".into(),
            "--power".into(),
            "high".into(),
            "--quality".into(),
            "high16x16".into(),
            "--storage".into(),
            "disk".into(),
            "--open-url-mode".into(),
            "deny".into(),
            "--filesystem-access-mode".into(),
            "allow".into(),
            "--base".into(),
            base_url.into(),
            "--width".into(),
            "1980".into(),
            "--height".into(),
            "1455".into(),
            "--fullscreen".into(),
            "--gamepad-button".into(),
            "south=j".into(),
            swf.into_os_string(),
        ];
        assert_eq!(actual, expected);
    }

    #[test]
    fn formats_windows_drive_and_unc_file_urls() {
        assert_eq!(
            windows_path_to_file_url(r"C:\Games\KOF Wing#1").expect("drive URL"),
            "file:///C:/Games/KOF%20Wing%231/"
        );
        assert_eq!(
            windows_path_to_file_url(r"\\server\share\KOF Wing").expect("UNC URL"),
            "file://server/share/KOF%20Wing/"
        );
    }

    #[test]
    fn rejects_invalid_controls_during_argument_assembly() {
        let temp = tempdir().expect("temp dir");
        let overlay = temp.path().join("overlay");
        fs::create_dir(&overlay).expect("overlay directory");
        let profile = ProfilePaths::from_roots(
            "arcade",
            temp.path().join("config"),
            temp.path().join("data"),
            temp.path().join("cache"),
        );
        let mut controls = Controls::default();
        controls.gamepad.insert("south".into(), "j=bad".into());
        let options = crate::cli::PlayArgs {
            fullscreen: false,
            window: true,
            scale: 2,
            graphics: Graphics::Vulkan,
        };
        assert!(
            arguments(
                &temp.path().join("game.swf"),
                &overlay,
                &profile,
                &controls,
                &options
            )
            .is_err()
        );
    }
}
