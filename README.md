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

## Historical 0.1.0 LAN boundary

The first 0.1.0 launcher slice implemented only `lan doctor`: it reported UDP
bind, FFmpeg, and pinned-runtime capabilities while discovery, input transport,
media streaming, reconnect, and security were all unimplemented. The current
negotiation status is recorded below; the LAN gameplay release gate is still not
complete.

## Verification

`make check` runs formatting verification, strict Clippy, all tests, a release
build, ShellCheck, and pinned local-tool checks. Tests use synthetic fixtures and
do not require or reproduce proprietary data.

## LAN transport (current)

The earlier 0.1.0 diagnostic-only status above is superseded by a bounded LAN
slice: discovery and session negotiation are implemented, and the launcher can
invoke pinned Sunshine/Moonlight binaries for host-authoritative video, audio,
and remote controller input. This integration is not a claim that capture or
input works on any particular machine. Flash/Ruffle lockstep determinism is
still not assumed.
Compatibility now binds the protocol, audited SWF, Ruffle policy, canonical
profile configuration, Sunshine/Moonlight versions, all four pinned archive
hashes, app name, resolution, frame rate, bitrate, display mode, audio layout,
multi-controller, game-optimization, frame-pacing, codec, quit-after, explicit
PIN/stdin policy, encryption, UPnP, web-origin, and private-interface policies.

```console
./target/release/kof-wing --profile default lan doctor
./target/release/kof-wing --profile default lan discover
./target/release/kof-wing --profile default lan host
./target/release/kof-wing --profile default lan join
./target/release/kof-wing --profile default lan join --host 192.168.1.20:27881
```

Those existing commands retain negotiation-only behavior. A transport session
is an explicit sequence:

```console
# Host, once per profile. Run the release binary so apps.json records it.
./target/release/kof-wing --source "/path/to/KOF Wing EX 1.0" --profile default lan setup-host

# Host, each session. Use the host's private address, not 0.0.0.0.
./target/release/kof-wing --profile default lan host --interface 192.168.1.20 --transport sunshine

# Client, once per host. Moonlight displays a one-time PIN; a person must type
# that PIN into the running host terminal. There is no --pin automation.
./target/release/kof-wing --profile default lan pair --interface 192.168.1.21 --host 192.168.1.20:27881

# Client, after pairing.
./target/release/kof-wing --profile default lan join --interface 192.168.1.21 --host 192.168.1.20:27881 --transport moonlight
```

`lan host` answers directed same-subnet UDP discovery on port 27880 and strict
session negotiation on port 27881. `lan join` discovers exactly one compatible
host unless `--host` is supplied. Every command exposes `--bind`, `--interface`,
`--prefix-len`, `--discovery-port`, and `--session-port`; host input/media
metadata ports and discovery timeouts are also configurable. Automatic interface
selection uses only the active default IPv4 route and verifies that address
against OS interface metadata. The selected interface's real netmask, prefix,
and broadcast are authoritative. `--prefix-len` is an optional assertion and is
rejected when it differs from the OS prefix; it never invents a subnet. A `/32`,
point-to-point, inactive, nonexistent, or non-broadcast interface fails with an
actionable error instead of falling back to a Docker, virtual bridge, or tunnel.
Explicit loopback remains available to `lan doctor` only. A concrete `--bind`
must equal the selected interface address;
the wildcard host bind remains the default so directed broadcasts are received.

Private, loopback, or link-local addresses and same-prefix peers are required by
default. `--allow-public` is an explicit policy override, not port forwarding or
internet traversal. The launcher uses no cloud, account, relay, UPnP, VPN,
firewall mutation, or router configuration.

With `--transport sunshine`, the negotiated input and media endpoint is the
pinned Sunshine host. With `--transport moonlight`, the client invokes pinned
Moonlight 6.2.0 as `stream --720 --fps 60 --bitrate 10000 --display-mode
fullscreen --audio-config stereo --multi-controller ... <negotiated-host> "KOF
Wing EX 1.0"`. The full fixed argument contract also pins packet size, V-Sync,
host audio off, game optimization, frame pacing, H.264/automatic decode,
quit-after, input/window behavior, HDR/YUV 4:4:4 off, and system-key capture so
saved Moonlight preferences cannot silently change a session. Linux AppImages
are always launched through
`--appimage-extract-and-run`. Moonlight receives per-profile `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `TMPDIR` (Moonlight writes Linux logs to
the temporary directory); Windows portable tools are extracted into that
profile's data directory and run there so `portable.dat` keeps settings, pairing
state, logs, and caches beside the profile-local executable. No global Moonlight
or Sunshine state is used.

`lan setup-host` writes one per-profile Sunshine config and an `apps.json` with
exactly one application: `KOF Wing EX 1.0`, pointing to the selected source,
profile, and current `kof-wing` executable. It never adds Desktop. Sunshine is
bound at launch to the negotiated private interface, UPnP is disabled, and
`origin_web_ui_allowed = pc` denies remote web UI requests even though all
Sunshine protocols share that interface bind. LAN stream encryption is
mandatory. On Linux the launcher also redirects Sunshine's XDG appdata so
non-config state such as display persistence stays inside the profile; generated
app environment entries restore the launcher's normal profile roots for KOF.
On Windows, Sunshine's executable-relative `config` directory remains below the
profile-local extraction. The launcher does not install services, mutate the
firewall, configure a router, or expose an internet transport.

### Pinned LAN tools

| Tool | SHA-256 |
|---|---|
| `Sunshine_2026.914.233613_x86_64.AppImage` | `6f80297a853bcf8cda114ff131b5c0a8c9a4d96a2a9c3dee1f04c39c54206846` |
| `Sunshine-Windows-AMD64-lite.zip` | `233008e46f4c0e501a586cbfd6c4fd4a4c0d414a0b5fc7f13c070eb92ec3824b` |
| `Moonlight-6.2.0-x86_64.AppImage` | `787b0dcbd42ae90620e76fb8d77d7c441ba6473a91c2d7fc3a0fc2466f992437` |
| `MoonlightPortable-x64-6.2.0.zip` | `6f91d268b41ed5ca8066d9d7578dc403b8db190991ff48f00b1700465f8f7ebf` |

Negotiation itself is compatibility checking, not peer authentication. Sunshine
pairing and mandatory LAN encryption protect the selected transport only after
the explicit one-time PIN ceremony. A successful negotiation, pairing, or local
process launch is not yet proof of full gameplay: controller mapping, host input
permissions/drivers, capture/encoding, latency, audio, reconnect, and simultaneous
two-player behavior still require the physical two-PC release gate.
