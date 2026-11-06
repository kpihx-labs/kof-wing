# Changelog

## 0.1.0 - Unreleased

- Establish the cross-platform preservation architecture.
- Pin Linux and Windows Ruffle 0.7.1 plus FFDec 26.4.0.
- Extract and verify the original SWF without modifying source data.
- Add project-local checks, research evidence, and launcher prototype.
- Implement the modular Rust 2024 launcher (`cli`, `config`, `source`,
  `profile`, `runtime`, and `lan`).
- Add hash-gated, cached projector extraction and pinned Linux/Windows Ruffle
  runtime contracts.
- Add platform-native isolated profiles, typed stage/music configuration, and
  generated read-only source overlays.
- Add `doctor`, `play`, `profile list`, `profile create`, and diagnostic-only
  `lan doctor` commands.
- Add proprietary-data-free unit and integration coverage plus strict Rust
  formatting, lint, test, release, shell, and tool gates in `make check`.
- Isolate the generated runtime tree with real file copies instead of writable
  links to original assets.
- Anchor source-tree copying to retained directory handles and reject symlinks
  and Windows reparse points on every child open, including concurrent path
  replacement during traversal.
- Pass a canonical percent-encoded cross-platform `file://` base URL to Ruffle
  and lock its exact argument contract with parser-to-command regression tests.
- Validate Ruffle gamepad button and Flash key names before process launch.
- Keep `make push` scoped to the configured GitHub remote.
- Add `lan discover`, `lan host`, and `lan join` with same-subnet directed UDP
  discovery and strict protocol/SWF/runtime/profile/session hash negotiation.
- Default LAN peers to private/local addresses and the configured IPv4 prefix;
  expose bind, interface, prefix, discovery, negotiation, input-metadata, and
  media-metadata controls without mutating firewall or router state.
- Keep the LAN contract truthful: the host is authoritative, while remote input
  injection and low-latency media streaming remain explicitly unimplemented.
- Pin Sunshine 2026.914.233613 and Moonlight 6.2.0 for Linux and Windows, bind
  their exact archive hashes into protocol-v2 compatibility, and verify them in
  project checks.
- Add per-profile `lan setup-host`, explicit HITL `lan pair`, and opt-in
  `host --transport sunshine` / `join --transport moonlight` without changing
  the existing negotiation-only defaults.
- Generate a KOF-only Sunshine app/config with no Desktop, UPnP, broad web UI,
  global config, or internet binding; isolate Moonlight state and preserve the
  physical two-PC gameplay gate.
- Validate LAN IPv4 selection against `if-addrs` metadata, use the interface's
  actual prefix and broadcast, reject topology override mismatches, and never
  fall back from an unusable default-route tunnel to inactive private bridges.
- Add a strict version-1 per-profile input schema for multiple players and
  logical keyboard/gamepad devices, including buttons, keys, axes, deadzones,
  thresholds, selectors, and chords with safe identifiers and explicit conflict
  detection; an empty schema remains valid.
- Add optional `authentic` and `modern` input presets plus `input show`, `set`,
  `reset`, `import`, `export`, and `validate` commands with atomic TOML/JSON
  writes, non-destructive export, and explicit selector/deadzone clearing.
- Compile only exactly representable mappings to Ruffle 0.7.1
  `--gamepad-button` arguments, reject unsupported active transformations at
  play time, and report keyboard/device/axis/deadzone/chord limits explicitly.
- Bind input configuration into LAN compatibility, derive Sunshine keyboard and
  controller channels from enabled devices, and select Moonlight
  `--multi-controller`/`--no-multi-controller` from the profile without claiming
  backend capabilities those tools do not expose.
- Canonicalize semantically equivalent input mappings for LAN hashes, reject
  stale Sunshine channel policy, and reverify extracted Windows transport files
  against their pinned archives before reuse.
