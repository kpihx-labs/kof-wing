# TODO

- [x] Implement the Rust launcher and profile model.
- [x] Generate stage and music overlays from typed configuration.
- [ ] Validate controllers and saves on Ubuntu and Windows.
- [ ] Add targeted, hash-gated difficulty patches.
- [ ] Add zero-service LAN for the original two-player mode with remote input,
  hardware-accelerated low-latency media, discovery, reconnect, and latency
  diagnostics.
- [x] Add named profiles.
- [ ] Add deterministic input replay, save snapshots, and authentic/free presets.

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
