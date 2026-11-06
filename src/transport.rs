use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read},
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::{
    input::{InputConfig, atomic_write},
    profile::ProfilePaths,
    runtime::Platform,
    source::verify_hash,
};

pub const SUNSHINE_VERSION: &str = "2026.914.233613";
pub const MOONLIGHT_VERSION: &str = "6.2.0";
pub const SUNSHINE_PORT: u16 = 47_989;
pub const KOF_APP_NAME: &str = "KOF Wing EX 1.0";
const STREAM_BITRATE_KBPS: &str = "10000";
const STREAM_FPS: &str = "60";
const STREAM_PACKET_SIZE: &str = "1392";
const STREAM_VIDEO_CODEC: &str = "H.264";
pub const LINUX_SUNSHINE_SHA256: &str =
    "6f80297a853bcf8cda114ff131b5c0a8c9a4d96a2a9c3dee1f04c39c54206846";
pub const WINDOWS_SUNSHINE_SHA256: &str =
    "233008e46f4c0e501a586cbfd6c4fd4a4c0d414a0b5fc7f13c070eb92ec3824b";
pub const LINUX_MOONLIGHT_SHA256: &str =
    "787b0dcbd42ae90620e76fb8d77d7c441ba6473a91c2d7fc3a0fc2466f992437";
pub const WINDOWS_MOONLIGHT_SHA256: &str =
    "6f91d268b41ed5ca8066d9d7578dc403b8db190991ff48f00b1700465f8f7ebf";

const LINUX_SUNSHINE: &str = "tools/Sunshine_2026.914.233613_x86_64.AppImage";
const WINDOWS_SUNSHINE: &str = "tools/Sunshine-Windows-AMD64-lite.zip";
const LINUX_MOONLIGHT: &str = "tools/Moonlight-6.2.0-x86_64.AppImage";
const WINDOWS_MOONLIGHT: &str = "tools/MoonlightPortable-x64-6.2.0.zip";

#[derive(Debug)]
pub struct HostSetup {
    pub config: PathBuf,
    pub apps: PathBuf,
}

pub struct HostProcess(Child);

impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Err(error) = self.0.kill()
            && error.kind() != io::ErrorKind::InvalidInput
        {
            eprintln!("warning: could not stop Sunshine transport: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("warning: could not reap Sunshine transport: {error}");
        }
    }
}

#[derive(Serialize)]
struct AppsFile {
    env: std::collections::BTreeMap<String, String>,
    apps: Vec<App>,
}

#[derive(Serialize)]
struct App {
    name: &'static str,
    cmd: String,
    #[serde(rename = "auto-detach")]
    auto_detach: bool,
    #[serde(rename = "wait-all")]
    wait_all: bool,
}

pub fn contract() -> String {
    format!(
        "sunshine={SUNSHINE_VERSION};linux-sunshine={LINUX_SUNSHINE_SHA256};windows-sunshine={WINDOWS_SUNSHINE_SHA256};moonlight={MOONLIGHT_VERSION};linux-moonlight={LINUX_MOONLIGHT_SHA256};windows-moonlight={WINDOWS_MOONLIGHT_SHA256};app={KOF_APP_NAME};resolution=1280x720;fps={STREAM_FPS};bitrate-kbps={STREAM_BITRATE_KBPS};packet-size={STREAM_PACKET_SIZE};display-mode=fullscreen;vsync=true;audio-config=stereo;audio-on-host=false;multi-controller=input-profile-derived;game-optimization=true;frame-pacing=true;video-codec={STREAM_VIDEO_CODEC};video-decoder=auto;quit-after=true;absolute-mouse=false;mouse-buttons-swap=false;touchscreen-trackpad=false;mute-on-focus-loss=false;background-gamepad=false;reverse-scroll=false;swap-gamepad-buttons=false;keep-awake=true;performance-overlay=false;hdr=false;yuv444=false;capture-system-keys=fullscreen;pairing=explicit-pin;pin-stdin=true;upnp=false;web-ui-origin=pc;lan-encryption=mandatory;wan-encryption=mandatory;bind-policy=selected-private-or-local-ipv4"
    )
}

pub fn verify_archives(work: &Path) -> Result<()> {
    for (relative, expected, label) in [
        (
            LINUX_SUNSHINE,
            LINUX_SUNSHINE_SHA256,
            "pinned Linux Sunshine AppImage",
        ),
        (
            WINDOWS_SUNSHINE,
            WINDOWS_SUNSHINE_SHA256,
            "pinned Windows Sunshine archive",
        ),
        (
            LINUX_MOONLIGHT,
            LINUX_MOONLIGHT_SHA256,
            "pinned Linux Moonlight AppImage",
        ),
        (
            WINDOWS_MOONLIGHT,
            WINDOWS_MOONLIGHT_SHA256,
            "pinned Windows Moonlight archive",
        ),
    ] {
        verify_hash(&work.join(relative), expected, label)?;
    }
    Ok(())
}

pub fn setup_host(
    work: &Path,
    profile_name: &str,
    source: &Path,
    launcher: &Path,
) -> Result<HostSetup> {
    verify_archives(work)?;
    ensure!(
        source.is_dir(),
        "game source directory is missing: {}",
        source.display()
    );
    ensure!(
        launcher.is_file(),
        "kof-wing executable is missing: {}",
        launcher.display()
    );
    let profile = ProfilePaths::discover(profile_name)?;
    profile.require_config()?;
    let input = profile.require_input()?;
    input
        .ruffle_mappings()
        .context("input profile cannot be applied by the pinned host runtime")?;
    let source = source
        .canonicalize()
        .with_context(|| format!("cannot resolve game source {}", source.display()))?;
    let launcher = launcher
        .canonicalize()
        .with_context(|| format!("cannot resolve launcher {}", launcher.display()))?;
    let platform = Platform::current().map_err(anyhow::Error::msg)?;
    write_host_files(&profile, &input, &source, &launcher, platform)?;
    if platform == Platform::Windows {
        prepare_windows_tool(
            &work.join(WINDOWS_SUNSHINE),
            &profile.transport_tools.join("sunshine"),
            "Sunshine/sunshine.exe",
        )?;
    }
    Ok(HostSetup {
        config: profile.sunshine_config,
        apps: profile.sunshine_apps,
    })
}

pub fn start_host(work: &Path, profile_name: &str, interface: IpAddr) -> Result<HostProcess> {
    ensure!(
        private_or_local_ipv4(interface),
        "Sunshine transport requires a selected private, loopback, or link-local IPv4 interface"
    );
    let profile = ProfilePaths::discover(profile_name)?;
    let input = profile.require_input()?;
    input
        .ruffle_mappings()
        .context("input profile cannot be applied by the pinned host runtime")?;
    ensure!(
        profile.sunshine_config.is_file() && profile.sunshine_apps.is_file(),
        "isolated Sunshine profile is missing; run `kof-wing --source <DIR> --profile {profile_name} lan setup-host`"
    );
    ensure_host_input_channels(&profile, &input)?;
    let platform = Platform::current().map_err(anyhow::Error::msg)?;
    let (program, mut args) = sunshine_command(work, &profile, platform)?;
    args.push(format!("bind_address={interface}").into());
    args.push("upnp=disabled".into());
    let mut command = Command::new(&program);
    command.args(&args);
    if platform == Platform::Linux {
        command
            .env("XDG_CONFIG_HOME", sunshine_xdg_config_home(&profile)?)
            .env_remove("CONFIGURATION_DIRECTORY")
            .env("SUNSHINE_MIGRATE_CONFIG", "0");
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map(HostProcess)
        .with_context(|| format!("cannot start pinned Sunshine {}", program.display()))
}

pub fn pair_client(work: &Path, profile_name: &str, host: IpAddr) -> Result<()> {
    run_moonlight(work, profile_name, pair_arguments(host))
}

pub fn stream_client(work: &Path, profile_name: &str, host: IpAddr) -> Result<()> {
    let profile = ProfilePaths::discover(profile_name)?;
    let input = profile.require_input()?;
    run_moonlight(
        work,
        profile_name,
        stream_arguments(host, input.enabled_gamepad_devices() > 1),
    )
}

fn stream_arguments(host: IpAddr, multi_controller: bool) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = [
        "stream",
        "--720",
        "--fps",
        STREAM_FPS,
        "--bitrate",
        STREAM_BITRATE_KBPS,
        "--packet-size",
        STREAM_PACKET_SIZE,
        "--display-mode",
        "fullscreen",
        "--vsync",
        "--audio-config",
        "stereo",
        "--no-absolute-mouse",
        "--no-mouse-buttons-swap",
        "--no-touchscreen-trackpad",
        "--game-optimization",
        "--no-audio-on-host",
        "--frame-pacing",
        "--no-mute-on-focus-loss",
        "--no-background-gamepad",
        "--no-reverse-scroll-direction",
        "--no-swap-gamepad-buttons",
        "--keep-awake",
        "--no-performance-overlay",
        "--no-hdr",
        "--no-yuv444",
        "--capture-system-keys",
        "fullscreen",
        "--video-codec",
        STREAM_VIDEO_CODEC,
        "--video-decoder",
        "auto",
        "--quit-after",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([host.to_string().into(), KOF_APP_NAME.into()])
    .collect();
    arguments.insert(
        13,
        if multi_controller {
            "--multi-controller".into()
        } else {
            "--no-multi-controller".into()
        },
    );
    arguments
}

fn pair_arguments(host: IpAddr) -> Vec<OsString> {
    vec!["pair".into(), host.to_string().into()]
}

fn run_moonlight(work: &Path, profile_name: &str, action: Vec<OsString>) -> Result<()> {
    ensure!(
        private_or_local_ipv4(
            action
                .iter()
                .find_map(|argument| argument.to_str()?.parse::<IpAddr>().ok())
                .context("Moonlight action omitted the host address")?
        ),
        "Moonlight transport requires a private, loopback, or link-local IPv4 host"
    );
    let profile = ProfilePaths::discover(profile_name)?;
    profile.require_config()?;
    let platform = Platform::current().map_err(anyhow::Error::msg)?;
    let (program, mut args) = moonlight_command(work, &profile, platform)?;
    args.extend(action);
    for directory in [
        &profile.moonlight_config_home,
        &profile.moonlight_data_home,
        &profile.moonlight_cache_home,
    ] {
        fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
    }
    let mut command = moonlight_process(&program, &args, &profile, platform)?;
    let status = command
        .status()
        .with_context(|| format!("cannot start pinned Moonlight {}", program.display()))?;
    ensure!(
        status.success(),
        "Moonlight exited unsuccessfully with {status}"
    );
    Ok(())
}

fn moonlight_process(
    program: &Path,
    args: &[OsString],
    profile: &ProfilePaths,
    platform: Platform,
) -> Result<Command> {
    let mut command = Command::new(program);
    command
        .args(args)
        .env("XDG_CONFIG_HOME", &profile.moonlight_config_home)
        .env("XDG_DATA_HOME", &profile.moonlight_data_home)
        .env("XDG_CACHE_HOME", &profile.moonlight_cache_home)
        .env("TMPDIR", &profile.moonlight_cache_home);
    if platform == Platform::Windows {
        command.current_dir(moonlight_portable_directory(program)?);
    }
    Ok(command)
}

fn write_host_files(
    profile: &ProfilePaths,
    input: &InputConfig,
    source: &Path,
    launcher: &Path,
    platform: Platform,
) -> Result<()> {
    for directory in [
        profile.sunshine_config.parent(),
        profile.sunshine_state.parent(),
        profile.sunshine_log.parent(),
    ]
    .into_iter()
    .flatten()
    {
        fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
    }
    let apps = serde_json::to_string_pretty(&AppsFile {
        env: app_environment(profile, platform)?,
        apps: vec![App {
            name: KOF_APP_NAME,
            cmd: launch_command(launcher, source, &profile.name, platform)?,
            auto_detach: false,
            wait_all: true,
        }],
    })?;
    atomic_write(&profile.sunshine_apps, format!("{apps}\n").as_bytes())?;
    let credentials = profile
        .sunshine_state
        .parent()
        .context("Sunshine state path has no parent")?
        .join("credentials");
    fs::create_dir_all(&credentials)?;
    let file_apps = sunshine_path_value(&profile.sunshine_apps)?;
    let file_state = sunshine_path_value(&profile.sunshine_state)?;
    let log_path = sunshine_path_value(&profile.sunshine_log)?;
    let pkey_path = credentials.join("cakey.pem");
    let cert_path = credentials.join("cacert.pem");
    let pkey = sunshine_path_value(&pkey_path)?;
    let cert = sunshine_path_value(&cert_path)?;
    let config = format!(
        "sunshine_name = KOF Wing {}\nfile_apps = {}\nfile_state = {}\ncredentials_file = {}\nlog_path = {}\npkey = {}\ncert = {}\nupnp = disabled\naddress_family = ipv4\nexternal_ip = 127.0.0.1\norigin_web_ui_allowed = pc\nlan_encryption_mode = 2\nwan_encryption_mode = 2\nnotify_pre_releases = disabled\nsystem_tray = disabled\ncontroller = {}\nkeyboard = {}\nmouse = disabled\n",
        profile.name,
        file_apps,
        file_state,
        file_state,
        log_path,
        pkey,
        cert,
        sunshine_toggle(input.enabled_gamepad_devices() > 0),
        sunshine_toggle(input.enabled_keyboard_devices() > 0),
    );
    atomic_write(&profile.sunshine_config, config.as_bytes())
}

fn ensure_host_input_channels(profile: &ProfilePaths, input: &InputConfig) -> Result<()> {
    let config = fs::read_to_string(&profile.sunshine_config)
        .with_context(|| format!("cannot read {}", profile.sunshine_config.display()))?;
    for (key, expected) in [
        (
            "controller",
            sunshine_toggle(input.enabled_gamepad_devices() > 0),
        ),
        (
            "keyboard",
            sunshine_toggle(input.enabled_keyboard_devices() > 0),
        ),
    ] {
        let values: Vec<_> = config
            .lines()
            .filter_map(|line| line.strip_prefix(&format!("{key} = ")))
            .collect();
        ensure!(
            values == [expected],
            "isolated Sunshine config has stale or invalid {key} input policy; rerun `kof-wing --source <DIR> --profile {} lan setup-host`",
            profile.name
        );
    }
    Ok(())
}

const fn sunshine_toggle(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

fn sunshine_path_value(path: &Path) -> Result<&str> {
    let value = path
        .to_str()
        .with_context(|| format!("Sunshine path is not valid Unicode: {}", path.display()))?;
    ensure!(
        !value.contains(['\r', '\n', '\0']),
        "Sunshine path contains an unsupported control character: {}",
        path.display()
    );
    Ok(value)
}

fn sunshine_xdg_config_home(profile: &ProfilePaths) -> Result<&Path> {
    profile
        .sunshine_state
        .parent()
        .and_then(Path::parent)
        .context("Sunshine state path has no profile data root")
}

fn moonlight_portable_directory(program: &Path) -> Result<&Path> {
    program
        .parent()
        .context("portable Moonlight executable has no parent directory")
}

fn app_environment(
    profile: &ProfilePaths,
    platform: Platform,
) -> Result<std::collections::BTreeMap<String, String>> {
    if platform == Platform::Windows {
        return Ok(std::collections::BTreeMap::new());
    }
    [
        ("XDG_CONFIG_HOME", &profile.config_home),
        ("XDG_DATA_HOME", &profile.data_home),
        ("XDG_CACHE_HOME", &profile.cache_home),
    ]
    .into_iter()
    .map(|(name, path)| Ok((name.to_owned(), sunshine_path_value(path)?.to_owned())))
    .collect()
}

fn private_or_local_ipv4(address: IpAddr) -> bool {
    let IpAddr::V4(address) = address else {
        return false;
    };
    address.is_private() || address.is_loopback() || address.is_link_local()
}

fn launch_command(
    launcher: &Path,
    source: &Path,
    profile_name: &str,
    platform: Platform,
) -> Result<String> {
    [
        launcher.as_os_str(),
        OsStr::new("--source"),
        source.as_os_str(),
        OsStr::new("--profile"),
        OsStr::new(profile_name),
        OsStr::new("play"),
        OsStr::new("--fullscreen"),
    ]
    .into_iter()
    .map(|part| quote_command_part(part, platform))
    .collect::<Result<Vec<_>>>()
    .map(|parts| parts.join(" "))
}

fn quote_command_part(part: &OsStr, platform: Platform) -> Result<String> {
    let text = part.to_str().with_context(|| {
        format!(
            "command path is not valid Unicode: {}",
            Path::new(part).display()
        )
    })?;
    match platform {
        Platform::Linux => Ok(format!("'{}'", text.replace('\'', "'\\''"))),
        Platform::Windows => Ok(quote_windows_argument(text)),
    }
}

fn quote_windows_argument(value: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in value.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(character);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

fn sunshine_command(
    work: &Path,
    profile: &ProfilePaths,
    platform: Platform,
) -> Result<(PathBuf, Vec<OsString>)> {
    match platform {
        Platform::Linux => {
            let appimage = work.join(LINUX_SUNSHINE);
            verify_hash(
                &appimage,
                LINUX_SUNSHINE_SHA256,
                "pinned Linux Sunshine AppImage",
            )?;
            Ok((appimage, sunshine_appimage_arguments(profile)))
        }
        Platform::Windows => {
            let archive = work.join(WINDOWS_SUNSHINE);
            verify_hash(
                &archive,
                WINDOWS_SUNSHINE_SHA256,
                "pinned Windows Sunshine archive",
            )?;
            let executable = prepare_windows_tool(
                &archive,
                &profile.transport_tools.join("sunshine"),
                "Sunshine/sunshine.exe",
            )?;
            Ok((
                executable,
                vec![
                    "-0".into(),
                    profile.sunshine_config.clone().into_os_string(),
                ],
            ))
        }
    }
}

fn sunshine_appimage_arguments(profile: &ProfilePaths) -> Vec<OsString> {
    vec![
        "--appimage-extract-and-run".into(),
        "-0".into(),
        profile.sunshine_config.clone().into_os_string(),
    ]
}

fn moonlight_command(
    work: &Path,
    profile: &ProfilePaths,
    platform: Platform,
) -> Result<(PathBuf, Vec<OsString>)> {
    match platform {
        Platform::Linux => {
            let appimage = work.join(LINUX_MOONLIGHT);
            verify_hash(
                &appimage,
                LINUX_MOONLIGHT_SHA256,
                "pinned Linux Moonlight AppImage",
            )?;
            Ok((appimage, vec!["--appimage-extract-and-run".into()]))
        }
        Platform::Windows => {
            let archive = work.join(WINDOWS_MOONLIGHT);
            verify_hash(
                &archive,
                WINDOWS_MOONLIGHT_SHA256,
                "pinned Windows Moonlight archive",
            )?;
            let executable = prepare_windows_tool(
                &archive,
                &profile.transport_tools.join("moonlight"),
                "Moonlight.exe",
            )?;
            Ok((executable, Vec::new()))
        }
    }
}

fn prepare_windows_tool(archive: &Path, destination: &Path, executable: &str) -> Result<PathBuf> {
    let target = destination.join(executable);
    ensure_real_directory(destination)?;
    if extracted_archive_matches(archive, destination)? {
        ensure!(
            fs::symlink_metadata(&target)
                .map(|metadata| metadata.is_file())
                .unwrap_or(false),
            "archive did not contain expected executable {executable}"
        );
        return Ok(target);
    }
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("cannot read ZIP archive {}", archive.display()))?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        reject_unsupported_zip_entry(&entry)?;
        let relative = entry
            .enclosed_name()
            .with_context(|| format!("unsafe ZIP entry {}", entry.name()))?;
        let output = destination.join(&relative);
        if entry.is_dir() {
            ensure_archive_directories(destination, &relative)?;
        } else {
            if let Some(parent) = relative.parent() {
                ensure_archive_directories(destination, parent)?;
            }
            let mut temporary = NamedTempFile::new_in(
                output
                    .parent()
                    .context("archive output path has no parent directory")?,
            )?;
            io::copy(&mut entry, temporary.as_file_mut())?;
            temporary.as_file_mut().sync_all()?;
            temporary
                .persist(&output)
                .map_err(|error| error.error)
                .with_context(|| format!("cannot install extracted file {}", output.display()))?;
        }
    }
    ensure!(
        extracted_archive_matches(archive, destination)?,
        "extracted Windows tool does not match its pinned archive"
    );
    ensure!(
        fs::symlink_metadata(&target)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false),
        "archive did not contain expected executable {executable}"
    );
    Ok(target)
}

fn extracted_archive_matches(archive: &Path, destination: &Path) -> Result<bool> {
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("cannot read ZIP archive {}", archive.display()))?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        reject_unsupported_zip_entry(&entry)?;
        let relative = entry
            .enclosed_name()
            .with_context(|| format!("unsafe ZIP entry {}", entry.name()))?;
        let parent = if entry.is_dir() {
            relative.as_path()
        } else {
            relative.parent().unwrap_or_else(|| Path::new(""))
        };
        if !archive_directories_are_real(destination, parent)? {
            return Ok(false);
        }
        let output = destination.join(&relative);
        let Ok(metadata) = fs::symlink_metadata(&output) else {
            return Ok(false);
        };
        if entry.is_dir() {
            if !metadata.is_dir() {
                return Ok(false);
            }
        } else if !metadata.is_file()
            || sha256_reader(&mut entry)? != sha256_reader(File::open(&output)?)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn reject_unsupported_zip_entry(entry: &zip::read::ZipFile<'_, File>) -> Result<()> {
    if let Some(mode) = entry.unix_mode() {
        let file_type = mode & 0o170_000;
        ensure!(
            matches!(file_type, 0 | 0o040_000 | 0o100_000),
            "unsupported non-file ZIP entry {}",
            entry.name()
        );
    }
    Ok(())
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Windows tool directory is not a real directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .with_context(|| format!("cannot create {}", path.display()))?;
            let metadata = fs::symlink_metadata(path)?;
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Windows tool directory is not a real directory: {}",
                path.display()
            );
        }
        Err(error) => {
            return Err(error).with_context(|| format!("cannot inspect {}", path.display()));
        }
    }
    Ok(())
}

fn ensure_archive_directories(root: &Path, relative: &Path) -> Result<()> {
    let mut directory = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            anyhow::bail!("unsafe ZIP directory {}", relative.display());
        };
        directory.push(component);
        ensure_real_directory(&directory)?;
    }
    Ok(())
}

fn archive_directories_are_real(root: &Path, relative: &Path) -> Result<bool> {
    let mut directory = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            anyhow::bail!("unsafe ZIP directory {}", relative.display());
        };
        directory.push(component);
        let Ok(metadata) = fs::symlink_metadata(&directory) else {
            return Ok(false);
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn sha256_reader(mut reader: impl Read) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, net::Ipv4Addr};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn transport_contract_pins_all_exact_hashes_and_versions() {
        let actual = contract();
        for required in [
            SUNSHINE_VERSION,
            MOONLIGHT_VERSION,
            LINUX_SUNSHINE_SHA256,
            WINDOWS_SUNSHINE_SHA256,
            LINUX_MOONLIGHT_SHA256,
            WINDOWS_MOONLIGHT_SHA256,
            "pairing=explicit-pin",
            "bitrate-kbps=10000",
            "packet-size=1392",
            "audio-config=stereo",
            "audio-on-host=false",
            "multi-controller=input-profile-derived",
            "game-optimization=true",
            "frame-pacing=true",
            "video-codec=H.264",
            "video-decoder=auto",
            "quit-after=true",
            "lan-encryption=mandatory",
            "bind-policy=selected-private-or-local-ipv4",
        ] {
            assert!(
                actual.contains(required),
                "transport contract omitted {required}"
            );
        }
    }

    #[test]
    fn generated_host_files_expose_only_kof_and_harden_sunshine() {
        let temp = tempdir().expect("temp dir");
        let profile = test_profile(temp.path());
        write_host_files(
            &profile,
            &InputConfig::preset(crate::input::InputPreset::Modern),
            Path::new("/games/KOF Wing"),
            Path::new("/opt/kof wing/kof-wing"),
            Platform::Linux,
        )
        .expect("write host files");
        let apps = fs::read_to_string(&profile.sunshine_apps).expect("read apps");
        assert!(apps.contains(KOF_APP_NAME));
        assert!(apps.contains("'/opt/kof wing/kof-wing' '--source' '/games/KOF Wing'"));
        assert!(!apps.contains("Desktop"));
        for root in [
            &profile.config_home,
            &profile.data_home,
            &profile.cache_home,
        ] {
            assert!(apps.contains(root.to_str().unwrap()));
        }
        let config = fs::read_to_string(&profile.sunshine_config).expect("read config");
        for required in [
            "upnp = disabled",
            "origin_web_ui_allowed = pc",
            "external_ip = 127.0.0.1",
            "lan_encryption_mode = 2",
            "system_tray = disabled",
            "controller = enabled",
            "keyboard = disabled",
            "mouse = disabled",
        ] {
            assert!(config.contains(required), "config omitted {required}");
        }
        ensure_host_input_channels(
            &profile,
            &InputConfig::preset(crate::input::InputPreset::Modern),
        )
        .expect("generated channels match input");
        let error = ensure_host_input_channels(&profile, &InputConfig::default()).unwrap_err();
        assert!(error.to_string().contains("stale"));
    }

    #[test]
    fn sunshine_paths_reject_config_line_injection() {
        let temp = tempdir().expect("temp dir");
        let profile = test_profile(&temp.path().join("unsafe\nupnp = enabled"));
        let error = write_host_files(
            &profile,
            &InputConfig::default(),
            Path::new("/games/KOF Wing"),
            Path::new("/opt/kof-wing"),
            Platform::Linux,
        )
        .unwrap_err();
        assert!(error.to_string().contains("control character"));
    }

    #[test]
    fn windows_argument_quoting_handles_quotes_and_trailing_backslashes() {
        assert_eq!(
            quote_windows_argument(r"C:\Program Files\"),
            "\"C:\\Program Files\\\\\""
        );
        assert_eq!(quote_windows_argument(r#"a"b"#), "\"a\\\"b\"");
    }

    #[test]
    fn windows_tool_extraction_repairs_tampered_archive_files() {
        let temp = tempdir().expect("temp dir");
        let archive = temp.path().join("tool.zip");
        let file = File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("Moonlight.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"pinned executable").unwrap();
        zip.finish().unwrap();

        let destination = temp.path().join("portable");
        let executable = prepare_windows_tool(&archive, &destination, "Moonlight.exe").unwrap();
        fs::write(&executable, b"tampered").unwrap();
        prepare_windows_tool(&archive, &destination, "Moonlight.exe").unwrap();
        assert_eq!(fs::read(executable).unwrap(), b"pinned executable");
    }

    #[cfg(unix)]
    #[test]
    fn windows_tool_extraction_rejects_symlinked_destination() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp dir");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let destination = temp.path().join("portable");
        symlink(&outside, &destination).unwrap();
        let error =
            prepare_windows_tool(&temp.path().join("missing.zip"), &destination, "tool.exe")
                .unwrap_err();
        assert!(error.to_string().contains("real directory"));
    }

    #[test]
    fn exact_appimage_and_moonlight_stream_arguments_are_stable() {
        let temp = tempdir().expect("temp dir");
        let profile = test_profile(temp.path());
        assert_eq!(
            sunshine_appimage_arguments(&profile),
            vec![
                OsString::from("--appimage-extract-and-run"),
                OsString::from("-0"),
                profile.sunshine_config.clone().into_os_string(),
            ]
        );
        assert_eq!(
            pair_arguments(Ipv4Addr::new(192, 168, 1, 20).into()),
            vec![OsString::from("pair"), OsString::from("192.168.1.20")]
        );
        assert_eq!(
            stream_arguments(Ipv4Addr::new(192, 168, 1, 20).into(), true),
            [
                "stream",
                "--720",
                "--fps",
                "60",
                "--bitrate",
                "10000",
                "--packet-size",
                "1392",
                "--display-mode",
                "fullscreen",
                "--vsync",
                "--audio-config",
                "stereo",
                "--multi-controller",
                "--no-absolute-mouse",
                "--no-mouse-buttons-swap",
                "--no-touchscreen-trackpad",
                "--game-optimization",
                "--no-audio-on-host",
                "--frame-pacing",
                "--no-mute-on-focus-loss",
                "--no-background-gamepad",
                "--no-reverse-scroll-direction",
                "--no-swap-gamepad-buttons",
                "--keep-awake",
                "--no-performance-overlay",
                "--no-hdr",
                "--no-yuv444",
                "--capture-system-keys",
                "fullscreen",
                "--video-codec",
                "H.264",
                "--video-decoder",
                "auto",
                "--quit-after",
                "192.168.1.20",
                KOF_APP_NAME,
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
        assert!(
            stream_arguments(Ipv4Addr::LOCALHOST.into(), false)
                .contains(&OsString::from("--no-multi-controller"))
        );
        assert_ne!(profile.moonlight_config_home, profile.moonlight_data_home);
        assert_ne!(profile.moonlight_config_home, profile.moonlight_cache_home);
        assert_eq!(
            sunshine_xdg_config_home(&profile).unwrap(),
            profile.data_home.join("profiles/arcade")
        );
        assert_eq!(
            moonlight_portable_directory(Path::new(
                "/profiles/arcade/transport-tools/moonlight/Moonlight.exe"
            ))
            .unwrap(),
            Path::new("/profiles/arcade/transport-tools/moonlight")
        );
        let windows_program = Path::new("/profiles/arcade/transport-tools/moonlight/Moonlight.exe");
        let windows = moonlight_process(windows_program, &[], &profile, Platform::Windows).unwrap();
        assert_eq!(
            windows.get_current_dir(),
            Some(Path::new("/profiles/arcade/transport-tools/moonlight"))
        );
        let linux = moonlight_process(
            Path::new("/tools/moonlight"),
            &[],
            &profile,
            Platform::Linux,
        )
        .unwrap();
        let environment = linux
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_os_string(),
                    value.expect("environment value").to_os_string(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            environment.get(OsStr::new("TMPDIR")),
            Some(&profile.moonlight_cache_home.clone().into_os_string())
        );
        assert!(private_or_local_ipv4(Ipv4Addr::new(192, 168, 1, 20).into()));
        assert!(!private_or_local_ipv4(Ipv4Addr::new(8, 8, 8, 8).into()));
    }

    fn test_profile(root: &Path) -> ProfilePaths {
        ProfilePaths::from_roots(
            "arcade",
            root.join("config"),
            root.join("data"),
            root.join("cache"),
        )
    }
}
