# Test characters

Ready-made characters for starting the viewer somewhere later in the game, without the
opening or the face menu (`nv-viewer … --character FILE`, `world::character`).

Each file is the game's script language, one line at a time, as its console runs it, on a
new game before its first frame. Use editor IDs, never form IDs (those depend on the load
order). Blank lines and `;` comments are skipped. One line isn't script: `level N` sets the
player's level and experience directly, with no level-up menus. The viewer prints any line
that didn't take: a name that isn't a form, or a function nv-rs doesn't carry out yet.

Files hold record names only: no game text, models or other assets.

| File | Where | What |
| --- | --- | --- |
| `dead-money-entry.txt` | `SLBoSBunkerINT` | Dead Money step D2: level 20, past character creation, the Sierra Madre broadcast followed (`NVDLC01MQ00` stage 10) |
