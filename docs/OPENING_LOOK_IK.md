# Opening Doc look IK trace

Research status, 2026-10-04. FNV executable addresses below are 1.4.0.525
(Steam). Earlier sections used the `nv-re/decomp/codex-m1` Ghidra project; the
2026-10-04 trace below used the private Decompiling-FNV exports
(`research/lookik`, Ghidra 12.1.4: five roots, callees two levels deep, plus
disassembly and data bytes). The solver, easing and target rules are now
implemented as pure functions in `world::look_ik` (12 tests); nothing calls
them yet, because the per-actor set-up and setting values are still untraced.

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
That helper subtracts matrix translation from target xyz, applies the inverse
controller rotation to get a local direction, and normalizes it. Thus the
target-to-local-direction transform is established and this is the actor's
LookIK path, not a generic look-at inference.

**Correction (2026-10-04, disassembly).** `00c78160`'s `+0x1a4` test is the
sign of the target's model-space **Y** (forward) component after
normalization (`00c781ea`-`00c7826f`: dot with (0, 1, 0), FCOMP against 0): it
marks a target *behind* the actor, not below it. The constant at `011b05a8`
(5.0, a plain static float among RTTI descriptors, read only here) does not
limit per-update movement: the routine's tail pulls the stored target to
within 5 units of the head bone's position along the same direction, so only
the direction from the head matters afterwards. With `+0xb2` clear and the
target behind, the target becomes the head's model-space position offset
±5 along model X (the target's side; `0101712c` 5.0, `0102caf8` −5.0). With
`+0xb2` set, the target is a fixed direction (`+0x150`) turned by the head
bone and the controller, ×3 (`01017718`), from the head.

## Rotation chain (traced 2026-10-04)

Helpers (Havok `hkQsTransform` = translation, rotation quaternion xyzw,
scale; `hkaPose` = skeleton, local array `[1]`, model array `[4]`, flags `[7]`):
`00c7f840` inverse-transforms a point, `00c7f7a0` transforms one, `00cda180`
/`00cdad80` give a bone's model-space transform, `00cda8e0` its local one.
`00cb2450` builds an axis-angle quaternion (the angle ×0.5 at `01011588`
before sin/cos), `00c66320` is the Hamilton product (ECX output, first stack
argument the left operand, checked in `00c75754`), `00c74ac0` is `a ⊗ b⁻¹`,
`00c74c20` gives `2·acos|w|`. `00ec9f70`/`00eca0a0`/`00eca1d0` are the CRT's
x87 cos/sin/acos (argument in ST0, hidden in the C).

`00c7aa60(controller, worldXf)`: update the target (`00c78160`); if `+0x181`
or not `+0x190`, set `+0x194`=1 and solve mode 1 (`00c78610(1, worldXf)`);
then multiply the head's model rotation on the right by `+0xc0`
(`00c7aad9`: `R = R ⊗ q(+0xc0)`) and re-propagate children. With `+0x190`
set it also solves mode 0 with the head's world transform and keeps two
asin angles (`+0x19c`, `+0x1a0`, π/2 − acos at `01030f38`) against limits
`+0x134`/`+0x138`.

`00c78610(mode, xf)` per mode block (`this + 0xf0 + mode·0x50`: pose
pointer, bone `+4` (the BPNI head for mode 1, `+0x144`), second bone `+6`,
limit angle `+8`, forward `+0x10`, axis `+0x20`, previous rotation `+0x30`,
eased flag `+0x40`) builds the solver set-up on the stack in
`hkaLookAtIkSolver::Setup` layout: forward (`+0x100`), eye offset (zero,
unless `+0x190` and mode 1: the head's local translation), limit axis
(`+0x110` turned by the second bone's model rotation, normalized), limit angle
(`+0xf8`). The target is brought into model space with `00c7f840`. If the
previous rotation `+0x120` equals the static at `01267e30` (within 0.001 in
x, y, z) it is first set to the bone's current local rotation. Then:

1. `00ce0290(setup, target, gain 1.0, bone model transform, no limits)`:
   direction = normalize(target − bone position); outside the cone
   (dot(axis, direction) < cos(limit)) it is replaced by the axis turned by
   the limit angle towards it, and the result flag cleared; forward =
   bone rotation applied to the forward axis; turn = axis-angle(normalize(
   forward × direction), acos(forward · direction) × gain) (0 or π at
   |cos| ≥ 1); the bone's model rotation becomes `turn ⊗ R` (`00ce0c1c`).
   The eye-offset branch (`|eye| > 0`) is not traced.
2. The bone's local rotation is re-derived from the solved model pose.
3. If the eased flag is set, `00c755e0` eases: nothing when the solved and
   previous rotations agree within 0.001 in x, y, z (returns 0); else
   δ = solved ⊗ previous⁻¹, angle = 2·acos|δw|, and if the angle exceeds
   the step (`01267d24`, or `01267d30` while `+0xb2`; degrees × the double at
   `01023128`), δ = axis-angle(normalize(±δxyz, sign of δw), step). The
   bone's local rotation and `+0x120` become normalize(δ ⊗ previous)
   (`005611c0`). It returns the angle turned. A fixed step per update, not
   scaled by frame time.
4. `00cb24c0`: if the local rotation's squared length is not within 0.001 of
   1 (or `00c7f500` fails), the animated local transform is restored.
5. The eased flag is set. While `+0xb2` and the returned step is below
   `01267d3c` degrees, `+0x120` returns to the identity (`010c71b0`) and
   `+0xb2`/`+0xb3` clear (mode 0 or no `+0x190`), or the mode count drops.

**Settings (inference, strong).** Nothing decompiled writes `01267d24`,
`01267d30` or `01267d3c`; they are runtime values. The ragdoll block copied at
`00c7e9a0` (bhkRagdollShareData) reads twelve consecutive 12-byte `Setting` values `01267c7c`-
`01267d00`, matching the twelve float settings `fHierarchyGain:RagdollAnim` to
`fSnapMaxAngularDistance:RagdollAnim` in string order. Continuing that order
gives `fMaxTrackingDist:LookIK` 01267d0c, `fMinTrackingDist` 01267d18,
`fAngleMax` 01267d24 (normal step), `fAngleMaxEase` 01267d30 (step while
turning off), `fEaseAngleShutOff` 01267d3c. Their registration (static
initializers) is not in the Ghidra output, so the defaults are unknown.

## Unresolved, exact next trace

The mode-1 selected node is BPNI (`Bip01 Head` for DefaultBodyPartData; see
the earlier assembly note at `0063d04a`, `005e427c`, `005e42b0`). Still
needed before the viewer can call `world::look_ik`:

1. The set-up values per mode: forward `+0x100`, axis `+0x110`, limit angle
   `+0xf8`, second bone `+0xf6`, and `+0xc0`, written by the controller's
   constructor `00c7f060` and initializer `00c7de60`/`00c79340`. Export
   those three with callees.
2. The LookIK setting defaults: the static initializers that register the
   settings named at `010c5034`-`010c5090` (Ghidra missed them; find them
   through the `__xc_a`/`__xc_z` initializer table or by searching the
   decrypted code for pushes of those string addresses), plus any `[LookIK]`
   section in the installation's INI files.
3. When `+0x190`, `+0xb2`, `+0x181` and `+0x194` are set (`008a3100` and
   its callers), and the target selection's use of the tracking distances.
4. The eye-offset branch of `00ce0290` (needed only when `+0x190`).

The opening's `SayTo Player` is a scripted talk event, not a dialogue package;
it is an explicit reason for Doc to have the player as a target, but the
viewer currently has no native actor look-target/effect wiring. Random
`SitChairRelax` idle variation is separate from the absent gaze behavior.
`viewer/src/faces.rs` documents that head-turn and eye tracking are absent;
the seated body-turn guard in `viewer/src/ai.rs` must remain intact.

## Evidence locations

Decompilation and disassembly: `%USERPROFILE%\\nv-re\\decomp\\codex-m1`, and
Decompiling-FNV `research/lookik` (branch
`claude/fallout-new-vegas-decompile-dgw3v2`, private; not copied here)
(`008a3100`, `008a3b70`, `00c7f060`, `00c7d630`, `00c7aa60`, `00c78160`,
`00c7f840`, `00c78610`). Opening route and package evidence:
`docs/OPENING.md` and `%USERPROFILE%\\nv-re\\work\\codex-m1\\animation-repro.log`.
