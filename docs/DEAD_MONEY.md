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
| Bomb collar | beeps and then explodes near radios and speakers; it is defused by destroying them or moving away | radios present (UI/Pip-Boy); no collar behaviour found |
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

## Open questions

* Which DLCs are installed in the maintainer's Data folder (the data pass's `info`)?
* What does the test character start with: base-game defaults, or a level and gear chosen
  to match a typical arrival?
* Where are the DLC bits at `011c3f2c` read, and does any rule depend on them?
