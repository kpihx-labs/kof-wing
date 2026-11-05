# Initial audit

- Original executable: Windows PE32 Flash projector.
- Extracted payload: SWF version 12, `660x485`, 35 FPS.
- Payload SHA-256: `d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334`.
- Ruffle 0.7.1 loads the payload on Ubuntu through Vulkan.
- The game uses external `stage/`, `sound/` and `data/` directories.
- FFDec identifies hundreds of text fields and named AI/role classes, including
  several explicit AI levels. The first default-heap export exhausted memory;
  the reproducible export uses a 12 GiB heap.

Pinned local tools:

- Ruffle 0.7.1 archive SHA-256:
  `67950dfdde643f9087ae478eb3637fe52e0deefc79bdb55fe43cd69dab9fdd99`
- Ruffle 0.7.1 Windows x86-64 archive SHA-256:
  `a5f6df9881771226e330e08a1fa7666f4caff50ddf9f2f993cdd95293edca627`
- FFDec 26.4.0 archive SHA-256:
  `56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9`
- `dump-projector` is built from `refs/dump-projector`.

No original or extracted proprietary asset belongs in Git.

## Modularity found

- `stage/list.txt` is a plain key/value manifest for 14 external stage SWFs and
  per-character stage assignment.
- `sound/list.txt` is a plain manifest for 17 external MP3 themes.
- `data/skillList.swf` is loaded externally.
- The main symbol table contains 185 AI classes: 37 fighter/form variants with
  five explicit AI levels each.
- The payload uses Flash `SharedObject`, so saves and preferences can be mapped
  to a project-owned storage directory on every OS.

This makes stages, music, difficulty and persistence directly configurable.
Characters and core combat remain embedded and require targeted SWF patches.

## Decompiler boundary

Whole-payload FFDec exports are not viable: the default heap, a 12 GiB
ActionScript pass, and a 4 GiB P-code-requested pass all exhausted memory on
deep AI/player classes. Invalid legacy property-index warnings also occur. This
is an FFDec reconstruction limitation, not a runtime failure: Ruffle loads the
same SWF successfully. Future inspection must export only selected symbol IDs
and keep bytecode/P-code as the authoritative patch representation.
