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
check_hash a5f6df9881771226e330e08a1fa7666f4caff50ddf9f2f993cdd95293edca627 \
    "$ROOT/work/tools/ruffle-0.7.1-windows-x86_64.zip"
check_hash 56ce960aeafc7caf034e2052ba49f878a883f7c7ca690da6eeffca9587b5e4b9 \
    "$ROOT/work/tools/ffdec_26.4.0.zip"
check_hash fd05314b2f3cddaf9c85aa512bc796e6c57d3bca21022b3e9b59bf74f1ce1513 \
    "$ROOT/work/tools/dump-projector"
check_hash 6f80297a853bcf8cda114ff131b5c0a8c9a4d96a2a9c3dee1f04c39c54206846 \
    "$ROOT/work/tools/Sunshine_2026.914.233613_x86_64.AppImage"
check_hash 233008e46f4c0e501a586cbfd6c4fd4a4c0d414a0b5fc7f13c070eb92ec3824b \
    "$ROOT/work/tools/Sunshine-Windows-AMD64-lite.zip"
check_hash 787b0dcbd42ae90620e76fb8d77d7c441ba6473a91c2d7fc3a0fc2466f992437 \
    "$ROOT/work/tools/Moonlight-6.2.0-x86_64.AppImage"
check_hash 6f91d268b41ed5ca8066d9d7578dc403b8db190991ff48f00b1700465f8f7ebf \
    "$ROOT/work/tools/MoonlightPortable-x64-6.2.0.zip"
[[ -x "$ROOT/work/tools/ruffle-0.7.1/ruffle" ]]
[[ -x "$ROOT/work/tools/dump-projector" ]]

if [[ -n "$SOURCE" ]]; then
    [[ -f "$SOURCE/The King Of Fighters Wing EX v1.0.exe" ]]
    [[ -f "$SOURCE/stage/list.txt" ]]
    [[ -f "$SOURCE/sound/list.txt" ]]
    [[ -f "$SOURCE/data/skillList.swf" ]]
fi

printf 'kof-wing tools: ok\n'
