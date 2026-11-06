#!/usr/bin/env python3
"""Local-only embedded-text inventory, NLLB translation, QA, and FFDec patching.

All source/replacement text and generated SWFs stay below ignored ``work/``.
Tracked reports contain counts, IDs, hashes, and blockers, never extracted text.
Run translation with the exact dependency versions used by k-translate:

    uv run --with ctranslate2==4.8.1 --with transformers==5.15.0 \
      scripts/i18n-pipeline.py translate
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


BASE_SHA256 = "d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334"
FFDEC_SHA256 = "56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9"
MODEL_NAME = "nllb-200-distilled-600M-ct2-int8"
EXPECTED_HAN = 241
LANGUAGES = {"en": "eng_Latn", "fr": "fra_Latn"}
MAX_JSON_BYTES = 32 * 1024 * 1024
MAX_TEXT_BYTES = 2 * 1024 * 1024
SUBPROCESS_TIMEOUT_SECONDS = 1800
HAN = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")
NUMBER = re.compile(r"(?<![\w])[-+]?\d+(?:[.,:/-]\d+)*(?![\w])")
PLACEHOLDER = re.compile(
    r"(?:https?://\S+|www\.\S+|\{[^{}\r\n]+\}|%\([^)]+\)[#0+\- .\d]*[a-zA-Z]|"
    r"%[#0+\- .\d]*[a-zA-Z]|<[^<>\r\n]+>|[A-Za-z][A-Za-z0-9_.+/@:'\-]*(?:\s+[A-Za-z0-9_.+/@:'\-]+)*)"
)
LINE_ENDING = re.compile(r"\r\n|\r|\n")


@dataclass(frozen=True)
class Paths:
    root: Path

    @property
    def work(self) -> Path:
        return self.root / "work/i18n"

    @property
    def base(self) -> Path:
        return self.root / "work/extracted/kof-wing-ex-v1.0.swf"

    @property
    def ffdec_archive(self) -> Path:
        return self.root / "work/tools/ffdec_26.4.0.zip"

    @property
    def ffdec_jar(self) -> Path:
        return self.root / "work/tools/ffdec-26.4.0/ffdec-cli.jar"

    @property
    def inventory(self) -> Path:
        return self.work / "inventory/manifest.json"

    def translation(self, language: str) -> Path:
        return self.work / f"translations/{language}.json"

    def qa(self, language: str) -> Path:
        return self.work / f"reports/qa-{language}.json"

    def activation(self, language: str) -> Path:
        return self.work / f"activation/{language}.json"

    def candidate(self, language: str) -> Path:
        return self.work / f"candidates/kof-wing-ex-v1.0.{language}.swf"

    def patched(self, language: str) -> Path:
        return self.work / f"patched/kof-wing-ex-v1.0.{language}.swf"


def require_language(language: str) -> str:
    if language not in LANGUAGES:
        raise RuntimeError(f"unsupported language '{language}'")
    return language


def _root_path(paths: Paths) -> Path:
    root = paths.root.expanduser()
    if not root.is_absolute():
        root = Path.cwd() / root
    if root.is_symlink():
        raise RuntimeError(f"repository root must not be a symlink: {root}")
    return Path(os.path.normpath(root))


def under_root(paths: Paths, path: Path) -> Path:
    """Return a path under the repo without following any symlink component."""
    root = _root_path(paths)
    absolute = path.expanduser()
    if not absolute.is_absolute():
        absolute = root / absolute
    normalized = Path(os.path.normpath(absolute))
    try:
        relative = normalized.relative_to(root)
    except ValueError as error:
        raise RuntimeError(f"path escapes repository root: {path}") from error
    current = root
    for part in relative.parts:
        current = current / part
        if current.is_symlink():
            raise RuntimeError(f"symlink path component refused: {current}")
    return current


def ensure_real_directory(paths: Paths, directory: Path) -> Path:
    """Create or validate a real directory; never create or traverse via symlink."""
    root = _root_path(paths)
    target = under_root(paths, directory)
    relative = target.relative_to(root)
    current = root
    for part in relative.parts:
        current = current / part
        if current.is_symlink():
            raise RuntimeError(f"symlink path component refused: {current}")
        if current.exists():
            if not current.is_dir():
                raise RuntimeError(f"expected a real directory under the repository: {current}")
            continue
        current.mkdir(parents=False, exist_ok=False)
        if current.is_symlink() or not current.is_dir():
            raise RuntimeError(f"expected a real directory under the repository: {current}")
    return target


def require_regular_file(paths: Paths, path: Path, *, max_bytes: int | None = None) -> Path:
    resolved = under_root(paths, path)
    if resolved.is_symlink() or not resolved.is_file():
        raise RuntimeError(f"expected a real regular file under the repository: {path}")
    if max_bytes is not None and resolved.stat().st_size > max_bytes:
        raise RuntimeError(f"{path} exceeds {max_bytes} bytes")
    return resolved


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def text_hash(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def write_json(paths: Paths, path: Path, value: Any) -> None:
    target = under_root(paths, path)
    ensure_real_directory(paths, target.parent)
    temporary = under_root(paths, target.with_suffix(target.suffix + ".partial"))
    if temporary.exists() or temporary.is_symlink():
        if temporary.is_symlink() or temporary.is_dir():
            raise RuntimeError(f"refusing to write through symlink or directory: {temporary}")
        temporary.unlink()
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if temporary.is_symlink():
        temporary.unlink()
        raise RuntimeError(f"refusing to publish symlink JSON: {temporary}")
    under_root(paths, target)
    temporary.replace(target)
    if target.is_symlink() or not target.is_file():
        raise RuntimeError(f"JSON destination must remain a real regular file: {target}")


def read_json(paths: Paths, path: Path) -> Any:
    resolved = require_regular_file(paths, path, max_bytes=MAX_JSON_BYTES)
    return json.loads(resolved.read_text(encoding="utf-8"))


def read_text_file(paths: Paths, path: Path) -> str:
    resolved = require_regular_file(paths, path, max_bytes=MAX_TEXT_BYTES)
    return resolved.read_text(encoding="utf-8")


def verify_inputs(paths: Paths) -> None:
    for path, expected, label in [
        (paths.base, BASE_SHA256, "audited base SWF"),
        (paths.ffdec_archive, FFDEC_SHA256, "pinned FFDec archive"),
    ]:
        resolved = require_regular_file(paths, path)
        actual = sha256(resolved)
        if actual != expected:
            raise RuntimeError(f"{label} hash mismatch: expected {expected}, got {actual}")
    require_regular_file(paths, paths.ffdec_jar)


def run_ffdec(paths: Paths, *arguments: object) -> None:
    java = shutil.which("java")
    if java is None:
        raise RuntimeError("java executable is not available on PATH")
    command = [java, "-jar", str(require_regular_file(paths, paths.ffdec_jar)), *map(str, arguments)]
    for argument in arguments:
        if isinstance(argument, Path):
            under_root(paths, argument)
    subprocess.run(
        command,
        cwd=paths.root,
        check=True,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def reset_generated(paths: Paths, directory: Path) -> None:
    target = under_root(paths, directory)
    if target.exists() or target.is_symlink():
        if target.is_symlink() or not target.is_dir():
            raise RuntimeError(f"refusing to reset non-directory path: {directory}")
        shutil.rmtree(target)
    ensure_real_directory(paths, target)


def publish_or_retract_patched(paths: Paths, language: str, candidate: Path, eligible: bool) -> None:
    """Only eligible complete patches may occupy the launcher activation path.

    Ineligible artifacts are retracted before any final activation state is kept.
    Eligible publication is atomic (copy to ``.partial`` then replace).
    """
    language = require_language(language)
    patched = under_root(paths, paths.patched(language))
    ensure_real_directory(paths, patched.parent)
    # Retract first so a failed or ineligible publish cannot leave a stale SWF.
    if patched.exists() or patched.is_symlink():
        patched.unlink()
    if not eligible:
        return
    source = require_regular_file(paths, candidate)
    temporary = under_root(paths, patched.with_suffix(patched.suffix + ".partial"))
    if temporary.exists() or temporary.is_symlink():
        if temporary.is_dir() and not temporary.is_symlink():
            raise RuntimeError(f"refusing to overwrite directory partial: {temporary}")
        temporary.unlink()
    shutil.copy2(source, temporary)
    if temporary.is_symlink() or not temporary.is_file():
        if temporary.exists() or temporary.is_symlink():
            temporary.unlink()
        raise RuntimeError(f"eligible patch partial must be a real regular file: {temporary}")
    temporary.replace(patched)
    if patched.is_symlink() or not patched.is_file():
        raise RuntimeError(f"eligible patch must remain a real regular file: {patched}")


def formatted_parts(value: str) -> tuple[str, str]:
    """Return an import prefix and semantic text from FFDec formatted output.

    DefineEditText has one metadata block followed by text. DefineText can have
    many interleaved style blocks; the replacement intentionally keeps the bounds
    block and first style/font block, then emits one replacement run.
    """
    blocks: list[tuple[int, int, str]] = []
    cursor = 0
    semantic: list[str] = []
    while cursor < len(value):
        if value[cursor] != "[":
            next_block = value.find("[", cursor)
            end = len(value) if next_block < 0 else next_block
            semantic.append(value[cursor:end])
            cursor = end
            continue
        end = value.find("]", cursor + 1)
        if end < 0:
            semantic.append(value[cursor:])
            break
        content = value[cursor + 1 : end]
        if not re.search(r"(?m)^(?:xmin|font|height|color|x|y|readonly|noselect|useoutlines)\b", content):
            semantic.append(value[cursor : end + 1])
            cursor = end + 1
            continue
        blocks.append((cursor, end + 1, value[cursor : end + 1]))
        cursor = end + 1
    if not blocks or blocks[0][0] != 0:
        raise ValueError("FFDec formatted text lacks leading metadata")
    prefix = blocks[0][2]
    if "font " not in prefix and len(blocks) > 1:
        prefix += blocks[1][2]
    return prefix, "".join(semantic)


def line_endings(value: str) -> list[str]:
    return LINE_ENDING.findall(value)


def inventory(paths: Paths) -> dict[str, Any]:
    verify_inputs(paths)
    export_root = paths.work / "inventory/export"
    reset_generated(paths, export_root)
    plain = export_root / "plain"
    formatted = export_root / "formatted"
    run_ffdec(paths, "-format", "text:plain", "-export", "text", plain, paths.base)
    run_ffdec(paths, "-format", "text:formatted", "-export", "text", formatted, paths.base)

    entries = []
    nonempty = 0
    for plain_path in sorted(plain.glob("*.txt"), key=lambda item: int(item.stem)):
        source = read_text_file(paths, plain_path)
        if not source:
            continue
        nonempty += 1
        if not HAN.search(source):
            continue
        formatted_path = formatted / plain_path.name
        formatted_value = read_text_file(paths, formatted_path)
        header, formatted_body = formatted_parts(formatted_value)
        source = formatted_body
        if not source:
            continue
        font_ids = sorted({int(value) for value in re.findall(r"(?m)^font\s+(\d+)\s*$", header)})
        entries.append(
            {
                "id": int(plain_path.stem),
                "source": source,
                "source_sha256": text_hash(source),
                "formatted_sha256": text_hash(formatted_value),
                "font_ids": font_ids,
                "line_count": len(line_endings(source)) + 1,
                "characters": len(source),
            }
        )
    if nonempty != 675 or len(entries) != EXPECTED_HAN:
        raise RuntimeError(
            f"inventory count drift: non-empty={nonempty} (expected 675), "
            f"Han={len(entries)} (expected {EXPECTED_HAN})"
        )
    manifest = {
        "schema_version": 1,
        "base_sha256": BASE_SHA256,
        "nonempty_text_symbols": nonempty,
        "han_text_symbols": len(entries),
        "entries": entries,
    }
    write_json(paths, paths.inventory, manifest)
    write_public_inventory_report(paths, manifest)
    return manifest


def write_public_inventory_report(paths: Paths, manifest: dict[str, Any]) -> None:
    entries = manifest["entries"]
    duplicate_groups = defaultdict(list)
    for entry in entries:
        duplicate_groups[entry["source_sha256"]].append(entry["id"])
    public = {
        "schema_version": 1,
        "base_sha256": BASE_SHA256,
        "nonempty_text_symbols": manifest["nonempty_text_symbols"],
        "han_text_symbols": len(entries),
        "multiline_symbols": sum(entry["line_count"] > 1 for entry in entries),
        "duplicate_source_groups": [ids for ids in duplicate_groups.values() if len(ids) > 1],
        "ids": [entry["id"] for entry in entries],
        "notice": "No extracted or translated game text is present in this report.",
    }
    write_json(paths, paths.work / "reports/inventory.json", public)


def protected_spans(value: str) -> list[tuple[int, int]]:
    spans = [match.span() for match in PLACEHOLDER.finditer(value)]
    spans.extend(match.span() for match in NUMBER.finditer(value))
    spans.sort()
    merged: list[tuple[int, int]] = []
    for start, end in spans:
        if merged and start < merged[-1][1]:
            if end > merged[-1][1]:
                merged[-1] = (merged[-1][0], end)
        else:
            merged.append((start, end))
    return merged


def split_translatable(value: str) -> list[tuple[str, bool]]:
    result: list[tuple[str, bool]] = []
    cursor = 0
    for start, end in protected_spans(value):
        if cursor < start:
            result.append((value[cursor:start], True))
        result.append((value[start:end], False))
        cursor = end
    if cursor < len(value):
        result.append((value[cursor:], True))
    return result


def clauses(value: str, limit: int = 450) -> list[str]:
    if len(value) <= limit:
        return [value]
    parts = re.split(r"(?<=[。！？!?；;，,])", value)
    output: list[str] = []
    current = ""
    for part in parts:
        if len(current) + len(part) <= limit:
            current += part
            continue
        if current:
            output.append(current)
        while len(part) > limit:
            output.append(part[:limit])
            part = part[limit:]
        current = part
    if current:
        output.append(current)
    return output


def translatable_units(entries: list[dict[str, Any]]) -> tuple[list[str], list[list[tuple[str, int | str]]]]:
    units: list[str] = []
    recipes: list[list[tuple[str, int | str]]] = []
    for entry in entries:
        recipe: list[tuple[str, int | str]] = []
        for piece in re.split(r"(\r\n|\r|\n)", entry["source"]):
            if LINE_ENDING.fullmatch(piece):
                recipe.append(("literal", piece))
                continue
            for fragment, may_translate in split_translatable(piece):
                if may_translate and HAN.search(fragment):
                    for clause in clauses(fragment):
                        if HAN.search(clause):
                            recipe.append(("unit", len(units)))
                            units.append(clause)
                        else:
                            recipe.append(("literal", clause))
                else:
                    recipe.append(("literal", fragment))
        recipes.append(recipe)
    return units, recipes


def require_local_k_translate_model() -> Path:
    executable = shutil.which("k-translate")
    if executable is None:
        raise RuntimeError("k-translate executable is not available on PATH")
    probe = subprocess.run(
        [executable, "ls"],
        check=True,
        text=True,
        capture_output=True,
        timeout=60,
    )
    if MODEL_NAME not in probe.stdout:
        raise RuntimeError(f"local k-translate model is not installed: {MODEL_NAME}")
    model = Path.home() / ".local/share/ctranslate2" / MODEL_NAME
    if model.is_symlink() or not model.is_dir():
        raise RuntimeError(f"k-translate listed the model but its canonical directory is missing: {model}")
    return model


def nllb_translate(units: list[str], target: str, batch_size: int) -> list[str]:
    try:
        import ctranslate2  # type: ignore[import-not-found]
        import transformers  # type: ignore[import-not-found]
    except ImportError as error:
        raise RuntimeError(
            "translation dependencies missing; run with: uv run --with ctranslate2==4.8.1 "
            "--with transformers==5.15.0 scripts/i18n-pipeline.py translate"
        ) from error
    model = require_local_k_translate_model()
    translator = ctranslate2.Translator(str(model), device="cpu", intra_threads=4)
    tokenizer = transformers.AutoTokenizer.from_pretrained(model, local_files_only=True)
    tokenizer.src_lang = "zho_Hans"
    output: list[str] = []
    for offset in range(0, len(units), batch_size):
        chunk = units[offset : offset + batch_size]
        sources = [tokenizer.convert_ids_to_tokens(tokenizer.encode(value)) for value in chunk]
        results = translator.translate_batch(
            sources,
            target_prefix=[[target] for _ in sources],
            max_batch_size=batch_size,
            batch_type="examples",
        )
        for result in results:
            tokens = result.hypotheses[0][1:]
            output.append(tokenizer.decode(tokenizer.convert_tokens_to_ids(tokens)).strip())
    return output


def ambiguity_reasons(source: str) -> list[str]:
    reasons = []
    han_count = len(HAN.findall(source))
    if han_count <= 2:
        reasons.append("short_context")
    if len(source) > 450:
        reasons.append("long_or_credit_block")
    if re.search(r"(?:版权|制作|作者|鸣谢|©|copyright)", source, re.IGNORECASE):
        reasons.append("credit_or_attribution")
    if any(character.isascii() and character.isalpha() for character in source):
        reasons.append("mixed_script_or_proper_name")
    return reasons


def translate(paths: Paths, languages: Iterable[str], batch_size: int) -> None:
    if paths.inventory.is_file():
        manifest = read_json(paths, paths.inventory)
    else:
        manifest = inventory(paths)
    entries = manifest["entries"]
    units, recipes = translatable_units(entries)
    for language in languages:
        language = require_language(language)
        translated_units = nllb_translate(units, LANGUAGES[language], batch_size)
        translated_entries = []
        for entry, recipe in zip(entries, recipes, strict=True):
            text = "".join(
                str(value) if kind == "literal" else translated_units[int(value)]
                for kind, value in recipe
            )
            translated_entries.append(
                {
                    "id": entry["id"],
                    "source_sha256": entry["source_sha256"],
                    "text": text,
                    "text_sha256": text_hash(text),
                    "ambiguities": ambiguity_reasons(entry["source"]),
                    "human_reviewed": False,
                    "provenance": {
                        "engine": "k-translate/CTranslate2",
                        "model": MODEL_NAME,
                        "source_language": "zho_Hans",
                        "target_language": LANGUAGES[language],
                    },
                }
            )
        apply_local_overrides(paths, language, translated_entries)
        write_json(
            paths,
            paths.translation(language),
            {
                "schema_version": 1,
                "language": language,
                "source_language": "zho_Hans",
                "target_language": LANGUAGES[language],
                "engine": "k-translate/CTranslate2",
                "model": MODEL_NAME,
                "base_sha256": BASE_SHA256,
                "entries": translated_entries,
            },
        )


def apply_local_overrides(paths: Paths, language: str, entries: list[dict[str, Any]]) -> None:
    language = require_language(language)
    path = paths.work / f"overrides/{language}.json"
    if not path.is_file() or path.is_symlink():
        return
    overrides = read_json(paths, path)
    by_id = {entry["id"]: entry for entry in entries}
    for raw_id, replacement in overrides.get("replacements", {}).items():
        identifier = int(raw_id)
        if identifier not in by_id or not isinstance(replacement, str) or not replacement:
            raise RuntimeError(f"invalid local {language} override for ID {raw_id}")
        by_id[identifier]["text"] = replacement
        by_id[identifier]["text_sha256"] = text_hash(replacement)
        by_id[identifier]["provenance"] = {
            "engine": "author-override",
            "model": None,
            "source_language": "author",
            "target_language": language,
        }
    reviewed = {int(value) for value in overrides.get("reviewed_ids", [])}
    for identifier in reviewed:
        if identifier not in by_id:
            raise RuntimeError(f"unknown reviewed ID {identifier} in {path}")
        by_id[identifier]["human_reviewed"] = True


def token_counter(pattern: re.Pattern[str], value: str) -> Counter[str]:
    return Counter(match.group(0) for match in pattern.finditer(value))


def missing_tokens(pattern: re.Pattern[str], source: str, target: str) -> bool:
    expected = token_counter(pattern, source)
    return any(target.count(token) < count for token, count in expected.items())


def qa_report(paths: Paths, language: str) -> dict[str, Any]:
    language = require_language(language)
    inventory_data = read_json(paths, paths.inventory)
    translation = read_json(paths, paths.translation(language))
    source_by_id = {entry["id"]: entry for entry in inventory_data["entries"]}
    target_by_id = {entry["id"]: entry for entry in translation["entries"]}
    expected = set(source_by_id)
    actual = set(target_by_id)
    issues: list[dict[str, Any]] = []
    layout_risks: list[dict[str, Any]] = []
    for identifier in sorted(expected & actual):
        source_entry = source_by_id[identifier]
        target_entry = target_by_id[identifier]
        source = source_entry["source"]
        target = target_entry["text"]
        if target_entry["source_sha256"] != source_entry["source_sha256"]:
            issues.append({"id": identifier, "kind": "source_hash_mismatch"})
        if line_endings(source) != line_endings(target):
            issues.append({"id": identifier, "kind": "line_breaks_changed"})
        if missing_tokens(PLACEHOLDER, source, target):
            issues.append({"id": identifier, "kind": "placeholder_or_name_lost"})
        if missing_tokens(NUMBER, source, target):
            issues.append({"id": identifier, "kind": "number_lost"})
        if HAN.search(target):
            issues.append({"id": identifier, "kind": "untranslated_han"})
        ratio = len(target) / max(1, len(source))
        if ratio > 2.2 or len(target) > len(source) + 40:
            layout_risks.append(
                {"id": identifier, "source_characters": len(source), "target_characters": len(target)}
            )
        if not target_entry.get("human_reviewed", False):
            issues.append({"id": identifier, "kind": "nllb_not_human_reviewed"})
        for reason in target_entry.get("ambiguities", []):
            issues.append({"id": identifier, "kind": f"ambiguity:{reason}"})
    hashes = defaultdict(list)
    for identifier, entry in target_by_id.items():
        hashes[entry["text_sha256"]].append(identifier)
    duplicate_targets = [sorted(ids) for ids in hashes.values() if len(ids) > 1]
    report = {
        "schema_version": 1,
        "language": language,
        "base_sha256": BASE_SHA256,
        "expected_ids": len(expected),
        "translated_ids": len(actual & expected),
        "missing_ids": sorted(expected - actual),
        "unexpected_ids": sorted(actual - expected),
        "issues": issues,
        "issue_counts": dict(sorted(Counter(issue["kind"] for issue in issues).items())),
        "duplicate_target_groups": duplicate_targets,
        "layout_risks": layout_risks,
        "structural_pass": not (
            expected - actual
            or actual - expected
            or any(
                issue["kind"]
                in {
                    "source_hash_mismatch",
                    "line_breaks_changed",
                    "placeholder_or_name_lost",
                    "number_lost",
                    "untranslated_han",
                }
                for issue in issues
            )
        ),
        "quality_review_pass": not issues,
        "notice": "This report contains IDs and metrics only, not game text.",
    }
    write_json(paths, paths.qa(language), report)
    return report


def patch(paths: Paths, language: str) -> dict[str, Any]:
    language = require_language(language)
    verify_inputs(paths)
    report = qa_report(paths, language)
    if report["translated_ids"] != EXPECTED_HAN or report["missing_ids"]:
        raise RuntimeError(f"cannot patch {language}: translation coverage is incomplete")
    if not report["structural_pass"]:
        raise RuntimeError(
            f"cannot patch {language}: structural QA failed "
            "(placeholders, numbers, line breaks, or residual Han)"
        )
    translation = read_json(paths, paths.translation(language))
    target_by_id = {entry["id"]: entry["text"] for entry in translation["entries"]}
    import_root = paths.work / f"imports/{language}"
    reset_generated(paths, import_root)
    texts = import_root / "texts"
    texts.mkdir()
    formatted = paths.work / "inventory/export/formatted"
    for identifier in sorted(target_by_id):
        header, _ = formatted_parts(read_text_file(paths, formatted / f"{identifier}.txt"))
        (texts / f"{identifier}.txt").write_text(
            header + target_by_id[identifier], encoding="utf-8", newline=""
        )
    candidate = under_root(paths, paths.candidate(language))
    ensure_real_directory(paths, candidate.parent)
    if candidate.exists() or candidate.is_symlink():
        candidate.unlink()
    run_ffdec(paths, "-importText", paths.base, candidate, import_root)
    patched_hash = sha256(require_regular_file(paths, candidate))
    if patched_hash == BASE_SHA256:
        raise RuntimeError(f"FFDec produced an unchanged {language} SWF")

    verify_dir = paths.work / f"roundtrip/{language}"
    reset_generated(paths, verify_dir)
    ids = ",".join(str(identifier) for identifier in sorted(target_by_id))
    run_ffdec(
        paths,
        "-selectid",
        ids,
        "-format",
        "text:plain",
        "-export",
        "text",
        verify_dir,
        candidate,
    )
    mismatches = []
    for identifier, expected in sorted(target_by_id.items()):
        exported = verify_dir / f"{identifier}.txt"
        if (
            not exported.is_file()
            or exported.is_symlink()
            or exported.read_bytes() != expected.encode("utf-8")
        ):
            mismatches.append(identifier)
    roundtrip_exact = not mismatches
    # Retract ineligible (or republish eligible atomically) before the final
    # activation JSON is written, so filesystem and status cannot disagree.
    status = activation_status(paths, language, patched_hash, report, roundtrip_exact, mismatches)
    publish_or_retract_patched(paths, language, candidate, bool(status["eligible"]))
    write_json(paths, paths.activation(language), status)
    return status


def strict_int_id(value: Any) -> int:
    """Accept only exact JSON integers; reject bools and string/float coercion."""
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"text ID must be an exact integer, got {type(value).__name__}")
    return value


def entry_ids(entries: Any) -> list[int]:
    if not isinstance(entries, list):
        raise ValueError("entries must be a list")
    ids = [strict_int_id(entry.get("id")) for entry in entries if isinstance(entry, dict)]
    if len(ids) != len(entries):
        raise ValueError("entries must be objects with integer ids")
    return ids


def expected_han_ids(paths: Paths, language: str | None = None) -> frozenset[int] | None:
    """Return the canonical inventory Han ID set, optionally cross-checked with translation."""
    try:
        inventory_data = read_json(paths, paths.inventory)
        inventory_ids = entry_ids(inventory_data.get("entries"))
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError, TypeError, AttributeError):
        return None
    if len(inventory_ids) != EXPECTED_HAN or len(set(inventory_ids)) != EXPECTED_HAN:
        return None
    expected = frozenset(inventory_ids)
    if language is None:
        return expected
    language = require_language(language)
    translation_path = paths.translation(language)
    try:
        if translation_path.is_symlink() or not translation_path.is_file():
            return expected
        translation = read_json(paths, translation_path)
        translation_ids = frozenset(entry_ids(translation.get("entries")))
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError, TypeError, AttributeError):
        return None
    if translation_ids != expected:
        return None
    return expected


def reviewed_ids_complete(approval: dict[str, Any], expected_ids: frozenset[int]) -> bool:
    """Require exact equality with inventory/translation IDs; no duplicates or coercion."""
    if len(expected_ids) != EXPECTED_HAN:
        return False
    reviewed = approval.get("reviewed_ids")
    if not isinstance(reviewed, list) or len(reviewed) != EXPECTED_HAN:
        return False
    values: list[int] = []
    for value in reviewed:
        try:
            values.append(strict_int_id(value))
        except ValueError:
            return False
    if len(values) != len(set(values)):
        return False
    return frozenset(values) == expected_ids


def activation_status(
    paths: Paths,
    language: str,
    patched_hash: str,
    report: dict[str, Any],
    roundtrip_exact: bool,
    mismatches: list[int],
) -> dict[str, Any]:
    language = require_language(language)
    approval_path = paths.work / f"approvals/{language}.json"
    try:
        if approval_path.is_file() and not approval_path.is_symlink():
            approval = read_json(paths, approval_path)
        else:
            approval = {}
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError):
        approval = {}
    expected_ids = expected_han_ids(paths, language)
    approval_matches = (
        expected_ids is not None
        and approval.get("base_sha256") == BASE_SHA256
        and approval.get("patched_sha256") == patched_hash
        and reviewed_ids_complete(approval, expected_ids)
    )
    font_valid = bool(approval_matches and approval.get("font_valid") is True)
    layout_valid = bool(approval_matches and approval.get("layout_valid") is True)
    gameplay_valid = bool(approval_matches and approval.get("gameplay_valid") is True)
    prerequisites = {
        "coverage_100": report["translated_ids"] == EXPECTED_HAN and not report["missing_ids"],
        "qa_valid": report["structural_pass"] and report["quality_review_pass"],
        "roundtrip_exact": roundtrip_exact,
        "font_valid": font_valid,
        "layout_valid": layout_valid,
        "gameplay_valid": gameplay_valid,
    }
    blockers = [name for name, valid in prerequisites.items() if not valid]
    if mismatches:
        blockers.append(f"roundtrip_mismatch_ids:{','.join(map(str, mismatches))}")
    if not approval_matches:
        blockers.append("missing_or_stale_local_visual_approval")
    return {
        "schema_version": 1,
        "language": language,
        "base_sha256": BASE_SHA256,
        "patched_sha256": patched_hash,
        "coverage_total": EXPECTED_HAN,
        "coverage_translated": report["translated_ids"],
        "prerequisites": prerequisites,
        "eligible": all(prerequisites.values()),
        "blockers": blockers,
        "artifact": {
            "candidate": str(paths.candidate(language).relative_to(paths.root)),
            "patched": str(paths.patched(language).relative_to(paths.root)),
            "activation_path_published": all(prerequisites.values()),
        },
    }


def command_all(paths: Paths, batch_size: int) -> None:
    inventory(paths)
    translate(paths, LANGUAGES, batch_size)
    for language in LANGUAGES:
        report = qa_report(paths, language)
        print(
            f"{language}: structural_pass={report['structural_pass']} "
            f"quality_review_pass={report['quality_review_pass']} issues={len(report['issues'])}"
        )
        status = patch(paths, language)
        print(
            f"{language}: patched_sha256={status['patched_sha256']} "
            f"eligible={status['eligible']} blockers={','.join(status['blockers'])}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["inventory", "translate", "qa", "patch", "all"])
    parser.add_argument("--language", choices=sorted(LANGUAGES))
    parser.add_argument("--batch-size", type=int, default=32)
    return parser.parse_args()


def main() -> None:
    arguments = parse_args()
    if arguments.batch_size < 1 or arguments.batch_size > 128:
        raise SystemExit("--batch-size must be between 1 and 128")
    root = Path(__file__).resolve().parent.parent
    paths = Paths(root)
    under_root(paths, paths.work)
    languages = (
        [require_language(arguments.language)] if arguments.language else list(LANGUAGES)
    )
    if arguments.command == "inventory":
        result = inventory(paths)
        print(f"inventoried {result['han_text_symbols']} Han-bearing text IDs")
    elif arguments.command == "translate":
        translate(paths, languages, arguments.batch_size)
        print(f"translated locally with {MODEL_NAME}: {', '.join(languages)}")
    elif arguments.command == "qa":
        for language in languages:
            print(json.dumps(qa_report(paths, language), indent=2))
    elif arguments.command == "patch":
        for language in languages:
            print(json.dumps(patch(paths, language), indent=2))
    else:
        command_all(paths, arguments.batch_size)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
