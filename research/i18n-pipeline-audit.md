# Reproducible embedded-localization pipeline audit

## Scope and data policy

`scripts/i18n-pipeline.py` inventories only FFDec text resources from the audited
base SWF, translates locally with the installed `k-translate` NLLB/CTranslate2
stack, runs structural QA, builds FFDec formatted imports, creates EN/FR SWF
copies, and re-exports every targeted ID. Extracted source text, translations,
formatted records, generated SWFs, and screenshots remain below ignored
`work/i18n/`. The tracked example manifest is synthetic. This report contains
only counts, IDs, hashes, and failure reasons.

The audited base is
`d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334`.
The inventory reproduced 675 non-empty symbols and exactly 241 Han-bearing IDs;
15 contain semantic line breaks and 50 source-hash groups contain duplicates.

## Local NLLB result (2026-11-06)

Both languages contain generated candidates for all 241 IDs. Line-ending
sequences, source numbers, placeholders, URLs, and protected ASCII names were
preserved, and the final candidates contain no Han. This is structural coverage,
not reviewed translation coverage:

| Gate | English | French |
|---|---:|---:|
| Candidate IDs | 241/241 | 241/241 |
| Human-reviewed IDs | 0/241 | 0/241 |
| Short-context ambiguities | 53 | 53 |
| Mixed-script/proper-name ambiguities | 85 | 85 |
| Credit/attribution ambiguities | 12 | 12 |
| Long/credit-block ambiguities | 1 | 1 |
| Length/layout risks | 202 | 201 |

The model output is therefore not accepted as production-quality EN or FR.
Local author overrides and reviewed IDs can be recorded only in ignored
`work/i18n/overrides/<language>.json`; no game text is committed.

## FFDec candidates and exact round trip

Incomplete FFDec outputs are research candidates only under
`work/i18n/candidates/`. They are not published to `work/i18n/patched/` and must
not be named, captured, or documented as finished EN/FR products:

| Language | Candidate SWF SHA-256 | Exact selected-ID round trips |
|---|---|---:|
| EN | `5ddbfe77552c4b6ec9c034643fbc0c0dd50a1841f1643bd8d2f75e03ba37406a` | 1/241 |
| FR | `cf61eb2712d59f086404d928f6622aaf5692b9a55ba345d30cc24aed864f2cd4` | 1/241 |

Only ID `26696` round-tripped exactly in each candidate. FFDec emitted extensive
`Selected source font does not contain character` errors, notably for embedded
font `40090`, and rejected spacing metadata for ID `25878` after replacement.
The changed SWF hashes prove that FFDec wrote candidate bytes, not that 241
translations were accepted. The exact fresh export proves the opposite: 240 IDs
differ from their requested replacement in each language.

## Launcher and visual status

The launcher reads only fixed-path `work/i18n/activation/{en,fr}.json` manifests.
It selects `work/i18n/patched/kof-wing-ex-v1.0.{en,fr}.swf` only when coverage,
QA, exact round trip, font, layout, and gameplay flags all pass, counts equal
241, blockers are empty, and the generated file hash matches. Local visual
approval must list exactly the inventory/translation Han ID set (no duplicate,
boolean, or coerced values); any other 241 integers are rejected. Eligible
publication to `patched/` is atomic; ineligible outputs are retracted before the
final activation JSON is written. Both manifests are currently `eligible=false`,
so the patched activation path stays unpublished and `play`/`doctor`/`language
show` report an embedded block while keeping the audited base SWF. Incomplete
candidates therefore cannot be launched. Synthetic FR selection proof covers the
same blocked path `play` would use; a proprietary-free `play --dry-run` still
cannot run because source/Ruffle archives are required.

A local Ruffle probe opened the EN candidate and reached a loading screen. The
ignored capture under `work/i18n/captures/` is research evidence only: it is not
an accepted translation product, layout gate, or gameplay gate. A comparable
FR/full-flow validation was not accepted because the FFDec round-trip/font gates
had already failed.

## Reproduction

```console
uv run --no-project scripts/i18n-pipeline.py inventory
uv run --with ctranslate2==4.8.1 --with transformers==5.15.0 scripts/i18n-pipeline.py translate
uv run --no-project scripts/i18n-pipeline.py qa
uv run --no-project scripts/i18n-pipeline.py patch
```

The translation command first verifies the canonical local model through
`k-translate ls` and loads it with `local_files_only=True`. No source or
translation is uploaded. `make check` runs only proprietary-data-free synthetic
pipeline tests; the local-data pipeline remains an explicit manual audit.

## Technical blocker resolution (2026-11-06, superseding result)

The 1/241 result above came from two independent pipeline defects rather than an
FFDec inability to write the selected tags. First, the formatted parser treated
`spacing`, `spacingpair`, `resetspacing`, `letterspacing`, and `variablename`
blocks as visible text, so ID `25878` and related records fed FFDec malformed
directives. Second, the importer discarded all style runs after the first,
selecting font `40090` without the required Latin outlines and collapsing long
static strings into a single SWF `TEXTRECORD` whose glyph count is an unsigned
byte.

The corrected recipe keeps every exported metadata block byte-for-byte,
distributes replacement text proportionally over the existing visible runs, and
fails if any static multi-run `TEXTRECORD` exceeds 255 Unicode code points (SWF
`glyphCount` is UI8). A sole `DefineEditText` run is not subject to that count.
Before text import, the local Java helper `scripts/FfdecFontGlyphs.java` uses
FFDec's own `FontTag.addCharacter`
API to add only requested BMP outlines to all 13 referenced embedded fonts;
supplementary code points are rejected because the API accepts Java `char` only.
The outlines come from locally installed libre Noto Sans, Noto Sans Symbols,
Noto Sans Symbols 2, and Noto Sans CJK SC; the fonts are copied only into ignored
work state. Fontconfig fallback substitution is rejected by normalized family
identity (so `Noto Sans Symbols 2` matches fontconfig's `Noto Sans Symbols2`).
The locally installed Debian Noto packages identify these faces as SIL Open Font
License 1.1; because no font file is redistributed, no font license payload is
added to this repository. No cloud service or proprietary replacement font is
used.

Fresh `text:formatted` exports now reproduce the exact requested semantic text
for all IDs:

| Gate | English | French |
|---|---:|---:|
| Exact semantic round trips | 241/241 | 241/241 |
| Residual Han in inventoried IDs | 0 | 0 |
| Referenced embedded fonts covered | 13/13 | 13/13 |
| Formatted metadata normalized by FFDec | 55 | 51 |
| Candidate SHA-256 | `66eaa8716a2f73c038cc1ab600b3230c3763b71e0138432b4a9d9ed7b3db0863` | `bf118b7171b610163a06bda04852615fa4f3825d52f45c1bae9837d9b820e7fb` |

The metadata-normalized IDs are explicitly reported in each activation manifest.
They are not semantic mismatches: FFDec recalculates spacing pairs after glyph
substitution while the concatenated visible runs remain byte-exact UTF-8.

Local Ruffle captures are stored under the clearly named ignored directories
`work/i18n/captures/en-candidate-roundtrip-241/` and
`work/i18n/captures/fr-candidate-roundtrip-241/`; hash-specific reruns use
`en-candidate-46bd21d6/` and `fr-candidate-4d97dd64/`. No capture directory uses
`final`, because none is an accepted product. They show that Latin and French
glyphs render, but also show substantial clipping in the original Chinese-sized
text bounds. Accordingly, both activation manifests remain `eligible=false`:
human review, layout approval, and gameplay approval are still false, and no file
is published under `work/i18n/patched/`.

Correction after the latest deterministic tokenizer pass: `fix_mistral_regex`, a
192-token decoding bound, trigram repetition prevention, and a 1.1 repetition
penalty prevent pathological local NLLB loops without weakening any structural
gate. The final candidate hashes are
`46bd21d67fd8694e8db9bc1a9b4ec9d7d5fa3a359c3385af9cc417b508647fe1`
for EN and
`4d97dd64fe8de1c0764b501aaf0df5d0a7cbcca7356e0890cabc82a6f7119683`
for FR. Exact semantic round trip remains 241/241 for each language; final FFDec
metadata normalization counts are 52 EN and 51 FR. Current QA reports 207 EN
and 223 FR layout-risk IDs, 241 unreviewed IDs per language, 53 short-context
ambiguities, 74 mixed-script/proper-name ambiguities, 12 credit/attribution
ambiguities, and one long/credit-block ambiguity per language. Structural QA
passes, but quality review does not. The exact activation blockers for both
languages are `qa_valid`, `font_valid`, `layout_valid`, `gameplay_valid`, and
`missing_or_stale_local_visual_approval`; technical `font_coverage` is true,
while visual font approval remains false.
