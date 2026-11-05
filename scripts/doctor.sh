#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${KOF_SOURCE:-}"

check_hash() {
    local expected="$1" file="$2" actual
    actual="$(sha256sum "$file" | cut -d' ' -f1)"
    [[ "$actual" == "$expected" ]] || {
        printf 'hash mismatch: %s\n' "$file" >&2
        return 1
    }
}

check_hash 67950dfdde643f9087ae478eb3637fe52e0deefc79bdb55fe43cd69dab9fdd99 \
    "$ROOT/work/tools/ruffle-0.7.1-linux-x86_64.tar.gz"
check_hash 56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9 \
    "$ROOT/work/tools/ffdec_26.4.0.zip"
[[ -x "$ROOT/work/tools/ruffle-0.7.1/ruffle" ]]
[[ -x "$ROOT/work/tools/dump-projector" ]]

if [[ -n "$SOURCE" ]]; then
    [[ -f "$SOURCE/The King Of Fighters Wing EX v1.0.exe" ]]
    [[ -f "$SOURCE/stage/list.txt" ]]
    [[ -f "$SOURCE/sound/list.txt" ]]
    [[ -f "$SOURCE/data/skillList.swf" ]]
fi

printf 'kof-wing tools: ok\n'
