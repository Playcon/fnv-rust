# Face creation: M1 working evidence

Original-game reference inspected 2026-10-03 after the user opened ShowRaceMenu:
the Reflectron cabinet fills most of the view, oval live head/upper-body preview
on the left, green sex-selection screen on the right (Male selected, Female,
Next), and Sex/Race/Face/Hair labels with indicator lamps below the preview.
The room remains visible and blurred behind it. This confirms the starting
page's presentation only, not the unobserved editing controls or transitions.
Opening animation failures reported by the user take priority before extending
this menu; see OPENING.md.

2026-10-03. Choice reader implemented; no replacement menu published yet.
Executable hash and shared priorities: OPENING.md and MILESTONES.md.
Decompilation stays outside the project in nv-re/decomp/codex-m1.

ShowRaceMenu handler005cedf0 calls00705870 with mode1 outside the script
thread flag, mode0 inside. Barber005cee30 passes2; surgeon005cee50 passes3.
00705870 opens immediately or schedules mode5 with the argument retained.
007ac730 loads Data/Menus/CharGen/race_sex_menu.xml. The original XML
successfully parses through nvinspect (codex-m1/racesex-menu.txt).

RaceSexMenu constructor007ac1f0; vtable01075974. SetTile007ac500 accepts
IDs0..5 into+78. Click007adce0; drag start007ae1b0; update007ae420;
special keys007aecb0. Update checks whether the menu is active before input.
Setup007acb60 loads Terminals/NV_reflectron_UI.NIF for the opening;
BarberInterface01.NIF and PlasticSurgeryInterface01.NIF for modes2/3.
The opening starts page0, barber3, surgeon2. Detailed page-tree/preview
behavior still needs tracing; do not invent controls or a substitute panel.

Choice rules traced so far:
- Setup enumerates races filtered by0059f610: runtime flags at+70 bit1.
  Existing functions.md maps this field to RACE DATA u32 at32.
- Hair page007af300 enumerates the global hair list, requires playable
  (005fdf40, hair+48 bit1) and005fdfa0: membership in the actor's race list,
  then sex restriction (005fdf60: male allowed when bit2 clear;
  005fdf80: female allowed when bit4 clear).
- Eyes page007af450 similarly requires playable005fc4d0 and005fc5f0
  membership/sex restrictions. Eyes flags are at+30; bits1/2/4 have the
  same meanings.005fdcb0 (hair) and005fc220 (eyes) each copy the single
  DATA byte into those fields.00610cd0 copies36-byte RACE DATA to+50;
  HNAM and ENAM are mapped form arrays, skipped unless length is divisible4.
- Changing race/sex validates current hair/eyes in007b1ca0; a race default
  hair is considered before scanning playable sex-compatible hair. Exact
  defaults and eye fallback still need tracing.

Existing infrastructure: actor::ActorLook and Face hold assembled models
and symmetric/asymmetric morphs; Game::actor_scene builds their meshes.
Player first_person_look deliberately has no head. GameState/save currently
persist only player_name/player_female for identity, no editable race/hair/
eyes/morphs. Menu creation needs those fields, selection rules, a preview
and native XML callbacks before replacing the current face auto-accept.

The menu's pages, items, clicks and sliders: [RACE_SEX_MENU.md](RACE_SEX_MENU.md).

## Race and sex changes (traced 2026-10-04)

`007b1ca0` runs after the race or sex changes (menu `+0xd8` is the
player's base NPC):

* Hair is kept when it suits (`005fdfa0`: in the race's hair list and
  allowed for the sex; playable isn't asked). Otherwise the race's default
  for the sex (`00613870`: race `+0x94` male, `+0x98` female, from `RACE`
  `DNAM`'s two IDs, `00610cd0` case `DNAM`), taken as it is; only when there
  is none, the first hairstyle in the race's hair list (`+0x8c`, walked from
  its head) that is playable (`005fdf40`) and allowed for the sex; else none
  (`006031e0`), then the head is rebuilt (`007b2b50(1)`).
* Eyes are kept when they suit (`005fc5f0`: in the race's eyes list and
  allowed for the sex). Otherwise the first entry of the race's eyes list
  (`007b1e50`: race `+0xa8`), unchecked; none when it is empty
  (`00603200`, `007b27c0` rebuilds the eyes).
* Tiles 0 and 1 are marked for refresh (`007adc60`, bits of `+0xec`).

The race's hair and eyes lists keep record order: `00610cd0` reads each
`HNAM`/`ENAM` (skipping one whose length isn't a multiple of 4), drops IDs
that aren't hair/eyes ("MASTERFILE: Could not find hair (%08X) for race"),
and appends with `00613810`/`00613910` → `00905820`, which adds at the tail
and skips an ID already listed.

Implemented as `world::chargen::appearance::{default_hair, hair_fits,
eyes_fit, fit_to_race}`; `GameState` keeps the player's chosen race, hair
and eyes (`player_race`, `player_hair`, `player_eyes`, saved as `race`,
`hair`, `eyes` lines), and the player's race in dialogue conditions follows
`player_race`. No menu sets them yet.

## Implemented and checked

`world::chargen::appearance::{races,hair,eyes}` enumerates eligible choices
from winning records, rejects deleted entries, resolves membership IDs via
the race record's owning plugin, and keeps actual FULL names (missing names
stay missing). Native menu ordering is not yet established. No default
selection or appearance mutation is invented here.

Six generated-input regressions cover flags/sex, malformed lists (including
a valid ID followed by a bad tail), wrong/missing records, deleted winning
overrides, master-index remapping, and missing/empty labels. A surviving race
is also checked against deleted hair/eyes, independently of a deleted race.

Real official-data harness outside the project:
`nv-re/work/codex-m1/appearance-observations`; `appearance-real-data.log`.
Observed four playable races: African American, Asian, Hispanic, Caucasian.
Each exposes18 male/23 female hair choices and4 eyes per sex. These are data
observations, not yet a native menu comparison. Full check status: OPENING.md.

Original game process40680 opened via Steam (no capture DLL installed).
Computer Use keys/clicks did not affect its main menu, including after an
explicit focus retry. User was asked to load a save and open the tester with
ShowLoveTesterMenuParams40 for reference inspection. No original saves changed.
