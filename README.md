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

`work/` is user-owned local state and a trust boundary: the launcher assumes
that no untrusted process with the same user permissions modifies it while a
command runs. Source data is not trusted this way; required files are opened
relative to a retained source-directory handle without following symbolic links
or Windows reparse points. Generated destination paths may use ordinary
path-based writes only inside `work/`.

### Pinned runtime contract

- Linux executable: `work/tools/ruffle-0.7.1/ruffle`
- Windows executable: `work/tools/ruffle-0.7.1-windows-x86_64/ruffle.exe`
- Linux archive SHA-256: `67950dfdde643f9087ae478eb3637fe52e0deefc79bdb55fe43cd69dab9fdd99`
- Windows archive SHA-256: `a5f6df9881771226e330e08a1fa7666f4caff50ddf9f2f993cdd95293edca627`

The launcher verifies both archives and locates only the pinned platform
executable. Windows extraction is intentionally an explicit local setup step;
the repository does not redistribute Ruffle or proprietary game data.

## Profiles, input, and overlays

Profiles use the operating system's config, data, and cache directories. Each
profile receives separate Ruffle configuration, SharedObject saves, cache, and
typed TOML configuration. `profile create <name>` prints the exact config path
and creates an empty `input.toml`: no binding or preset is mandatory.

```toml
[stage.files]
training = "mods/stages/training.swf"

[stage.assignments]
kyo = "training"

[music.tracks]
training = "mods/music/training.mp3"
```

### Versioned input schema and CLI

Input is a separate, strict, versioned per-profile schema. It models any number
of players and logical keyboard/gamepad devices, safe identifiers, button/key/
axis sources, normalized gamepad deadzones in `[0, 1)`, axis thresholds in
`(0, 1]`, and optional chords. TOML and JSON import/export contain the complete
schema and are portable across Linux and Windows. All writes use a temporary
file in the destination directory followed by atomic replacement; export
refuses to overwrite an existing file.

```console
# Empty is the default and is valid.
kof-wing --profile default input show
kof-wing --profile default input validate

# Optional presets: original documented keyboard defaults, or one P1 gamepad.
kof-wing --profile default input reset --preset authentic
kof-wing --profile default input reset --preset modern

# Control forms are key:..., button:..., or axis:...:direction.
kof-wing --profile default input set --player p1 --device pad-main --kind gamepad \
  --control button:south --target k
kof-wing --profile default input set --player p1 --device pad-main --kind gamepad \
  --control axis:left-x:positive --target right --deadzone 0.15 --threshold 0.6
# Omit these options to preserve existing values; clear them explicitly.
kof-wing --profile default input set --player p1 --device pad-main --kind gamepad \
  --control axis:left-x:positive --target right --threshold 0.6 \
  --clear-selector --clear-deadzone

kof-wing --profile default input export ./input.json
kof-wing --profile tournament input import ./input.json
kof-wing --profile default input reset
```

`authentic` records the commonly documented native keyboard defaults (P1:
WASD + J/K/L/U/I/O; P2: arrows + 1–6) as identity bindings; the game's own
in-game configuration remains authoritative. `modern` maps one standard
gamepad's D-pad and supported buttons to the P1 keyboard defaults. `input set`
clears the preset label because the resulting schema is custom; imports cannot
retain a preset label unless their contents exactly match that built-in preset.

Conflict detection rejects duplicate player/device names, duplicate controls,
invalid kind/source combinations, invalid target keys, unsafe names, invalid
deadzone/threshold ranges, and incompatible global Ruffle button targets before
an atomic write. `input validate` also reports active features that the pinned
backends cannot enforce.

#### Backend limits (exact, not emulated)

- Ruffle 0.7.1 exposes only repeated
  `--gamepad-button <supported-button>=<Flash-key>` arguments. The launcher emits
  exactly those arguments for representable button mappings. Native keyboard
  identity bindings need no argument.
- Ruffle has no argument for keyboard remapping, physical-device selection,
  per-player routing, axes, deadzones, chords/hotkeys, or live remapping. Such a
  profile remains valid and portable, but `doctor` and `play` fail clearly
  rather than silently ignoring active transformations. Restart after changing
  a mapping.
- Sunshine receives only `keyboard = enabled|disabled` and
  `controller = enabled|disabled`, derived from enabled profile devices. It
  forwards remote input; it does not apply bindings, selectors, axes, deadzones,
  or chords.
- Moonlight 6.2.0 receives `--multi-controller` only for more than one enabled
  gamepad, otherwise `--no-multi-controller`. It exposes no per-binding mapping,
  deadzone, or chord argument. The host's Ruffle process applies supported
  mappings globally across controllers.
- An empty profile is valid. Generated Sunshine input channels are then
  disabled; LAN media may start, but no remote input is promised.

Player/device identifiers are logical schema identities, not a claim that
Ruffle can bind a USB GUID. Axis and chord entries are retained for future
backends that expose those capabilities; this launcher does not emulate them.

At launch, the required external game tree is copied into
`work/profiles/<name>/overlay/`; the projector itself is excluded. This runtime
copy is intentionally independent—Ruffle cannot write through it into the
original source. Generated `stage/list.txt` and `sound/list.txt` append typed
profile overrides to copied manifests. The launcher passes the runtime copy to
Ruffle as a canonical, percent-encoded `file://` directory URL with a trailing
slash, so relative Flash loads resolve consistently on Linux and Windows.
Source manifests and assets remain untouched.

### Versioned languages and honest coverage

Language selection is part of each versioned profile and therefore part of the
canonical LAN compatibility fingerprint. The repository-owned `locales/en.json`
and `locales/fr.json` catalogues both cover all 15 launcher/wrapper messages with
no fallback. Catalogues use canonical, path-safe BCP 47 IDs (`en`, `fr`,
`fr-CA`, `zh-Hans-CN`); schema and catalogue versions are explicit.

Here "canonical" means **project syntax/casing normalization only**, not full
BCP 47 registry canonicalization. The manual parser accepts a path-safe ASCII
subset limited to 2-63 bytes, normalizes language/script/region casing and
rejects repeated variants/extension singletons. It does not validate the IANA
registry, extlang/variant prefixes or extension-specific semantics, resolve
grandfathered tags, rewrite deprecated/preferred values (`iw` remains `iw`, not
`he`), or sort extension sequences. Some grandfathered forms fail syntax checks;
others happen to fit the grammar but are not recognized as registry entries.
Aliases remain distinct profile/LAN identities; registry-equivalent IDs are not
promised equal fingerprints. Use the exact documented ID for interoperability.

```console
kof-wing --profile default language list
kof-wing --profile default language show
kof-wing --profile default language set fr
kof-wing --profile default language validate
kof-wing --profile default language import ./fr-CA.json
kof-wing --profile default language export ./active-language.json
kof-wing --profile default language export --id en ./english.json
```

`language show` always emits a machine-readable coverage report followed by any
fallback and blocker lines. An incomplete catalogue is usable only when its
declared fallback resolves every required key; every fallback key count and chain
is reported. Missing keys, cycles, unavailable fallbacks, unsafe IDs, unknown
schema fields, zero versions, oversized files, and destructive exports fail.
Imports replace only the profile-local catalogue atomically. Replacing the
selected catalogue with a different `catalog_version` requires an explicit
`language set` refresh before `show`, `doctor`, `play`, or LAN compatibility
reuse it. Exports never overwrite a file or follow a destination symlinked path.

Installed JSON filenames must be exactly `<catalog.id>.json` with the project's
normalized casing. Duplicate local IDs fail explicitly rather than depending on
directory enumeration order; one correctly named local override of a builtin is
allowed. Import source filenames are unrestricted; installation assigns the
validated canonical filename.

Overlay refresh holds a per-profile OS-backed exclusive file lock outside the
replaceable `overlay/` directory. The returned overlay guard keeps it held through
wrapper writing and runtime use, serializing concurrent doctor/play processes.
The persistent `overlay.lock` file is never unlinked; the OS releases ownership
on handle drop or process termination. No symlink/hardlink is used. This needs
Rust 1.89+ and a filesystem supporting OS file locks; lock errors are surfaced,
not bypassed. Different profiles remain independent.

`doctor` and `play` generate
`work/profiles/<name>/overlay/kof-wing-menu.html`, a complete EN/FR companion
wrapper menu. It does not pretend to replace the SWF UI. The audited main SWF has
675 non-empty exported static text symbols, 241 containing Han characters;
production-verified embedded translation coverage is currently **0/241**.
`data/skillList.swf` and script-generated text also need a complete inventory.
Consequently no partial FFDec translation is activated and the game can still
show Chinese internally. This avoids silently shipping a half-Chinese interface.

FFDec 26.4.0 was locally verified to patch a selected static text ID when given
the required formatted `texts/` import tree: the generated SWF hash changed and a
fresh selected-ID export round-tripped the replacement exactly. That mechanism
remains blocked for production until every translated symbol passes the audited
base hash, exact round-trip, embedded-font coverage, layout screenshots, Ruffle
smoke, and gameplay gates. Generated SWFs remain under ignored `work/`; no
proprietary binary is versioned. See `research/i18n-audit.md`.

The reproducible local pipeline is now `scripts/i18n-pipeline.py`. It exports
plain and formatted text from the audited base, inventories exactly 241
Han-bearing IDs, translates EN/FR in local NLLB batches through the installed
`k-translate` model store, preserves line endings and protected tokens, emits QA
without source text, imports formatted replacements, and freshly re-exports all
selected IDs. Generated text and SWFs stay under ignored `work/i18n/`; the only
tracked replacement manifest is synthetic.

The current run is deliberately **disabled**. NLLB produced 241/241 research
candidates for each language with no lost source number/placeholder/line break
and no Han, but none has human review; 202 EN and 201 FR IDs have length/layout
risk. FFDec changed both candidate hashes, yet only ID `26696` round-tripped
exactly (1/241 per language). It reported missing Latin glyphs in embedded fonts
and rejected one spacing record. Incomplete outputs stay under
`work/i18n/candidates/` and are never published to the launcher activation path
`work/i18n/patched/`, never launched, and never presented as finished EN/FR
products. `doctor`, `language show`, and `play` report a clear embedded block and
keep the audited base SWF. The launcher selects a patched SWF only when the
generated activation manifest proves 100% coverage plus QA, exact round trip,
font, layout, and gameplay prerequisites and its hash matches. An activated
patch hash also replaces the base SWF hash in the canonical LAN/session
contract. See `research/i18n-pipeline-audit.md` for exact hashes and evidence.

Technical blocker resolution (2026-11-06): a subsequent local-only pass
supersedes the measured 1/241 mechanism result above. The parser now recognizes
all observed FFDec directives (including `spacing`, `spacingpair`,
`resetspacing`, `letterspacing`, and `variablename`), preserves every formatted
metadata block byte-for-byte in the import recipe, and distributes long static
text over existing SWF UI8-sized text records. A small Java helper uses FFDec's
local library API to add required outlines from locally installed libre Noto
Sans, Symbols, and CJK faces to each referenced embedded font; no font or game
asset is uploaded or committed. The helper rejects Fontconfig substitutions and
uses only exact Noto families. The locally installed Debian packages declare
SIL Open Font License 1.1; this repository redistributes no font file. Fresh
formatted re-export now yields exact semantic text for **241/241 EN and 241/241
FR IDs**, with zero residual Han and complete requested glyph coverage. FFDec
legitimately recalculates spacing
metadata for 55 EN and 51 FR IDs after glyph substitution; these normalized IDs
are recorded separately and are not confused with semantic mismatches.

This does **not** activate either patch. Candidate screenshots visibly confirm
that Latin/French glyphs render, but also expose severe clipping/overflow in the
existing Chinese-sized layout. Human review is still 0/241 and layout/gameplay
approval is absent, so activation manifests remain ineligible and `patched/`
remains unpublished. Candidate captures are clearly separated under
`work/i18n/captures/{en,fr}-candidate-roundtrip-241/`.

Latest deterministic rerun note: enabling the tokenizer's documented regex fix
and bounded no-repeat decoding changed the generated candidate hashes to
`46bd21d67fd8694e8db9bc1a9b4ec9d7d5fa3a359c3385af9cc417b508647fe1`
(EN) and `4d97dd64fe8de1c0764b501aaf0df5d0a7cbcca7356e0890cabc82a6f7119683`
(FR). Exact round trip remains 241/241 in both languages; FFDec normalized
formatted metadata for 52 EN and 51 FR IDs. Current QA flags 207 EN and 223 FR
layout-risk IDs. The exact activation blockers for both languages are
`qa_valid`, `font_valid`, `layout_valid`, `gameplay_valid`, and
`missing_or_stale_local_visual_approval`.

## Historical 0.1.0 LAN boundary

The first 0.1.0 launcher slice implemented only `lan doctor`: it reported UDP
bind, FFmpeg, and pinned-runtime capabilities while discovery, input transport,
media streaming, reconnect, and security were all unimplemented. The current
negotiation status is recorded below; the LAN gameplay release gate is still not
complete.

## Verification

`make check` runs formatting verification, strict Clippy, Rust tests, synthetic
Python pipeline tests, a release build, ShellCheck, and pinned local-tool checks.
Tests do not require or reproduce proprietary data.

## LAN transport (current)

The earlier 0.1.0 diagnostic-only status above is superseded by a bounded LAN
slice: discovery and session negotiation are implemented, and the launcher can
invoke pinned Sunshine/Moonlight binaries for host-authoritative video, audio,
and remote controller input. This integration is not a claim that capture or
input works on any particular machine. Flash/Ruffle lockstep determinism is
still not assumed.
Compatibility now binds the protocol, audited SWF, Ruffle policy, canonical
profile and input configuration, Sunshine/Moonlight versions, all four pinned
archive
hashes, the effective resolved language contract and wrapper messages, app name,
resolution, frame rate, bitrate, display mode, audio layout,
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
# Host, initially and whenever enabled input channels, source path, or launcher
# location changes. Run the release binary so apps.json records it.
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
fullscreen --audio-config stereo --multi-controller|--no-multi-controller ...
<negotiated-host> "KOF Wing EX 1.0"`. Multi-controller mode is derived from the
input profile. The remaining fixed contract pins packet size and V-Sync,
host audio off, game optimization, frame pacing, H.264/automatic decode,
quit-after, input/window behavior, HDR/YUV 4:4:4 off, and system-key capture so
saved Moonlight preferences cannot silently change a session. Linux AppImages
are always launched through
`--appimage-extract-and-run`. Moonlight receives per-profile `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `TMPDIR` (Moonlight writes Linux logs to
the temporary directory); Windows portable tools are extracted into that
profile's data directory and run there so `portable.dat` keeps settings, pairing
state, logs, and caches beside the profile-local executable. Every archived
program file is rechecked against the pinned ZIP before reuse. No global
Moonlight or Sunshine state is used.

`lan setup-host` writes one per-profile Sunshine config and an `apps.json` with
exactly one application: `KOF Wing EX 1.0`, pointing to the selected source,
profile, and current `kof-wing` executable. It never adds Desktop. Keyboard and
controller channels are derived from enabled input devices, but Sunshine does
not apply their bindings. `lan host --transport sunshine` refuses a stale
generated channel policy and instructs the user to rerun `lan setup-host`.
Sunshine is bound at launch to the negotiated private interface, UPnP is
disabled, and `origin_web_ui_allowed = pc` denies remote web UI requests even
though all Sunshine protocols share that interface bind. LAN stream encryption
is mandatory. On Linux the launcher also redirects Sunshine's XDG appdata so
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
