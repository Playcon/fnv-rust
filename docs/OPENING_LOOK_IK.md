# Opening Doc look IK trace

Research status, 2026-10-04. FNV executable addresses below are build
1.4.0.525 (Steam, Steamless-unpacked), the same addresses as the
`nv-re/decomp/codex-m1` Ghidra project. The solver, limits, setup values
and on/off logic are traced (see "Native solver"); the head chain is
implemented in `world::look_ik` and the viewer (see "Implementation").
Not yet compared with the original game.

## Confirmed native path

Doc's actor update `008a3100` chooses a tracked actor, obtains its anchor from
the target actor's virtual `+0x194` method, and calls `008a3b70` on the actor's
controller stored at actor `+0xac`. `008a3b70` writes the supplied xyz into
controller `+0xd0` (fourth component zero), then invokes `00c75580` to enable
the look system. The controller is created by `0087e130` through constructor
`00c7f060`; its initialization path includes a specific LookIK initialization
failure diagnostic.

The visual consumer is verified, including register/stack roles:
`00c7d630` has the controller in ECX and passes `this+0x10` on the stack to
`00c7aa60` (`dis_00c7d630.txt`, `00c7d744-00c7d747`). The callee keeps ECX as
the controller and calls `00c78160` with that same `this`. `00c78160` reads
controller `+0xd0`, alongside its matrix at `+0x10`, and calls `00c7f840`.
That helper is the inverse of a translation/rotation/scale transform: it
subtracts the translation, applies the inverse rotation and divides by the
scale. It does not normalize (corrected 2026-10-04; `00c7f840` has no
length step), so its result is a position, not a direction. Thus the
target-to-local-direction transform is established and this is the actor's
LookIK path, not a generic look-at inference.

`00c78160` prepares the target point each update; see "Aim point" below
(decoded 2026-10-04; the earlier "below target" reading was wrong: it tests
whether the target is *behind*).

`00c7aa60` calls `00c78610` with `param_2=1` for the active result, then
passes a second `param_2=0` call using a temporary matrix. `00c78610` reads the
same target at controller `+0xd0`, selected skeleton index `+0x144`, and
per-mode index at `+0xf4`/`+0xf6`. Its body builds and composes quaternion
transforms and writes node transforms, but the Ghidra output is too large and
ambiguous to claim the final rotation formula or limits without tracing the
exact output writes and helpers.

### Head node (traced 2026-10-03)

The controller constructor initializes short `+0x144` to `-1`. The setter is
`00c79340`: it writes the result of `00cdd390` to
`controller + mode * 0x50 + 0xf4`; mode1 therefore writes `+0x144`.
`00c7de60` calls it with mode1. Its sole caller is `0087e575`, passing the
string returned through `0063d040 -> 00559450` from the body-part iterator
at local `-0x290`. **Assembly confirms the name is BPNI**: `0063d04a`
adds 0x14 before the string getter; Ghidra's C output omitted that adjustment.
The loader `005e427c` checks BPNI and `005e42b0` writes its string at +0x14.
The iterator (`005e5320`) scans 15 BPTD part slots +0x34, selecting flag 0x02;
`0087e940` selects flag 0x20 for LookIK. DefaultBodyPartData0000001D's Head
entry has flags 0x3b and BPNI `Bip01 Head`. Thus mode1's selected node is
data-driven, and is Bip01 Head for the standard human body data. BPNN is
different (Bip01 Neck1, stored at +4); do not confuse the two. Evidence:
look-node-getter.txt, look-body-strings.txt and look-body-data.txt in
nv-re/work/codex-m1.

## Native solver (traced 2026-10-04)

Each item cites the function and, where it matters, the instruction address.
Quaternions are (x, y, z, w); `a ⊗ b` is the standard product.

### Per-mode block

The controller holds two look chains ("modes") of `0x50` bytes each at
`controller + mode*0x50`:

| Offset | Meaning |
| --- | --- |
| `+0xf0` | the pose it works on (Havok `hkaPose` layout: skeleton, local-space array, model-space array `+0x10`, per-bone flags `+0x1c`; flag 1 = local stale, 2 = model stale) |
| `+0xf4` | look bone index (short) |
| `+0xf6` | reference bone index: the look bone's parent (short) |
| `+0xf8` | cone half-angle, radians |
| `+0x100` | forward axis in the look bone's space |
| `+0x110` | cone axis in the reference bone's space |
| `+0x120` | previous local rotation of the look bone; `(0,0,0,0)` means none yet |
| `+0x130` | step limiter armed (byte) |

Mode 1 is the head (`+0x144` look bone = the BPNI node, `+0x148` angle,
`+0x150` forward, `+0x160` axis, `+0x170` previous, `+0x180` armed).

### Setup values

* `00c79340(pose, node name, mode)`: look bone = the named bone; reference
  bone = its parent. Using model-space forward `(0,1,0)` and the pose at
  setup time, forward axis = look bone's model rotation⁻¹ · `(0,1,0)`,
  normalized; cone axis = reference bone's model rotation⁻¹ · `(0,1,0)`.
  The mode's pose pointer is set to controller `+0x88`. For mode 1 only,
  if the object at `(controller+0x2a4)+0x1c` exists, its `+0x10` vector
  replaces the cone axis and its `+0x20` vector replaces the forward axis.
  What that object is has not been traced.
* Head cone angle: `0087e130` (`0087e57a`–`0087e5a9`) reads the float at
  body part `+0x70` and passes it × π/180 to `00607810(1, angle)`, which
  writes `+0xf8 + 1*0x50`. The loader copies the 84-byte `BPND` to part
  `+0x5c` (`005e42fe`–`005e430a`), so `+0x70` is **`BPND` byte 20**, the part's
  tracking max angle in degrees (`world::body_parts` does not expose it
  yet). Same part as the BPNI node above. Then `005ba4f0(1)` sets
  `+0xb1 = +0xb0 && 1`; if that is false the game logs "AI: Could not
  initialize LookIK system for %s."
* `00c7de60` also sets up mode 0 as a separate two-bone pose (stored at
  `+0x54`, used as mode 0's `+0xf0`): look bone 1, reference bone 0, forward
  and cone axes `(0,1,0)`. No writer of mode 0's angle `+0xf8` was found.

### Per-update order (`00c7aa60`)

1. `00c78160` prepares the aim point (next section).
2. Head pass `00c78610(1, worldFromModel)`. It runs when `+0x181` is set or
   `+0x190` is clear.
3. The extra head rotation at `+0xc0` is applied on top: head model rotation
   := head model rotation ⊗ q(`+0xc0`), then the head's descendants are
   marked for recomputation. It is always identity (below), so this changes
   nothing.
4. Only when `+0x190` is set: mode 0 is solved in the frame worldFromModel ⊗
   head model transform (`00c7ae96`), and `+0x181` records its result.

The only direct write of `+0x190` found by an executable-wide scan is in the
constructor `00c7f060` (`00c7f289`, zero). So in this build step 4 and the
mode-1 eye-offset branch appear never to run, and the head pass runs every
update. A write through a computed address would not show in that scan.

### Aim point (`00c78160`, decoded 2026-10-04)

With `+0x190` clear (always, see above), on the pose the update starts from:

1. local = inverse(worldFromModel) · target `+0xd0` (`00c7f840`), normalized;
   `+0x1a4` := local.y < 0, the target is **behind** the actor (the model's
   forward is +Y). The comparison is `00c7824f`–`00c78269`.
2. If easing (`+0xb2`, branch at `00c78283`): target := head world position
   + 3 × (world rotation `+0x20` · head model rotation · forward axis
   `+0x150`). The point is straight ahead of the head as the animation has
   it, so easing turns the head back toward the animation, at
   `fAngleMaxEase` per update.
3. Else if behind (`00c7842e`): target := worldFromModel · (head model
   position + (s, 0, 0)), s = +5 (`0101712c`), or −5 (`0102caf8`) when
   local.x < 0. The head looks over the shoulder on the target's side
   (then the cone limits it).
4. Else the stored target is kept.
5. Always (`00c784db` on): if the target is farther than the global
   `011b05a8` (5.0 in the executable's data) from the head's world
   position, it is moved onto that line at that distance.

The result is written back to `+0xd0`, so when nothing refreshes the
target (see "On and off") the look stays on that nearby point. The head
position is the look bone's model transform (`+0x144` in the pose at
`+0x88`, recomputed by `00cda180` when flagged stale) under
worldFromModel (`+0x10`).

### Solve (`00c78610` → `00ce0290`)

* Target in model space = inverse(worldFromModel) · target `+0xd0`
  (`00c7f840`).
* Limit axis = normalize(reference bone's current model rotation · cone
  axis); forward = `+0x100`; eye offset = zero; limit angle = `+0xf8`.
* If the previous rotation `+0x120` is `(0,0,0,0)`, it is first set to the
  look bone's current local rotation. The pre-solve local transform is kept.
* `00ce0290` is called with gain **1.0** (`00c78cf3`), the look bone's model
  transform (fetched with propagate-to-children) and no range limits
  (`push 0` at `00c78ce8`). It matches Havok's look-at IK step:
  * dir = normalize(target − look bone model translation);
  * if dir · limit axis < cos(limit angle), dir is swung onto the cone
    boundary toward the target and the result flag is false;
  * f = look bone model rotation · forward; rotation axis =
    normalize(f × dir), angle = acos(f · dir) × gain (0 or π at the
    extremes);
  * look bone model rotation := q(axis, angle) ⊗ old model rotation.
* Step limit, when `+0x130` is set: `00c755e0` (below), on the new local
  rotation.
* Validity (`00cb24c0`, `00c78fb5`): if the look bone's model rotation is not
  finite with |q|² within 0.001 of 1, the pre-solve local transform is
  restored and the descendants marked stale.
* `+0x130` := 1. Ease shut-off (`00c792bd`): if easing (`+0xb2`) and the
  returned step angle < `fEaseAngleShutOff` × π/180, previous := identity;
  for mode 0 or when `+0x190` is clear, `+0xb2` and `+0xb3` are cleared
  (look off).

### Step limiter (`00c755e0(mode, new local rotation)`)

If the new rotation's x, y, z each differ from the previous one by at most
0.001, nothing changes and it returns 0. Otherwise:
change = new ⊗ previous⁻¹; angle = 2·acos(change.w); limit = `fAngleMax`
(or `fAngleMaxEase` while `+0xb2` is set) × π/180. If angle > limit, the
change is replaced by a rotation of `limit` about the same axis. The result,
normalize(change ⊗ previous), is written as the look bone's **local**
rotation and as the new previous rotation. It returns the step applied
(angle, or limit when clamped) on the x87 stack (`00c75791`).

So the head turns at most 3.5° per update while tracking, 1° while easing.

### Settings

Defaults read from the static initializers (`00fbcdd0`, `00fbd0a0`–`00fbd160`).
A setting object is `{vtable, value, name}`; the code reads `object + 4`.

| Setting | Default | Value address | Used by |
| --- | --- | --- | --- |
| `bLookIK:RagdollAnim` | true | `01267c4c` | `00c7d630`, with `+0xb3` |
| `fMaxTrackingDist:LookIK` | 1500 units | `01267d0c` | `008a3c10` |
| `fMinTrackingDist:LookIK` | 12 units | `01267d18` | `008a3c30` |
| `fAngleMax:LookIK` | 3.5° | `01267d24` | `00c755e0` |
| `fAngleMaxEase:LookIK` | 1.0° | `01267d30` | `00c755e0` |
| `fEaseAngleShutOff:LookIK` | 0.5° | `01267d3c` | `00c78610` |

### On and off (Doc's update `008a3100`)

* Tracks when the actor has a look target and fMinTrackingDist < distance ≤
  fMaxTrackingDist (`008a3c30`, `008a3c10`): easing off (`008a3bf0(0)`),
  `00c75580(1)`, then `008a3b70` with the target's `+0x194` anchor, which
  stores the target xyz at `+0xd0` (w = 0). The two points the distance is
  measured between are the two actors' positions: the target's from its
  virtual `+0x1f4` (`008a3100`, stored in locals `-0x10..-0x8`) and the
  looking actor's from the same virtual.
* A target outside those distances changes nothing: both comparisons jump
  to the end of the function (`008a3a72`, `008a3a83` → `008a3b2a`). The
  look stays on, aiming at the point it stored (corrected 2026-10-04; the
  earlier text had it ease out).
* With no usable target (no controller, no target, the target itself, or
  the conditions at `008a3100`'s `local_15`), if the look is on (`+0xb3`)
  and not already easing, easing is switched on (`008a3bf0(1)`). The aim
  point is then straight ahead of the animated head (see "Aim point"), so
  the head returns toward the animation at 1° per update; once a step is
  under 0.5° (`00c792b0`–`00c79312`), previous := identity, and `+0xb2` and
  `+0xb3` are cleared. The limiter stays armed (`00c792a9`).
* `00c75580(on)`: `+0xb3` := `+0xb1 && on`, unless `+0x43` is set, in which
  case `+0xb3` is left as is. Off also resets both modes' previous rotation
  to identity and arms both step limiters.
* `00c7d630` runs the update only when `bLookIK:RagdollAnim` and `+0xb3`.

### Inputs that never change (traced 2026-10-04)

Each of these was checked by scanning the executable for writes (direct
stores to the offset, and in the second case stores through a register
loaded from controller `+0x2a4`). A write through a computed address
wouldn't show.

* Extra head rotation `+0xc0`: the constructor stores `(0,0,0,1)`
  (`00c7f1ee`–`00c7f201`, constant `010c71b0`, identity in Havok's x,y,z,w
  order). No other store; `00c7aa60` only reads it (`00c7aaca`). Identity.
* Hold flag `+0x43`: stored only as zero, by the constructor (`00c7f24a`)
  and the resets `00c7a8d0`, `00c7c150` and `00c7d630` (`00c7d680`). So
  `00c75580(on)` always sets `+0xb3` := `+0xb1 && on`.
* Axis override `(+0x2a4)+0x1c`: `+0x2a4` is the controller's
  `bhkRagdollShareData` (vftable `010c4b74`, created or shared by
  skeleton name in `00c7e9a0`, which zeroes its `+0x1c`). Its `+0x1c` is
  only created in `00c7de60` after the `+0x190` test (`00c7deb9`–`00c7deca`),
  so the head's forward and cone axes are never overridden.

### Look anchor (virtual `+0x194`)

* `PlayerCharacter` (`00952ff0`): unless `+0x64a` is set, the world
  position (`node + 0x8c`, `0045bb80`) of the node in global `011e07d0`,
  which `0094e1d0` looks up by the name stored in `011c626c`: the
  `Camera1st` node (`004b8b99`–`004b8bac`). Otherwise as `Actor`.
* `Actor` (`008a2fa0`, traced 2026-10-04), in order:
  1. The first node of the bone cache from virtual `+0x1e8` (`004ab230(0)`
     reads cache `+8`). `Character` returns its cache at `+0x1b4`
     (`005d9f90`); `004aad00` fills it from the root's `Bip01` and the five
     names at `01188b74`: `Bip01 Head`, `Weapon`, `Bip01 L ForeTwist`,
     `Bip01 Spine2`, `Bip01 Neck1` (flag byte at `+4+8i`, node at `+8+8i`;
     a missing one logs "MODELS: Missing bone '%s' for '%s'"). So the first
     node is `Bip01 Head`. `Creature`'s `+0x1e8` (`00acbb70`) returns none.
  2. Failing that, the 3D root's (virtual `+0x1d0`) time controller of
     type `NiControllerManager` (RTTI `011f36ac`, found by `00a5c570`), its
     object palette (`00559450`), and the palette's node (virtual `+0x8c`)
     named by `011c61ac`, which `004b7920` sets to `Bip01 Head`.
  3. With a node: its world translation (`+0x8c`). If the actor has a look
     controller (`+0xac`), `00c757b0` would replace z with the controller's
     own head position, but only while controller `+0x190` is set (see
     above), so not in this build.
  4. Without one: the position (virtual `+0x1f4`) with z raised by 0.9
     (double `0106b9e8`) × the height from `008853a0`: the bounds' z extent
     (virtual `+0x1dc` max − `+0x1d8` min; `MobileObject` takes them from
     the character controller when there is one) × `00567400`'s scale (the
     reference's `+0x3c` × `TESNPC +0x1f4` for NPCs, form type `0x2a`, or
     `00567470` for creatures, `0x2b`). The process caches the height at
     `+0x42c` (`00885490`/`008854b0`).

## Still open

* The `BPND` byte 20 value of `DefaultBodyPartData0000001D`'s Head part
  (read it with the inspection tools).
* Whether the pose each update starts from is the animation's (the
  implementation assumes so; easing back depends on it).
* What `PlayerCharacter +0x64a` is (it sends the player's anchor down
  `Actor`'s path).
* What sets the controller's previous rotation and step limit before the
  first update (see "Implementation").
* An in-game comparison of tracking, easing and release on Doc.

## Implementation (2026-10-04)

`world::look_ik` implements the head chain as traced: settings with the
game's defaults (INI overrides), set-up from the head-tracking part
(`world::body_parts::BodyPartData::head_tracking_part`: flags `0x02` and
`0x20`, its `BPNI` bone and `BPND` byte 20), the tracking decision, the
solve, the step limit, the validity check, propagation to the bones below
and the ease-out. The viewer (`viewer/src/look.rs`) runs it in
`animate_actors` after the animation pose, with the player's eye (the
camera, standing in for `Camera1st`) as the target and the actors'
positions for the distance, once per frame as the game does per update.

Mode 0 is left out (it needs `+0x190`). The `+0xc0` rotation, `+0x43` and
the axis override are left out because they never change (see "Inputs
that never change"). The aim point (`00c78160`) is implemented
(`HeadLook::aim`): easing aims ahead of the animated head, a target behind
gives the over-the-shoulder point, and the kept target is brought within 5
of the head. A target out of range leaves the look as it is.

Other actors are looked at by their `Bip01 Head`
(`world::look_ik::anchor`, `ANCHOR_BONE`): the viewer keeps each living
actor's head position after posing it (`look::record`), and an actor that
looks at someone posed later in the same frame sees the previous frame's
head (inference: the game reads the node's world transform, updated after
the animation, and its actor update order isn't traced). The 0.9 × height
fallback is in the core (`anchor`, `actor_height`) but the viewer doesn't
know actors' bounds, so an actor without the bone isn't looked at; nor is
a dead one.

Two choices are inferences, labelled in the code:

* The set-up pose is the skeleton's own pose (`nif::posed` with no
  sequence); which pose the game has at `0087e130` isn't traced.
* The controller starts with no previous rotation and the step limit
  armed. The constructor writes neither; the solver's "previous is
  `(0,0,0,0)`" branch only does anything when the limit is armed in the
  same update, and every reset path writes identity, never zero.

Tests in `world::look_ik` cover the defaults and INI, set-up, the distance
bounds (and that an out-of-range target changes nothing), a 3.5° step per
update and convergence, the cone clamp, the over-the-shoulder point for a
target behind, the 5-unit reach, bones below following, the
world-to-skeleton transform, easing back to the animation at 1° and the
shut-off, the setting and on/off gates, and an invalid target leaving the
head alone. `world::body_parts` tests the new `BPNI` and angle fields.

## Opening context

The opening's `SayTo Player` is a scripted talk event, not a dialogue package;
it is an explicit reason for Doc to have the player as a target, but the
viewer currently has no native actor look-target/effect wiring. Random
`SitChairRelax` idle variation is separate from the absent gaze behavior.
`viewer/src/faces.rs` documents that head-turn and eye tracking are absent;
the seated body-turn guard in `viewer/src/ai.rs` must remain intact.

## Evidence locations

Decompilation and disassembly: `%USERPROFILE%\\nv-re\\decomp\\codex-m1`
(`008a3100`, `008a3b70`, `00c7f060`, `00c7d630`, `00c7aa60`, `00c78160`,
`00c7f840`, `00c78610`). Opening route and package evidence:
`docs/OPENING.md` and `%USERPROFILE%\\nv-re\\work\\codex-m1\\animation-repro.log`.

The 2026-10-04 solver trace (decompiled C and disassembly for `00c78610`,
`00c7aa60`, `00c755e0`, `00c78160`, `00c7f840`, `00c7f060`, `00c7de60`,
`00c79340`, `00c7d630` and their callees two levels deep, plus the setting
initializers) is kept outside this repository, in the contributor's private
research repository under `research/lookik` and `research/lookik-setup`.
