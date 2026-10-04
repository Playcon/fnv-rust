//! Head tracking on screen: `world::look_ik` run on each actor's pose.
//!
//! The rules (when to look, how far and fast the head turns) are the core's;
//! this module finds each actor's head-tracking body part, works out whom
//! it looks at in the game's terms, and hands the pose over.
//!
//! The look target is only supported for the player, the case the opening
//! needs (Doc's `SayTo Player`). The game asks the target for its look
//! anchor (virtual `+0x194`): for the player in first person it is the
//! world position of the `Camera1st` node (`PlayerCharacter` `00952ff0`, the
//! node named in `004b8b99`–`004b8bac`), which is the viewer's camera. For
//! other actors it is a node of theirs (`Actor` `008a2fa0`) that isn't
//! traced yet, so actors looking at someone else keep their heads as the
//! animation has them.

use bevy::prelude::*;
use esm::{FormId, LoadOrder};
use world::body_parts::{self, BodyPartData};
use world::dialogue::PLAYER_REF;
use world::look_ik::{HeadLook, Settings, Target};

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

/// Whom `walker` looks at, as the look takes it: the player's eye and how
/// far the player is, when the player is whom it looks at.
pub fn target(walker: &Walker, anchor: &PlayerAnchor) -> Option<Target> {
    if walker.looking_at()? != PLAYER_REF {
        return None;
    }
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
    anchor: &PlayerAnchor,
    settings: &Settings,
    bones: &[nif::Bone],
    pose: &mut [nif::Transform],
) {
    look.track(target(walker, anchor), settings);
    look.update(bones, pose, &walker.placement(), settings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_the_players_eye_at_the_players_distance() {
        let mut walker = Walker::at(FormId(0x1234), [0.0, 0.0, 0.0], 0.0, 1.0, false);
        let anchor = PlayerAnchor {
            eye: Some([0.0, 300.0, 120.0]),
            feet: Some([0.0, 300.0, 0.0]),
        };
        assert_eq!(target(&walker, &anchor), None);
        walker.look_at_for_test(PLAYER_REF);
        assert_eq!(
            target(&walker, &anchor),
            Some(Target {
                position: [0.0, 300.0, 120.0],
                distance: 300.0
            })
        );
        // Someone else: their anchor isn't traced, so no target.
        walker.look_at_for_test(FormId(0x5678));
        assert_eq!(target(&walker, &anchor), None);
        // No camera yet: nothing to look at.
        walker.look_at_for_test(PLAYER_REF);
        assert_eq!(target(&walker, &PlayerAnchor::default()), None);
    }
}
