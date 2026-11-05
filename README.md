# KOF Wing

Cross-platform preservation and launcher project for a locally owned copy of
The King of Fighters Wing EX 1.0.

The original game data stays outside this repository. `work/` contains local
derivatives and pinned tools; `refs/` contains upstream research sources;
`research/` contains reproducible findings.

Current direction: extract the Flash projector, run the SWF with pinned Ruffle,
and apply explicit ActionScript patches instead of modifying the Windows
projector or original assets.
