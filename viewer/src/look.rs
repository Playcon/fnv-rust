//! Head tracking on screen: `world::look_ik` run on each actor's pose.
//!
//! The rules (when to look, how far and fast the head turns) are the core's;
//! this module finds each actor's head-tracking body part, works out whom
//! it looks at in the game's terms, and hands the pose over.
//!
//! The game asks the target for its look anchor (virtual `+0x194`): for
//! the player in first person it is the world position of the `Camera1st`
//! node (`PlayerCharacter` `00952ff0`, the node named in
//! `004b8b99`–`004b8bac`), which is the viewer's camera; for other actors
//! it is their `Bip01 Head` (`Actor` `008a2fa0`, [`world::look_ik::anchor`]),
//! which [`record`] keeps for each living actor as it is posed.

use std::collections::HashMap;

use bevy::prelude::*;
use esm::{FormId, LoadOrder};
use world::body_parts::{self, BodyPartData};
use world::dialogue::PLAYER_REF;
use world::look_ik::{HeadLook, Settings, Target, ANCHOR_BONE};

use crate::actors::ActorRig;
use crate::ai::Walker;
use crate::GameFiles;

/// The LookIK settings from the INI (`[LookIK]`, `[RagdollAnim]
/// bLookIK`), else the game's defaults.
#[derive(Resource, Clone, Copy)]
pub struct LookSettings(pub Settings);

impl LookSettings {
    pub fn read(ini: &assets::IniSettings) -> LookSettings {
        LookSettings(Settings::read(|section, key| ini.float(section, key)))
    }
}

/// A placed actor's base record, for its body part data.
#[derive(Component, Clone, Copy)]
pub struct ActorBase(pub FormId);

/// An actor's head look; `None` when its body part data has no
/// head-tracking part or its skeleton lacks the part's bone (the game then
/// logs "AI: Could not initialize LookIK system").
#[derive(Component)]
pub struct HeadTracking(pub Option<HeadLook>);

/// Where the player is for the look: the eye (`Camera1st`, the camera) and
/// the feet the distance is measured from (the game measures between the
/// two actors' positions, virtual `+0x1f4`, `008a3100`). Game units.
#[derive(Resource, Default, Clone, Copy)]
pub struct PlayerAnchor {
    pub eye: Option<[f32; 3]>,
    pub feet: Option<[f32; 3]>,
}

/// Where each living actor other than the player is looked at, by
/// reference: its `Bip01 Head` (world, game units) and its feet.
///
/// An actor looking at another one posed later in the frame sees where
/// that one's head was the frame before. The game reads the target's head
/// node's world transform, which the scene graph updates after the
/// animation; in which order its actors update isn't traced.
#[derive(Resource, Default)]
pub struct HeadAnchors(pub HashMap<FormId, PlayerAnchor>);

/// Whom looks can be at: the player and the other actors.
pub struct Anchors<'a> {
    pub player: &'a PlayerAnchor,
    pub actors: Option<&'a HeadAnchors>,
}

/// The head look for an actor whose base record is `base` and whose
/// skeleton is `bones`, set up on the skeleton's own pose.
pub fn head_look(order: &LoadOrder, base: FormId, bones: &[nif::Bone]) -> Option<HeadLook> {
    let data = BodyPartData::load(order, body_parts::data_form(order, base)?)?;
    let part = data.head_tracking_part()?;
    let pose = nif::posed(bones, None, 0.0);
    HeadLook::new(bones, &pose, &part.ik_node, part.tracking_max_angle)
}

/// Gives each newly placed actor its head look.
pub fn attach_head_tracking(
    mut commands: Commands,
    game: Res<GameFiles>,
    new: Query<(Entity, &ActorRig, &ActorBase), Without<HeadTracking>>,
) {
    for (entity, rig, base) in &new {
        let look = head_look(&game.0.order, base.0, &rig.skeleton.bones);
        commands.entity(entity).insert(HeadTracking(look));
    }
}

/// Keeps [`PlayerAnchor`] on the camera and the player's feet.
pub fn follow_player(
    state: Res<crate::dialogue::DialogueState>,
    camera: Query<&Transform, With<Camera3d>>,
    mut anchor: ResMut<PlayerAnchor>,
) {
    anchor.eye = camera
        .iter()
        .next()
        .map(|t| crate::walk::game_point(t.translation));
    anchor.feet = state.0.player_position;
}

/// Whom `walker` looks at, as the look takes it: the anchor of whom it
/// looks at and how far away they stand.
pub fn target(walker: &Walker, anchors: &Anchors) -> Option<Target> {
    let whom = walker.looking_at()?;
    let anchor = if whom == PLAYER_REF {
        *anchors.player
    } else {
        *anchors.actors?.0.get(&whom)?
    };
    let (eye, feet) = (anchor.eye?, anchor.feet?);
    let d = [0, 1, 2].map(|k| feet[k] - walker.position[k]);
    Some(Target {
        position: eye,
        distance: (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(),
    })
}

/// One update of an actor's head look on its pose (skeleton space, game
/// units), once the animation has posed it.
pub fn apply(
    look: &mut HeadLook,
    walker: &Walker,
    anchors: &Anchors,
    settings: &Settings,
    bones: &[nif::Bone],
    pose: &mut [nif::Transform],
) {
    look.track(target(walker, anchors), settings);
    look.update(bones, pose, &walker.placement(), settings);
}

/// Keeps where `walker` is looked at, from its final pose this frame.
/// Without a `Bip01 Head` the game looks 0.9 of the actor's height up
/// ([`world::look_ik::anchor`]); its bounds aren't known here, so such an
/// actor isn't looked at.
pub fn record(
    walker: &Walker,
    bones: &[nif::Bone],
    pose: &[nif::Transform],
    anchors: &mut HeadAnchors,
) {
    let head = bones
        .iter()
        .position(|b| b.name == ANCHOR_BONE)
        .and_then(|i| pose.get(i));
    match head {
        Some(head) => {
            let eye = walker.placement().apply_point(head.translation);
            anchors.0.insert(
                walker.reference,
                PlayerAnchor {
                    eye: Some(eye),
                    feet: Some(walker.position),
                },
            );
        }
        None => {
            anchors.0.remove(&walker.reference);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_the_players_eye_at_the_players_distance() {
        let mut walker = Walker::at(FormId(0x1234), [0.0, 0.0, 0.0], 0.0, 1.0, false);
        let player = PlayerAnchor {
            eye: Some([0.0, 300.0, 120.0]),
            feet: Some([0.0, 300.0, 0.0]),
        };
        let anchors = Anchors {
            player: &player,
            actors: None,
        };
        assert_eq!(target(&walker, &anchors), None);
        walker.look_at_for_test(PLAYER_REF);
        assert_eq!(
            target(&walker, &anchors),
            Some(Target {
                position: [0.0, 300.0, 120.0],
                distance: 300.0
            })
        );
        // Someone not posed (yet): nothing to look at.
        walker.look_at_for_test(FormId(0x5678));
        assert_eq!(target(&walker, &anchors), None);
        // No camera yet: nothing to look at.
        walker.look_at_for_test(PLAYER_REF);
        let nobody = PlayerAnchor::default();
        let anchors = Anchors {
            player: &nobody,
            actors: None,
        };
        assert_eq!(target(&walker, &anchors), None);
    }

    #[test]
    fn another_actor_is_looked_at_by_its_head() {
        let bone = |name: &str, parent, z| nif::Bone {
            name: name.into(),
            parent,
            local: nif::Transform {
                translation: [0.0, 0.0, z],
                ..nif::Transform::IDENTITY
            },
        };
        let bones = vec![bone("Bip01", None, 60.0), bone(ANCHOR_BONE, Some(0), 60.0)];
        let pose = nif::posed(&bones, None, 0.0);
        // Doc at (100, 0, 10), turned a quarter (east), slightly bigger.
        let doc = Walker::at(FormId(0x5678), [100.0, 0.0, 10.0], 1.5, 1.1, false);
        let mut heads = HeadAnchors::default();
        record(&doc, &bones, &pose, &mut heads);
        let kept = heads.0[&FormId(0x5678)];
        let eye = kept.eye.unwrap();
        assert!(
            (eye[0] - 100.0).abs() < 1e-3 && eye[1].abs() < 1e-3,
            "{eye:?}"
        );
        assert!((eye[2] - (10.0 + 120.0 * 1.1)).abs() < 1e-3, "{eye:?}");

        let mut walker = Walker::at(FormId(0x1234), [0.0, 0.0, 0.0], 0.0, 1.0, false);
        walker.look_at_for_test(FormId(0x5678));
        let player = PlayerAnchor::default();
        let anchors = Anchors {
            player: &player,
            actors: Some(&heads),
        };
        let t = target(&walker, &anchors).unwrap();
        assert_eq!(t.position, eye);
        assert!((t.distance - (100.0f32 * 100.0 + 100.0).sqrt()).abs() < 1e-3);

        // Without a head bone it isn't looked at.
        let headless = vec![bone("Bip01", None, 60.0)];
        record(
            &doc,
            &headless,
            &nif::posed(&headless, None, 0.0),
            &mut heads,
        );
        assert_eq!(target(&walker, &anchors_of(&heads)), None);
    }

    fn anchors_of(heads: &HeadAnchors) -> Anchors<'_> {
        static NOBODY: PlayerAnchor = PlayerAnchor {
            eye: None,
            feet: None,
        };
        Anchors {
            player: &NOBODY,
            actors: Some(heads),
        }
    }
}
