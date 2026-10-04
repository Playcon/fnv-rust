# Opening foundations: shared handoff

2026-10-03. Current owner: Codex; opening camera, attention, HUD and activation fixes.
Use with MILESTONES.md.

## Current reported defects (thirteenth batch published; M1 still open)

The user reports a brief mouse-input leak during lying-to-sitting that leaves
heading changed, Doc looking away, incorrect interaction UI/timing, and an
unusable vigor tester. Do not treat the eleventh batch as acceptance.

- Input: the initial stage runs after mouse input in the Update chain. Input
  now waits for the player to be ready and the pending start stage to run,
  then obeys the traced package lock. Regression holds mouse motion across
  600 frames and package replacements, and checks release. This establishes
  the startup gap; reproducing the user's exact transition is still needed.
- Activation: VCG01VigorTester (00104C0A), reference00104C13, has zero OBND
  despite its cabinet geometry. Exporting actual placed model bounds fixes
  the missed target. This is a bounds-based picking correction, not yet a
  complete port of the native physics picking path. The authored activation
  still requires stage60 and objective30 displayed, then advances to65.
- HUD: native00771700 selects movement-disabled mode12 mask0x1514, hiding
  Info along with HP/AP, compass and reticle. Normal rollover suppression
  clears bit0x200. Info uses the existing XML justify_center_hotrect child,
  its native `_x` field and the native target-text template; the old Bevy
  interaction text overlay is removed. Basic actions and ownership colors are
  wired; advanced item rows, furniture/terminal variants and controller glyphs
  remain incomplete.
- Doc: the intro uses SayTo Player, not a dialogue package. The viewer lacks
  native head/eye tracking over the chair idle. 2026-10-04: the rotation
  chain is traced (Havok look-at solver `00ce0290`, easing `00c755e0`,
  target rules `00c78160`) and implemented as `world::look_ik`, not yet
  called. It corrects two earlier readings: `+0x1a4` means behind (model −Y),
  and `011b05a8` caps target distance from the head, not per-update motion.
  Set-up values and LookIK setting defaults remain untraced, so no look-at is
  applied. See [OPENING_LOOK_IK.md](OPENING_LOOK_IK.md).

Checks: 910 core tests and 82 viewer tests pass; the corrected real-XML Info
layout additionally passed all 126 UI tests. Both workspaces passed clippy,
format checks and release builds. Published to nv-rs-play/app, SHA256
`D2860A56D7D43BBDC19902A3CC77950E0340FB3CB21CDFBD911F9AE6D499E136`.

Live verification used an isolated copy of the user's stage55 save, with only
the copied player's position moved to the tester. Entering its trigger ran
stage60, Doc spoke INFO001074A2, its result enabled objective30, and pressing
E opened the original SPECIAL scene/menu. The Info prompt hid during speech
and returned afterward. Log: nv-re/work/codex-m1/interaction-layout2.log.
This verifies that segment, not the full opening or exact camera transition.
Desktop automation recovered after selecting a fresh test window.

An initial same-cell reload attempt retained the old trigger occupancy and
did not emit OnTriggerEnter. Restarting outside the trigger before loading
established the live route above. Follow-up now clears CellScripts when
replacing the saved GameState, including occupancy and the old seat. Its
regression enters the volume before stage55, loads stage55 in the same cell,
and verifies stage60 runs. All 83 viewer tests, clippy, formatting and release
build pass. Live same-cell F9 at the tester now runs Doc's instruction and E
opens SPECIAL (interaction-reload.log). Thirteenth batch installed in play/app,
matching the built viewer SHA256
`30A5413AD0D737B45E7E3ACF118B764BA897EE091972F88D48DB734B4B92ED07`.
Raw records, assembly, copied saves
and decompilation remain outside the repository in nv-re/work/codex-m1 and
nv-re/decomp/codex-m1.

## Current priority: opening camera and assistance behavior

User clarification: the clips play. The defects are player camera input
during lying/sit-up, unnatural looking while seated, and Doc not assisting
the player getting up. Start with Ghidra's FNV decompilation. Earlier
track/request logs and stage55 progression did not establish correct behavior.
Scope confirmed by user: opening sequence only, not later bed use.
The input gate and Doc's skipped transition are now corrected; exact
choreography remains an M1 verification item before calling the opening done.
The original Reflectron sex page was inspected after the user opened it;
the reference is recorded in FACE_CREATION.md.

Ghidra-first follow-up (same executable build/hash used below):
- `005c95a0` (EVP) calls `008a6ce0(0,0)`, setting the actor's deferred
  evaluation flag +0x145 and nudging its package timer. `008da670` explicitly
  permits reevaluation in settled sit states 0/4/9. Our furniture early return
  skipped that request. At stage45 the authored EVP can select Doc's bedside
  standing package (GetStage >=40) and `NVchairStandDoc` (stage10..50,
  marker14). Waiting until stage55 ResetAI skips that eligible chair exit.
- Camera trace: `009445b0` reads mouse axes and rejects looking when control
  mask2 is set (`005a03f0`, interface+0x680), or `0093a740` player flags
  +0x798..79b are active. `00931d90` applies the seated downward pitch limit;
  `00931b60`/`00953f20` distinguish body heading from seated look offset.
  `00952290` writes pitch to Bip01 Looking and heading to the first-person
  root; `0094ae40` reads Camera1st and applies residual pitch. The VCG01
  stage0 script explicitly enables looking. The missing independent gate is
  now traced: `005cc4f0` AddScriptPackage calls `005cc7a0(1)` for the player
  in the supported package path, setting +0x79b; `005cc7c0` RemoveScriptPackage
  calls that setter with0. The earlier instruction scan incorrectly limited
  its address range to actor code, missing the script-command setter.
  This blocks mouse looking throughout the player script package, including
  bedsit, irrespective of EnablePlayerControls. No clip-name rule is needed.
  Raw decompilation and assembly remain in nv-re/decomp/codex-m1, including
  the newly recovered player update at `0093e860` and camera-follow.asm.

Original replay observations, 2026-10-03: read-only process sampling captured
player+0x79b=1, other three look-block flags=0, constant pitch0/headingpi
and changing Camera1st transforms during the seated opening. After stage55,
all four flags were0 and the player angles changed. Evidence outside repo:
codex-m1/original-opening-memory.jsonl and original-after-opening-memory.jsonl;
reader observe-opening.ps1 only uses OpenProcess(PROCESS_VM_READ).
Snapshots after character creation showed Doc standing immediately in front
of the player, followed by normal movement instructions. They do not resolve
the precise hand-contact timing. Physical Escape stopped the first capture;
the user explicitly resumed. No original game memory was written.

Eleventh batch implementation: GameState::player_looking_blocked combines the
saved player package with the separate looking-control mask; viewer
look_around obeys it in walking mode. Generated tests exercise script command
replacement/removal, invalid/NPC targets, independent control mask, save/load,
and the actual Bevy mouse-input system including release and free camera.
907 core tests/doc tests and78 viewer tests pass, both clippy and formatting
checks pass, both release builds pass (package-look-* logs). The release
new-game GPU smoke capture reaches bedsit and Doc's bearings line, then exits
normally: codex-m1/package-look-opening.png and accompanying logs. Input
suppression/release is checked by the actual Bevy system regression; this
capture alone does not test mouse motion or the full opening route.
Eleventh batch installed in nv-rs-play; project/play SHA256 match:
223C24345C8412D24B5F606120AB252437F86D47CC37F20F6E15275AACEA4636.
No test viewer, build, memory sampler or agent remains running. Next: precise
Doc/player assist timing and the unfinished face editor; do not retrace the
now-established package look gate. Original game remains open for the user.

Tenth batch: viewer/src/ai.rs now consumes queued package evaluation before
the settled-furniture early return. Both request sources are consumed once;
entry/exit states retain their request until settled. Existing package
selection and stand-up handling choose the animation; no Doc-specific rule.
Two generated regressions cover the real package switch and deferred entry/
exit request. Viewer77 tests, clippy, formatting and release build pass.
Core unchanged. Logs: codex-m1/doc-evp-{tests,clippy,build}.log.

Live new-game run (base-only plugins, research viewer40672/window920990):
accepted Courier with Return. Stage45 EVP now starts the original
SpecialIdle_NVDocChairFrontExit.kf at77.7s; it releases furniture at89.7s,
before stage55 ResetAI at89.9s. Observed Doc leaving his chair. This proves
the previously skipped transition; precise assistance timing/placement has
not been compared with the original. Face creation still auto-accepts, so
this is not full-route acceptance. Logs: codex-m1/doc-evp-opening{,-error}.log.
The authored exit contains NPCDocPlyStand at7.067s (doc-exit-kf.txt).

Historical tenth-batch publication (superseded above):
BED7C70C0BA305878F5DA645FD0848CE9D52F1ED0A9DCEC0AC06F1B65C89533B.
That tenth build does not include the new package look gate described above.
Research viewer40672 has been stopped; no build or agent remains running.
Original game window724636 has now passed character creation and reached
normal movement. The user's replay supplied the input-gate evidence above.
Sky input still does not drive the original game's DirectInput. Earlier research
viewer43928/window722718 is stopped; its animation-repro logs are historical.
Read-only audit identified a possible furniture endpoint overshoot between
Sitter's clock and ActorRig::drive. Code review (2026-10-03, Linux session
without game data) rules it out: Sitter::play clamps elapsed to the KF's
stop-start, and drive() advances section SPECIAL_IDLE by dt then sync_time
overwrites it with that clamped elapsed each frame, so the clocks cannot
diverge or pass the clip end. Not a live comparison; no code changed.

## Current handoff: original tester compared; composition fixed

User opened the original tester. Observation: Strength7, remaining00;
original cabinet framing corresponds to our scene but room remains blurred
behind it and bulbs were brighter. Fixed our composition: explicit REPLACE
and transparent output clear on the 3D menu camera; transparent intermediate
clear on the HUD camera. Bevy's implicit blend choice depended on camera query
order and could blend transparent scene output over opaque black. The fixed
GPU capture shows the room behind the menu and stronger glows without tuning
materials. Evidence: VIGOR.md and codex-m1/vigor-background-fixed.png.

Viewer75 tests, clippy, formatting and final release build pass.
Ninth batch installed in nv-rs-play; project/play SHA256 verified equal:
6AFE4F581E5FA8B29144CAB5B6111BA00F2CC75C6788CEC1D502F756C15A2626.
Core unchanged (906 tests, clippy/format/release passed in previous batch).
Only runtime file changed: viewer/src/game_menus/vigor.rs. Logs:
codex-m1/vigor-background-{tests,clippy,build,capture}.log.
Background blur and exact colour/bloom match remain open; this is not a full
visual acceptance. User was asked to open ShowRaceMenu next for reference.
Original game is now process40676 (window724636); earlier handles are stale.
That publication completed with no test viewers, builds or agents remaining.
The subsequent animation investigation above supersedes its next action.

## Previous foundation batch

Current batch adds `world::chargen::appearance` (and its module export):
traced playable/race/sex filters for race, hair and eye choices, with remapped
plugin IDs and deleted-winning-record exclusion. Six generated regression
tests pass. Official-data observations:4 races;18 male/23 female hair and4
eyes per race/sex. Evidence and missing face-editor pieces: FACE_CREATION.md.
All checks complete:906 core tests/doc tests,74 viewer tests; both workspaces'
clippy, format and release pass (appearance-*-tests/clippy/build.log).
Eighth batch installed in nv-rs-play; project/play SHA256 verified equal:
FEFB03580647831CD9E6156EFA2369762599A2A520138AA3204673DD7FA1182E.
WHATS-NEW distinguishes the internal choice reader from the unfinished editor.

Original-game automated input did not work, even after refocusing. The user
subsequently opened the tester manually (current evidence above).
Research viewer42320 was stopped for the build; its stage55 quicksave remains
in nv-re/work/codex-m1. Face auto-accept is still an M1 blocker.

Seventh batch implements the original Vit-o-matic scene through the game's
XML: animated pages, live number/button/bulb appearances, keyboard controls
and original-triangle mouse picking. The old SPECIAL text substitute is
removed. Detailed executable/asset evidence and remaining limits are in
[VIGOR.md](VIGOR.md). MeshData preserves exact NIF shape names separately
from diagnostic labels. Controller input and embedded-light replacement
models remain unsupported.

Checks: 900 core tests/doc tests, 74 viewer tests, both clippy and formatting
clean; both release builds passed. Installed in nv-rs-play at 05:33;
project/play SHA256 match:
7403F13C84093DC74B244203A550C19E4330A766E561ABEF6B7345C4AC89F9BB.
WHATS-NEW.txt records implemented and unverified scope. Logs:
`%USERPROFILE%\nv-re\work\codex-m1\vigor-*-tests/clippy/build.log`.
The viewer regression changes live values on each click, enforces the budget
and limits, rejects early Done, and closes at the exact budget.

Real-model harness: `nv-re/work/codex-m1/vigor-observations`; log
`vigor-scene-check.log`. It loads 274 pieces, all 16 page sequences and all
replacement textures. Rays hit main/index/LookInside buttons in animated
poses. Built-in GPU screenshot mode produced the inspected Strength page
`codex-m1/vigor-page1-clean.png` (base-game-only plugin list avoids DLC notices).
It shows the real cabinet, page, remaining digits and five lit bulbs.
This is implementation/render evidence, not original-game comparison.

Live PC mouse/keyboard validation now passes (VIGOR.md records the sequence).
The tester updated values, played its page transition, rejected early Done,
and closed at40/restored the room. This was an isolated script-opened check,
not the quest route. Process42320 then loaded the existing stage55 quicksave,
and was later stopped for the build (current process status above).
Do not mark M1 accepted; the face menu is still auto-accepted,
movie playback and mid-animation save restoration remain incomplete.

Changed files: cellview/src/{vigor,lib,lockpick}.rs, cellview/tests/load.rs,
ui/src/menus/{vigor,mod}.rs, viewer/src/game_menus/{vigor,mod}.rs,
viewer/src/{main,lockpick,menus}.rs, viewer/Cargo.{toml,lock} and tracking docs.
The internal testdata dev-dependency creates menu test fixtures from scratch.
No original assets or decompilations are stored in the project.

Previous six batches: package action ordering, physical player/camera
separation, actual opening camera tracks, deferred ResetAI furniture release,
NPC scripted loaded-idle ownership and forced held-pose replacement, then the
same player replacement after its request gate. The isolated opening reached
stage55; F5/F9 and cold restart there preserved player/Doc positions, headings
and quest stage. Last published batch6 hash (before this publication):
02E484E377CE6FCC87DCD21F471ACC37537E2FA61C4D8CD8D92F459414924F0A.
Detailed earlier evidence follows.

**Fifth batch checked and published:** NPC script PlayIdle now
queues one pending IDLE per actor, as process +0x350/flag0x80 does. The
viewer consumes requests for loaded actors and unconditional leaf group-7
IDLE records, using their actual KF and record loop count. Other groups,
trees/conditions and unloaded actors are diagnosed, not substituted.
The script owns the animation clock through completion; the free-idle and
furniture overlay clocks cannot replace it. ResetAI clears that ownership.
Active animation persistence remains unsupported.

Replacement evidence: 00498290 checks pending +0x128 and calls
00498910(0,1); that clears current +0x124's section slot when its sequence
matches, then 00498670 removes the sequence through 00a2ec50. It promotes
the pending request before playback. Consequently 004949a0 blends from
the actor's current pose using the incoming blend-in, without the freed
sequence's blend-out. The read-only agent's suggested ordinary cross-fade
was rejected after inspecting the slot writes. Core regression covers
interruption during TransDest, pose continuity, blend duration and repeated
requests. Viewer regression covers clock ownership and completion.
Full checks passed: 888 core tests/doc tests and 73 viewer tests; both
clippy, format and release builds passed. Logs: codex-m1/batch5-*.
Live opening log confirms stage30 starts VCG01DocGiveMirror, the normal
seated idle resumes, and stage55 ResetAI takes Doc to the tester. Report006
captures that checkpoint. This was not an original-game visual comparison;
the existing missing face-menu auto-accept still prevents acceptance.
Published to nv-rs-play/app, SHA-256
27CDEAA002A9B90C709202E5A5E30BEDBA5C4917A8FBC5C8D921527CFD7A9750.
Test viewer and all agents from that batch are closed/finished.
Changed: world/animation.rs; viewer/actors.rs, player_idle.rs, scripts.rs,
sitting.rs, ai.rs and main.rs.

**Fourth batch, checked and published:** ResetAI is now a separate deferred
actor request in world/scripting.rs. The viewer consumes it before furniture
can return early, clears the old procedure/path, and releases furniture
without relocating the actor. Trace: 005c9530 -> 008a6ce0(0,1), then the full
reset path -> 0088d640 -> HighProcess +0x84 / 009287e0. The represented scope
is furniture/procedure restart, not every original combat/process reset.
Two core regressions and a viewer path/position regression pass. Full checks:
887 core tests/doc tests, 72 viewer tests, both clippy/format/release builds.
Installed in nv-rs-play/app, SHA-256 matches the built executable:
CA730B10F3BAE7DEAF43A247B14F30850F57FA240B94E4F9855A4664C3BBC569.
Also fixed non-forced special-idle release to preserve the same startup
EaseIn/TransDest refusal as 00498910; its regression passes.

Live evidence: batch4-opening.log/error in codex-m1. At VCG01 stage55,
ResetAI released Doc and he followed VCG01DocMitchellTravelToPlayerAtTester
for 577 units to the machine. Reports 003 and 004 bracket an F5/F9 test:
player position/heading, Doc position/heading and stage55 match exactly;
neither report keeps Doc in furniture. A separate cold viewer launch then
loaded that save: report 005 matches the same values (batch4-restart.log).
This checks that checkpoint through process restart, not active animation
persistence or the complete creation route. Test viewer is closed.
No build/subagent is running. Source edits this batch: world/scripting.rs,
world/animation.rs, world/tests/script_reset_ai.rs, viewer/ai.rs,
viewer/player_idle.rs, README and these shared tracking docs.

NPC PlayIdle trace confirmed the retail stage-30 mirror request in the live
log. 005cb2d0 stores IDLE at process +0x350 via 008d9410 and sets flag 0x80
via 00903180. Consumer found in 008ba600 (existing furniture/ decomp): it
reads/clears flags at entry, and flag0x80 gets +0x718 pending IDLE then calls
00497f20 with mode3, bypassing the immediate-request busy gate. Flag0x10
instead calls +0x70c. 00498290 frees the prior loaded idle binding with
00498910(0,1), then plays the resolved group. Forced replacement/blending
is now represented by the fifth batch above, awaiting its live checks.
The read-only audit agent has finished. Parent owns Ghidra now.

The viewer now dispatches unconditional, idle-only player package actions
through the actual first-person skeleton and KF files. Camera1st sets the
view while the walking capsule and input heading stay independent. F5,
scripted saves and F12 use physical position/heading. Loading clears transient
camera playback; restoring its clock/sequence from a save is still missing.

Rules: `world::animation::Player::request_special_idle` preserves the
EaseIn/TransDest gate and duplicate rejection. `008dab40` clears the player's
pending request after the immediate `008dae00` call even when busy; this is
not the NPC deferred path. `animation::camera` holds camera placement and
hand-follow math, `viewer/player_idle.rs` loads assets and presents the view.
It runs after walking and before aiming/interactions; script requests enter
the next frame. No package IDs, timed camera stand-ins or fitted offsets.

Camera provenance (same executable hash below): `00888b50` / `00889812`
place the first-person root at actor feet; `00952290` applies root heading
and pitch around Bip01 Looking. `004a0c90`, `00524ac0`, `0043f8d0` establish
matrix order/signs. `0094ae40` reads Camera1st world position/rotation and
right-multiplies residual pitch. Hand-follow defaults are 0.85, chase 2 s
and attack chase 0.05 s (`00f5b5e0`, `00f5b610`, `00f5b640`). The chase
advances dt/chase of the remaining difference, clamped to the target.

The `00933840` override requires UI state and process byte +0x364, whose
meaning is unresolved; it retains ordinary input rotation. Do not equate
it with all dialogue. The viewer leaves interactive-dialogue, weapon-out
and V.A.T.S. camera combinations out; opening line-only SayTo is supported.
IDLE conditions/tree dispatch, action scripts/topics, animation text-key
sounds and in-progress save restoration are also not implemented here.

Live evidence: `codex-m1/batch3-opening.log` and error log; report 002.
All four expected camera idles started; accepted four DLC notices and
Courier name, then reached vigor-tester instruction. The existing face menu
auto-accept still occurred, so this is NOT faithful opening acceptance.
This development run preceded the final body-heading/load-reset/scheduling
fixes. Final release smoke exited successfully (`batch3-final-smoke.png` and
log). Core tests/doc tests: 885 passed; viewer: 71 passed. Both workspaces'
clippy, formatting and release builds passed (`codex-m1/batch3-*`). Installed
the checked release in nv-rs-play/app; executable hashes match. No test
viewer from that batch remains running. Later checks supersede these counts;
all agents from that batch have finished.

Changed: world/animation.rs, new world/animation/camera.rs; viewer/main.rs,
new viewer/player_idle.rs, viewer/scripts.rs and viewer/report.rs. Earlier
batch notes below are historical; their “not dispatched” claims are superseded
by this bounded player-camera implementation.

## Observed baseline

Launched the release viewer with the installed Data folder and
`--official --new-game`, working directory
`%USERPROFILE%\nv-re\work\codex-m1` so tests did not overwrite play saves.
Accepted Classic, Mercenary, Caravan and Tribal Pack notices, then the default
name Courier. Doc's spoken/result-script chain reached VCG01 stage 55 and
asked the player to use the vigor tester. No console stage jumps were used.
The existing face-menu path auto-accepted the missing menu; this prevents
counting the run as a faithful character-creation test.

The view remained upright, facing away from Doc while the player should be
in the scripted bed sequence. `AddScriptPackage` only records a package and
requests evaluation; the player's capsule/camera is outside NPC Walker AI.
NPC `PlayIdle` events are also logged and discarded by viewer/scripts.rs.
SPECIAL still has a text-panel substitute in viewer/menus.rs. These are
foundation blockers, not cosmetic leftovers.

The opening quest's installed stage-0 script requests FNVIntro.bik and jumps
to stage 90. Many earlier graveyard stages remain in the file but are skipped
by that retail path. Do not recreate unused stages merely because their
assets exist. Bink playback is currently log-only. The old description that
`--new-game` starts after the movie was inaccurate.

Local baseline evidence (not for redistribution): `opening.log`,
`opening-error.log`, `reports/001/` (picture, position and complete state),
`vcg01-source.txt`, `vcg00-script.txt`, `vcg01-script.txt`, package/idle
dumps, all under `%USERPROFILE%\nv-re\work\codex-m1`.
The baseline viewer was closed after recording. The user's play saves were
not loaded or overwritten. No original-game screenshot comparison yet.

## Implemented this batch

`world::ai::actions` reads three independent package actions:
POBA (begin), POEA (end), POCA (change). Each owns an INAM idle, embedded
script and TNAM topic. TNAM ends the action. It preserves compiled code,
source, header and local-variable fields. SCRO form IDs follow load order;
SCRV variable indices and compiled bytes stay unchanged; nulls stay null.
Repeated markers replace the prior action, as the original loader does.

`Package::load` exposes these actions; the synthetic trespass package has
empty actions. `nvinspect <Data> ai <PACK>` prints the actions, linked
animation models and loop counts. `ai <REF>` also prints the selected
package's actions. It explicitly reports that action playback is pending.
Four synthetic tests cover separation/boundaries, compiled-only scripts,
load-order remapping, absent/truncated data and repeated action markers.

This is a data-reading foundation. It does not dispatch callbacks, play
animations or repair the observed camera behavior yet.

## Evidence for the reader

Executable: local FalloutNV_unpacked.exe SHA-256
`19406942E48724D797300C4EA6BE9AC69A32F670B8C35DB09279A2258422739F`.
Ghidra: package loader 00673a40 dispatches all three markers to 0067dd20.
0067dd20 clears the action, reads INAM into its idle field, reads the embedded
script fields, then reads TNAM and returns. The existing decompilation is
under `nv-re\decomp\ai\n`; this batch's focused decompilation of 0067dd20
and PlayIdle handler 005cb2d0 is under `nv-re\decomp\codex-m1`.
These addresses prove parsing, not the runtime callback sequence.

| Package | Begin idle | Change idle | Animation loops |
| --- | --- | --- | --- |
| VCG01PlayerSection0 (001055BE) | LooseVCG01PlayerWakeup (001055BF) | None | Wakeup FF/FF |
| VCG01PlayerSection1 (001055C3) | LooseVCG01PlayerSitup (001055C0) | LooseVCG01PlayerBedsit (001055C1) | Situp 00/00, bedsit FF/FF |
| VCG01PlayerSection2 (001055C4) | LooseVCG01PlayerStandup (001055C2) | None | Standup 00/00 |

All three end actions are empty. The linked models are
`Characters\_Male\IdleAnims\SpecialIdle_NVCG01PlayerWakeup.kf`,
`SpecialIdle_NVCG01PlayerSitUp.kf`, `SpecialIdle_NVCG01PlayerBedsit.kf`,
and `SpecialIdle_NVCG01PlayerStandup.kf`. IDLE DATA group byte 0x47:
special-idle group 7 plus loose-idle flag 0x40. Use animation::Player, which
honors StartLoop/EndLoop and transitions, not the simpler IdleClock. The
normal IDLA list is not a replacement for these action fields.

## Next bounded task

### Runtime trace and second foundations batch (2026-10-03)

`AddScriptPackage` (005cc4f0) invokes HighProcess +0x59c (00903bf0)
on the old package, then +0x598 (00903a80) on the new one, before installing
it. This happens even when the package IDs match. Disassembly confirms the
old action is +0x6c (change) and the new action +0x4c (begin); within each,
the script precedes the idle, which precedes the topic. Ghidra incorrectly
labels some wrappers as MFC methods; use their instructions/offsets.

The script runner now emits ordered `PackageAction` requests, including
same-package replacement, and rejects invalid package/actor forms. Three
synthetic regressions pass. Requests do **not** execute action scripts,
topics or idles yet; viewer/inspector diagnostics explicitly say pending.
Removal and ordinary AI package lifecycle dispatch remain unimplemented.

Player positions used by walking-mode scripts, F5 and F12 now come from
the collision capsule rather than subtracting a height from the camera.
Free-camera mode retains its prior convention. Two new viewer regressions
cover displaced cameras and the free-camera behavior. Report/save heading
still comes from the view; separate body heading before animated rotation.

All four opening KFs contain Camera1st tracks (priority 95), and sample
through `meshes\\characters\\_1stperson\\skeleton.nif`. Durations: wakeup
6.000 s, situp 5.400 s, bedsit 8.167 s, standup 11.133 s. Their KF cycle
type is once; IDLE loop counts determine repetition. Camera3rd on the male
skeleton is static under Scene Root, so sampling that node cannot supply
these motions. Raw samples/harness: `nv-re\\work\\codex-m1\\kf-observations`.

Camera trace: 0094e1d0 finds Camera1st into global 011e07d0. The first-person
branch in 0094ae40 reads its world rotation (00461130, node+0x68) and world
position (0045bb80, node+0x8c). It combines rotation with the negative
011e0770 angle except in its dialogue guard (00933840). Skeleton placement,
rotation basis and special-idle replacement/completion still need tracing.
008dab40 validates a requested idle and 008dae00 hands it to 00497f20;
00498f80 guards loading/transition states. Do not flatten two same-frame
requests into a guessed last-wins animation rule.

Second-batch changed files: world/scripting.rs, world/tests/script_package_actions.rs,
nvinspect/play_cmd.rs, viewer/scripts.rs, viewer/walk.rs, viewer/report.rs.
Core tests/doc-tests: 878 passed; viewer tests: 70 passed. Both workspaces'
clippy, format and release builds passed (viewer final logs after annotating
the Bevy system's resource argument count). A walking-mode house screenshot
run exited successfully; `batch2-smoke.png` and log are in `codex-m1`.
Published to `nv-rs-play/app/nv-viewer.exe`, with matching SHA-256; release
notes explain playback is still pending. Logs are `codex-m1/batch2-*`.
Parent continues Ghidra tracing; no viewer/build is running. Bounded agents
have finished and own no ongoing work.

Trace the runtime use of the package's action structures, including same-
package reassignment (the VCG01 source explicitly uses this for bedsit).
Determine action ordering, idle completion/loop handling, and how the
player's animation drives its camera node in the original executable.
Read actual KF tracks and skeleton transforms. Add regression tests around
the resulting state transitions before connecting the viewer. Do not add
package-ID-specific quest logic, arbitrary camera offsets or guessed timing.

Then replay the opening against the original game and continue to menus,
movement, exit, and save/reload. Bink and the missing menus remain required
for M1; passing tests for the package reader does not complete that gate.

## Changed files and checks

Code: `crates/world/src/ai.rs`, new `ai/actions.rs`,
`crates/world/src/living/trespass.rs`, new
`crates/world/tests/package_actions.rs`, `crates/nvinspect/src/play_cmd.rs`,
`crates/nvinspect/src/main.rs`, `crates/nvinspect/src/coverage_table.rs`,
and `viewer/src/args.rs` (help text only).
Documentation: AGENTS.md, README.md, MILESTONES.md and this file. Removed the
redundant docs/SESSION_INSTRUCTIONS.md copy; AGENTS.md is the single shared
instruction source and CLAUDE.md already points there. Archived notes remain.

Core workspace tests/doc-tests passed: 875 total, including four new reader
regressions. All 68 viewer tests passed; clippy for all targets and formatting
checks passed in both workspaces. Build/test logs are under the same evidence
directory. Real-data inspector outputs
are `inspected-VCG01PlayerSection0.txt` through Section2. The source hash
manifest is `source-sha256.txt` there; there is no Git commit for this batch.

Both final release builds passed. Published the checked viewer into
`nv-rs-play\app\nv-viewer.exe` and verified its SHA-256 matches the project
build. Publication used `& %USERPROFILE%\nv-re\work\publish-play.ps1` in
the current PowerShell: the older `powershell -File` invocation refused to
load scripts. No execution-policy setting was changed. No viewer, build or
Ghidra process from this batch remains running.
