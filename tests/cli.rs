use std::process::Command;

use tempfile::tempdir;

#[test]
fn help_exposes_preservation_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .arg("--help")
        .output()
        .expect("run launcher help");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    for command in ["doctor", "play", "profile", "lan"] {
        assert!(stdout.contains(command), "help omitted {command}");
    }
}

#[test]
fn doctor_without_source_fails_with_actionable_message() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .env_remove("KOF_SOURCE")
        .arg("doctor")
        .output()
        .expect("run launcher doctor");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("--source <DIR>"));
}

#[test]
fn lan_doctor_reports_negotiation_without_claiming_gameplay() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .args(["lan", "doctor"])
        .output()
        .expect("run LAN doctor");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.contains("not implemented"));
    assert!(stdout.contains("[implemented] discovery and strict session negotiation"));
    assert!(!stdout.contains("[ok] discovery and strict session negotiation implemented"));
    assert!(stdout.contains("gameplay readiness: unavailable"));
}

#[test]
fn lan_help_exposes_all_zero_service_commands_and_network_controls() {
    let binary = env!("CARGO_BIN_EXE_kof-wing");
    let lan = Command::new(binary)
        .args(["lan", "--help"])
        .output()
        .expect("run LAN help");
    assert!(lan.status.success());
    let stdout = String::from_utf8(lan.stdout).expect("UTF-8 LAN help");
    for command in ["doctor", "discover", "host", "join"] {
        assert!(stdout.contains(command), "LAN help omitted {command}");
    }

    for command in ["doctor", "discover", "host", "join"] {
        let output = Command::new(binary)
            .args(["lan", command, "--help"])
            .output()
            .expect("run LAN command help");
        assert!(output.status.success(), "{command} help failed");
        let help = String::from_utf8(output.stdout).expect("UTF-8 command help");
        for option in [
            "--bind",
            "--interface",
            "--prefix-len",
            "--discovery-port",
            "--session-port",
            "--allow-public",
        ] {
            assert!(help.contains(option), "{command} help omitted {option}");
        }
    }
}

#[test]
fn lan_discovery_rejects_public_interface_by_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .args(["lan", "discover", "--interface", "8.8.8.8"])
        .output()
        .expect("run LAN discovery policy check");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("not private, loopback, or link-local"));
}

#[test]
fn profile_create_and_list_use_platform_directories() {
    let temp = tempdir().expect("temp dir");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let cache_home = temp.path().join("cache");
    let work = temp.path().join("work");
    let binary = env!("CARGO_BIN_EXE_kof-wing");

    let create = Command::new(binary)
        .args(["profile", "create", "arcade"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("KOF_WING_WORK", &work)
        .output()
        .expect("create profile");
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );

    let list = Command::new(binary)
        .args(["profile", "list"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("KOF_WING_WORK", &work)
        .output()
        .expect("list profiles");
    assert!(list.status.success());
    assert_eq!(
        String::from_utf8(list.stdout).expect("UTF-8 list"),
        "arcade\n"
    );
    assert!(data_home.join("kof-wing/profiles/arcade/saves").is_dir());
    assert!(cache_home.join("kof-wing/profiles/arcade").is_dir());
}
