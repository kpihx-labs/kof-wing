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
- Add platform-native isolated profiles, typed stage/music/control TOML, and
  generated read-only source overlays.
- Add `doctor`, `play`, `profile list`, `profile create`, and diagnostic-only
  `lan doctor` commands.
- Add proprietary-data-free unit and integration coverage plus strict Rust
  formatting, lint, test, release, shell, and tool gates in `make check`.
- Isolate the generated runtime tree with real file copies instead of writable
  links to original assets.
- Pass a canonical percent-encoded cross-platform `file://` base URL to Ruffle
  and lock its exact argument contract with parser-to-command regression tests.
- Validate Ruffle gamepad button and Flash key names before process launch.
- Keep `make push` scoped to the configured GitHub remote.
