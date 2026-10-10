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
0 NOT PLAYED, 1 NEVER STARTED). No stopgap code was needed. AI disclosure as above.

| Branch | Quests |
|---|---|
| `pr/suites-christine` | MQ03c Last Luxuries |
| `pr/theater-dean` | MQ03b Curtain Call at the Tampico |
| `pr/vault-ending` | MQ03, the ending, the bunker transition (MQ03d stays NEVER STARTED) |
| `pr/bark-timers` | Dog, Dean and Christine bark timers |
| `pr/fade-timers` | fade to credits, the Auto-Doc fade |
| `pr/enemy-test` | the developer's enemy test quest |
| `pr/radio-quests` | the eight radio quests (blocked by the radio engine) |
| `pr/elijah-barks` | Elijah's lobby intercom, the suites terminal effect, the Starlet counter |
| `pr/follower-fire` | dismissing the Mojave companions, Arcade's goodbye |
| `pr/toxic-quests` | the toxic cloud, global toxic damage (blocked by Hardcore) |
| `pr/casino-support` | chip reward, comps (blocked by the games), hologram vendor, challenges |
| `pr/wt-support` | the West Town support quest |
| `pr/gala-fireworks` | the Gala fireworks quest |

(`pr/dean-escort`, `pr/christine-escort`, `pr/gala-event` and `pr/casino-quests` are described above.)
