"""Selected-ID local FFDec probe; generated payload stays in ignored work/.

Run from the repository root with uv run --no-project scripts/i18n-probe.py.
This is a manual research probe, not part of the synthetic test suite.
"""

import hashlib
from pathlib import Path
import subprocess
import sys


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    base = root / "work/extracted/kof-wing-ex-v1.0.swf"
    archive = root / "work/tools/ffdec_26.4.0.zip"
    for path, expected in [
        (base, "d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334"),
        (archive, "56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9"),
    ]:
        if sha256(path) != expected:
            raise ValueError(f"audited input hash mismatch: {path}")
    # A fresh output tree prevents stale exports from masquerading as evidence.
    output = root / "work/patch-test" / (sys.argv[1] if len(sys.argv) > 1 else "reproduction")
    if output.parent != root / "work/patch-test" or output.name in {".", ".."}:
        raise ValueError("output must be one new directory name below work/patch-test")
    output.mkdir(exist_ok=False)
    command = ["java", "-jar", str(root / "work/tools/ffdec-26.4.0/ffdec-cli.jar")]

    def ffdec(*args: object) -> None:
        subprocess.run([*command, *map(str, args)], check=True, cwd=root)

    ffdec("-selectid", "12941", "-format", "text:formatted", "-export", "text", output / "formatted", base)
    formatted = (output / "formatted/12941.txt").read_text(encoding="utf-8")
    header, separator, _ = formatted.rpartition("]")
    if not separator:
        raise ValueError("selected-ID export lacks formatted text header")
    import_dir = output / "import/texts"
    import_dir.mkdir(parents=True)
    replacement = import_dir / "12941.txt"
    # Keep locally exported layout metadata; change only the selected text value.
    replacement.write_bytes((header + "]Loading").encode("utf-8"))
    patched = output / "patched.swf"
    negative = output / "leaf-noop.swf"
    ffdec("-importText", base, patched, import_dir.parent)
    ffdec("-importText", base, negative, import_dir)
    ffdec("-selectid", "12941", "-format", "text:plain", "-export", "text", output / "verify", patched)
    exported = output / "verify/12941.txt"
    if exported.read_bytes() != b"Loading":
        raise ValueError("selected-ID round-trip differs from replacement")
    if sha256(patched) == sha256(base) or sha256(negative) != sha256(base):
        raise ValueError("positive/negative SWF hash controls failed")
    for path in [archive, base, replacement, patched, negative, exported]:
        print(f"{sha256(path)}  {path.relative_to(root)}")
    print("PASS: ID 12941 round-trip = Loading (7 UTF-8 bytes); parent import changes SWF; leaf import is byte-identical")


if __name__ == "__main__":
    main()
