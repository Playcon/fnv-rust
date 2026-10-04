# nv-rs milestones

Updated 2026-10-03. Priority tracker and session handoff.

## Baseline

README and previous research notes report readers, rendering, streaming,
walking, actors, AI, combat, dialogue, quests, inventory, menus, audio,
V.A.T.S. and custom saves. Many corresponding source modules exist.
The first foundations batch built and tested both workspaces and ran the
opening to Doc's instruction to use the vigor tester. Missing cutscene and
menu behavior prevents calling that a faithful opening. Evidence remains in
ENGINE_REFERENCE.md, CLAUDE_REFERENCE.md and %USERPROFILE%\nv-re\findings.
The clean public source history was prepared on 2026-10-03. Earlier private
history is retained separately; do not publish its build outputs.

## Ordered gates

| Milestone | Completion gate | Status |
| --- | --- | --- |
| M1: Opening and persistent world | Retail opening movie and scripted wakeup, animations/movement, Doc Mitchell's creation sequence, exit into Goodsprings, talk/interact, save, restart and reload. Player, NPC, quest, inventory and reference state survives. Compare with the original game. | Active; initial live run and package-action reader complete; choreography and full route pending. |
| M2: Core gameplay loop | Sunny's tutorial and a representative Goodsprings quest branch through their own scripts: movement, weapons/reloads, damage/death, AI, dialogue, loot, trade and progression. Save/reload at intermediate stages. Verify melee and V.A.T.S.; track other weapon classes explicitly. | Partial implementation reported; acceptance pending. |
| M3: Base-game systems and campaign | Coverage matrix for quests, actor/creature types, weapon classes, effects, factions/crime, companions, travel and menus. Representative routes and ultimately campaign completion, with evidence and regression tests for blockers. | Inventory and acceptance routes needed. |
| M4: Stability and performance | Recorded routes and extended play without crashes or lost state. Measure frame times, memory, loading and streaming stalls on target hardware. Publish traces/settings and agreed budgets; remove measured stalls without changing behavior. | Not measured here; instrument earlier when it helps M1/M2. |
| M5: DLC and mods | Official DLC progression and reproducible plugin/archive/loose-file, script and content-extension cases; document interfaces and exclusions. | Load-order infrastructure exists; broad compatibility unverified. |
| M6: VR | Shared simulation with action inputs, independent aim and multiple views. Headset-tested tracking, controllers, menus, combat, comfort and frame budget. | Architecture documented; headset validation pending. |

Preserve save correctness, mod semantics and VR boundaries throughout; order
does not postpone foundational fixes. Set performance budgets from measured
hardware/display requirements. Original save compatibility and native binary
mod compatibility need separate researched scope; custom saves and plugin
reading do not establish them.

## Active work: M1

**Current blockers (user follow-up, 2026-10-03):** brief persistent camera
turn during lying-to-sitting, Doc looking away, incorrect interaction HUD
and a vigor tester that cannot be activated. Earlier clip logs and quest
progression are not acceptance. Start with executable and game data.
Scope of the camera report is the opening only.

Doc's queued stage45 chair exit and the native player-package look lock
are in the published baseline. This follow-up adds a pending-start-stage
input guard, native Info HUD/masks and geometry bounds for the tester's
empty OBND. Twelfth batch published: 910 core / 82 viewer tests, both
clippy/format/release checks. Live stage55 save -> tester trigger -> Doc's
instruction -> E opened the original SPECIAL interface (OPENING.md).
Thirteenth batch additionally resets trigger/seat caches on F9; 83 viewer
tests/checks pass and the same-cell reload-to-tester route passed live.
Doc's missing head tracking and exact assist timing remain open; no guessed
pose correction. Next: native gaze solver, exact camera-transition replay,
full opening acceptance and in-progress animation save restoration.
Gaze (2026-10-04): the native LookIK head chain is traced
(OPENING_LOOK_IK.md) and implemented in `world::look_ik`, with the viewer
turning heads toward the player (`viewer/src/look.rs`). Core tests pass;
not yet run with game data or compared in game. Next action: run the
opening with Doc and compare head tracking, easing and release against
the original.
The original face-menu reference has now
been inspected; detailed observations are in FACE_CREATION.md.

Handoff and evidence: [OPENING.md](OPENING.md). Ordered package actions now
drive the opening's actual first-person skeleton and Camera1st KF tracks.
The isolated live run played wakeup, situp, bedsit and standup and reached
Doc's vigor-tester instruction. Walking-mode script/save/report positions
and headings follow the physical player independently of animated camera
motion. This is a partial opening, not an acceptance pass: existing code
still skips the face menu and the movie; the original SPECIAL scene is now
implemented, with live route verification pending.

Latest verification/publication status is recorded at the top of
OPENING.md. The player path supports unconditional, idle-only package
actions while holstered; general callback scripts/topics and
restoring an active camera animation from a save remain unfinished.

Camera and ResetAI batches are checked and installed in nv-rs-play (latest:
888 core / 73 viewer tests, both clippy/format/release builds). The live
opening now gets Doc out of his chair and to the vigor tester through
stage55's ResetAI. An F5/F9 check there preserved player/Doc positions and
headings and quest stage, including a cold process restart. Mid-animation
saves remain open. The fifth batch is published too: loaded unconditional
NPC special idles now play, including stage30's mirror gesture. The live
opening continued to stage55; original-game visual comparison remains due.
The sixth published batch also corrects the player's loaded-idle pose blend
after the request gate; checks and live route passed at the same counts.

Vit-o-matic batch: original XML, animated models, number/bulb callbacks,
keyboard and triangle picking are connected; the old text substitute is gone.
900 core /74 viewer tests, clippy, formatting and both releases pass. Installed
in nv-rs-play; matching hashes are in OPENING.md. Allocation/closing has a viewer regression and the
clean Strength page render was inspected. Live PC mouse/keyboard allocation,
page turning and closing now pass; quest progression remains due.
Original tester inspected: framing corresponds, background blur remains missing.
Its visible room/bright bulbs exposed a composition bug; explicit transparent
camera output fixes the black background and dimmed glows (VIGOR.md).
The isolated stage55 checkpoint remains available.
Face choice filters now read race, hair and eyes with traced eligibility;
six regressions and an official-data check pass. Eighth batch is installed
in nv-rs-play:906 core/74 viewer tests, both clippy/format/release checks pass.
Original-game input automation failed; user opened its tester manually and
was asked to open ShowRaceMenu next. Latest publication/check status: OPENING.md.
Next action: reproduce and diagnose the reported opening animation failures.
Subsequent face-creation state/preview is traced in
[FACE_CREATION.md](FACE_CREATION.md);
[VIGOR.md](VIGOR.md) records the implementation evidence and limits.
Keep this work on the opening route before expanding coverage elsewhere.

Known blockers: Bink playback, general idle dispatch, face menu,
vigor-tester menu fidelity and mid-animation persistence. Exit to Goodsprings
and full-route save/restart/reload remain untested. Acceptance runs must not bypass
progression or silently accept missing menus.

## Deferred

Cosmetic material/lighting discrepancies, isolated facial polish, sun glare,
distant water and other rendering leftovers stay in the research archives.
Promote only for a milestone blocker or explicit user priority. Small gameplay
fixes needed by the active route count as blockers; never invent a substitute.

## Handoff

After each batch replace the active-work section with the latest outcome,
evidence links, blockers and one next action. Keep this file short; detailed
logs belong in topic references. Completion requires a reproducible acceptance
run, not a count of parsed records or implemented functions.
