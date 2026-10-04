# Research tools and handoff

How behaviour is traced from `FalloutNV.exe` (1.4.0.525, Steam) for this
project, with the tools in `tools/re/`, and where each traced topic stands.
Anyone with their own copy of the game can reproduce every step; nothing here
contains game data.

What may be committed: these tools, and prose in `docs/` that describes
behaviour with executable addresses and labels its inferences. What may not:
the executable or any part of it, decompiled code, disassembly, Ghidra
projects or exports (CONTRIBUTING.md). Keep those in a private folder or
repository of your own.

## TL;DR for the next person

* **Where this picked up.** M1 (the opening) was already under way: rendering, animation and
  scripts worked. The game's look/eye behaviour and the face menu were missing, and there was
  no shared way to read the original executable.
* **What was done.**
  * The executable was decompiled in a separate private repository.
  * Research tools were written (this folder's `tools/re/`).
  * The executable was traced and rebuilt here in stacked pull requests #1–#7 and #9:
    * head turning (LookIK solver, look anchor, aim point, release);
    * whom actors look at (this fixes Doc never looking at the player on his scripted lines);
    * eye darting;
    * face menu groundwork and its full trace;
    * FaceGen's `SI.CTL` slider, age and gender maths.
  * Each has a write-up in `docs/` and tests. All were green on CI when last checked; none
    has been merged or compared in game yet.
* **What's next.** Merge #1 → #7 → #9 (#8 is independent). Then compare the look behaviour
  in the original game. Then build the face menu on the game's XML (Next steps below).

## Working locally

Everything shared lives in this repository: the code, the write-ups in `docs/`, the
tools in `tools/re/`, and this handoff. The maintainer's decompilation is a separate,
personal repository that is never shared or merged into this one. Only the tools' default
paths point at it.

First time, in PowerShell (any folder; the examples use your user folder):

```powershell
cd $env:USERPROFILE
git clone https://github.com/zzxxbartfiolxxzz-design/fnv-rust   # or, in an existing clone: git fetch origin
cd fnv-rust
git checkout claude/facegen-controls     # the top of the stack: contains #1-#7 and #9
cargo test --workspace                   # core checks; the viewer builds from viewer\
```

The research tools need Python 3.8+ and Capstone. They read your own Steamless-decrypted
executable; the decompiled export is optional:

```powershell
py -m pip install -r tools\re\requirements.txt
$env:FNV_EXE = "$env:USERPROFILE\work\FalloutNV.unpacked.exe"   # where Steamless put it
$env:FNV_DECOMP = "$env:USERPROFILE\Decompiling-FNV\decomp"     # optional: your own export
py tools\re\disasm.py 009016a0     # should list DEFAULT ACTION SCRIPT COMBAT DIALOG
```

Without `FNV_EXE`, the tools look in `..\work\FalloutNV.unpacked.exe` next to the clone.
Without `FNV_DECOMP`, they look in `..\Decompiling-FNV\decomp`; if that is missing too,
callees stay unnamed but strings and settings are still shown.

Each new step: make a branch on top of the stack (`git checkout -b <topic>
claude/facegen-controls`), then trace, write up in `docs/`, implement with a test, and run
the checks in `AGENTS.md`. Push with `git push -u origin <topic>` and open a pull request
using the template.

Not yet done:

* None of the look, eye or face work has been compared in the original game.
* `SI.CTL` has not been checked against the real file (counts, labels, end of file).
* One viewer test (vigor) fails on Linux and passes on Windows CI; this predates the
  work here.

## Setup

1. Decrypt your own `FalloutNV.exe` with
   [Steamless](https://github.com/atom0s/Steamless) (Windows). The Steam
   build's code is SteamStub-encrypted; the GOG build is not, but its
   addresses may differ from those cited here. Put the output at
   `../work/FalloutNV.unpacked.exe` next to your clone, or set `FNV_EXE`.
2. Python 3.8+ and `pip install -r tools/re/requirements.txt` (Capstone).
3. Optional, for decompiled C: Ghidra 12.1.4. Import the decrypted file
   (x86:LE:32, Windows cspec), auto-analyse, then run the scripts in
   `tools/re/ghidra/` in order:
   * `ApplyRtti.java`: labels the ~3,000 RTTI classes and their vftables
     (Ghidra skips RTTI on this binary by default);
   * `NameClassFunctions.java`: moves virtual functions and
     constructors/destructors into their classes;
   * `ExportFNV.java <out>`: every function decompiled into
     `<out>/src/`, plus `functions.tsv`, `strings.tsv`, `classes.tsv`,
     `imports.tsv`;
   * `python3 tools/re/split_decomp.py <out>`: one file per function
     (`<out>/fn/<first 4 hex digits>/<address>.c`) and indexes by class and by
     original source file (`<out>/index/`);
   * `ExportCallTree.java <dir> <depth> <address>...`: C and disassembly of
     some functions and their callees, for a topic.

   Point `FNV_DECOMP` at `<out>` (default: `../Decompiling-FNV/decomp` next
   to your clone).

## Tools

| command | use |
| --- | --- |
| `python3 tools/re/disasm.py <address>...` | annotated disassembly: callee names (with an export), strings, game/INI setting names. Use it when the decompiler lost `this` (ECX, MSVC `thiscall`) or a register argument, and to read jump tables. Works with only the executable. |
| `python3 tools/re/settings_map.py <regex>` | setting name → setting object. The value is at object + 4, which is what the code reads (`DAT_<object+4>` in Ghidra). |
| `python3 tools/re/settings_map.py --all [out.tsv]` | every setting (about 4,500), for `disasm.py`'s annotations |

## Method

1. Find an entry point: a string or setting name, an RTTI class, an original
   source file path left in the asserts, or an address from the NVSE / JIP
   LN NVSE headers (same build).
2. Read the function, check registers and tables in the disassembly, find
   its callers.
3. Write the behaviour up in `docs/<TOPIC>.md`: what happens, under which
   conditions, with addresses and settings (names and defaults). Say which
   parts are read from the code and which are inferred; list what is still
   open.
4. Implement it in the owning crate (rules in `world`, presentation in
   `viewer`) with a regression test built from `testdata` fixtures.
5. Update `docs/MILESTONES.md` and the topic table below.

## Topics

| topic | write-up | pull request | state |
| --- | --- | --- | --- |
| LookIK solver (head/neck turn) | `OPENING_LOOK_IK.md` | #1 docs, #2 code | implemented |
| Look anchor (Bip01 Head / palette / 0.9 × height) | `OPENING_LOOK_IK.md` "Look anchor" | #3 | implemented |
| Aim point, release, out-of-range targets | `OPENING_LOOK_IK.md` "Aim point" | #4 | implemented |
| Head-track target (slots, chooser, timers) | `HEAD_TRACK_TARGET.md` | #5 | implemented |
| FaceGen eye darting | `HEAD_TRACK_TARGET.md` "Eyes (FaceGen)" | #6 | implemented |
| Face menu (RaceSexMenu) | `RACE_SEX_MENU.md`, `FACE_CREATION.md` | #7 | race/sex fallbacks implemented; menu traced, not built |
| FaceGen controls (`SI.CTL`) | `FACEGEN_CONTROLS.md` | #9 | reader and slider/age/gender maths implemented; Randomize partly traced |

Pull requests #1 to #7 and #9 are stacked; merge them in that order.

## Next steps (M1, the face menu)

1. Done in #9: the `SI.CTL` reader and maths. Left: a check against the
   real file, Randomize's base face and scale (`00601830`), the preset faces
   (`00877a30`).
2. The menu (`ui::menus`) on the game's XML (`RSM_list_item_template`,
   `RSM_slider_option_template`); open handlers are listed in
   `RACE_SEX_MENU.md` "Still open".
3. The preview (camera, rotation, lighting).

Smaller open items: the BPND head angle (needs data), LookIK's initial
previous rotation, `004938e0` and process `+0x2d8`/`+0x30c`, head-track slot
5 setters, expressions.
