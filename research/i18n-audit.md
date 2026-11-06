# Internationalization and SWF patch audit

## Audited local payload

- SWF SHA-256: `d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334`.
- FFDec archive SHA-256: `56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9`.
- FFDec 26.4.0 exported 675 non-empty static text symbols; 241 contain Han
  characters. The counts come from `work/decompiled/main/texts/` and are source
  evidence, not bundled game data.
- The symbol table contains named menu/settings clips (`select`, `option`, `ok`,
  `mc_keysetting`, `mc_fightsetting`, `mc_displaysetting`) and two Chinese HUD
  symbol names. Symbol names alone do not prove that all visible strings are
  static text tags.
- `data/skillList.swf` is loaded externally and is outside the 675-symbol main
  SWF export. Script-generated strings may also be absent from static text tags.

## What FFDec can verifiably patch

The CLI accepts formatted text imports below a `texts/` directory. A local probe
on text symbol `12941` changed the exported value from the original loading label
to `Loading`, changed the output SWF hash, and re-exported exactly `Loading` from
the generated SWF. Passing the leaf directory instead of its parent silently
left the SWF unchanged; the safe patcher must therefore verify both output hash
change and round-trip text export.

This proves that selected static `DefineText`/`DefineEditText` symbols are
structurally patchable. It does **not** prove complete translation or correct
rendering: the original embedded font may lack Latin glyphs, longer EN/FR text
may overflow, and code-generated text remains unaudited. No production patch is
activated until every selected symbol has a reviewed translation, exact
round-trip verification, font coverage, layout screenshots, and a gameplay
smoke test. Generated patches must always start from the immutable audited base;
disabling a patch therefore restores the original byte-for-byte source.

## Current honest coverage

- Launcher-owned EN/FR catalog and generated wrapper menu: 15/15 required keys,
  no fallback.
- Main SWF static text: 0/241 Han-bearing symbols production-verified.
- External `skillList.swf` and script-generated strings: not fully inventoried.
- Result: EN/FR infrastructure and wrapper are complete; the embedded game UI is
  explicitly blocked, not claimed complete and not partially patched into a
   mixed Chinese/European-language interface.

## Independent reproduction (2026-11-06)

Only commands, hashes and the researcher-authored replacement `Loading` are
recorded here. No SWF, original string, font/layout export, or game data is
included. Java and uv must be available, the pinned FFDec archive must already
be unpacked at `work/tools/ffdec-26.4.0/`, and the locally owned extracted base
must exist. All commands below run from the repository root.

```console
uv run --no-project scripts/i18n-probe.py review-reproduction
```

The script verifies the archive/base hashes before doing anything, exports only
character ID `12941`, preserves its locally exported formatted header (normalized
to LF), and replaces only its text with the seven UTF-8 bytes `Loading`. It
creates a **new** `work/patch-test/review-reproduction/` directory and refuses to
reuse an existing directory, preventing stale exports from passing the check.
For another run, pass a fresh single directory name instead of
`review-reproduction`; never erase the previous evidence to rerun it.

These are the exact FFDec operations expanded from that invocation:

```console
java -jar work/tools/ffdec-26.4.0/ffdec-cli.jar -selectid 12941 -format text:formatted -export text work/patch-test/review-reproduction/formatted work/extracted/kof-wing-ex-v1.0.swf
java -jar work/tools/ffdec-26.4.0/ffdec-cli.jar -importText work/extracted/kof-wing-ex-v1.0.swf work/patch-test/review-reproduction/patched.swf work/patch-test/review-reproduction/import
java -jar work/tools/ffdec-26.4.0/ffdec-cli.jar -importText work/extracted/kof-wing-ex-v1.0.swf work/patch-test/review-reproduction/leaf-noop.swf work/patch-test/review-reproduction/import/texts
java -jar work/tools/ffdec-26.4.0/ffdec-cli.jar -selectid 12941 -format text:plain -export text work/patch-test/review-reproduction/verify work/patch-test/review-reproduction/patched.swf
```

Between export and import, the script writes only the selected formatted record
to `import/texts/12941.txt`. No whole-file decompilation, script export, or
production activation occurs. The leaf-path negative control intentionally
omits the required parent tree and must remain byte-identical to the base.

Observed SHA-256 values:

| File | SHA-256 |
|---|---|
| `work/tools/ffdec_26.4.0.zip` | `56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9` |
| `work/extracted/kof-wing-ex-v1.0.swf` | `d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334` |
| `work/patch-test/review-reproduction/import/texts/12941.txt` | `7b83b617807dd617df5099ab6a1bf9c6e1fb52879ff43ab302945bd222534a4e` |
| `work/patch-test/review-reproduction/patched.swf` | `96d7b846a091ef319d72d167081ddee41e11f208aaf742911c88a182eca57287` |
| `work/patch-test/review-reproduction/leaf-noop.swf` | `d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334` |
| `work/patch-test/review-reproduction/verify/12941.txt` | `dc380888c4e2c7762212480ff86eb39150ec70b45009c33bc6adcbd0041384b1` |

Textual result (timings and local absolute paths omitted):

```text
JPEXS Free Flash Decompiler v.26.4.0
Exported text 1/1 DefineEditText (chid: 12941)
OK
PASS: ID 12941 round-trip = Loading (7 UTF-8 bytes); parent import changes SWF; leaf import is byte-identical
```

All subprocesses exited successfully, including the no-op negative control;
exit status alone is therefore insufficient. The probe separately requires a
changed positive SWF hash, an unchanged negative-control hash, and exact raw
round-trip bytes (no trimming or newline normalization in that assertion).
The pre-existing manually prepared `work/patch-test/import/texts/12941.txt`
has hash `7810ab5c652197b129b2736fe5a1fbc672973cb5843b574eab4a1d109d8f9114`
and produces `10af61418d392bfc7e89056e852e7cdea7aae79e25d80855223a85f28f52dd18`;
that is a different prepared record, not the LF-normalized reproduction above.
These are byte-level mechanism results, not font/layout/gameplay approval.

Repository hygiene check:

```console
git check-ignore work/patch-test/review-reproduction/patched.swf work/patch-test/review-reproduction/import/texts/12941.txt work/patch-test/review-reproduction/verify/12941.txt
git ls-files work refs target
git diff --check
```

The three generated paths are ignored; the tracked-file query is empty. The
research probe is intentionally excluded from `make check`: automated tests use
synthetic fixtures only and require no locally owned game data.
