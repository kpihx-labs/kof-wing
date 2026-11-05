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
fn lan_doctor_is_explicitly_diagnostic_only() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .args(["lan", "doctor"])
        .output()
        .expect("run LAN doctor");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.contains("not implemented"));
    assert!(stdout.contains("unavailable by design"));
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
