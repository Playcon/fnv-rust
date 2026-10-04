//! Head tracking ("LookIK"): how an actor's head turns toward whom it looks
//! at. Traced from `FalloutNV.exe` 1.4.0.525; every address and the
//! reasoning behind it are in `docs/OPENING_LOOK_IK.md`.
//!
//! The game keeps a look controller per actor (`00c7f060`). Its head chain
//! ("mode 1") turns the head-tracking body part's `BPNI` bone (`Bip01 Head`
//! for people) so that a fixed forward axis points at the target, within a
//! cone around where the bone's parent points, at most a few degrees per
//! update. Only the head chain is implemented: the second chain ("mode 0")
//! only runs when controller `+0x190` is set, and nothing found in the
//! executable sets it.
//!
//! Not implemented because they are not traced yet (see the topic doc's
//! "Still open"):
//! * the extra head rotation at controller `+0xc0`, applied after the solve
//!   (`00c7aa60`): its value's source is unknown, so none is applied;
//! * `00c78160`'s below-target smoothing of the target point;
//! * the object at `(controller+0x2a4)+0x1c` that can override the forward
//!   and cone axes;
//! * controller `+0x43`, which can hold the look on or off (`00c75580`).
//!
//! Space: everything works on a skeleton pose in the skeleton's own space
//! ("model space", game axes, Z up), as [`nif::anim::posed`] produces it.

use std::f32::consts::PI;

use nif::anim::{quat_matrix, Bone, Quat};
use nif::math::{mat_vec, normalize, Mat3, Vec3};
use nif::Transform;

use crate::animation::quat_from_matrix;

/// Model-space forward of a character (`00c79340` sets up both axes from
/// `(0, 1, 0)`).
const MODEL_FORWARD: Vec3 = [0.0, 1.0, 0.0];

/// The solver's gain (`00c78cf3`: `fld1` before the call to `00ce0290`).
const GAIN: f32 = 1.0;

/// The tolerance of the "unchanged" and "valid rotation" tests
/// (`01017d00`, 0.001).
const EPSILON: f32 = 0.001;

const IDENTITY: Quat = [1.0, 0.0, 0.0, 0.0];

/// The LookIK settings, with the game's own defaults (the static
/// initializers at `00fbcdd0` and `00fbd0a0`–`00fbd160`). The INI can
/// override them (`[LookIK]`, `[RagdollAnim]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// `fMaxTrackingDist:LookIK`: tracking only up to this distance, units.
    pub max_tracking_dist: f32,
    /// `fMinTrackingDist:LookIK`: and only beyond this one.
    pub min_tracking_dist: f32,
    /// `fAngleMax:LookIK`: the most the head turns in one update, degrees.
    pub angle_max: f32,
    /// `fAngleMaxEase:LookIK`: the same while easing out.
    pub angle_max_ease: f32,
    /// `fEaseAngleShutOff:LookIK`: easing ends, and the look with it, when
    /// an update turns the head less than this, degrees.
    pub ease_angle_shut_off: f32,
    /// `bLookIK:RagdollAnim`: whether the look runs at all (`00c7d630`).
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            max_tracking_dist: 1500.0,
            min_tracking_dist: 12.0,
            angle_max: 3.5,
            angle_max_ease: 1.0,
            ease_angle_shut_off: 0.5,
            enabled: true,
        }
    }
}

impl Settings {
    /// The defaults, with any value the INI gives (`float(section, key)`).
    pub fn read(float: impl Fn(&str, &str) -> Option<f32>) -> Settings {
        let base = Settings::default();
        let f = |key: &str, default: f32| float("LookIK", key).unwrap_or(default);
        Settings {
            max_tracking_dist: f("fMaxTrackingDist", base.max_tracking_dist),
            min_tracking_dist: f("fMinTrackingDist", base.min_tracking_dist),
            angle_max: f("fAngleMax", base.angle_max),
            angle_max_ease: f("fAngleMaxEase", base.angle_max_ease),
            ease_angle_shut_off: f("fEaseAngleShutOff", base.ease_angle_shut_off),
            enabled: float("RagdollAnim", "bLookIK").map_or(base.enabled, |v| v != 0.0),
        }
    }
}

/// Whom the actor looks at this update: where their look anchor is (game
/// world space; the game asks the target for it, its virtual `+0x194`) and
/// how far away they are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub position: Vec3,
    pub distance: f32,
}

/// One actor's head look: the game's controller, head chain only.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadLook {
    /// The bone that turns (controller `+0x144`).
    pub bone: usize,
    /// Its parent, whose rotation carries the cone (`+0x146`).
    pub reference: usize,
    /// The bone's own axis that is turned to the target (`+0x150`).
    forward: Vec3,
    /// The cone's axis in the parent's space (`+0x160`).
    cone_axis: Vec3,
    /// The cone's half-angle, radians (`+0x148`).
    cone_angle: f32,
    /// The bone's local rotation after the last update (`+0x170`); `None`
    /// is the game's `(0,0,0,0)`, "none yet".
    previous: Option<Quat>,
    /// The step limit is armed (`+0x180`).
    limited: bool,
    /// Easing out (`+0xb2`).
    easing: bool,
    /// The look is on (`+0xb3`).
    active: bool,
    /// The target point, game world space (`+0xd0`).
    target: Vec3,
}

impl HeadLook {
    /// Sets the head chain up (`00c79340` through `00c7de60`, the angle by
    /// `0087e130`): `bone` is the head-tracking part's `BPNI` node and
    /// `tracking_max_angle` its `BPND` angle in degrees
    /// ([`crate::body_parts::BodyPart`]). `pose` is the skeleton's pose in
    /// its own space when the actor is set up. `None` when the bone isn't in
    /// the skeleton or has no parent, which is when the game logs "AI: Could
    /// not initialize LookIK system".
    ///
    /// Which pose the game has at that moment (the skeleton's own or an
    /// animated one) is not traced; callers pass the skeleton's own.
    ///
    /// The controller starts with no previous rotation and the step limit
    /// armed. Neither is traced: the constructor (`00c7f060`) writes
    /// neither field. It is inferred from the solver: it treats an all-zero
    /// previous rotation as "none yet" and takes the current pose instead,
    /// which only matters when the limit is armed in the same update, and
    /// every reset path (`00c75580(0)`, `00c7c150`) writes identity, never
    /// zero. So the head turns from where the animation has it, a limited
    /// step at a time, from the first update.
    pub fn new(
        bones: &[Bone],
        pose: &[Transform],
        bone: &str,
        tracking_max_angle: f32,
    ) -> Option<HeadLook> {
        let index = bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(bone))?;
        let reference = bones[index].parent?;
        let bone_rotation = pose.get(index)?.rotation;
        let reference_rotation = pose.get(reference)?.rotation;
        Some(HeadLook {
            bone: index,
            reference,
            forward: normalize(mat_vec(&transpose(&bone_rotation), MODEL_FORWARD)),
            cone_axis: mat_vec(&transpose(&reference_rotation), MODEL_FORWARD),
            cone_angle: tracking_max_angle.to_radians(),
            previous: None,
            limited: true,
            easing: false,
            active: false,
            target: [0.0; 3],
        })
    }

    /// Whether the look is on.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether it is easing out.
    pub fn is_easing(&self) -> bool {
        self.easing
    }

    /// One update of the actor's choice (`008a3100`): with a target between
    /// `fMinTrackingDist` (exclusive) and `fMaxTrackingDist` (inclusive),
    /// easing stops, the look goes on and aims at the target (`008a3bf0(0)`,
    /// `00c75580(1)`, `008a3b70`); otherwise a look that is on starts easing
    /// out (`008a3bf0(1)`), still at the last target.
    ///
    /// The game measures the distance between two points of the actors that
    /// have not been traced; callers give their own.
    pub fn track(&mut self, target: Option<Target>, settings: &Settings) {
        match target {
            Some(t)
                if settings.min_tracking_dist < t.distance
                    && t.distance <= settings.max_tracking_dist =>
            {
                self.easing = false;
                self.set_active(true);
                self.target = t.position;
            }
            _ => {
                if self.active && !self.easing {
                    self.easing = true;
                }
            }
        }
    }

    /// Turns the look on or off (`00c75580`). Off also resets the previous
    /// rotation to none-turned and arms the step limit.
    pub fn set_active(&mut self, on: bool) {
        self.active = on;
        if !on {
            self.previous = Some(IDENTITY);
            self.limited = true;
        }
    }

    /// One update of the head chain (`00c7d630` → `00c7aa60` →
    /// `00c78610`): turns the bone in `pose`, with every bone below it,
    /// toward the target. `world_from_model` places the skeleton in the
    /// world (game axes). Returns the angle the step limit let through, or
    /// `None` when the look didn't run.
    pub fn update(
        &mut self,
        bones: &[Bone],
        pose: &mut [Transform],
        world_from_model: &Transform,
        settings: &Settings,
    ) -> Option<f32> {
        if !settings.enabled || !self.active {
            return None;
        }
        let (bone, reference) = (self.bone, self.reference);
        if bone >= pose.len() || reference >= pose.len() {
            return None;
        }
        let parent = pose[reference];
        let locals = descendant_locals(bones, pose, bone);
        let original_local = parent.inverse().then_child(&pose[bone]);
        let parent_q = quat_from_matrix(&parent.rotation);
        if self.previous.is_none() {
            self.previous = Some(quat_from_matrix(&original_local.rotation));
        }

        // The solve (`00ce0290`), in model space.
        let target = world_from_model.inverse().apply_point(self.target);
        let limit_axis = normalize(mat_vec(&parent.rotation, self.cone_axis));
        let head = pose[bone];
        let mut dir = normalize(sub(target, head.translation));
        if dot(dir, limit_axis) < self.cone_angle.cos() {
            // Outside the cone: swing the direction onto its edge.
            let axis = normalize(cross(limit_axis, dir));
            dir = rotate(axis_angle(axis, self.cone_angle), limit_axis);
        }
        let facing = mat_vec(&head.rotation, self.forward);
        let d = dot(facing, dir);
        let angle = if d.abs() < 1.0 {
            d.acos()
        } else if d > 0.0 {
            0.0
        } else {
            PI
        };
        let turn = axis_angle(normalize(cross(facing, dir)), angle * GAIN);
        let model_q = quat_mul(turn, quat_from_matrix(&head.rotation));
        let mut local_q = quat_mul(conjugate(parent_q), model_q);

        // The step limit (`00c755e0`), on the bone's local rotation.
        let step = if self.limited {
            let (step, limited) = self.limit_step(local_q, settings);
            local_q = limited;
            step
        } else {
            0.0
        };

        // Keep the result only if it is a proper rotation (`00cb24c0`).
        let check = quat_mul(parent_q, local_q);
        let length = dot4(check, check);
        let local = if check.iter().all(|c| c.is_finite()) && (length - 1.0).abs() < EPSILON {
            Transform {
                rotation: quat_matrix(local_q),
                ..original_local
            }
        } else {
            original_local
        };
        pose[bone] = parent.then_child(&local);
        recompose(bones, pose, bone, &locals);
        self.limited = true;

        // Easing ends, and the look with it, once a step is small enough.
        if self.easing && step < settings.ease_angle_shut_off.to_radians() {
            self.previous = Some(IDENTITY);
            self.easing = false;
            self.active = false;
        }
        Some(step)
    }

    /// `00c755e0`: from the previous rotation, the change to `new` is cut
    /// to `fAngleMax` (`fAngleMaxEase` while easing) about the same axis.
    /// Returns the angle let through and the rotation to use; an unchanged
    /// rotation (x, y and z each within 0.001) is left as it is, step 0.
    fn limit_step(&mut self, new: Quat, settings: &Settings) -> (f32, Quat) {
        let previous = self.previous.unwrap_or(IDENTITY);
        if (1..4).all(|k| (new[k] - previous[k]).abs() <= EPSILON) {
            return (0.0, new);
        }
        let mut change = quat_mul(new, conjugate(previous));
        let angle = if change[0].abs() >= 1.0 {
            0.0
        } else {
            2.0 * change[0].acos()
        };
        let limit = if self.easing {
            settings.angle_max_ease
        } else {
            settings.angle_max
        }
        .to_radians();
        let step = if limit < angle {
            let mut axis = normalize([change[1], change[2], change[3]]);
            if change[0] < 0.0 {
                axis = axis.map(|c| -c);
            }
            change = axis_angle(axis, limit);
            limit
        } else {
            angle
        };
        let result = normalize_quat(quat_mul(change, previous));
        self.previous = Some(result);
        (step, result)
    }
}

/// The local transforms of `bone`'s descendants, before it moves (the
/// skeleton lists parents before children).
fn descendant_locals(bones: &[Bone], pose: &[Transform], bone: usize) -> Vec<Option<Transform>> {
    let mut below = vec![false; bones.len()];
    let mut locals = vec![None; bones.len()];
    for (i, b) in bones.iter().enumerate().skip(bone + 1) {
        if let Some(p) = b.parent {
            if p == bone || below.get(p).copied().unwrap_or(false) {
                below[i] = true;
                if let (Some(pp), Some(pi)) = (pose.get(p), pose.get(i)) {
                    locals[i] = Some(pp.inverse().then_child(pi));
                }
            }
        }
    }
    locals
}

/// Puts `bone`'s descendants back under it (the game's propagate).
fn recompose(bones: &[Bone], pose: &mut [Transform], bone: usize, locals: &[Option<Transform>]) {
    for i in bone + 1..bones.len().min(pose.len()) {
        if let (Some(local), Some(p)) = (locals[i], bones[i].parent) {
            pose[i] = pose[p].then_child(&local);
        }
    }
}

fn transpose(m: &Mat3) -> Mat3 {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
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

fn dot4(a: Quat, b: Quat) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalize_quat(q: Quat) -> Quat {
    let len = dot4(q, q).sqrt();
    if len > 0.0 {
        q.map(|c| c / len)
    } else {
        IDENTITY
    }
}

/// A rotation of `angle` radians about `axis` (w, x, y, z; `00cb2450`).
fn axis_angle(axis: Vec3, angle: f32) -> Quat {
    let (s, c) = (angle / 2.0).sin_cos();
    [c, axis[0] * s, axis[1] * s, axis[2] * s]
}

/// The Hamilton product `a ⊗ b`: the rotation `b`, then `a`.
fn quat_mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

fn conjugate(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

fn rotate(q: Quat, v: Vec3) -> Vec3 {
    mat_vec(&quat_matrix(q), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: usize = 2;
    const EYE: usize = 3;

    /// A neck straight up at the origin, a head 10 above it, an eye 5 in
    /// front of the head and 5 up; all facing +Y.
    fn skeleton() -> Vec<Bone> {
        let bone = |name: &str, parent: Option<usize>, at: Vec3| Bone {
            name: name.into(),
            parent,
            local: Transform {
                translation: at,
                ..Transform::IDENTITY
            },
        };
        vec![
            bone("Bip01", None, [0.0; 3]),
            bone("Bip01 Neck1", Some(0), [0.0, 0.0, 100.0]),
            bone("Bip01 Head", Some(1), [0.0, 0.0, 10.0]),
            bone("Bip01 Eye", Some(2), [0.0, 5.0, 5.0]),
        ]
    }

    fn model_pose(bones: &[Bone]) -> Vec<Transform> {
        let mut pose: Vec<Transform> = Vec::new();
        for b in bones {
            let t = match b.parent {
                Some(p) => pose[p].then_child(&b.local),
                None => b.local,
            };
            pose.push(t);
        }
        pose
    }

    fn look(angle: f32) -> (Vec<Bone>, Vec<Transform>, HeadLook) {
        let bones = skeleton();
        let pose = model_pose(&bones);
        let look = HeadLook::new(&bones, &pose, "bip01 head", angle).unwrap();
        (bones, pose, look)
    }

    fn facing(look: &HeadLook, pose: &[Transform]) -> Vec3 {
        mat_vec(&pose[look.bone].rotation, look.forward)
    }

    fn angle_between(a: Vec3, b: Vec3) -> f32 {
        dot(normalize(a), normalize(b))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    /// "On target": 32-bit `acos` near 1 can't resolve much below 0.02°,
    /// and the game's own "unchanged" tolerance (0.001 on the quaternion)
    /// is about 0.1°.
    const ON_TARGET: f32 = 0.1;

    fn near(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
    }

    fn at(position: Vec3, distance: f32) -> Option<Target> {
        Some(Target { position, distance })
    }

    #[test]
    fn settings_default_to_the_games_and_read_the_ini() {
        let s = Settings::default();
        assert_eq!((s.max_tracking_dist, s.min_tracking_dist), (1500.0, 12.0));
        assert_eq!(
            (s.angle_max, s.angle_max_ease, s.ease_angle_shut_off),
            (3.5, 1.0, 0.5)
        );
        assert!(s.enabled);
        let read = Settings::read(|section, key| match (section, key) {
            ("LookIK", "fAngleMax") => Some(10.0),
            ("RagdollAnim", "bLookIK") => Some(0.0),
            _ => None,
        });
        assert_eq!(read.angle_max, 10.0);
        assert_eq!(read.angle_max_ease, 1.0);
        assert!(!read.enabled);
    }

    #[test]
    fn set_up_needs_the_bone_and_its_parent() {
        let (_, _, look) = look(40.0);
        assert_eq!((look.bone, look.reference), (HEAD, 1));
        assert!(near(look.forward, MODEL_FORWARD));
        assert!((look.cone_angle - 40f32.to_radians()).abs() < 1e-6);
        let bones = skeleton();
        let pose = model_pose(&bones);
        assert!(HeadLook::new(&bones, &pose, "Bip01 Missing", 40.0).is_none());
        // The root has no parent to carry a cone.
        assert!(HeadLook::new(&bones, &pose, "Bip01", 40.0).is_none());
    }

    #[test]
    fn the_forward_axis_is_the_models_in_the_bones_own_space() {
        let mut bones = skeleton();
        // A head turned 90° left in its set-up pose: the model's +Y, seen
        // from the head, is its own +X.
        bones[HEAD].local.rotation = quat_matrix(axis_angle([0.0, 0.0, 1.0], PI / 2.0));
        let pose = model_pose(&bones);
        let look = HeadLook::new(&bones, &pose, "Bip01 Head", 40.0).unwrap();
        assert!(near(look.forward, [1.0, 0.0, 0.0]));
        assert!(near(facing(&look, &pose), MODEL_FORWARD));
    }

    #[test]
    fn tracks_only_between_the_distances() {
        let s = Settings::default();
        let (_, _, mut near_one) = look(40.0);
        near_one.track(at([0.0, 12.0, 110.0], 12.0), &s);
        assert!(!near_one.is_active());
        near_one.track(at([0.0, 1500.0, 110.0], 1500.0), &s);
        assert!(near_one.is_active());
        let (_, _, mut far) = look(40.0);
        far.track(at([0.0, 1500.5, 110.0], 1500.5), &s);
        assert!(!far.is_active());
    }

    #[test]
    fn the_head_turns_toward_the_target_a_limited_step_each_update() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(60.0);
        let world = Transform::IDENTITY;
        // 30° to the right, level with the head: 3.5° an update from the
        // animation's pose, from the very first update.
        let target = [
            100.0 * 30f32.to_radians().sin(),
            100.0 * 30f32.to_radians().cos(),
            110.0,
        ];
        look.track(at(target, 100.0), &s);
        let step = look.update(&bones, &mut pose, &world, &s).unwrap();
        assert!((step.to_degrees() - 3.5).abs() < 1e-3);
        assert!((angle_between(MODEL_FORWARD, facing(&look, &pose)) - 3.5).abs() < 0.01);
        for _ in 0..10 {
            look.update(&bones, &mut pose, &world, &s);
        }
        let to_target = sub(target, pose[HEAD].translation);
        assert!(angle_between(facing(&look, &pose), to_target) < ON_TARGET);
        // There, nothing more to do.
        assert_eq!(look.update(&bones, &mut pose, &world, &s), Some(0.0));

        // Now 30° to the left: 60° away, again 3.5° an update.
        let target = [-target[0], target[1], target[2]];
        look.track(at(target, 100.0), &s);
        let before = facing(&look, &pose);
        let step = look.update(&bones, &mut pose, &world, &s).unwrap();
        assert!((step.to_degrees() - 3.5).abs() < 1e-3);
        // Measured from the stored rotation, which the head may sit up to
        // the "unchanged" tolerance away from.
        assert!((angle_between(before, facing(&look, &pose)) - 3.5).abs() < ON_TARGET);
        for _ in 0..20 {
            look.update(&bones, &mut pose, &world, &s);
        }
        let to_target = sub(target, pose[HEAD].translation);
        assert!(angle_between(facing(&look, &pose), to_target) < ON_TARGET);
    }

    #[test]
    fn the_cone_holds_the_head_back_from_a_target_behind() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(40.0);
        // Behind and to the right.
        look.track(at([100.0, -100.0, 110.0], 141.0), &s);
        for _ in 0..20 {
            look.update(&bones, &mut pose, &Transform::IDENTITY, &s);
        }
        // The cone's axis is the neck's forward, +Y: the head stops 40° off it.
        let off = angle_between(facing(&look, &pose), MODEL_FORWARD);
        assert!((off - 40.0).abs() < 0.01, "{off}");
        assert!(facing(&look, &pose)[0] > 0.0);
    }

    #[test]
    fn bones_below_the_head_turn_with_it() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(120.0);
        look.track(at([100.0, 0.0, 110.0], 100.0), &s);
        for _ in 0..30 {
            look.update(&bones, &mut pose, &Transform::IDENTITY, &s);
        }
        // The eye, 5 in front of the head, is now 5 to its right.
        assert!(near(pose[EYE].translation, [5.0, 0.0, 115.0]));
        assert!(near(pose[1].translation, [0.0, 0.0, 100.0]));
    }

    #[test]
    fn the_target_is_taken_into_the_skeletons_space() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(60.0);
        // The actor stands at (1000, 0, 0) turned 90° left: its forward is
        // the world's -X, so a target straight ahead of it is at (900, 0).
        let world = Transform {
            rotation: quat_matrix(axis_angle([0.0, 0.0, 1.0], PI / 2.0)),
            translation: [1000.0, 0.0, 0.0],
            scale: 1.0,
        };
        look.track(at([900.0, 0.0, 110.0], 100.0), &s);
        look.update(&bones, &mut pose, &world, &s);
        assert!(angle_between(facing(&look, &pose), MODEL_FORWARD) < ON_TARGET);
    }

    #[test]
    fn losing_the_target_eases_out_then_turns_the_look_off() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(60.0);
        let world = Transform::IDENTITY;
        look.track(at([50.0, 100.0, 110.0], 112.0), &s);
        for _ in 0..10 {
            look.update(&bones, &mut pose, &world, &s);
        }
        look.track(None, &s);
        assert!(look.is_easing() && look.is_active());
        // Still on the last target, nothing more to turn: the step is under
        // 0.5°, so the look goes off and forgets its rotation.
        assert_eq!(look.update(&bones, &mut pose, &world, &s), Some(0.0));
        assert!(!look.is_active() && !look.is_easing());
        assert_eq!(look.previous, Some(IDENTITY));
        assert_eq!(look.update(&bones, &mut pose, &world, &s), None);
    }

    #[test]
    fn easing_steps_are_the_smaller_limit() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(60.0);
        let world = Transform::IDENTITY;
        look.track(at([0.0, 100.0, 110.0], 100.0), &s);
        look.update(&bones, &mut pose, &world, &s);
        // A new target 40° off, then the target is lost at once.
        look.track(
            at(
                [
                    100.0 * 40f32.to_radians().sin(),
                    100.0 * 40f32.to_radians().cos(),
                    110.0,
                ],
                100.0,
            ),
            &s,
        );
        look.track(None, &s);
        let step = look.update(&bones, &mut pose, &world, &s).unwrap();
        assert!((step.to_degrees() - 1.0).abs() < 1e-3);
        assert!(look.is_active());
    }

    #[test]
    fn nothing_runs_when_the_setting_is_off_or_the_look_is_off() {
        let off = Settings {
            enabled: false,
            ..Settings::default()
        };
        let (bones, mut pose, mut look) = look(60.0);
        let before = pose.clone();
        assert_eq!(
            look.update(&bones, &mut pose, &Transform::IDENTITY, &off),
            None
        );
        look.track(at([100.0, 0.0, 110.0], 100.0), &off);
        assert_eq!(
            look.update(&bones, &mut pose, &Transform::IDENTITY, &off),
            None
        );
        assert_eq!(pose, before);
    }

    #[test]
    fn a_broken_target_leaves_the_head_as_it_was() {
        let s = Settings::default();
        let (bones, mut pose, mut look) = look(60.0);
        let before = pose[HEAD];
        look.track(at([f32::NAN, 0.0, 0.0], 100.0), &s);
        assert!(look.is_active());
        look.update(&bones, &mut pose, &Transform::IDENTITY, &s);
        assert!(near(pose[HEAD].translation, before.translation));
        for row in 0..3 {
            assert!(near(pose[HEAD].rotation[row], before.rotation[row]));
        }
    }
}
