//! Head tracking ("LookIK"), the rules the game's look controller applies
//! to a head bone each update, read from `FalloutNV.exe` 1.4.0.525 (notes
//! and the remaining gaps in `docs/OPENING_LOOK_IK.md`).
//!
//! The controller (`00c7aa60` each update) first settles where to look
//! (`00c78160`: [`clamp_target`], [`is_behind`], [`behind_target`]), then
//! turns the head bone (`00c78610`): it takes the bone's pose in model
//! space, aims the bone's forward axis at the target inside a cone
//! ([`solve`], the solver `00ce0290`), reads the result back as the bone's
//! local rotation and eases from the rotation it applied last time towards
//! it by at most a fixed angle per update ([`ease`], `00c755e0`). A result
//! that isn't a unit quaternion is thrown away and the animated pose kept
//! ([`is_unit`], `00cb24c0`). Once the controller is told to stop it eases
//! back by a second step angle until a step is below a third angle
//! ([`shuts_off`]).
//!
//! Everything here is a plain rule over values the caller supplies: the
//! forward axis, the cone, the step angles and the bone's pose. Where the
//! game gets those for a given actor (the controller's set-up, `00c7f060` /
//! `00c79340`, and the `LookIK` settings' defaults) isn't traced yet, so no
//! values are given here.
//!
//! Quaternions are `[w, x, y, z]` like [`nif::anim::Quat`]; the game's are
//! stored `x, y, z, w`. Rotations compose as the Hamilton product, `a ⊗ b`
//! applying `b` first.

use nif::math::Vec3;

pub type Quat = [f32; 4];

pub const IDENTITY: Quat = [1.0, 0.0, 0.0, 0.0];

/// How far the stored target may lie from the head (`00c78160`'s end, the
/// constant at `011b05a8`): a target further away is pulled in along the
/// same direction, so only the direction from the head counts afterwards.
pub const TARGET_REACH: f32 = 5.0;

/// How far beside the head a target behind it is put (`00c78160`: the
/// constants at `0101712c` and `0102caf8`, along the model's X axis).
pub const BEHIND_OFFSET: f32 = 5.0;

/// The game's degrees-to-radians factor for the step and shut-off angles
/// (the double at `01023128`, `3f91df46a0000000`: π/180 rounded to a
/// float, then widened).
const DEGREES: f64 = 0.017_453_292_f32 as f64;

/// The tolerance the game compares quaternions and lengths with (the float
/// at `01017d00`).
const TOLERANCE: f32 = 0.001;

/// The solver's set-up (`hkaLookAtIkSolver::Setup`'s layout: the 0x40
/// bytes `00c78610` builds for `00ce0290`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    /// The bone's forward axis in its own frame (controller `+0x100` per
    /// mode).
    pub forward: Vec3,
    /// The cone's axis in model space: the controller's axis (`+0x110`)
    /// turned by a second bone's model rotation (`+0xf6`), normalized.
    pub limit_axis: Vec3,
    /// The cone's half angle, radians (`+0xf8`).
    pub limit_angle: f32,
}

/// Turns a bone so its forward axis points at `target` (`00ce0290` with
/// no eye offset and no range limits, as `00c78610` calls it): the
/// direction from the bone's model-space position to the target, pulled
/// back onto the cone's edge when it's outside the cone; then the
/// shortest turn from the bone's current forward to it, scaled by `gain`,
/// applied in model space (`turn ⊗ rotation`). Whether the target was
/// inside the cone.
pub fn solve(
    setup: &Setup,
    target: Vec3,
    gain: f32,
    bone_position: Vec3,
    bone_rotation: &mut Quat,
) -> bool {
    let mut to = normalize(sub(target, bone_position));
    let mut inside = true;
    if dot(setup.limit_axis, to) < setup.limit_angle.cos() {
        let axis = normalize(cross(setup.limit_axis, to));
        to = rotate(axis_angle(axis, setup.limit_angle), setup.limit_axis);
        inside = false;
    }
    let forward = rotate(*bone_rotation, setup.forward);
    let axis = normalize(cross(forward, to));
    let c = dot(forward, to);
    let angle = if c.abs() < 1.0 {
        c.acos()
    } else if c > 0.0 {
        0.0
    } else {
        std::f32::consts::PI
    };
    *bone_rotation = mul(axis_angle(axis, angle * gain), *bone_rotation);
    inside
}

/// One update's easing of a bone's local rotation (`00c755e0`): from
/// `previous` (the rotation applied last update) towards `solved`, by at
/// most `max_step_degrees`. Gives the rotation to apply (and keep as the
/// next update's `previous`) and the angle it turned, radians.
///
/// When the two differ by at most the tolerance in x, y and z, nothing is
/// eased: `solved` stays, and the angle is 0. Otherwise the turn between
/// them (`solved ⊗ previous⁻¹`, its angle `2·acos|w|`) is cut to the step
/// along the same axis when longer, applied to `previous` and normalized.
pub fn ease(previous: Quat, solved: Quat, max_step_degrees: f32) -> (Quat, f32) {
    if (1..4).all(|i| (solved[i] - previous[i]).abs() <= TOLERANCE) {
        return (solved, 0.0);
    }
    let mut turn = mul(solved, conjugate(previous));
    let mut angle = rotation_angle(turn);
    let step = (f64::from(max_step_degrees) * DEGREES) as f32;
    if angle > step {
        let flip = if turn[0] < 0.0 { -1.0 } else { 1.0 };
        let axis = normalize([turn[1] * flip, turn[2] * flip, turn[3] * flip]);
        turn = axis_angle(axis, step);
        angle = step;
    }
    (normalize_quat(mul(turn, previous)), angle)
}

/// Whether a head easing back to its rest pose stops tracking now
/// (`00c78610`'s end): while turning off, once the last step was below
/// `shut_off_degrees`. The game then forgets the eased rotation (back to
/// the identity, so the next start eases from the animated pose).
pub fn shuts_off(turning_off: bool, last_step: f32, shut_off_degrees: f32) -> bool {
    turning_off && f64::from(last_step) < f64::from(shut_off_degrees) * DEGREES
}

/// Whether a rotation is fit to apply (`00cb24c0`): its squared length
/// within the tolerance of 1.
pub fn is_unit(q: Quat) -> bool {
    let n: f32 = q.iter().map(|c| c * c).sum();
    (n - 1.0).abs() < TOLERANCE
}

/// The target kept for the head (`00c78160`'s end): pulled in to
/// [`TARGET_REACH`] of the head's position along the same direction.
pub fn clamp_target(target: Vec3, head: Vec3) -> Vec3 {
    let d = sub(target, head);
    let length = dot(d, d).sqrt();
    if length > TARGET_REACH {
        let n = normalize(d);
        [
            head[0] + n[0] * TARGET_REACH,
            head[1] + n[1] * TARGET_REACH,
            head[2] + n[2] * TARGET_REACH,
        ]
    } else {
        target
    }
}

/// Whether a target, in the actor's model space, lies behind it
/// (`00c78160`, kept at controller `+0x1a4`): its direction from the
/// model's origin points backwards (negative Y, the model's forward axis).
pub fn is_behind(target_model: Vec3) -> bool {
    normalize(target_model)[1] < 0.0
}

/// Where a tracking head looks instead of a target behind it (`00c78160`,
/// when not turning off): beside the head, [`BEHIND_OFFSET`] along the
/// model's X axis on the target's side (model-space positions).
pub fn behind_target(head_model: Vec3, target_model: Vec3) -> Vec3 {
    let side = if target_model[0] < 0.0 {
        -BEHIND_OFFSET
    } else {
        BEHIND_OFFSET
    };
    [head_model[0] + side, head_model[1], head_model[2]]
}

/// The rotation by `angle` radians about the unit `axis` (`00cb2450`).
pub fn axis_angle(axis: Vec3, angle: f32) -> Quat {
    let (s, c) = (angle * 0.5).sin_cos();
    [c, axis[0] * s, axis[1] * s, axis[2] * s]
}

/// The Hamilton product: `b` then `a`.
pub fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// A vector turned by a unit quaternion.
pub fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let p = mul(mul(q, [0.0, v[0], v[1], v[2]]), conjugate(q));
    [p[1], p[2], p[3]]
}

fn conjugate(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

/// The angle a quaternion turns by (`00c74c20`): `2·acos|w|`, 0 when
/// `|w|` reaches 1.
fn rotation_angle(q: Quat) -> f32 {
    let w = q[0].abs();
    if w >= 1.0 {
        0.0
    } else {
        2.0 * w.acos()
    }
}

fn normalize_quat(q: Quat) -> Quat {
    let n: f32 = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    if n > 0.0 {
        q.map(|c| c / n)
    } else {
        q
    }
}

/// A unit vector, or zero for a zero vector (as the game's masked
/// reciprocal square root gives).
fn normalize(v: Vec3) -> Vec3 {
    let n = dot(v, v);
    if n == 0.0 {
        return [0.0; 3];
    }
    let r = 1.0 / n.sqrt();
    [v[0] * r, v[1] * r, v[2] * r]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn close(a: &[f32], b: &[f32], eps: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < eps)
    }

    /// Forward +Y, a cone about +Y.
    fn setup(limit_degrees: f32) -> Setup {
        Setup {
            forward: [0.0, 1.0, 0.0],
            limit_axis: [0.0, 1.0, 0.0],
            limit_angle: limit_degrees.to_radians(),
        }
    }

    #[test]
    fn aims_forward_at_a_target_inside_the_cone() {
        let mut r = IDENTITY;
        let inside = solve(&setup(60.0), [10.0, 10.0, 0.0], 1.0, [0.0; 3], &mut r);
        assert!(inside);
        let f = rotate(r, [0.0, 1.0, 0.0]);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(&f, &[s, s, 0.0], 1e-5), "{f:?}");
        // A 45° turn clockwise seen from +Z (towards +X): about −Z.
        assert!(close(&r, &axis_angle([0.0, 0.0, -1.0], FRAC_PI_4), 1e-5));
    }

    #[test]
    fn a_target_outside_the_cone_is_pulled_onto_its_edge() {
        let mut r = IDENTITY;
        // Straight to the side: 90° from the axis, the cone allows 30°.
        let inside = solve(&setup(30.0), [5.0, 0.0, 0.0], 1.0, [0.0; 3], &mut r);
        assert!(!inside);
        let f = rotate(r, [0.0, 1.0, 0.0]);
        let (s, c) = 30f32.to_radians().sin_cos();
        assert!(close(&f, &[s, c, 0.0], 1e-5), "{f:?}");
    }

    #[test]
    fn measures_from_the_bone_and_turns_in_model_space() {
        // The bone already turned 90° about Z (forward now −X) and standing
        // at (0, 0, 10): a target at (0, 5, 10) is straight +Y from it.
        let start = axis_angle([0.0, 0.0, 1.0], FRAC_PI_2);
        let mut r = start;
        let cone = Setup {
            limit_axis: [-1.0, 0.0, 0.0],
            limit_angle: PI,
            ..setup(0.0)
        };
        solve(&cone, [0.0, 5.0, 10.0], 1.0, [0.0, 0.0, 10.0], &mut r);
        assert!(close(&rotate(r, [0.0, 1.0, 0.0]), &[0.0, 1.0, 0.0], 1e-5));
        // The turn is applied on the left: r = turn ⊗ start.
        let turn = mul(r, conjugate(start));
        assert!(close(&turn, &axis_angle([0.0, 0.0, -1.0], FRAC_PI_2), 1e-5));
    }

    #[test]
    fn gain_scales_the_turn() {
        let mut r = IDENTITY;
        solve(&setup(90.0), [10.0, 0.0, 0.0], 0.5, [0.0; 3], &mut r);
        assert!(close(&r, &axis_angle([0.0, 0.0, -1.0], FRAC_PI_4), 1e-5));
    }

    #[test]
    fn a_target_straight_behind_gives_the_games_degenerate_turn() {
        // Forward and the direction are opposite: no axis (zero), half a
        // turn: the quaternion is all zeros, which the unit check rejects.
        let cone = Setup {
            limit_angle: PI,
            ..setup(0.0)
        };
        let mut r = IDENTITY;
        solve(&cone, [0.0, -10.0, 0.0], 1.0, [0.0; 3], &mut r);
        assert!(!is_unit(r));
    }

    #[test]
    fn easing_turns_at_most_the_step_and_reaches_the_target() {
        let solved = axis_angle([0.0, 0.0, 1.0], 30f32.to_radians());
        let (q, a) = ease(IDENTITY, solved, 10.0);
        assert!((a - 10f32.to_radians()).abs() < 1e-5);
        assert!(close(
            &q,
            &axis_angle([0.0, 0.0, 1.0], 10f32.to_radians()),
            1e-5
        ));
        let (q, _) = ease(q, solved, 10.0);
        let (q, a) = ease(q, solved, 10.0);
        assert!(close(&q, &solved, 1e-4), "{q:?}");
        assert!((a - 10f32.to_radians()).abs() < 1e-4);
        // Within a step: straight there, by the remaining angle.
        let near = axis_angle([0.0, 0.0, 1.0], 4f32.to_radians());
        let (q, a) = ease(IDENTITY, near, 10.0);
        assert!(close(&q, &near, 1e-5));
        assert!((a - 4f32.to_radians()).abs() < 1e-4);
    }

    #[test]
    fn easing_takes_the_short_way_round() {
        // The same rotation written with w < 0: no turn needed.
        let q = axis_angle([1.0, 0.0, 0.0], 0.3);
        let (_, a) = ease(q.map(|c| -c), q, 10.0);
        assert!(a < 1e-3, "{a}");
        // 350° about +Z is 10° the other way: one 10° step reaches it.
        let target = axis_angle([0.0, 0.0, 1.0], 350f32.to_radians());
        let (r, a) = ease(IDENTITY, target, 10.0);
        assert!((a - 10f32.to_radians()).abs() < 1e-4);
        assert!(close(
            &rotate(r, [1.0, 0.0, 0.0]),
            &rotate(target, [1.0, 0.0, 0.0]),
            1e-4
        ));
    }

    #[test]
    fn nearly_equal_rotations_are_left_alone() {
        let a = axis_angle([0.0, 1.0, 0.0], 0.5);
        let b = [a[0] + 0.01, a[1] + 0.0005, a[2] - 0.0005, a[3]];
        // Only x, y and z are compared; w may differ.
        assert_eq!(ease(a, b, 1.0), (b, 0.0));
    }

    #[test]
    fn degrees_factor_is_the_games_double() {
        assert_eq!(DEGREES.to_bits(), 0x3f91_df46_a000_0000);
    }

    #[test]
    fn shutting_off_needs_the_turning_off_flag_and_a_small_step() {
        assert!(shuts_off(true, 0.5f32.to_radians(), 1.0));
        assert!(!shuts_off(true, 2f32.to_radians(), 1.0));
        assert!(!shuts_off(false, 0.0, 1.0));
    }

    #[test]
    fn far_targets_are_pulled_in_along_their_direction() {
        let head = [1.0, 2.0, 3.0];
        assert_eq!(clamp_target([1.0, 2.0, 7.0], head), [1.0, 2.0, 7.0]);
        let t = clamp_target([1.0, 102.0, 3.0], head);
        assert!(close(&t, &[1.0, 7.0, 3.0], 1e-5), "{t:?}");
    }

    #[test]
    fn behind_is_negative_y_and_goes_to_the_targets_side() {
        assert!(is_behind([3.0, -1.0, 50.0]));
        assert!(!is_behind([3.0, 0.0, 50.0]));
        assert_eq!(
            behind_target([0.0, 1.0, 9.0], [-4.0, -1.0, 0.0]),
            [-5.0, 1.0, 9.0]
        );
        assert_eq!(
            behind_target([0.0, 1.0, 9.0], [4.0, -1.0, 0.0]),
            [5.0, 1.0, 9.0]
        );
    }
}
