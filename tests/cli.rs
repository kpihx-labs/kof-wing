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
    for command in ["doctor", "play", "profile", "input", "lan"] {
        assert!(stdout.contains(command), "help omitted {command}");
    }
}

#[test]
fn play_help_exposes_non_executing_command_inspection() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .args(["play", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("--dry-run"));
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
    assert!(stdout.contains("[not verified] physical two-PC gameplay"));
    assert!(stdout.contains("optional Sunshine/Moonlight transport"));
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
    for command in ["doctor", "setup-host", "discover", "host", "join", "pair"] {
        assert!(stdout.contains(command), "LAN help omitted {command}");
    }

    for command in ["doctor", "discover", "host", "join", "pair"] {
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
fn transport_flags_and_explicit_pairing_are_documented_by_cli() {
    let binary = env!("CARGO_BIN_EXE_kof-wing");
    let host = Command::new(binary)
        .args(["lan", "host", "--help"])
        .output()
        .expect("run host help");
    let host_help = String::from_utf8(host.stdout).expect("UTF-8 host help");
    assert!(host_help.contains("--transport"));
    assert!(host_help.contains("sunshine"));

    let join = Command::new(binary)
        .args(["lan", "join", "--help"])
        .output()
        .expect("run join help");
    let join_help = String::from_utf8(join.stdout).expect("UTF-8 join help");
    assert!(join_help.contains("--transport"));
    assert!(join_help.contains("moonlight"));

    let pair = Command::new(binary)
        .args(["lan", "pair", "--help"])
        .output()
        .expect("run pair help");
    let pair_help = String::from_utf8(pair.stdout).expect("UTF-8 pair help");
    assert!(pair_help.contains("one-time Moonlight PIN pairing"));
    assert!(!pair_help.contains("--pin"));
}

#[test]
fn readme_keeps_pairing_hitl_and_physical_gate_truthful() {
    let readme = include_str!("../README.md");
    for required in [
        "lan setup-host",
        "--transport sunshine",
        "lan pair",
        "--transport moonlight",
        "physical two-PC",
        "--multi-controller",
        "--appimage-extract-and-run",
        "portable.dat",
        "origin_web_ui_allowed = pc",
        "non-config state",
        "saved Moonlight preferences cannot silently change a session",
    ] {
        assert!(readme.contains(required), "README omitted {required}");
    }
    assert!(readme.contains("not yet proof of full gameplay"));
}

#[test]
fn lan_discovery_rejects_explicit_nonexistent_interface() {
    let output = Command::new(env!("CARGO_BIN_EXE_kof-wing"))
        .args(["lan", "discover", "--interface", "8.8.8.8"])
        .output()
        .expect("run LAN discovery policy check");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("is not assigned to this host"));
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
    assert!(
        config_home
            .join("kof-wing/profiles/arcade/input.toml")
            .is_file()
    );
}

#[test]
fn input_cli_covers_empty_presets_set_conflicts_and_backend_limits() {
    let temp = tempdir().expect("temp dir");
    let binary = env!("CARGO_BIN_EXE_kof-wing");
    let environment = [
        ("XDG_CONFIG_HOME", temp.path().join("config")),
        ("XDG_DATA_HOME", temp.path().join("data")),
        ("XDG_CACHE_HOME", temp.path().join("cache")),
        ("KOF_WING_WORK", temp.path().join("work")),
    ];
    let run = |arguments: &[&str]| {
        let mut command = Command::new(binary);
        command.args(arguments);
        for (name, value) in &environment {
            command.env(name, value);
        }
        command.output().expect("run input command")
    };

    let help = run(&["input", "--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("UTF-8 input help");
    for command in ["show", "set", "reset", "import", "export", "validate"] {
        assert!(help.contains(command), "input help omitted {command}");
    }
    let set_help = run(&["input", "set", "--help"]);
    let set_help = String::from_utf8(set_help.stdout).expect("UTF-8 input set help");
    for option in ["--clear-selector", "--clear-deadzone"] {
        assert!(set_help.contains(option), "input set help omitted {option}");
    }

    assert!(run(&["profile", "create", "inputs"]).status.success());
    let empty = run(&["--profile", "inputs", "input", "validate"]);
    assert!(empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stdout).contains("mappings: 0"));

    assert!(
        run(&[
            "--profile",
            "inputs",
            "input",
            "reset",
            "--preset",
            "modern",
        ])
        .status
        .success()
    );
    let show = run(&["--profile", "inputs", "input", "show", "--format", "json"]);
    let json = String::from_utf8(show.stdout).expect("UTF-8 input JSON");
    assert!(json.contains("\"version\": 1"));
    assert!(json.contains("\"preset\": \"modern\""));

    let conflict = run(&[
        "--profile",
        "inputs",
        "input",
        "set",
        "--player",
        "p2",
        "--device",
        "pad2",
        "--kind",
        "gamepad",
        "--control",
        "button:south",
        "--target",
        "j",
    ]);
    assert!(!conflict.status.success());
    assert!(
        String::from_utf8_lossy(&conflict.stderr).contains("Ruffle conflict"),
        "{}",
        String::from_utf8_lossy(&conflict.stderr)
    );

    assert!(
        run(&["--profile", "inputs", "input", "reset"])
            .status
            .success()
    );
    let axis = run(&[
        "--profile",
        "inputs",
        "input",
        "set",
        "--player",
        "p1",
        "--device",
        "pad",
        "--kind",
        "gamepad",
        "--control",
        "axis:left-x:positive",
        "--target",
        "right",
        "--selector",
        "primary",
        "--deadzone",
        "0.15",
        "--threshold",
        "0.6",
    ]);
    assert!(axis.status.success());
    let validation = run(&["--profile", "inputs", "input", "validate"]);
    assert!(validation.status.success());
    let report = String::from_utf8(validation.stdout).expect("UTF-8 validation");
    assert!(report.contains("no axis mapping argument"));
    assert!(report.contains("no deadzone argument"));

    let clear = run(&[
        "--profile",
        "inputs",
        "input",
        "set",
        "--player",
        "p1",
        "--device",
        "pad",
        "--kind",
        "gamepad",
        "--control",
        "axis:left-x:positive",
        "--target",
        "right",
        "--threshold",
        "0.6",
        "--clear-selector",
        "--clear-deadzone",
    ]);
    assert!(clear.status.success());
    let show = run(&["--profile", "inputs", "input", "show", "--format", "json"]);
    let json = String::from_utf8(show.stdout).expect("UTF-8 cleared input JSON");
    assert!(!json.contains("\"selector\""));
    assert!(!json.contains("\"deadzone\""));

    for conflicting in [
        ["--selector", "primary", "--clear-selector"],
        ["--deadzone", "0.1", "--clear-deadzone"],
    ] {
        let mut arguments = vec![
            "--profile",
            "inputs",
            "input",
            "set",
            "--player",
            "p1",
            "--device",
            "pad",
            "--kind",
            "gamepad",
            "--control",
            "button:south",
            "--target",
            "k",
        ];
        arguments.extend(conflicting);
        let conflict = run(&arguments);
        assert!(!conflict.status.success());
        assert!(String::from_utf8_lossy(&conflict.stderr).contains("cannot be used with"));
    }
}

#[test]
fn input_import_export_is_typed_atomic_and_non_destructive() {
    let temp = tempdir().expect("temp dir");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let cache_home = temp.path().join("cache");
    let export = temp.path().join("portable/input.JSON");
    let binary = env!("CARGO_BIN_EXE_kof-wing");
    let run = |arguments: &[&str]| {
        Command::new(binary)
            .args(arguments)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("XDG_DATA_HOME", &data_home)
            .env("XDG_CACHE_HOME", &cache_home)
            .output()
            .expect("run input command")
    };

    assert!(run(&["profile", "create", "portable"]).status.success());
    assert!(
        run(&[
            "--profile",
            "portable",
            "input",
            "reset",
            "--preset",
            "modern",
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "--profile",
            "portable",
            "input",
            "export",
            export.to_str().expect("Unicode path"),
        ])
        .status
        .success()
    );
    assert!(export.is_file());
    let overwrite = run(&[
        "--profile",
        "portable",
        "input",
        "export",
        export.to_str().expect("Unicode path"),
    ]);
    assert!(!overwrite.status.success());
    assert!(String::from_utf8_lossy(&overwrite.stderr).contains("refusing to overwrite"));

    assert!(
        run(&["--profile", "portable", "input", "reset"])
            .status
            .success()
    );
    assert!(
        run(&[
            "--profile",
            "portable",
            "input",
            "import",
            export.to_str().expect("Unicode path"),
        ])
        .status
        .success()
    );
    let show = run(&["--profile", "portable", "input", "show", "--format", "json"]);
    assert!(String::from_utf8_lossy(&show.stdout).contains("\"preset\": \"modern\""));
}
