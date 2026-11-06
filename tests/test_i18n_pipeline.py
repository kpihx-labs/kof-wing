"""Synthetic tests for the local embedded-text pipeline."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/i18n-pipeline.py"
SPEC = importlib.util.spec_from_file_location("i18n_pipeline", SCRIPT)
assert SPEC and SPEC.loader
PIPELINE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = PIPELINE
SPEC.loader.exec_module(PIPELINE)

# Synthetic inventory IDs: exact length, no overlap with range(EXPECTED_HAN).
SYNTHETIC_IDS = list(range(10_000, 10_000 + PIPELINE.EXPECTED_HAN))


def write_synthetic_inventory(paths: PIPELINE.Paths, ids: list[int] | None = None) -> list[int]:
    values = list(ids if ids is not None else SYNTHETIC_IDS)
    PIPELINE.write_json(
        paths,
        paths.inventory,
        {
            "schema_version": 1,
            "base_sha256": PIPELINE.BASE_SHA256,
            "nonempty_text_symbols": 675,
            "han_text_symbols": len(values),
            "entries": [
                {
                    "id": identifier,
                    "source": "x",
                    "source_sha256": "a",
                    "formatted_sha256": "b",
                    "font_ids": [],
                    "line_count": 1,
                    "characters": 1,
                }
                for identifier in values
            ],
        },
    )
    return values


class PipelineTests(unittest.TestCase):
    def test_protected_spans_preserve_placeholders_names_urls_and_numbers(self) -> None:
        source = "你好 {player} KOF Wing 2026 https://example.test/%s"
        pieces = PIPELINE.split_translatable(source)
        protected = "".join(value for value, translatable in pieces if not translatable)
        self.assertIn("{player}", protected)
        self.assertIn("KOF Wing 2026", protected)
        self.assertIn("https://example.test/%s", protected)
        self.assertTrue(any(translatable and "你好" in value for value, translatable in pieces))

    def test_recipe_preserves_exact_line_endings(self) -> None:
        entries = [{"source": "第一行\r\nKOF 2002\n第三行"}]
        units, recipes = PIPELINE.translatable_units(entries)
        translated = [f"T{index}" for index in range(len(units))]
        rebuilt = "".join(
            str(value) if kind == "literal" else translated[int(value)]
            for kind, value in recipes[0]
        )
        self.assertEqual(PIPELINE.line_endings(entries[0]["source"]), ["\r\n", "\n"])
        self.assertEqual(PIPELINE.line_endings(rebuilt), ["\r\n", "\n"])
        self.assertIn("KOF 2002", rebuilt)

    def test_long_clauses_reassemble_without_loss(self) -> None:
        source = "甲" * 451 + "。" + "乙" * 451
        parts = PIPELINE.clauses(source)
        self.assertGreater(len(parts), 1)
        self.assertEqual("".join(parts), source)
        self.assertTrue(all(len(part) <= 450 for part in parts))

    def test_qa_token_check_allows_new_target_words_but_not_lost_source_tokens(self) -> None:
        source = "KOF {player} 2002"
        self.assertFalse(
            PIPELINE.missing_tokens(
                PIPELINE.PLACEHOLDER, source, "Play KOF now {player} version 2002"
            )
        )
        self.assertTrue(
            PIPELINE.missing_tokens(PIPELINE.PLACEHOLDER, source, "Play now {player} 2002")
        )

    def test_formatted_parser_keeps_header_exact(self) -> None:
        value = "[\nfont 42\nheight 200\n]示例"
        header, body = PIPELINE.formatted_parts(value)
        self.assertEqual(header, "[\nfont 42\nheight 200\n]")
        self.assertEqual(body, "示例")

    def test_formatted_parser_collapses_styled_define_text(self) -> None:
        value = "[\nxmin 1\nymin 2\n][\nfont 42\nheight 200\n]前[\nfont 43\n]后"
        header, body = PIPELINE.formatted_parts(value)
        self.assertEqual(header, "[\nxmin 1\nymin 2\n][\nfont 42\nheight 200\n]")
        self.assertEqual(body, "前后")

    def test_path_helpers_reject_escape_and_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = PIPELINE.Paths(root)
            (root / "work/i18n").mkdir(parents=True)
            inside = root / "work/i18n/ok.json"
            inside.write_text("{}", encoding="utf-8")
            self.assertEqual(PIPELINE.under_root(paths, inside), inside)
            outside = Path("/tmp") / "kof-wing-escape-test.json"
            with self.assertRaises(RuntimeError):
                PIPELINE.under_root(paths, outside)
            link = root / "work/i18n/link.json"
            link.symlink_to(inside)
            with self.assertRaises(RuntimeError):
                PIPELINE.require_regular_file(paths, link)
            with self.assertRaises(RuntimeError):
                PIPELINE.under_root(paths, link)

            nested_real = root / "work/i18n/real-dir"
            nested_real.mkdir()
            linked_dir = root / "work/i18n/via-link"
            linked_dir.symlink_to(nested_real)
            nested_file = nested_real / "inner.json"
            nested_file.write_text("{}", encoding="utf-8")
            with self.assertRaises(RuntimeError):
                PIPELINE.under_root(paths, linked_dir / "inner.json")
            with self.assertRaises(RuntimeError):
                PIPELINE.require_regular_file(paths, linked_dir / "inner.json")
            with self.assertRaises(RuntimeError):
                PIPELINE.write_json(paths, linked_dir / "written.json", {"ok": False})
            self.assertFalse((nested_real / "written.json").exists())

            parent_link = root / "work/i18n/parent-link"
            parent_link.symlink_to(nested_real)
            with self.assertRaises(RuntimeError):
                PIPELINE.write_json(paths, parent_link / "child.json", {"ok": False})

    def test_ineligible_candidate_is_retracted_before_final_state(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = PIPELINE.Paths(root)
            candidate = paths.candidate("fr")
            patched = paths.patched("fr")
            PIPELINE.ensure_real_directory(paths, candidate.parent)
            PIPELINE.ensure_real_directory(paths, patched.parent)
            candidate.write_bytes(b"synthetic incomplete fr candidate")
            patched.write_bytes(b"must not remain when ineligible")
            PIPELINE.publish_or_retract_patched(paths, "fr", candidate, False)
            self.assertFalse(patched.exists())
            self.assertTrue(candidate.is_file())

            PIPELINE.publish_or_retract_patched(paths, "fr", candidate, True)
            self.assertTrue(patched.is_file())
            self.assertFalse(patched.is_symlink())
            self.assertEqual(patched.read_bytes(), candidate.read_bytes())
            self.assertFalse(patched.with_suffix(patched.suffix + ".partial").exists())

    def test_eligible_publish_is_atomic_replace(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = PIPELINE.Paths(root)
            candidate = paths.candidate("en")
            patched = paths.patched("en")
            PIPELINE.ensure_real_directory(paths, candidate.parent)
            PIPELINE.ensure_real_directory(paths, patched.parent)
            candidate.write_bytes(b"synthetic eligible en candidate")
            stale_partial = patched.with_suffix(patched.suffix + ".partial")
            stale_partial.write_bytes(b"stale")
            PIPELINE.publish_or_retract_patched(paths, "en", candidate, True)
            self.assertEqual(patched.read_bytes(), b"synthetic eligible en candidate")
            self.assertFalse(stale_partial.exists())

    def test_reviewed_ids_require_exact_inventory_ids(self) -> None:
        report = {
            "translated_ids": PIPELINE.EXPECTED_HAN,
            "missing_ids": [],
            "structural_pass": True,
            "quality_review_pass": True,
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = PIPELINE.Paths(root)
            expected = write_synthetic_inventory(paths)
            approval = paths.work / "approvals/fr.json"

            # Positive: exact inventory ID set.
            PIPELINE.write_json(
                paths,
                approval,
                {
                    "base_sha256": PIPELINE.BASE_SHA256,
                    "patched_sha256": "abc",
                    "reviewed_ids": list(expected),
                    "font_valid": True,
                    "layout_valid": True,
                    "gameplay_valid": True,
                },
            )
            status = PIPELINE.activation_status(paths, "fr", "abc", report, True, [])
            self.assertTrue(status["eligible"])
            self.assertTrue(status["artifact"]["activation_path_published"])
            self.assertTrue(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": list(reversed(expected))}, frozenset(expected)
                )
            )

            # Negative: any other 241 ints (range) must not pass.
            PIPELINE.write_json(
                paths,
                approval,
                {
                    "base_sha256": PIPELINE.BASE_SHA256,
                    "patched_sha256": "abc",
                    "reviewed_ids": list(range(PIPELINE.EXPECTED_HAN)),
                    "font_valid": True,
                    "layout_valid": True,
                    "gameplay_valid": True,
                },
            )
            status = PIPELINE.activation_status(paths, "fr", "abc", report, True, [])
            self.assertFalse(status["eligible"])
            self.assertIn("missing_or_stale_local_visual_approval", status["blockers"])

            # Negative: bool coercion (True -> 1) and string coercion are rejected.
            self.assertFalse(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": [True] + expected[1:]}, frozenset(expected)
                )
            )
            self.assertFalse(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": [str(expected[0])] + expected[1:]},
                    frozenset(expected),
                )
            )
            self.assertFalse(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": [float(expected[0])] + expected[1:]},
                    frozenset(expected),
                )
            )
            self.assertFalse(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": expected + [expected[0]]}, frozenset(expected)
                )
            )
            self.assertFalse(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": expected[:-1] + [expected[0]]}, frozenset(expected)
                )
            )
            self.assertFalse(
                PIPELINE.reviewed_ids_complete(
                    {"reviewed_ids": PIPELINE.EXPECTED_HAN}, frozenset(expected)
                )
            )


if __name__ == "__main__":
    unittest.main()
