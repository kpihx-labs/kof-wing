# KOF Wing

Cross-platform preservation and launcher project for a locally owned copy of
The King of Fighters Wing EX 1.0.

The original game data stays outside this repository. `work/` contains local
derivatives and pinned tools; `refs/` contains upstream research sources;
`research/` contains reproducible findings.

Current direction: extract the Flash projector, run the SWF with pinned Ruffle,
and apply explicit ActionScript patches instead of modifying the Windows
projector or original assets.

## Launcher

The Rust 2024 launcher is deliberately small and transparent. It verifies the
locally owned game layout, records the projector SHA-256, extracts only when the
projector identity changes, and accepts the extracted payload only when it
matches the audited SWF SHA-256
`d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334`.
The original directory is never written to. No game byte belongs in Git.

```console
cargo build --release
./target/release/kof-wing profile create default
./target/release/kof-wing --source "/path/to/KOF Wing EX 1.0" doctor
./target/release/kof-wing --source "/path/to/KOF Wing EX 1.0" play --window --scale 2
./target/release/kof-wing --source "/path/to/KOF Wing EX 1.0" play --fullscreen
./target/release/kof-wing profile list
./target/release/kof-wing lan doctor
```

`KOF_SOURCE` may replace `--source`. `KOF_WING_WORK` may relocate ignored,
reproducible extraction and overlay state; by default it is `work/` below the
current directory. Errors identify the missing source file, pinned tool, hash,
profile, or runtime contract instead of silently falling back.

### Pinned runtime contract

- Linux executable: `work/tools/ruffle-0.7.1/ruffle`
- Windows executable: `work/tools/ruffle-0.7.1-windows-x86_64/ruffle.exe`
- Linux archive SHA-256: `67950dfdde643f9087ae478eb3637fe52e0deefc79bdb55fe43cd69dab9fdd99`
- Windows archive SHA-256: `a5f6df9881771226e330e08a1fa7666f4caff50ddf9f2f993cdd95293edca627`

The launcher verifies both archives and locates only the pinned platform
executable. Windows extraction is intentionally an explicit local setup step;
the repository does not redistribute Ruffle or proprietary game data.

## Profiles and overlays

Profiles use the operating system's config, data, and cache directories. Each
profile receives separate Ruffle configuration, SharedObject saves, cache, and
typed TOML configuration. `profile create <name>` prints the exact config path.

```toml
[stage.files]
training = "mods/stages/training.swf"

[stage.assignments]
kyo = "training"

[music.tracks]
training = "mods/music/training.mp3"

[controls.gamepad]
south = "j"
east = "k"
```

At launch, the required external game tree is copied into
`work/profiles/<name>/overlay/`; the projector itself is excluded. This runtime
copy is intentionally independent—Ruffle cannot write through it into the
original source. Generated `stage/list.txt` and `sound/list.txt` append typed
profile overrides to copied manifests. The launcher passes the runtime copy to
Ruffle as a canonical, percent-encoded `file://` directory URL with a trailing
slash, so relative Flash loads resolve consistently on Linux and Windows.
Source manifests and assets remain untouched.

## LAN status

LAN play is not implemented in 0.1.0. `lan doctor` is a clean module boundary
that reports UDP bind, FFmpeg, and pinned-runtime capabilities, then explicitly
reports discovery, input transport, media streaming, reconnect, and security as
unimplemented. It never claims the LAN release gate is complete.

## Verification

`make check` runs formatting verification, strict Clippy, all tests, a release
build, ShellCheck, and pinned local-tool checks. Tests use synthetic fixtures and
do not require or reproduce proprietary data.
