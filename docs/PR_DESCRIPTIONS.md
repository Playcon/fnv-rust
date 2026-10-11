# Pull request descriptions (Dead Money, October 2026)

Drafts for pull requests against `slaterain/nv-rs` `main`, one per branch on
the fork `Playcon/nv-rs`. None is opened yet. Every branch is based on
`main` at `79f1038` and depends on no other branch, except where a
description says so. "Copied from the original" means translated or read
from `FalloutNV.exe` 1.4.0.525 (addresses in the code comments) or from the
game's own data and scripts; [G] marks a guess, [C] what needs the original
game.

**AI assistance (all branches).** The code, tests and text were written by
Claude (Claude Code, Sonnet 5.5) working with the contributor, who set the
goals, ran the viewer, read the results and answers for them. The contributor
runs the acceptance routes before opening each pull request; the checks below
were run by the agent in a local checkout (a few existing tests fail on
`main` itself: `a_squares_grass_becomes_one_mesh_per_grass` and two water
tests in `cellview`; no branch adds a failure).

Review order: the engine changes first (1 to 5), then the Dead Money content
(6 to 9).

## 1. `pr/travel-here-done`: a travel near the current location is done at once

**What it does.** A travel package whose location is "near the current
location" (location type 2) now has its own person as its destination, so
they are already there and the package reaches DONE; its end action runs.
Dead Money's Christine leaves her Auto-Doc through such a package whose end
action is `StartConversation Player`; before, the end action never fired and
the scene stalled.
**Copied from the original.** The location reference resolver `0067f2a0`
returns the actor for type 2; the end action at DONE is already in `main`
(`0091ecf0`). The new rule only applies to Travel packages (other kinds read
the place more than once).
**Guessed [G].** That the travel procedure with that destination reaches DONE
in the same frame; not read, inferred from the resolver.
**Needs the original [C].** The timing of Christine's first words after
the Auto-Doc (about 1.4 s here).
**Tests.** `a_travel_near_the_current_location_is_already_there` (world),
a testdata package `TO_HERE`. Live: the clinic scene to MQ01c stage 40.
**Base.** `main`. **Dependency.** None (the scene also needs 2).

## 2. `pr/animated-collision`: an object a script animates moves its keyframed collision

**What it does.** A placed object (not a door) whose model has sequences owns
its keyframed collision parts like a door's leaf, and while a script's
`PlayGroup` sequence plays the viewer moves them with it, holding the last
pose when a one-shot ends. Dead Money's Auto-Doc pod opens its door this way;
before, the door collision stayed shut and the person inside was stuck.
**Copied from the original.** Doors' leaves move with their sequence
(`0047ab40`, in `main`); the model's collision part and its node chain are
the game's data (layer ANIMSTATIC, "animated on" the door node).
**Guessed [G].** That the game moves a script-animated object's collision
along with the sequence frame by frame; `main` follows doors only at the end
of the sequence (their own INI setting). Not traced for other objects.
**Needs the original [C].** An Auto-Doc pod: can a person walk out through
its door while it opens?
**Tests.** `an_object_a_script_animates_owns_its_keyframed_collision`
(`preview`). Live: Christine walks out of the pod and talks.
**Base.** `main`. **Dependency.** None.

## 3. `pr/teammate-follow`: remove the guessed follow fallback

**What it does.** Removes the rule (behind `NV_GUESSES=1`) that gave a
teammate with no package an invented follow package at 200 units. Companions
follow and wait by their own packages (Dead Money's
`...FollowPlayerDEFAULT` and the guard package `...WAIT`, true while
`Waiting` is 1, as Boone's are); verified in the viewer with the guesses off,
Dog follows by his own package.
**Copied from the original.** None new; the follower rules traced for
`COMPANIONS.md` stay.
**Guessed [G].** None left in this area.
**Needs the original [C].** Test: tell Dog to wait, walk away, does he stay
(a guard package decides).
**Tests.** `a_teammate_follows_by_its_own_package_and_gets_none_invented`;
the guessed rule's two tests removed.
**Base.** `main`. **Dependency.** None. **Overlap.** Companions (Chazm's
area): please coordinate.

## 4. `pr/isanimplaying-people`: `IsAnimPlaying` on a person with no 3D loaded is 0

**What it does.** `005c14a0` takes a person's animation data from the actor's
`+0x1e4` slot; with none loaded the handler's other path finds no model
either and the result stays 0. Before, such a person made the script stop.
A loaded person's eight sequence slots (`00491040`) are not carried out;
that case stays behind `NV_GUESSES=1`.
**Copied from the original.** The 0 for no loaded 3D (`005c14a0`).
**Guessed [G].** The loaded case (1 standing, 0 down) remains a guess.
**Needs the original [C].** In the console, `prid` a person and type
`IsAnimPlaying` while they stand, walk, idle, are knocked down and dead;
write down the numbers.
**Tests.** The existing `objects_animations_playing` extended; the guesses
test reports the person loaded.
**Base.** `main`. **Dependency.** None.

## 5. `pr/dense-dead-money-worlds`: Dead Money's towns load one square each way

**What it does.** Adds the Dead Money towns to the worlds that load one
square on every side (`DENSE_WORLDS`), as the Residential District already
does. On a laptop GPU (integrated graphics) the viewer lost the graphics
device in 2 of 3 runs of the Villa and the substation and in 3 of 3 at the
bell tower with the usual radius; with one square the bell tower crashed in 1
of 3.
**Copied from the original.** Nothing: an accommodation for the graphics
memory, not the game's behaviour (the code comment says so).
**Needs the original [C].** None.
**Tests.** `dead_moneys_towns_are_dense_and_the_base_game_is_not`.
**Base.** `main`. **Dependency.** None.

## 6. `pr/dean-escort`: Dean's escort (MQ02b) played

Dead Money content: `characters/dead-money-act2-dean.txt`, notes in
`docs/DEAD_MONEY.md`, the matrix row and a milestone line. Played in the
viewer on `main` with no code change: Dean's rooftop trigger, his talk and
replies (`--say`), stage 30, both hologram terminals' items (as script lines;
the holograms killed a ghost), stage 40, and the ending's result lines to
completion. Not played by hand: the last replies (his second conversation
needs his Act 1 chair variables) and the terminal screens.
**Guessed [G].** None. **Needs the original [C].** How the escort ends by
hand. **Base.** `main`. **Dependency.** None. **Overlap.** Dean as a
companion (Chazm).

## 7. `pr/christine-escort`: Christine's escort (MQ02c) played

`characters/dead-money-act2-christine.txt`, notes, matrix, milestone line.
Played: the switching station interior, Christine's trigger and mute talk,
the electrical box repair (message buttons, Repair 60, XP, objective), the
elevator panel, and the ending's result lines to completion. Not played: the
toxic cloud and the terminals' screens (terminals are Chazm's), the ghost
gauntlet, going up with her. **[G]** none. **[C]** the later replies.
**Base.** `main`. **Dependency.** None.

## 8. `pr/gala-event`: the Gala event (MQ02) played from the bell tower

`characters/dead-money-gala.txt`, notes, matrix (MQ02 NOT PLAYED to PARTIAL,
the totals line updated), milestone line. Played: MQ02 stage 50's script, the
event's 33 second fireworks timer and the post-Gala enables, from script
lines. Not played: the climb, the panel's own talk (a talking activator with
no greeting line; `main`'s rule is "no line: nothing", so E on it opens
nothing; how the original offers its player topic is not traced), the ghost
gauntlet. **[G]** none. **[C]** what activating the panel opens in the
original. **Base.** `main`. **Dependency.** None.

## 9. `pr/casino-quests`: the casino quests' places and the kitchen leg of MQ03a

Notes, matrix and milestone line only. The kitchen, theater and suites load
and run their start scripts; the kitchen's three gas valves work with the
game's message buttons and Repair check; Dog's collar scene plays to his
death; one run ended in "Quest Failed" for MQ03 that is not explained.
Not played: the key, the replies, the theater and suites fights.
**[G]** none. **[C]** the kitchen with all three valves repaired quickly.
**Base.** `main`. **Dependency.** None.

## Route branches (October 2026, after the first nine)

Docs and test characters only (no engine code), each based on `main` and independent; they move
quests in `docs/DEAD_MONEY_COVERAGE.md` from NOT PLAYED to PARTIAL (the header counts on each branch
match its rows). `claude/dm-all-routes` on the fork merges them all (4 PLAYED, 52 PARTIAL,
0 NOT PLAYED, 1 NEVER STARTED). No stopgap code was needed for the route branches themselves
(the stopgaps and fixes found while playing them are in the next section). AI disclosure as above.

| Branch | Quests |
|---|---|
| `pr/suites-christine` | MQ03c Last Luxuries |
| `pr/theater-dean` | MQ03b Curtain Call at the Tampico |
| `pr/vault-ending` | MQ03, the ending, the bunker transition (MQ03d stays NEVER STARTED) |
| `pr/bark-timers` | Dog, Dean and Christine bark timers |
| `pr/fade-timers` | fade to credits, the Auto-Doc fade |
| `pr/enemy-test` | the developer's enemy test quest |
| `pr/radio-quests` | the eight radio quests (started; not re-run with the radio engine now in main) |
| `pr/elijah-barks` | Elijah's lobby intercom, the suites terminal effect, the Starlet counter |
| `pr/follower-fire` | dismissing the Mojave companions, Arcade's goodbye |
| `pr/toxic-quests` | the toxic cloud, global toxic damage (`SetHardcore 1` switches Hardcore on) |
| `pr/casino-support` | chip reward, comps (the games exist now; not played with the quest), hologram vendor, challenges |
| `pr/wt-support` | the West Town support quest |
| `pr/gala-fireworks` | the Gala fireworks quest |

(`pr/dean-escort`, `pr/christine-escort`, `pr/gala-event` and `pr/casino-quests` are described above.)

## Stopgaps and fixes found while playing the routes by hand (2026-10-11)

Each is on its own branch based on `main`, one thing each. Stopgaps are guesses [G] behind
`NV_GUESSES=1` (`crates/world/src/guesses.rs`); none is merged into `claude/dm-all-routes`.

| Branch | What | Kind |
|---|---|---|
| `pr/stopgap-talking-activator-actor` | A talking activator speaks as the actor a script gave it (`SetTalkingActivatorActor`), E on one talks, and a script's `Activate` of one that has an actor talks too: the vault's Elijah intercom (`NVDLC01VaultElijahTalkingActivatorScript`, started by the main terminal's script) and the Gala event control panel. Test: `contrib_guesses`. How the original starts the talk is not traced. | stopgap [G] |
| `pr/stopgap-walker-opens-sliding-door` | Someone stuck against a shut, unlocked door opens it: the navmesh only lists swing doors that are not sliding, and the vault's two entry doors are sliding (flag 0x10) and only ever closed by script, so Elijah stood at them. Test: `a_walker_stuck_at_a_shut_door_gets_that_door` (`world::doors::shut_door_near`). Seen working by hand (Elijah opens the vault entry doors). How the original opens a sliding door for a walker is not traced. | stopgap [G] |
| `pr/fix-terminal-prompt` | A terminal under the crosshair shows its "Access" prompt (the branch for it existed but terminals were not among the objects it accepts). | fix |
| `pr/fix-terminal-result-typing` | A terminal item's result line (rate 0) typed only its first letter: the step divided by the rate, which wraps to 0 characters in a release build. Test: `an_items_result_is_typed_in_full` (fails with an overflow before the fix). | fix |

**[G]** the two stopgaps. **[C]** how the original starts a talking activator's conversation and opens
a sliding door for a walker; what attacking Elijah (a ghost by script) does in the vault.
Known and not a Dead Money rule: the Intel Iris Xe Vulkan driver crashes when the Villa squares
load south of the Gala tower (`NV_LOAD_RADIUS=0` avoids it); the vault terminals show no monitor
frame (a rendered-terminal fade issue, not looked at here). AI disclosure as above.

## Pull requests for the stopgaps and fixes found while playing Dead Money by hand

Four small branches from the contributor's own playthrough of the Dead Money routes in the viewer
(2026-10-11). Each is based on `main` at the commit named in its row, depends on no other branch
(the two stopgaps touch the same doc line in `crates/world/src/guesses.rs`: the second to merge resolves
it by keeping both sentences), and was dry-merged against `main` at `a24bc31` without conflicts.
Compare links open the pull request form against `slaterain/nv-rs` `main`; none is opened.

Review order: 1 and 2 (plain fixes), then 3 and 4 (stopgaps behind `NV_GUESSES=1`).

### 1. `pr/fix-terminal-result-typing`: a terminal item's result line is typed in full

**What it does.** The terminal menu types an item's result (`RNAM`, "Disconnecting lock...") beside the
prompt at a rate of 0, meaning all at once. `Line::step` worked out the characters for a step as
`(now - next) / ms + 1`; with `ms` 0 that is infinity, `usize::MAX`, and the `+ 1` wraps to 0 in a release
build (and panics with an overflow in a debug one), so the line showed its first letter and never the rest.
A rate of 0 now types the whole line.
**Found by.** Opening the Sierra Madre vault's security terminal in the viewer and picking "Disengage Vault
Security Protocols": only "D" appeared. It also hid the result text of the Vera item.
**Copied from the original.** Nothing new: the rate, the queue and the typing are the existing translation
(`00759da0`, `006ffea0` in the code comments); only the arithmetic for a rate of 0 changes.
**Guessed [G].** That a rate of 0 types the line at once (the field's own comment says "0: at once").
**Needs the original [C].** The order of "show the result" and "go back a screen" for an item whose script says
`ForceTerminalBack` (the Vera item): not touched here.
**Tests.** `an_items_result_is_typed_in_full` (`ui`); before the change it fails with "attempt to add with
overflow".
**Base.** `main`. **Dependency.** None.
Compare: https://github.com/slaterain/nv-rs/compare/main...Playcon:nv-rs:pr/fix-terminal-result-typing?expand=1

### 2. `pr/fix-terminal-prompt`: a terminal under the crosshair shows its prompt

**What it does.** `object_in_view` (viewer) has a branch that builds the prompt "Access <name>" for a
terminal, but terminals were not among the objects it accepts, so a terminal with no `OnActivate` script
showed no prompt (E still opened it). One line adds terminals to the accepted objects.
**Found by.** The Dead Money vault and clinic terminals showed no prompt.
**Copied from the original.** The prompt text branch was already there.
**Guessed [G].** None. **Needs the original [C].** None.
**Tests.** None added: a viewer system; checked by hand in the viewer (the prompt shows; E opens the terminal).
**Base.** `main`. **Dependency.** None.
Compare: https://github.com/slaterain/nv-rs/compare/main...Playcon:nv-rs:pr/fix-terminal-prompt?expand=1

### 3. `pr/stopgap-talking-activator-actor`: a talking activator speaks as the actor a script gave it

**What it does.** Behind `NV_GUESSES=1`. (a) `SetTalkingActivatorActor` stores an actor for the activator's
base; a conversation with that activator now uses the actor's lines, voice and factions
(`world::dialogue::speaker_for`) while the activator stays where the sound and the view are. (b) E on a
talking activator talks, after its own `OnActivate` script let it through. (c) A script's `Activate` of a
talking activator that has an actor also talks: the Sierra Madre vault's main terminal runs
`VaultMainElijahIntercomREF.Activate Player 1`, whose `OnActivate` ends with an `Activate` that names no
activator, so the event carried none and nothing started the conversation. The vault's Elijah intercom
(about twenty topics) and the Gala event control panel need it.
**Copied from the original.** The setter (`005d4cc0` -> `004ff0e0`, which stores the actor in the base's +0x90)
is traced, and the scripts and lines are the game's own data.
**Guessed [G].** How the game uses that stored actor when a conversation starts, and what its `Activate` of a
talking activator does, are not traced (nothing here reads `FalloutNV.exe`'s activation of a talking activator),
so all three behaviours only run with `NV_GUESSES=1` and are named here.
**Needs the original [C].** In the original: activate the Gala panel (with `NVDLC01GalaEventQuest` running) and
the vault intercom and write down what opens.
**Tests.** `contrib_guesses` (the speaker before and after the script, a person's own, the switch off).
Seen by hand: the vault sequence (door terminal, main terminal's Vera item, Elijah's intercom conversation
through the compliance reply) and the Gala panel through stage 50.
**Base.** `main`. **Dependency.** None.
Compare: https://github.com/slaterain/nv-rs/compare/main...Playcon:nv-rs:pr/stopgap-talking-activator-actor?expand=1

### 4. `pr/stopgap-walker-opens-sliding-door`: someone stuck against a shut door opens it

**What it does.** Behind `NV_GUESSES=1`. When a walker's path is reported stuck and a shut, unlocked door lies
within a doorway's reach (160 units across and up or down), the door is opened for them
(`world::doors::shut_door_near`, `ai::open_door_in_the_way`). The game's path following opens the doors across a
path (`009e20c0`, `doors::walker_at_door`), but the navmesh only lists swing doors that are not sliding, and the
Sierra Madre vault's two entry doors are sliding (flag 0x10) and only ever closed by script
(`NVDLC01VaultCodeHolderScript`), so Elijah stood at them.
**Copied from the original.** Nothing: it is a guess.
**Guessed [G].** That the original opens a sliding door for a walker by the same activation; how it does so is
not traced.
**Needs the original [C].** Whether Elijah opens the vault's entry doors himself in the original.
**Tests.** `a_walker_stuck_at_a_shut_door_gets_that_door` (`world`). Seen by hand: Elijah walks through both
entry doors on his own.
**Base.** `main`. **Dependency.** None.
Compare: https://github.com/slaterain/nv-rs/compare/main...Playcon:nv-rs:pr/stopgap-walker-opens-sliding-door?expand=1


**Checks (run by the agent in a local checkout, each branch on its own base, 2026-10-11).**
`cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`,
`cargo build --release`, and from `viewer/` the same four. Every branch: 0 clippy warnings, fmt ok, the
repository hygiene test passes, both builds finish, and the only failing tests are the three that fail on
`main` itself (`a_squares_grass_becomes_one_mesh_per_grass`, `a_square_gets_its_own_water_and_its_placed_water`,
`water_at_the_distant_water_height_reflects_the_scene`). The viewer's 182 tests pass on 2, 3 and 4 (1 changes no
viewer code, so its viewer checks were not run).

| Branch | Workspace tests | Clippy | Fmt | Build |
|---|---|---|---|---|
| `pr/fix-terminal-result-typing` | 15,950 passed, 3 baseline failures | 0 | ok | ok |
| `pr/fix-terminal-prompt` | 15,949 passed, 3 baseline failures; viewer 182 passed | 0 (viewer 0) | ok | ok (viewer ok) |
| `pr/stopgap-talking-activator-actor` | 15,760 passed, 3 baseline failures; viewer 182 passed | 0 (viewer 0) | ok | ok (viewer ok) |
| `pr/stopgap-walker-opens-sliding-door` | 15,950 passed, 3 baseline failures; viewer 182 passed | 0 (viewer 0) | ok | ok (viewer ok) |
**AI assistance.** As described at the top of this file: Claude (Claude Code, Sonnet 5.5) wrote the code, tests
and text with the contributor, who set the goals, played the routes in the viewer, reported what happened, and
reviews each branch and runs the acceptance routes before opening any pull request.
