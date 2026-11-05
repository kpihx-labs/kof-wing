use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{profile::ProfilePaths, runtime::Platform, source::verify_hash};

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
        "sunshine={SUNSHINE_VERSION};linux-sunshine={LINUX_SUNSHINE_SHA256};windows-sunshine={WINDOWS_SUNSHINE_SHA256};moonlight={MOONLIGHT_VERSION};linux-moonlight={LINUX_MOONLIGHT_SHA256};windows-moonlight={WINDOWS_MOONLIGHT_SHA256};app={KOF_APP_NAME};resolution=1280x720;fps={STREAM_FPS};bitrate-kbps={STREAM_BITRATE_KBPS};packet-size={STREAM_PACKET_SIZE};display-mode=fullscreen;vsync=true;audio-config=stereo;audio-on-host=false;multi-controller=true;game-optimization=true;frame-pacing=true;video-codec={STREAM_VIDEO_CODEC};video-decoder=auto;quit-after=true;absolute-mouse=false;mouse-buttons-swap=false;touchscreen-trackpad=false;mute-on-focus-loss=false;background-gamepad=false;reverse-scroll=false;swap-gamepad-buttons=false;keep-awake=true;performance-overlay=false;hdr=false;yuv444=false;capture-system-keys=fullscreen;pairing=explicit-pin;pin-stdin=true;upnp=false;web-ui-origin=pc;lan-encryption=mandatory;wan-encryption=mandatory;bind-policy=selected-private-or-local-ipv4"
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
    let source = source
        .canonicalize()
        .with_context(|| format!("cannot resolve game source {}", source.display()))?;
    let launcher = launcher
        .canonicalize()
        .with_context(|| format!("cannot resolve launcher {}", launcher.display()))?;
    let platform = Platform::current().map_err(anyhow::Error::msg)?;
    write_host_files(&profile, &source, &launcher, platform)?;
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
    ensure!(
        profile.sunshine_config.is_file() && profile.sunshine_apps.is_file(),
        "isolated Sunshine profile is missing; run `kof-wing --source <DIR> --profile {profile_name} lan setup-host`"
    );
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
    run_moonlight(work, profile_name, stream_arguments(host))
}

fn stream_arguments(host: IpAddr) -> Vec<OsString> {
    [
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
        STREAM_VIDEO_CODEC,
        "--video-decoder",
        "auto",
        "--quit-after",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([host.to_string().into(), KOF_APP_NAME.into()])
    .collect()
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
        env: app_environment(profile, platform),
        apps: vec![App {
            name: KOF_APP_NAME,
            cmd: launch_command(launcher, source, &profile.name, platform)?,
            auto_detach: false,
            wait_all: true,
        }],
    })?;
    fs::write(&profile.sunshine_apps, format!("{apps}\n"))
        .with_context(|| format!("cannot write {}", profile.sunshine_apps.display()))?;
    let credentials = profile
        .sunshine_state
        .parent()
        .context("Sunshine state path has no parent")?
        .join("credentials");
    fs::create_dir_all(&credentials)?;
    let config = format!(
        "sunshine_name = KOF Wing {}\nfile_apps = {}\nfile_state = {}\ncredentials_file = {}\nlog_path = {}\npkey = {}\ncert = {}\nupnp = disabled\naddress_family = ipv4\nexternal_ip = 127.0.0.1\norigin_web_ui_allowed = pc\nlan_encryption_mode = 2\nwan_encryption_mode = 2\nnotify_pre_releases = disabled\nsystem_tray = disabled\ncontroller = enabled\nkeyboard = enabled\nmouse = disabled\n",
        profile.name,
        profile.sunshine_apps.display(),
        profile.sunshine_state.display(),
        profile.sunshine_state.display(),
        profile.sunshine_log.display(),
        credentials.join("cakey.pem").display(),
        credentials.join("cacert.pem").display(),
    );
    fs::write(&profile.sunshine_config, config)
        .with_context(|| format!("cannot write {}", profile.sunshine_config.display()))
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
) -> std::collections::BTreeMap<String, String> {
    if platform == Platform::Windows {
        return std::collections::BTreeMap::new();
    }
    [
        ("XDG_CONFIG_HOME", &profile.config_home),
        ("XDG_DATA_HOME", &profile.data_home),
        ("XDG_CACHE_HOME", &profile.cache_home),
    ]
    .into_iter()
    .map(|(name, path)| (name.to_owned(), path.to_string_lossy().into_owned()))
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
        Platform::Windows => Ok(format!("\"{}\"", text.replace('"', "\\\""))),
    }
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
    if target.is_file() {
        return Ok(target);
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("cannot create {}", destination.display()))?;
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("cannot read ZIP archive {}", archive.display()))?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .with_context(|| format!("unsafe ZIP entry {}", entry.name()))?;
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output_file = File::create(&output)?;
            io::copy(&mut entry, &mut output_file)?;
        }
    }
    ensure!(
        target.is_file(),
        "archive did not contain expected executable {executable}"
    );
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::{fs, net::Ipv4Addr};

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
            "mouse = disabled",
        ] {
            assert!(config.contains(required), "config omitted {required}");
        }
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
            stream_arguments(Ipv4Addr::new(192, 168, 1, 20).into()),
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
