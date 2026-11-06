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
- Add strict version-1 per-profile language selection and external versioned EN/FR
  catalogues with canonical path-safe BCP 47 IDs plus `language list`, `show`,
  `set`, `validate`, `import`, and `export` commands.
- Make fallback and translation coverage explicit, bind language/catalog version
  into LAN compatibility, and generate a complete translated wrapper menu while
  reporting the still-untranslated embedded SWF instead of claiming completeness.
- Require the selected catalog version to match the installed active catalog,
  hash the effective resolved language contract into LAN compatibility, reject
  symlinked i18n import/export directories, cap catalog size before parsing, and
  emit a restrictive CSP in the generated wrapper.
- Audit 675 exported static text symbols (241 Han-bearing), verify FFDec's
  hash-gated symbolic text-import/round-trip mechanism on the local payload, and
  keep production SWF patching blocked pending complete font/layout/gameplay
  validation; no proprietary binary is versioned.
- Reject mismatched local catalog filenames and duplicate local IDs explicitly;
  test multi-catalog fallback cycles and unavailable transitive fallbacks.
- Bound language normalization to the supported path-safe BCP 47 syntax/casing
  subset, without claiming IANA registry/preferred-value canonicalization.
- Serialize overlay refresh, wrapper writing, and runtime use with a per-profile
  cross-process OS file lock (Rust 1.89+), tested with synthetic concurrent workers
  and forced process termination; record a reproducible selected-ID FFDec probe.
- Correct the Windows `SYNCHRONIZE` import and Unix-only test imports; validate
  all targets/features with warning-denying Windows cross-target Clippy.
- Add a local-only, reproducible 241-ID FFDec/NLLB EN/FR pipeline with protected
  tokens and line endings, text-free QA reports, synthetic tests, generated SWF
  copies, and exact selected-ID re-export checks.
- Gate launcher selection of an embedded-language SWF on consistent 100%
  coverage, QA, exact round trip, font, layout, gameplay, fixed path, and hash
  prerequisites, and bind any activated patch hash into LAN compatibility;
  current generated EN/FR outputs remain disabled because only
  1/241 IDs round-trip and embedded fonts reject required Latin glyphs.
- Keep incomplete embedded candidates under `work/i18n/candidates/`, publish to
  `patched/` only when eligible, reject activation symlinks/oversized manifests,
  harden the local NLLB/FFDec pipeline path and subprocess limits, and make
  `play`/`doctor`/`language show` report a clear embedded block while launching
  the audited base SWF.
- Require local visual approval `reviewed_ids` to equal the inventory/translation
  Han ID set exactly (no duplicates, bools, or coerced values), reject every
  symlink path component on Python reads/writes, publish eligible patches
  atomically while retracting ineligible ones before final activation state, and
  extend synthetic FR selection proof through the play reporting path.
- Record the bounded Ruffle evidence: an EN research candidate reached a loading
  screen, which is not treated as a translated product or gameplay validation.
- Preserve every FFDec formatted metadata block during replacement, distribute
  long static text across existing UI8-sized text records, locally add required
  Noto glyph outlines to all 13 referenced embedded fonts, and reach exact
  semantic re-export for 241/241 EN and 241/241 FR IDs without residual Han.
- Keep both corrected candidates fail-closed: NLLB review, measured layout
  overflow, full visual review, and gameplay approval remain mandatory before
  publication to the launcher-owned `patched/` path.
- Reject Fontconfig family substitutions, unsupported supplementary glyphs,
  oversized helper inputs, duplicate translation IDs, and forged text hashes;
  bind font coverage and detailed round-trip state into the Rust activation
  contract instead of trusting the top-level eligibility claim alone.
