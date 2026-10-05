//! People and creatures moving: each placed actor gets a root entity where
//! it stands and one joint entity per bone of its skeleton; its pieces are
//! skinned meshes on those joints (the lit shader skins them, `SKINNED` in
//! `game_lit.wgsl`), and every frame the joints take the pose its
//! animations give.
//!
//! The animations are played as the game plays them
//! (`world::animation`): each body section has a group playing (the idle
//! loop under everything; the walk or run in the movement section while
//! moving, at the rate that makes the file's root travel the actor's
//! speed; the aim and attacks in the weapon section while fighting; the
//! seat's loop in place of the idle; sitting down, getting up and the
//! tree's idles in the special-idle section), and a change of group
//! cross-fades over the files' blend times, the sequences blended per
//! bone by their priorities. Here the frame's flags (`walking`,
//! `running`, `fighting`, …, set by `ai`, `fighting` and `sitting`) say
//! what each section should play, as the game's mover flags do for
//! `Actor::PickAnimations`.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use cellview::{space, ActorData, MeshData};
use preview::cell::ActorSkeleton;
use world::animation::{self, group, section, GroupData, MoveFlags, Player};
use world::movement::TurnSide;

/// The animation settings from the INI (`[General]`
/// `fAnimationDefaultBlend` 0.2, `fAnimationMult` 1).
#[derive(Resource, Clone, Copy)]
pub struct AnimSettings(pub animation::Settings);

impl AnimSettings {
    pub fn read(ini: &assets::IniSettings) -> AnimSettings {
        let base = animation::Settings::default();
        AnimSettings(animation::Settings {
            default_blend: ini
                .float("General", "fAnimationDefaultBlend")
                .unwrap_or(base.default_blend),
            mult: ini
                .float("General", "fAnimationMult")
                .filter(|m| *m > 0.0)
                .unwrap_or(base.mult),
        })
    }
}

/// A placed actor's skeleton, its joint entities (one per bone, in the
/// skeleton's order) and the animations playing on it.
#[derive(Component)]
pub struct ActorRig {
    pub skeleton: Arc<ActorSkeleton>,
    pub joints: Vec<Entity>,
    /// The sequences playing and blending (the game's `AnimData`).
    pub player: Player,
    /// The actor's scale: the rates are worked out before it.
    pub scale: f32,
    /// How fast it moves over the ground this frame, units a second with
    /// its scale (`ai`, `fighting`); 0 standing.
    pub speed: f32,
    /// Walking (`ai`), which plays the walk instead of the idle; running
    /// (in a fight or fleeing), which plays the run with the weapon ready.
    pub walking: bool,
    pub running: bool,
    /// Turning in place (`ai`: the game's turn, movement flags 0x10 left,
    /// 0x20 right), which plays the turn animation when not walking, at the
    /// turn's own scale (`rate`: `fAITurnSpeedScale` 1.5, `00888070`).
    pub turning: Option<(TurnSide, f32)>,
    /// Holding its pose (the dead).
    pub still: bool,
    /// In a fight (`ai`): the weapon held ready instead of put away.
    pub fighting: bool,
    /// When (elapsed seconds) the last attack began.
    pub attack_at: Option<f32>,
    /// The base loop the idle tree gave instead of the standing idle (the
    /// seated loop in a chair, `ai::Seats`), looping.
    pub dynamic_idle: Option<Arc<nif::Sequence>>,
    /// A whole-body animation playing over everything (sitting down,
    /// getting up, a seated or standing idle from the tree), and how many
    /// seconds into it its owner has it.
    pub overlay: Option<(Arc<nif::Sequence>, f32)>,
    /// A script-requested special idle owns its actual KF clock until it
    /// finishes; the furniture/free-idle clock must not overwrite it.
    pub scripted_idle: Option<esm::FormId>,
    /// Dead: the ragdoll its bones follow.
    pub ragdoll: Option<Box<DeadBody>>,
    /// Has dropped its weapon (`world::body_parts::hurt_part`): the weapon
    /// model, on the `Weapon` bone, is hidden by shrinking that bone.
    pub disarmed: bool,
    /// The attack the weapon section was last told to play.
    started_attack: Option<f32>,
    /// Whether the spine node faces up in the last pose (`IsFacingUp`;
    /// `None` without the node): see [`spine_up`].
    pub spine_up: Option<bool>,
}

/// A dead actor's ragdoll (`preview::ragdoll`): the bodies in motion, the
/// pose it died in, and where its skeleton stands in the world (game axes).
pub struct DeadBody {
    pub sim: physics::ragdoll::Ragdoll,
    pub died: Vec<nif::Transform>,
    pub placement: nif::Transform,
}

impl ActorRig {
    /// A rig standing idle: its idle loop already faded in (the game
    /// blends it in from the skeleton's own pose when the actor's 3D
    /// loads; a second of it has passed here so nobody appears in that
    /// pose), `phase` seconds into it.
    pub fn new(skeleton: Arc<ActorSkeleton>, scale: f32, phase: f32) -> ActorRig {
        let mut rig = ActorRig {
            joints: Vec::new(),
            player: Player::default(),
            scale: scale.max(1e-3),
            speed: 0.0,
            walking: false,
            running: false,
            turning: None,
            still: false,
            fighting: false,
            attack_at: None,
            dynamic_idle: None,
            overlay: None,
            scripted_idle: None,
            ragdoll: None,
            disarmed: false,
            started_attack: None,
            spine_up: None,
            skeleton,
        };
        if let Some(idle) = rig.skeleton.idle.clone() {
            rig.player.play(group::IDLE, &idle, -1, &rig.skeleton.bones);
            rig.player.update(1.0);
            rig.player.sync_time(section::IDLE, phase);
        }
        rig
    }

    /// Whether the weapon is held ready (the aim plays) rather than put
    /// away.
    fn ready(&self) -> bool {
        self.fighting && self.skeleton.aim.is_some()
    }

    /// The bones' transforms now (as `animate_actors` poses them).
    pub fn pose_now(&self, _now: f32) -> Vec<nif::Transform> {
        let bones = &self.skeleton.bones;
        let mut pose = self.player.pose(bones);
        if !self.ready() {
            if let Some(h) = &self.skeleton.holster {
                nif::hang_weapon(bones, &mut pose, h);
            }
        }
        if self.disarmed {
            hide_weapon(bones, &mut pose);
        }
        pose
    }

    /// One frame of the game's animation picking (`Actor::PickAnimations`,
    /// `00895110`, with the flags this viewer keeps instead of the mover's)
    /// and the sequences' update.
    fn drive(&mut self, dt: f32) {
        let sk = self.skeleton.clone();
        let bones = &sk.bones;
        // The base loop (section 0): the seat's, else the standing idle.
        match (&self.dynamic_idle, &sk.idle) {
            (Some(s), _) => {
                self.player.play(group::DYNAMIC_IDLE, s, -1, bones);
            }
            (None, Some(idle)) => {
                self.player.play(group::IDLE, idle, -1, bones);
            }
            (None, None) => {}
        }
        // Movement (section 1): the walk or run, at the rate that makes
        // the file's root travel the actor's speed; running without a run
        // file plays the walk that fast (the game's fallback); standing
        // still (or slower than 1 unit a second) eases the movement out.
        // Turning in place with no move direction (flags 0x10 / 0x20):
        // `TurnLeft` / `TurnRight`, `mtturnleft.kf` / `mtturnright.kf`
        // (`ActorSkeleton::turn_left`), at the turn's own rate: the
        // in-place scale `ai` gives (`00888070` → `00895110(1.0, scale)`).
        // With the weapon ready the group is the weapon kind's
        // (`1hpturnleft.kf` …), not loaded here: no turn plays then.
        let unscaled = self.speed / self.scale;
        let turn = self.turning.filter(|_| !self.walking && !self.ready());
        let flags = MoveFlags {
            forward: self.walking,
            running: self.running,
            turn_left: matches!(turn, Some((TurnSide::Left, _))),
            turn_right: matches!(turn, Some((TurnSide::Right, _))),
            ..Default::default()
        };
        let chosen = match animation::movement_group(flags, unscaled) {
            Some(g) if g == group::TURN_LEFT => sk.turn_left.as_ref().map(|s| (g, s)),
            Some(g) if g == group::TURN_RIGHT => sk.turn_right.as_ref().map(|s| (g, s)),
            Some(g) if g == group::FAST_FORWARD => match (&sk.run, &sk.walk) {
                (Some(run), _) => Some((g, run)),
                (None, Some(walk)) => Some((group::FORWARD, walk)),
                (None, None) => None,
            },
            Some(g) => match (&sk.walk, &sk.run) {
                (Some(walk), _) => Some((g, walk)),
                (None, Some(run)) => Some((g, run)),
                (None, None) => None,
            },
            None => None,
        };
        match chosen {
            Some((g, seq)) => {
                self.player.movement_rate = match turn {
                    Some((_, rate)) if g == group::TURN_LEFT || g == group::TURN_RIGHT => rate,
                    _ => animation::movement_rate(unscaled, GroupData::read(seq).speed()),
                };
                self.player.play(g, seq, -1, bones);
            }
            None => self.player.stop_section(section::MOVEMENT),
        }
        // The weapon section: the aim while fighting, each attack played
        // once over it (the aim comes back, blended from the pose, once
        // the attack has eased out).
        if self.ready() {
            let new_attack = self.attack_at.is_some() && self.attack_at != self.started_attack;
            match (&sk.attack, &sk.aim) {
                (Some(attack), _) if new_attack => {
                    self.started_attack = self.attack_at;
                    self.player.play(group::ATTACK_RIGHT, attack, 0, bones);
                }
                (_, Some(aim)) if self.player.playing(section::WEAPON).is_none() => {
                    self.player.play(group::AIM, aim, -1, bones);
                }
                _ => {}
            }
        } else {
            self.player.stop_section(section::WEAPON);
            self.started_attack = None;
        }
        // The special-idle section: whatever plays over everything, its
        // clock its owner's (`sitting`).
        match (&self.scripted_idle, &self.overlay) {
            (Some(_), _) => {}
            (None, Some((seq, _))) => {
                self.player.play(group::SPECIAL_IDLE, seq, -1, bones);
            }
            (None, None) => self.player.stop_section(section::SPECIAL_IDLE),
        }
        self.player.update(dt);
        if self.scripted_idle.is_some() {
            if self.player.playing(section::SPECIAL_IDLE).is_none() {
                self.scripted_idle = None;
            }
        } else if let Some((_, elapsed)) = &self.overlay {
            self.player.sync_time(section::SPECIAL_IDLE, *elapsed);
        }
    }

    /// Goes limp as the game's dead do, if its skeleton has a ragdoll:
    /// from the pose it's in now, placed by `placement`, thrown as the
    /// game throws the dead (`throw`: from where, and the speed every body
    /// gains, × its part's share: `world::combat::death_push_share`).
    /// False if it can't.
    pub fn go_limp(
        &mut self,
        now: f32,
        placement: nif::Transform,
        throw: Option<([f32; 3], f32)>,
    ) -> bool {
        let Some(rig) = &self.skeleton.ragdoll else {
            return false;
        };
        let died = self.pose_now(now);
        let mut sim = rig.start(&died, &placement);
        if let Some((origin, speed)) = throw {
            let shares: Vec<f32> = rig
                .ragdoll
                .bodies
                .iter()
                .map(|b| world::combat::death_push_share(b.part))
                .collect();
            sim.throw_from(origin, speed, &shares);
        }
        self.ragdoll = Some(Box::new(DeadBody {
            sim,
            died,
            placement,
        }));
        self.walking = false;
        self.still = true;
        true
    }
}

/// Shrinks the `Weapon` bone to nothing, so the weapon skinned to it isn't
/// seen.
fn hide_weapon(bones: &[nif::Bone], pose: &mut [nif::Transform]) {
    let weapon = bones
        .iter()
        .position(|b| b.name.eq_ignore_ascii_case("Weapon"));
    if let Some(t) = weapon.and_then(|i| pose.get_mut(i)) {
        t.scale = 0.0;
    }
}

/// A game-space transform as a Bevy one (still in game units and axes:
/// joints sit under the actor's root, which converts).
pub(crate) fn bevy_transform(t: &nif::Transform) -> Transform {
    let r = t.rotation;
    let rotation = Mat3::from_cols(
        Vec3::new(r[0][0], r[1][0], r[2][0]),
        Vec3::new(r[0][1], r[1][1], r[2][1]),
        Vec3::new(r[0][2], r[1][2], r[2][2]),
    );
    Transform {
        translation: Vec3::from(t.translation),
        rotation: Quat::from_mat3(&rotation).normalize(),
        scale: Vec3::splat(t.scale),
    }
}

/// Spawns an actor's root and joints; returns the root and the joints.
pub fn spawn_actor(
    commands: &mut Commands,
    actor: &ActorData,
    index: usize,
    root_tag: impl Bundle,
) -> (Entity, Vec<Entity>) {
    let root = commands
        .spawn((
            Transform::from_matrix(Mat4::from_cols_array(&space::matrix(&actor.transform))),
            Visibility::default(),
            root_tag,
        ))
        .id();
    // Spread the actors over their idle (a fixed spread, so pictures
    // repeat).
    let phase = (index as f32 * 0.618_034).fract() * 4.0;
    let m = &actor.transform;
    let scale = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    let mut rig = ActorRig::new(actor.skeleton.clone(), scale, phase);
    let pose = rig.pose_now(0.0);
    let joints: Vec<Entity> = pose
        .iter()
        .map(|t| commands.spawn((bevy_transform(t), ChildOf(root))).id())
        .collect();
    rig.joints = joints.clone();
    commands.entity(root).insert((
        rig,
        crate::ai::Walker::new(actor),
        crate::sitting::Life::default(),
    ));
    (root, joints)
}

/// A skinned piece as a Bevy mesh (in the piece's own space, game units),
/// with its inverse bind poses. `None` for pieces without a rig.
pub fn skinned_mesh(data: &MeshData) -> Option<(Mesh, SkinnedMeshInverseBindposes)> {
    let rig = data.rig.as_ref()?;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
    if let Some(tangents) = &data.tangents {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents.clone());
    }
    if let Some(colors) = &data.colors {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors.clone());
    }
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(rig.joint_indices.clone()),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, rig.joint_weights.clone());
    mesh.insert_indices(Indices::U16(data.indices.clone()));
    let binds: Vec<Mat4> = rig
        .joints
        .iter()
        .map(|(_, m)| Mat4::from_cols_array(m))
        .collect();
    Some((mesh, SkinnedMeshInverseBindposes::from(binds)))
}

/// The joints a piece's skin uses, from its actor's joints.
pub fn skin_joints(data: &MeshData, joints: &[Entity]) -> Vec<Entity> {
    data.rig
        .as_ref()
        .map(|rig| {
            rig.joints
                .iter()
                .map(|(bone, _)| joints.get(*bone).copied().unwrap_or(joints[0]))
                .collect()
        })
        .unwrap_or_default()
}

/// Deferred NPC PlayIdle requests (008ba600, process flag 0x80).
/// Only loaded, unconditional leaf special idles are represented here.
/// Trees/other groups need their own original dispatch, not a fallback.
pub fn script_idles(
    mut requests: ResMut<crate::player_idle::PlayerIdle>,
    game: Res<crate::GameFiles>,
    mut state: ResMut<crate::dialogue::DialogueState>,
    mut seats: ResMut<crate::sitting::Seats>,
    settings: Res<AnimSettings>,
    menus: Res<crate::menus::Menus>,
    mut actors: Query<(
        &crate::ai::Walker,
        &mut crate::sitting::Life,
        &mut ActorRig,
        &Visibility,
    )>,
) {
    if menus.is_open() {
        return;
    }
    for (who, requested) in std::mem::take(&mut requests.npc_requests) {
        let Some((_, mut life, mut rig, visibility)) =
            actors.iter_mut().find(|(w, _, _, _)| w.reference == who)
        else {
            eprintln!("Script idle {requested} for {who}: no loaded actor animation data");
            continue;
        };
        if *visibility == Visibility::Hidden || state.0.dead.contains(&who) {
            continue;
        }
        let Some(idle) = seats.tree.get(requested).cloned() else {
            continue;
        };
        if idle.group() != section::SPECIAL_IDLE
            || !idle.is_animation()
            || !idle.conditions.is_empty()
        {
            eprintln!("Script idle {requested}: group/tree/condition dispatch pending");
            continue;
        }
        let Some(seq) = seats.sequence(&game.0, &idle.model) else {
            eprintln!("Script idle {requested}: couldn't load {}", idle.model);
            continue;
        };
        let count =
            idle.extra_loops(|lo, hi| lo + (state.0.roll() % (u64::from(hi - lo) + 1)) as u8);
        let loops = if count == 255 { -1 } else { i32::from(count) };
        let bones = rig.skeleton.clone();
        rig.player.settings = settings.0;
        rig.player.play_script_idle(&seq, loops, &bones.bones);
        rig.scripted_idle = Some(requested);
        rig.overlay = None;
        life.idles.stop();
        life.idles.last = Some(requested);
        println!("{who}: script idle {} started", idle.editor_id);
    }
}

/// Every frame: each actor's animations are picked and run on, and its
/// joints take the pose; the dead's follow their ragdolls until they come
/// to rest. While the dialogue menu or a viewer menu is open the game is
/// in menu mode and stands still, animations included (someone talked to
/// mid-stride kept walking on the spot before), except, in the dialogue
/// menu, the speaker: they stop walking and turn to face the player.
pub fn animate_actors(
    time: Res<Time>,
    settings: Option<Res<AnimSettings>>,
    conversation: Option<Res<crate::dialogue::Conversation>>,
    menus: Option<Res<crate::menus::Menus>>,
    collision: Option<Res<crate::walk::CellCollision>>,
    mut rigs: Query<&mut ActorRig>,
    mut joints: Query<&mut Transform>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let in_dialogue = conversation
        .as_ref()
        .is_some_and(|c| c.0.as_ref().is_some_and(|t| !t.is_line_only()));
    if menus.as_ref().is_some_and(|m| m.is_open()) {
        return;
    }
    for mut rig in &mut rigs {
        let rig = &mut *rig;
        // In the dialogue menu only the speaker moves: `ai` holds everyone
        // else (`still`) and has the speaker stop walking and turn in place
        // to face the player (inferred, see `ai::move_actors`), which plays
        // here; the dead's ragdolls wait.
        if in_dialogue && (rig.still || rig.ragdoll.is_some()) {
            continue;
        }
        let pose = if let Some(dead) = rig.ragdoll.as_deref_mut() {
            if dead.sim.asleep {
                continue;
            }
            if let Some(c) = &collision {
                dead.sim.update(&c.0, time.delta_secs());
            }
            let Some(r) = &rig.skeleton.ragdoll else {
                continue;
            };
            r.bones(&dead.sim, &rig.skeleton.bones, &dead.died, &dead.placement)
        } else if rig.still {
            continue;
        } else {
            if let Some(s) = &settings {
                rig.player.settings = s.0;
            }
            rig.drive(dt);
            rig.pose_now(now)
        };
        rig.spine_up = spine_up(&rig.skeleton.bones, &pose);
        for (joint, t) in rig.joints.iter().zip(&pose) {
            if let Ok(mut transform) = joints.get_mut(*joint) {
                *transform = bevy_transform(t);
            }
        }
    }
}

/// Whether a pose's spine node (`Bip01 Spine`, else `Bip01 Spine01`) faces
/// up, as `IsFacingUp` asks (`005a0710` → `00c6b7b0`): its world rotation's
/// [2][1] above 0. An actor's placement only turns it about Z, which
/// leaves that row alone, so the pose's own (model-space) rotation answers.
/// `None` when the skeleton has neither node.
pub fn spine_up(bones: &[nif::Bone], pose: &[nif::Transform]) -> Option<bool> {
    let find = |name: &str| bones.iter().position(|b| b.name.eq_ignore_ascii_case(name));
    let i = find("Bip01 Spine").or_else(|| find("Bip01 Spine01"))?;
    Some(pose.get(i)?.rotation[2][1] > 0.0)
}

/// Tells the world which way each person's spine faces (`IsFacingUp`).
pub fn report_facing_up(
    mut state: ResMut<crate::dialogue::DialogueState>,
    rigs: Query<(&crate::ai::Walker, &ActorRig)>,
) {
    let facing = rigs
        .iter()
        .filter_map(|(w, rig)| Some((w.reference, rig.spine_up?)))
        .collect();
    world::more_functions::report_facing_up(&mut state.0, facing);
}

/// The skinned piece entity's components.
pub fn skinned(
    inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
    joints: Vec<Entity>,
) -> SkinnedMesh {
    SkinnedMesh {
        inverse_bindposes,
        joints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nif::anim::{Motion, Sequence, Track};

    #[test]
    fn the_spine_faces_up_by_its_rotation() {
        let bone = |name: &str| nif::Bone {
            name: name.into(),
            parent: None,
            local: nif::Transform::IDENTITY,
        };
        let bones = [bone("Bip01"), bone("Bip01 Spine")];
        // Standing: the spine's Y axis level, [2][1] = 0: not up.
        let mut pose = vec![nif::Transform::IDENTITY; 2];
        assert_eq!(spine_up(&bones, &pose), Some(false));
        // Turned 90° about X (Y → Z): up.
        pose[1].rotation = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
        assert_eq!(spine_up(&bones, &pose), Some(true));
        // The other name, and none at all.
        assert_eq!(
            spine_up(&[bone("Bip01"), bone("Bip01 Spine01")], &pose),
            Some(true)
        );
        assert_eq!(spine_up(&bones[..1], &pose), None);
    }

    #[test]
    fn a_dropped_weapon_shrinks_its_bone_away() {
        let bone = |name: &str, parent| nif::Bone {
            name: name.into(),
            parent,
            local: nif::Transform::IDENTITY,
        };
        let bones = [
            bone("Bip01", None),
            bone("Bip01 R Hand", Some(0)),
            bone("Weapon", Some(1)),
        ];
        let mut pose = vec![nif::Transform::IDENTITY; 3];
        hide_weapon(&bones, &mut pose);
        assert_eq!(pose[2].scale, 0.0);
        assert_eq!((pose[0].scale, pose[1].scale), (1.0, 1.0));
    }

    #[test]
    fn game_transforms_keep_their_meaning() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // 90° about z (x → y), moved and scaled.
        let t = nif::Transform {
            rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [1.0, 2.0, 3.0],
            scale: 2.0,
        };
        let b = bevy_transform(&t);
        let p = b.transform_point(Vec3::new(1.0, 0.0, 0.0));
        assert!((p - Vec3::new(1.0, 4.0, 3.0)).length() < 1e-5, "{p}");
        assert!((b.rotation.w - s).abs() < 1e-5);
    }

    /// A skeleton of a root and an arm, with an idle holding the arm at
    /// `idle_arm` and a walk whose root travels 102.3 in 1.2 s holding it
    /// at `walk_arm`.
    fn skeleton(idle_arm: f32, walk_arm: f32) -> Arc<ActorSkeleton> {
        let bone = |name: &str, parent, z| nif::Bone {
            name: name.into(),
            parent,
            local: nif::Transform {
                translation: [0.0, 0.0, z],
                ..nif::Transform::IDENTITY
            },
        };
        let holds = |node: &str, at: [f32; 3], priority: u8| Track {
            node: node.into(),
            priority,
            motion: Motion::Keys {
                translation: vec![(0.0, at)],
                rotation: Vec::new(),
                scale: Vec::new(),
                default: (None, None, None),
                euler: None,
            },
        };
        let idle = Sequence {
            name: "Idle".into(),
            start: 0.0,
            stop: 4.0,
            looping: true,
            accum_root: Some("Bip01".into()),
            materials: Vec::new(),
            text_keys: vec![(0.0, "start".into()), (4.0, "end".into())],
            tracks: vec![holds("Arm", [0.0, 0.0, idle_arm], 10)],
        };
        let walk = Sequence {
            name: "Forward".into(),
            start: 0.0,
            stop: 1.2,
            looping: true,
            accum_root: Some("Bip01".into()),
            materials: Vec::new(),
            text_keys: vec![(0.0, "start".into()), (0.033, "Blend:6".into())],
            tracks: vec![
                Track {
                    node: "Bip01".into(),
                    priority: 30,
                    motion: Motion::Keys {
                        translation: vec![(0.0, [0.0; 3]), (1.2, [0.0, 102.3, 0.0])],
                        rotation: Vec::new(),
                        scale: Vec::new(),
                        default: (None, None, None),
                        euler: None,
                    },
                },
                holds("Arm", [0.0, 0.0, walk_arm], 30),
            ],
        };
        Arc::new(ActorSkeleton {
            bones: vec![bone("Bip01", None, 68.0), bone("Arm", Some(0), 10.0)],
            idle: Some(Arc::new(idle)),
            walk: Some(Arc::new(walk)),
            ..Default::default()
        })
    }

    #[test]
    fn scripted_idle_keeps_its_clock_and_finishes_without_restarting() {
        let mut rig = ActorRig::new(skeleton(10.0, 20.0), 1.0, 0.0);
        let seq = Arc::new(Sequence {
            name: "SpecialIdle".into(),
            start: 0.0,
            stop: 1.0,
            looping: false,
            accum_root: None,
            materials: Vec::new(),
            text_keys: vec![(0.0, "start".into()), (1.0, "end".into())],
            tracks: vec![Track {
                node: "Arm".into(),
                priority: 95,
                motion: Motion::Keys {
                    translation: vec![(0.0, [0.0, 0.0, 40.0])],
                    rotation: Vec::new(),
                    scale: Vec::new(),
                    default: (None, None, None),
                    euler: None,
                },
            }],
        });
        rig.player.play_script_idle(&seq, 0, &rig.skeleton.bones);
        rig.scripted_idle = Some(esm::FormId(123));
        // A stale furniture/free-idle overlay must neither replace the
        // script sequence nor overwrite its clock.
        rig.overlay = Some((rig.skeleton.idle.clone().unwrap(), 10.0));
        rig.drive(0.2);
        assert!(Arc::ptr_eq(
            rig.player.sequence(section::SPECIAL_IDLE).unwrap(),
            &seq
        ));
        assert!(rig.player.time(section::SPECIAL_IDLE).unwrap() < 1.0);
        rig.overlay = None;
        for _ in 0..15 {
            rig.drive(0.1);
        }
        assert!(rig.scripted_idle.is_none());
        assert!(rig.player.sequence(section::SPECIAL_IDLE).is_none());
        rig.drive(0.2);
        assert!(rig.player.sequence(section::SPECIAL_IDLE).is_none());
        assert!((rig.pose_now(0.0)[1].translation[2] - 10.0).abs() < 1e-3);
    }

    #[test]
    fn starting_to_walk_blends_in_and_plays_at_the_actors_speed() {
        let mut rig = ActorRig::new(skeleton(10.0, 20.0), 1.0, 0.0);
        let arm = |rig: &ActorRig| rig.pose_now(0.0)[1].translation[2];
        // Standing: the idle, already in.
        assert!((arm(&rig) - 10.0).abs() < 1e-4, "{}", arm(&rig));
        // Walking at 77: the walk blends in over Blend:6 = 0.2 s, no jump.
        rig.walking = true;
        rig.speed = 77.0;
        rig.drive(0.0);
        assert!((arm(&rig) - 10.0).abs() < 1e-4, "{}", arm(&rig));
        rig.drive(0.1);
        let half = arm(&rig);
        assert!(half > 12.0 && half < 18.0, "{half}");
        rig.drive(0.1);
        assert!((arm(&rig) - 20.0).abs() < 1e-3, "{}", arm(&rig));
        assert_eq!(rig.player.playing(section::MOVEMENT), Some(group::FORWARD));
        // At 77 / 85 of the file's rate (85.3 units a second → 85).
        assert!((rig.player.movement_rate - 77.0 / 85.0).abs() < 1e-6);
        // Stopping: back to the idle over 0.2 s, no jump either.
        rig.walking = false;
        rig.speed = 0.0;
        rig.drive(0.0);
        assert!((arm(&rig) - 20.0).abs() < 1e-3, "{}", arm(&rig));
        rig.drive(0.1);
        let half = arm(&rig);
        assert!(half > 10.5 && half < 19.5, "{half}");
        rig.drive(0.2);
        assert!((arm(&rig) - 10.0).abs() < 1e-3, "{}", arm(&rig));
        assert_eq!(rig.player.playing(section::MOVEMENT), None);
    }

    #[test]
    fn turning_in_place_plays_the_turn_group_at_the_turns_scale() {
        // `mtturnleft.kf`: 1 s, looping, `Blend: 3`.
        let turn = Sequence {
            name: "TurnLeft".into(),
            start: 0.0,
            stop: 1.0,
            looping: true,
            accum_root: Some("Bip01".into()),
            materials: Vec::new(),
            text_keys: vec![
                (0.0, "start".into()),
                (0.03, "Blend: 3".into()),
                (1.0, "end".into()),
            ],
            tracks: vec![Track {
                node: "Arm".into(),
                priority: 30,
                motion: Motion::Keys {
                    translation: vec![(0.0, [0.0, 0.0, 30.0])],
                    rotation: Vec::new(),
                    scale: Vec::new(),
                    default: (None, None, None),
                    euler: None,
                },
            }],
        };
        let sk = Arc::new(ActorSkeleton {
            turn_left: Some(Arc::new(turn)),
            ..(*skeleton(10.0, 20.0)).clone()
        });
        let mut rig = ActorRig::new(sk, 1.0, 0.0);
        // Turning left on the spot at people's in-place scale 1.5: the
        // turn group, played 1.5 times as fast.
        rig.turning = Some((TurnSide::Left, 1.5));
        rig.drive(0.1);
        assert_eq!(
            rig.player.playing(section::MOVEMENT),
            Some(group::TURN_LEFT)
        );
        assert!((rig.player.movement_rate - 1.5).abs() < 1e-6);
        rig.drive(0.1);
        assert!((rig.pose_now(0.0)[1].translation[2] - 30.0).abs() < 1e-3);
        // No file for the group: the idle (`00495740`'s last fallback).
        rig.turning = Some((TurnSide::Right, 1.5));
        rig.drive(0.1);
        assert_eq!(rig.player.playing(section::MOVEMENT), None);
        // Walking takes the walk, whatever the turn.
        rig.turning = Some((TurnSide::Left, 1.5));
        rig.walking = true;
        rig.speed = 77.0;
        rig.drive(0.1);
        assert_eq!(rig.player.playing(section::MOVEMENT), Some(group::FORWARD));
        assert!((rig.player.movement_rate - 77.0 / 85.0).abs() < 1e-6);
    }

    #[test]
    fn a_scaled_actor_plays_at_its_unscaled_rate() {
        let mut rig = ActorRig::new(skeleton(10.0, 20.0), 1.1, 0.0);
        rig.walking = true;
        rig.speed = 77.0 * 1.1;
        rig.drive(0.1);
        assert!((rig.player.movement_rate - 77.0 / 85.0).abs() < 1e-5);
    }
}
