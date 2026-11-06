# TODO

- [x] Implement the Rust launcher and profile model.
- [x] Generate stage and music overlays from typed configuration.
- [ ] Validate controllers and saves on Ubuntu and Windows.
- [ ] Add targeted, hash-gated difficulty patches.
- [ ] Add zero-service LAN for the original two-player mode with remote input,
  hardware-accelerated low-latency media, discovery, reconnect, and latency
  diagnostics.
- [x] Add named profiles.
- [ ] Add deterministic input replay and save snapshots.
- [x] Add versioned per-profile i18n with complete external EN/FR catalogues,
  safe canonical BCP 47 IDs, strict reported fallback, language CLI import/export,
  translated wrapper generation, and LAN fingerprint binding.
- [ ] Translate and visually validate all 241 Han-bearing static main-SWF text
  symbols plus script-generated and external `skillList.swf` text before enabling
  any production embedded patch; current embedded coverage is intentionally 0.
- [ ] Add the production FFDec patch manifest only after every symbol passes base
  hash, round-trip export, embedded-font/layout, Ruffle smoke, and gameplay gates.
- [x] Add optional authentic and modern input presets.
- [x] Deliver a fully editable keyboard/gamepad schema and CLI with per-player
  and per-device profiles, modeled axes/deadzones/chords, conflict detection,
  and portable import/export; defaults remain optional presets only.
- [ ] Add a future input backend that can enforce physical-device selectors,
  per-player routing, axes/deadzones, chords, and live remapping; pinned Ruffle
  0.7.1 and Sunshine/Moonlight do not expose those controls.

## Completed for 0.1.0

- Implement the Rust 2024 launcher and named, platform-native profile model.
- Generate isolated stage and music manifest overlays from typed TOML without
  modifying the original game directory.
- Verify pinned tools and the audited SWF payload; cache extraction by projector
  identity and fail on unknown payload bytes.
- Add explicit Linux and Windows Ruffle 0.7.1 location contracts.
- Establish the LAN module boundary and diagnostic-only `lan doctor` command.

## Remaining after the core launcher

- Validate controls, saves, rendering, and complete matches on physical Ubuntu
  and Windows systems.
- Record and approve the projector wrapper SHA-256 in research evidence; the
  launcher currently records it and gates preservation on the known extracted
  SWF hash.
- Define the exact upstream stage/fighter manifest-key schema before publishing
  profile presets.
- Implement LAN discovery, input transport, hardware media, reconnect,
  local-network security, and latency diagnostics; do not advertise LAN play
  before the physical two-PC release gate passes.

## LAN status update

- [x] Add zero-service, directed same-subnet UDP discovery.
- [x] Add strict protocol/SWF/runtime/profile/session hash negotiation and reject
  stale, off-subnet, public-by-default, or incompatible peers.
- [x] Add configurable bind/interface/prefix and discovery/negotiation/metadata
  ports without cloud, accounts, relay, port forwarding, or firewall mutation.
- [x] Model an authoritative-host session plus honest remote-input and
  low-latency-media endpoint metadata.
- [x] Integrate pinned Sunshine/Moonlight remote input and media transport behind
  explicit opt-in commands with per-profile state.
- [ ] Validate host capture, hardware encoding, controller injection, decode,
  presentation, jitter handling, and A/V diagnostics on supported physical PCs.
- [ ] Implement reconnect/session resumption and complete-match failure handling.
- [x] Require explicit Sunshine PIN pairing and mandatory encrypted LAN streams.
- [ ] Authenticate the launcher negotiation channel before use on untrusted LANs.
- [ ] Pass the two-physical-PC Ubuntu/Windows release gate before describing LAN
  gameplay as available.
