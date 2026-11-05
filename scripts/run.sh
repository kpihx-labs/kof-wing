#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${KOF_SOURCE:-${1:-}}"
[[ -n "$SOURCE" ]] || {
    printf 'Set KOF_SOURCE or pass the original game directory.\n' >&2
    exit 2
}
SOURCE="$(realpath "$SOURCE")"
SWF="$ROOT/work/kof-wing-ex-v1.0.swf"

if [[ ! -f "$SWF" ]]; then
    "$ROOT/work/tools/dump-projector" \
        "$SOURCE/The King Of Fighters Wing EX v1.0.exe" "$SWF"
fi

mkdir -p "$ROOT/work/config/ruffle"
base_uri="file://${SOURCE// /%20}/"
exec "$ROOT/work/tools/ruffle-0.7.1/ruffle" \
    --config "$ROOT/work/config/ruffle" \
    --graphics vulkan --power high --quality high16x16 \
    --storage disk --open-url-mode deny --filesystem-access-mode allow \
    --base "$base_uri" --width 1320 --height 970 "$SWF"
