# Dead Money: research pass

Research status, 2026-10-05. This is the first step of the DLC track, which the
maintainer started alongside M1 (see MILESTONES.md): the main game's mechanics continue
on the M1 route, and this track works ahead on the DLCs, beginning with Dead Money. It
records:

* what the executable does for DLCs;
* what nv-rs can already do with Dead Money's files;
* what the DLC is expected to need;
* the data pass that turns those expectations into checked facts.

Nothing here is implemented yet.

Labels used below:

* **traced**: read in `FalloutNV.exe` 1.4.0.525, with the address;
* **in code**: present in nv-rs, at the file given, with its depth not yet checked against
  the DLC;
* **expected**: the DLC as players know it, not yet checked in `DeadMoney.esm`. The data
  pass confirms or corrects each expected item before any work builds on it.

## What the executable does for DLCs

* **traced**: no gameplay code specific to Dead Money. The executable holds none of the
  DLC's terms (Sierra Madre, collar, hologram, Ghost People, Elijah). Its only DLC names
  are the plugin names, used once, while loading plugins (`004624b0`, TESDataHandler):
  * a loaded file whose name starts with `DeadMoney`, `HonestHearts`, `OldWorldBlues` or
    `LonesomeRoad` is numbered 1–4 (`0046feb0`);
  * bit 1 << (n − 1) is then set in a byte of the object at `011c3f2c` (`00462e10`; that
    object is created in `0044fb20`).

  Where those bits are read is not traced yet.
* **traced**: a console/platform query counts the DLC packages (`dlccount`) and checks one
  by file or display name; its strings are at `0103dc90` and `0103dd20`.
* **Inference**: the DLC's mechanics are in `DeadMoney.esm` itself: records, scripts,
  effects and AI packages run by the same engine as the base game. Rebuilding Dead Money
  therefore means making the systems its data uses work. It does not mean finding new
  engine code.

## What nv-rs already does with the files

* **in code**: the load order knows the official DLC files and combines them with
  `FalloutNV.esm`, renumbering form IDs and resolving overrides
  (`crates/esm/src/load_order.rs`, `OFFICIAL_FILES`). It reads `plugins.txt`, or takes
  `--official` for the base game plus official DLC.
* **in code**: each plugin's archives are found by name, e.g. `DeadMoney - Main.bsa` for
  `DeadMoney.esm` (`crates/assets/src/lib.rs`).
* **in code**: the viewer opens any interior cell by editor ID, form ID or name, so a
  Dead Money cell can be opened directly (`nv-viewer <Data> <CELL> --official`).
* **in code**: nvinspect tags DLC records as DM/HH/OWB/LR in its coverage tables and works
  on a single plugin or a whole Data folder (`functions`, `scripts`, `scripted`, `play`,
  `hostile`, `list`, `cells` below).

## Systems the DLC is expected to need

The nv-rs column comes from a search of the code: present in some form, or nothing found.
Whether the present code covers Dead Money's use is unchecked.

| System | Dead Money's use (expected) | nv-rs |
| --- | --- | --- |
| Starting with an existing character | the DLC expects a finished character; a test route starts from a ready-made one at the entry point | own saves (`world/src/save.rs`); no reading of the game's `.fos` |
| Scripted intro | a radio broadcast in a bunker, the player knocked out, waking in the Villa | quest scripts and package actions (M1 opening work) |
| Inventory taken and returned | the player's gear removed at the start and given back at the end | `RemoveAllItems` and related functions present (`world/src/script_functions.rs`) |
| Bomb collar | **checked in the data**: scripts only, no radio engine. Each speaker or radio (`NVDLC01RadioSpeakerSCRIPT`) compares `GetDistance player` with a radius (512 for radios, 768 for speakers) and counts the player in or out of `NVDLC01BombCollarQuest.iNumRadii`; the quest script (`NVDLC01BombCollarQuestSCRIPT`) runs the countdown, beeps (`PlaySound`, `SetRumble`) and the explosion (`PlaceAtMe`, `Kill`), all gated by the global `NVDLC01Collars`. Destroying a speaker (`OnDestructionStageChange`) takes it out of the count | every function those scripts call is carried out; not run end to end yet |
| The Cloud | an area that damages the player over time | effects and image-space modifiers present; nothing DLC-specific found |
| Ghost People | stay down only when dismembered or disintegrated | dismemberment present (`world/src/stats.rs`) |
| Holograms | invulnerable guards that patrol and shoot on sight | nothing found |
| Companions (Dog/God, Dean, Christine) | follow, fight, talk; Dog/God switch personality | teammates present (`world/src/experience.rs`, UI) |
| Vending machines | Sierra Madre chips turned into items | barter present; chip vending not checked |
| Unconscious/knockout | the intro knock-out and wake-up | unconscious handling present (`world/src/functions.rs`) |

## Data pass (run locally, with your own game)

Each command reads the game's own files. Save outputs outside the repository, and copy
only counts, editor IDs and conclusions into this file: never dialogue or other game text.

All of it in one go, for all four story DLCs, into `dlc-data-pass.txt` (git ignores it):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\dlc-data-pass.ps1 -Game "<your Fallout New Vegas folder>"
```

Or by hand: build nvinspect first (`cargo build --release`, giving `target\release\nvinspect.exe`), then:

```powershell
$Data = "<your Fallout New Vegas folder>\Data"
nvinspect $Data info                                  # DeadMoney.esm in the load order?
nvinspect "$Data\DeadMoney.esm" types                 # what record types the DLC adds
nvinspect "$Data\DeadMoney.esm" list QUST             # its quests (editor IDs)
nvinspect "$Data\DeadMoney.esm" cells                 # its interior cells
nvinspect "$Data\DeadMoney.esm" worlds                # its worldspaces
nvinspect "$Data\DeadMoney.esm" functions             # script functions it calls that nv-rs lacks
nvinspect "$Data\DeadMoney.esm" scripts               # scripts that don't parse yet
nvinspect $Data scripted <entry cell>                 # scripted objects where the DLC begins
nvinspect $Data hostile <cell>                        # who attacks on sight (holograms, Ghost People)
nvinspect $Data play 60 <first quest> <stage>         # run its quest scripts, list missing functions
```

The results fill four things:

1. a Dead Money quest and cell inventory, with entry points;
2. the missing script functions, most used first, which become the track's first work;
3. the intro's actual sequence of records and scripts, replacing the "expected" rows
   above;
4. which systems the base game already exercises (the co-worker's track) and which only
   Dead Money needs (this track).

## Plan

1. **D1 Inventory.** The data pass above; turn the table above into checked rows.
2. **D2 Entry.** Open the DLC's entry cell in the viewer with a ready-made test character,
   with no opening or face menu.
3. **D3 Intro.** The bunker broadcast, the knock-out and the wake-up in the Villa, traced
   from the DLC's scripts and compared in the original game.
4. **D4 onwards**, in the order the DLC's quests need them: the collar, the Cloud, Ghost
   People, holograms, companions, vending.

Each step follows the usual method: trace, write up here with addresses or record IDs,
implement with tests, open a pull request, and compare in the original game.

## Radio

Dead Money calls six radio functions nv-rs lacked: the Pip-Boy radio switched off for the
intro and the trip to the Villa (`PipBoyRadioOff`), Elijah's broadcast on the collar
(`PipboyRadio Tune`, then `StartRadioConversation`), the Sierra Madre's ambient music
changing with the story (`StartRadioConversation` on `NVDLC01RadioStationAMBREF`), the
Starlet hologram at the fountain playing a station (`SetNPCRadio`), and a refresh at the
start (`ForceRadioStationUpdate`, `ResetPipboyManager`). None of them drives the collar
(above).

**traced** (`FalloutRadio`, `0083xxxx`; script handlers from the command table at
`01190910`):

* `PipboyRadio` (`005d7fb0`) takes a word and an optional station. A word starting with
  `1`, or `enable`/`on`, switches the Pip-Boy radio on (`008324e0`) and tunes it
  (`00832240`); one starting with `0`, or `disable`/`off`, switches it off; `tune` tunes
  it. The words are compared without case: Dead Money writes `Tune`.
* Switching off (`008324e0(0)`, also `PipBoyRadioOff`, `005dc580`) forgets the tuned
  station (`011dd42c`). The on flag is `011dd434`. A radio-wide "disabled" flag
  (`011dd436`) makes all of this do nothing; what sets it isn't traced.
* Tuning only works while on. The station object is found in the radio's list or made
  (`00832cb0`): a reference whose base is a talking activator (form type 0x16) is a
  station itself; activators, NPCs, creatures and levelled lists that aren't actors take
  their base's radio template (`004fd3c0` → `008356e0`). When none can be made, the radio
  switches off. A new station's first update is staggered by a random 0–30,000 ms
  (`00944460`).
* `StartRadioConversation` (`005d82a0` → `00835be0`), on a station, ends what it was
  playing and starts the topic given, or the default (`0061a2d0(7, 0)`: the first entry
  of the radio dialogue list). Its next update is 50 ms later. If the Pip-Boy is on and
  tuned to that station, the radio sound restarts.
* `SetNPCRadio` (`005d8100`), on an actor with a station: 1 plays the station through
  that actor (`00835810`), 0 stops it (`00835980`), other values do nothing.
* `ForceRadioStationUpdate` (`005d8280` → `00832ad0(1)`) makes the stations update at
  once instead of at `iRadioUpdateInterval`.
* `ResetPipboyManager` (`005db490`) sets the player's Pip-Boy manager's reset flag
  (+0x16c); its reader isn't traced.

**in code** (`crates/world/src/more_functions/radio.rs`, test
`the_pipboy_radio_and_its_stations`): the six functions keep the state above (on, tuned
station, each station's conversation, actors playing a station, the reset flag), saved
with the game. With them, nvinspect counts 178 of the 199 functions Dead Money uses as
carried out (172 before).

**Not done:** what a station plays (the conversation's lines in turn, `RadioConvTask`,
`008373a0`), signal range and static (`fRadioStaticAtOuterRadiusPct`), the sound and the
Pip-Boy Radio list, stations made from activators' or actors' radio templates, tuning with
no station given, and the default topic. Nothing has been compared in the original game.

## Animations playing (`IsAnimPlaying`)

Dead Money asks 26 times, on placed objects only: speakers, gates and Elijah's talking
activator (`LinkedRef.IsAnimPlaying Forward`, `Backward`, `Left`, `Right`), mostly to
avoid starting a group that's already playing.

**traced** (`005c14a0`): on a reference with actor animation data (vtable +0x1e4), its
eight sequence slots. Otherwise the model's controller manager: with a group, the
sequence named after the group (`00438170` → `0047a520`) is checked; without one, any
sequence. "Playing" is the sequence's state (+0x44, read by `008041a0`) not being 0
(inactive). No 3D loaded: 0. **Inference**: the only code that sets a sequence inactive is
its deactivation on request (`00a35030`) and the end of an ease-out; nothing does it when
a clamped sequence reaches its end. So a played `Forward` keeps counting as playing until
another sequence replaces it. Not compared in the game.

**in code**: the viewer reports each frame the sequences active on placed objects
(`world::more_functions::report_sequences`, from `move_pieces`): a script's `PlayGroup`
sequence, a door's `Open`/`Close`, or the model's running start-up sequences (ones holding
a frame aren't counted: a guess). `IsAnimPlaying` answers from that report, comparing
group names without case. People's animation data isn't carried out (the script stops, as
before). Test: `objects_animations_playing`. nvinspect: 179 of 199.

## Line of sight (`GetLineOfSight`)

42 uses in 6 scripts. **traced** (`005c1ce0` → `0059c990`): the caller must be an actor
and the target given; otherwise 0.

* **The player asking** (caller `011dea3c`): first whether the target is in view (its 3D
  bound against the camera, `004b5fc0`, or `00444ed0` with a bound test). In view, rays
  are cast (collision layer 0x25, `SpecificItemCollector`) from the camera position to the
  target at 0.75, 0.5 and 0.25 of its bound height (max z − min z, vtable +0x1dc/+0x1d8)
  above its position; a ray hitting nothing or the target itself gives 1. Otherwise the
  actor test below decides.
* **Anyone else** (`0088b880(0, target, 1, 0, 0)`): the target needs 3D. An actor
  target: the caller's AI process answers (vtable +0x2cc; not traced: probably its
  detection line of sight). Any other target: a view-cone test (`0088c570`), then rays
  from the caller's eye (position + eye height, `008be940`) to the target at 0.75, 0.5
  and 0.25 of its bound height (scaled, `00567400`; an actor's head and torso points
  first), with the same hit rule.
* With the console's debug flag, it prints "sees" or "can't see".

Dead Money's uses: `Player.GetLOS` on Ghost People (36: the camera path) and
`HoloA/B/C.GetLOS Player`, `NVDLC01DeanRef.GetLOS Player` (16: the actor test). The actor
test's process answer is the `HighProcess`'s (`008f6930`): its detection data on the target
(vtable +0x504), byte +0x1e, the line of sight its last detection run found; other
processes (`008d0510`) say 0. `005723b0` is the distance between the two (must exceed 2;
the exact comparison is unconfirmed).

**in code** (`crates/world/src/sight.rs`, test `line_of_sight`): the rules above in the
world. The viewer answers what they ask through `world::sight::Sight`, given to the
`Runner` that runs placed objects' and quests' scripts each frame
(`viewer/src/sight.rs`): a reference's bound (placed objects' rendered bounds; people's
collision shape, a stand-in for their model's bound), the camera's position and view (any
corner or the middle of the box inside the frustum), and rays through the cell's
collision. nv-rs's collision doesn't know which object a ray hit, so a ray stopping where
it enters the target's box counts as hitting the target (a stand-in). The viewer's
detection runs report their line of sight (`report_detection_sight`), which the actor
test reads. Not carried out: an object target for someone other than the player (view
cone `0088c570` and rays from the eyes), the player's test headless, and conditions
(`CTDA`) asking it. Nothing compared in the original game.

## Effect shaders (`PlayMagicShaderVisuals`, `StopMagicShaderVisuals`)

160 calls. Mostly the holograms' moods: scripts swap `NVDLC01HologramNeutral`,
`NVDLC01HologramAttention` and `NVDLC01HologramAggressive` on them (object scripts, the
hologram spell effects' `ScriptEffectStart`/`Finish` blocks, a terminal), plus the heated
knife's `Flames01`/`Smoke01`, Elijah and Dog/God.

**traced**:

* `PlayMagicShaderVisuals` (`005d1b80`) takes an effect shader and seconds (default −1).
  No reference: the player. Unless the reference's cell is attached (`004511e0`) and it
  has 3D (vtable +0x1d0), nothing happens (it still succeeds). Otherwise a new
  `MagicShaderHitEffect` (`0081f580`) is made for the reference and shader: seconds of 0
  or more are its lifetime, below 0 `FLT_MAX` (until stopped). If it initialises
  (vtable +0xc4) it joins the running effects (`00973fd0`); one already running isn't
  replaced, so they stack.
* `StopMagicShaderVisuals` (`005d2130` → `00974a50`) ends every running
  `MagicShaderHitEffect` on the reference with that shader.
* With the console's debug flag both print what they did.

**in code** (`crates/world/src/more_functions/shaders.rs`, test
`effect_shaders_on_references`): the running shaders (reference, shader, end time on the
state's clock) with those rules, and `Shown::ShaderVisual` / `ShaderVisualStopped` for the
viewer. The viewer reports each frame what has 3D (`report_loaded`: rendered placed
objects, people about, the player). nvinspect: 182 of 199.

**Not done**: drawing them. nv-rs doesn't read `EFSH` records yet (fill and edge
textures, colours and their timing, membrane and particle shaders); the viewer gets the
events and draws nothing. Not traced: what makes initialisation fail (taken here as no
shader), whether running shader effects are saved (not saved here), the player's
first-person flag (+0x4c). Nothing compared in the original game.

## Terminals going back (`ForceTerminalBack`)

20 calls in 17 terminal items, all in the Vault and the hologram vault terminals: a
confirmation sub-screen's "No" item runs `ForceTerminalBack` to return to the screen
before (`NVDLC01VaultMainInfoDeanTerminal` and others).

**traced** (`005dc4e0`): if the terminal menu (1057, `ComputersMenu`) is open
(`00a09030`), its screen stack is popped (`00758a80` → `0063f7b0`) and the new top shown
(`007586e0`); with no screen left the terminal closes (`00757ea0`). Otherwise nothing.

**in code**: the function sends `Shown::TerminalBack` while `menu_open` is the terminal
menu (`world::terminal::TERMINAL_MENU`), which the viewer now sets while a terminal is
shown. The viewer's terminal pops its screen stack for each one, closing past the first
screen, right after the item's script and each frame (`viewer/src/menus.rs`,
`terminal_backs`). Tests: `terminals_go_back_only_while_open`,
`force_terminal_back_pops_screens_then_closes`. nvinspect: 183 of 199. **Not traced**: for
an item with both a script and a sub-menu, whether the sub-menu opens before or after the
script's back (here: after). Nothing compared in the original game.

## Caravan cards (`GetContainer`, `RemoveMe`, `AddCardToPlayer`)

12 calls, all in one script: `NVDLC01CardAddToPlayerScript`, on the Sierra Madre's "Dead
Man's Hand" cards (a copy of the base game's `CardAddToPlayerScript`). Its `OnAdd` block:
if `GetContainer` is the player, `AddCardToPlayer` then `RemoveMe`. A card picked up joins
the player's Caravan cards and leaves the inventory.

**traced** (handlers take the script's item and its containing object):

* `GetContainer` (`005ce5c0`): the containing object when there's an item and a
  container; else 0.
* `RemoveMe` (`005b53d0`): with an item and a container, one of the item leaves the
  container (its RemoveItem, vtable +0x17c, count 1), into the container given if any; for
  an actor, the equipped instance's extra data (`004bfda0`); the player's inventory is
  refreshed (`00704af0`). The handler returns failure, which ends the script.
* `AddCardToPlayer` (`005cf3d0`): the item's base must be a `TESCaravanCard`; it joins the
  player's cards unless already there (`00969bc0`). Otherwise an error is reported only.

**in code**: `Runner::on_add` runs an item's `OnAdd` blocks (no container named, or this
one) with the item and its container (`Runner::container`); the viewer runs it when the
player picks up a placed item. The three functions are in
`crates/world/src/more_functions/carried.rs`; the cards are saved. Test:
`caravan_cards_picked_up`. nvinspect: 186 of 199.

**Not done**: `OnAdd` for items arriving other ways (taken from containers, `AddItem`,
barter): what the game gives their scripts as the item isn't traced. `RemoveMe` doesn't end
the script (nv-rs's statements can't; Dead Money calls it last); an actor's equipped
instance isn't told apart. The Caravan game itself and a deck aren't here. Nothing compared
in the original game.

## Companions and actors (`OpenTeammateContainer`, `PushActorAway`, `SetDisposition`, `GetCauseofDeath`)

9 calls. `OpenTeammateContainer` is the result of four companion dialogue lines (the
`FollowersTrade` topic). `PushActorAway`: Elijah pushes the player with 5 in a dialogue
result when `VaultCodeBox.bPlayerBlocks` is set (`NVDLC01ElijahVaultSentryDownTopic03`),
and a spell effect (`NVDLC01StarletKnockdownScript`) has the lobby hologram push whoever
it lands on, other than itself and the player, with 10. `SetDisposition` is in the lobby
hologram's `OnHit` (`NVDLC01StarletLobbyScript`: `NVDLC01StarLobby.SetDisposition player
100`; its comment says it keeps her from turning hostile). `GetCauseofDeath` is in Dog's
death script (`NVDLC01DogScript`): killed by an explosion (`GetCauseOfDeath == 0`) in the
restaurant with the gas traps not all set (`nGasTraps != 3`), the kitchen valves explode
and the player dies; any other death there starts the collar's countdown.

**traced**:

* `OpenTeammateContainer` (`005d9430`): on a person or creature who is the player's
  teammate (actor +0x18d), or any with the optional number not 0, the container menu
  opens on their things in its companion mode (`00709470`, mode 3). Always succeeds.
* `PushActorAway` (`005d6b60`): the caller pushes the actor given away. Not an actor: the
  game prints "SCRIPTS: PushActorAway in script '…' is attempting to push a non-actor
  reference." and nothing else. The force (`00646580`) is (`fKnockbackAgilBase` +
  `fKnockbackAgilMult` × Agility × 10) × (number × `fKnockbackDamageMult` +
  `fKnockbackDamageBase`), with the pushed actor's Agility. None of these settings is in
  `FalloutNV.esm`; the exe's defaults (`00f61a40`…`00f61ad0`) are 1, −0.008, 50 and 10.
  Agility 5 and Elijah's 5 give 60; Agility 10 gives 20. Only an actor with the high AI
  process (process +0x28 is 0) is pushed: from the caller's centre (`Actor` vtable +0x1f4,
  `008ae4c0`) through the process (`0091fee0`), which for an actor that can be knocked
  down sets its knock state to 2 and throws its ragdoll from that point with the force.
  With the player as the caller, something more goes to `005f5950` first (not traced).
* `SetDisposition` (`005d54a0`): on a person or creature, with an actor given, takes the
  disposition now (vtable +0x344, `0087fd90`) from the number and adds the difference
  (vtable +0x460, `0087fb40`). That adds only toward the player: the actor keeps a list of
  (amount, toward whom) at +0xfc (change flag 0x80000), and the amount is cut so the
  disposition stays within 0–100.
* `GetCauseofDeath` (`005be740` → `005a3d30` → `005730d0`): on a person or creature, the
  cause kept in its dismembered-limbs extra data (0x5f, +0x10); else −1. It is written at
  death (`00572fc0`, from `008b4d10`): the hit handler (`0089a760`) picks it by the form
  type of what struck — a weapon (a melee blow) 2, a missile, beam, flame or continuous
  beam projectile 1, a grenade or an explosion 0, an ingestible 5, debris 4, anything
  else (fists, a creature's attack) 3; a source with vtable +0x220 set makes it 0. `Kill`
  with a limb keeps its third number (default −1) as the cause (`005be2a0` → `008b51b0`).

**in code**: `crates/world/src/more_functions/actors.rs`. `OpenTeammateContainer` sends
`Shown::TeammateContainer`, and the viewer opens its container menu on the companion.
`PushActorAway` sends `Shown::PushedAway` with the force, for someone the viewer reports
loaded. Dispositions toward the player and causes of death are kept and saved. Killing
hits keep a cause: no weapon 3, a melee weapon 2, a lobber's projectile 0, other
projectiles 1. Test: `companions_pushes_dispositions_and_causes_of_death`. nvinspect: 190
of 199.

**Not done**: the container menu's companion mode is drawn as an ordinary container (mode
1). Being knocked down alive isn't drawn: the viewer only ragdolls the dead, and the knock
state and getting up aren't kept (`GetKnockedState` stays 0). The rest of the game's
disposition reckoning (`0087fd90`: factions, Charisma and more) isn't carried out, so
the disposition toward the player is only what scripts added (starting at 0), and
`GetDisposition`/`ModDisposition` aren't wired to it yet. nv-rs has no explosions,
poison or debris that kill, so causes 0 (except grenades), 4 and 5 don't happen. The
+0x220 source flag and the player-caller branch of `PushActorAway` aren't traced.
Nothing compared in the original game.

## Dispelling (`DispelAllSpells`)

3 calls, all `player.DispelAllSpells ; Removes all chem effects.`: on arrival
(`NVDLC01IntroSCRIPT`), in `NVDLC01MQ03Script`, and on the way back through the gate to
the Mojave (`NVDLC01SMGateMojaveSCRIPT`).

**traced**: `DispelAllSpells` (`005c2190`) on a person or creature walks its active
effects (`008249d0`) and dispels (`00804210`) each
one whose magic item's type (vtable +0x18) is 0 (a spell), 2 (a power), 3 (a lesser
power), 7 (an ingestible: `AlchemyItem` `009d2510`) or 8 (an ingredient: `IngredientItem`
`006e4ba0`). A spell's type is its `SPIT` type (`SpellItem` +0x1c on the magic item,
`00441110`). An enchantment (type 6, `009d6b60`) goes only when its own type (+0x34, the
first `ENIT` field) is 0. Diseases (1), abilities (4), poisons (5) and addictions stay.
Anything else does nothing; it always succeeds.

**in code**: `DispelAllSpells` in `crates/world/src/more_functions/actors.rs`
(`dispelled_by_all` picks the effects by their source's record); script effects run
their `ScriptEffectFinish`, as `Dispel` does here. Test:
`dispel_all_spells_leaves_abilities_and_poisons`. nvinspect: 191 of 199.

**Not done**: equipment enchantments aren't active effects in nv-rs, so the `ENCH` rule
has nothing to act on yet. Nothing compared in the original game.

## Traps (`SetVATSTarget`, `FireWeapon`)

3 calls. The two tripwire scripts (`NVDLC01TrapTripwireSCRIPT`,
`NVDLC01TrapGenericTripwireSCRIPT`) start their `OnActivate` with `setVatsTarget 1`. The
shotgun trap (`NVDLC01TrapShotgunSCRIPT`) fires `WeapNVSingleShotgun` once when something
other than the player activates it.

**traced**:

* `SetVATSTarget` (`005daae0`): only on a reference with the flag 0x01000000 (`00452370`;
  the destruction code `00477d10` asks the same flag, so it marks a destructible
  reference). It compares the number (not 0 = targetable) with the base's own
  "V.A.T.S. targetable" flag: its destructible data (`00475400`) `DEST` flags bit 0x01
  (`00576100`). Equal clears the reference's flag 0x04000000, different sets it
  (`004846e0`, change flag 1). V.A.T.S. asks `00576070` when it gathers objects
  (`007f52c0`): destructible, the base's flag, turned the other way by 0x04000000.
* `FireWeapon` (`005da570`): the argument must be a weapon (form type 0x28), else the game
  prints "SCRIPTS: FireWeapon in script '…' called with non-weapon parameter.". The
  reference then fires it through the game's weapon fire (`00523150`, deferred off the
  main thread by `008c7aa0`). For something that isn't an actor, the shot leaves the
  object's position along its X (pitch) and Z (heading) angles. When its 3D has a
  projectile node, it leaves that node instead, along the node's facing. The node is found
  by `00525700`: the weapon's own node name when it has one (`005256b0`), else
  `ProjectileNode`, else `##ProjectileNode`.

**in code**: `crates/world/src/more_functions/traps.rs`: the override is kept and saved,
`vats_targetable` is the V.A.T.S. test, `shot_from` works out the shot.
`Shown::WeaponFired` goes to the viewer's `combat::object_shots`. That system finds the
node in the model (`nif::Nif::placed_node`) and fires each pellet within the weapon's
cone, as the player's shots do. The first person met (the player or someone about, by
their bounds) before a wall takes the hit (`Runner::hit_at` with the object as attacker;
they don't fight the object back). Tests: `traps_vats_targets_and_weapons_fired`,
`shots_leave_along_the_objects_facing`, `finds_a_placed_node_by_name`. nvinspect: 193 of
199.

**Not done**: nv-rs's V.A.T.S. doesn't target objects yet, so `vats_targetable` isn't
asked by it. The reference flag 0x01000000 is taken to mean "the base has destruction
data" (inferred). The weapon's own projectile node name isn't read. Shots are instant
rays against bounds, not projectiles in flight against hit shapes; the facing's sign for
pitch follows nv-rs's placement convention (positive X tips the nose down), not checked
in the game. Objects made by `PlaceAtMe` can't fire yet. Nothing compared in the
original game.

## Conditions (`GetVATSValue`, `IsFacingUp`)

4 conditions, no script calls. The Hobbler perk (`NVDLC01Hobbler`) asks `GetVATSValue 5`
with the left and right leg's actor values (29, 30). The ghost people's get-up idles
(`GhostGetUpFaceUp`, `GhostGetUpFaceDown`) ask `IsFacingUp`.

**traced**:

* `GetVATSValue` (`00594dc0`, conditions `00594e40`): the attack V.A.T.S. is playing
  (the player's, `009c71c0`, or an NPC's, `007f5280`), by case. Case 5 is the attack's
  body part actor value against the number. nv-rs already answered it
  (`world::vats::function_value`), but the V.A.T.S. functions were missing from
  `scripting::HANDLED`, so `nvinspect` counted them as not carried out. Cases 2 and 9
  compare forms with TESForm vtable +0x10c (`00603880` for people), which gives 0 when
  they're the same. nv-rs's equality checks match that.
* `IsFacingUp` (`005cb720` → `005a0710`): on a person or creature, finds a node in its
  3D: first the name `004b7920` keeps at `011c61b4` ("Bip01 Spine"), then "Bip01
  Spine01". With a node, the answer is 1 when its world rotation's [2][1] (the node's
  +0x84) is above 0 (`00c6b7b0`). With no 3D or no node, the answer is 1 too. Anything
  else gives 0.

**in code**: the ten `GetVATS…` functions are in `HANDLED`. `IsFacingUp` is a read in
`world::more_functions`, answered from `report_facing_up`. The viewer's `animate_actors`
works out each pose's spine (`actors::spine_up`), and `report_facing_up` sends it every
frame. A person's placement turns only about Z, which leaves that row of the rotation
alone. Tests: `facing_up_as_the_viewer_reports_it`, `the_spine_faces_up_by_its_rotation`.
nvinspect: 195 of 199.

**Not done**: nv-rs has no knockdowns or getting up, so the get-up idles aren't played
yet; a ragdoll asleep keeps its last report. The player isn't reported (counts as no 3D:
1). Nothing compared in the original game.

## Crafting and casino menus (`ShowRecipeMenu`, `Show…MenuParams`)

6 calls.
* The Sierra Madre vending machines' scripts (`CraftingVendingMachineRecipesScript`,
  `NVDLC01CraftingMachineRecipeOFFScript`) call `player.ShowRecipeMenu
  NVDLC01VendingMachineRecipes`.
* The casino's tables call `ShowSlotMachineMenuParams SierraMadreCasinoData 1 25 0`,
  `ShowBlackjackMenuParams … 1 200 0` and `ShowRouletteMenuParams … 1 100 0`, unless the
  player is banned (`NVDLC01CasinoComps.bBanned`).

**traced**:

* `ShowRecipeMenu` (`005deb10`): the vendor is the person the script runs on. For a
  talking activator, the vendor is its base's speaker (+0x90), when the reference's
  +0x81 is set. The category (`RCCT`) is optional. `00704fc0` → `00726ff0` opens
  `Data\Menus\recipe_menu.xml` as menu 1077 (`007273e0`) with the vendor and category.
  With no vendor, the game prints "Recipe menu called with NULL vendor!  Oh, noes!".
* `ShowSlotMachineMenuParams` (`005cf040`), `ShowBlackJackMenuParams` (`005cf0f0`) and
  `ShowRouletteMenuParams` (`005cf1a0`) each take a casino (`CSNO`) and three numbers. They
  hand them to the game's menu: slots `007c0a40` (menu 1080), blackjack `00733630`
  (1081), roulette `007bbe20` (1082). Each menu keeps them in its globals. With no casino,
  the game prints an "Invalid EditorFormID" line; the roulette handler's message names
  blackjack.

**in code**: `crates/world/src/more_functions/menus.rs` sends `Shown::RecipeMenu` or
`Shown::CasinoMenu`, then the menu's number (`Event::Menu`). The viewer counts each as
opened and closed at once, so the scripts' `MenuMode` blocks run. Test:
`recipe_and_casino_menus_open_with_their_data`. nvinspect: 199 of 199.

**Not done**: nv-rs has no crafting (recipes `RCPE`, categories `RCCT`, the recipe menu)
and no casino games (slots, blackjack, roulette, the `CSNO` data, winnings), so nothing
can be crafted or played yet. That is a separate piece of work, larger than a function.
What the three numbers mean in each game isn't traced; the reference's +0x81 check for
talking activators is taken to be met. Nothing compared in the original game.

## Open questions

* Which DLCs are installed in the maintainer's Data folder (the data pass's `info`)?
* What does the test character start with: base-game defaults, or a level and gear chosen
  to match a typical arrival?
* Where are the DLC bits at `011c3f2c` read, and does any rule depend on them?
