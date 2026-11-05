# Architecture proposal

## Goal

Preserve the original KOF Wing combat and assets while removing the obsolete
Windows Flash projector and making configuration explicit and portable.

## Runtime

Use pinned Ruffle builds for Linux and Windows. A small launcher owns:

- source discovery and hash verification;
- extraction of the projector SWF when absent;
- project-local Ruffle configuration and SharedObject storage;
- window/fullscreen, scale, quality and input profiles;
- optional patch selection.

The launcher must not bundle the game. It consumes the locally owned directory.

## Customization

1. **Data overlay:** generate `stage/list.txt` and `sound/list.txt` from a plain
   project configuration. Custom stages and music remain separate files.
2. **Difficulty:** expose the five native AI levels consistently instead of
   changing timing or globally speeding the game.
3. **Persistence:** keep separate profiles and snapshots by mapping Ruffle
   SharedObjects into named directories.
4. **Code patches:** import only targeted ActionScript/P-code changes through
   FFDec, record source and output hashes, and reject unknown base payloads.

## LAN contract

KOF Wing already owns the two-player rules and local input path. The portable
LAN design keeps one authoritative Ruffle process on the host and transports
player-two input from a peer on the same subnet. The peer receives a low-latency
hardware-encoded video/audio stream. This avoids pretending that two Flash VMs
are deterministic enough for lockstep or rollback.

The implementation must provide discovery, explicit host/join roles, controller
mapping, latency/jitter display, reconnect, focus-loss key release and a local
network-only security boundary. It must not depend on a cloud signalling server,
account, public relay, VPN or port forwarding.

If later differential tests prove deterministic Ruffle snapshots, rollback can
replace streaming; it is not a v1 promise.

## What not to promise yet

- Arbitrary new fighters: characters and combat logic are embedded and deeply
  coupled.
- Perfect high-level ActionScript recovery: pathological AI classes require
  P-code inspection.
- Deterministic lockstep or rollback before Ruffle snapshot equivalence is
  proven; v1 LAN is host-authoritative.
