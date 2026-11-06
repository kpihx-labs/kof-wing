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
