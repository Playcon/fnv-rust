//! The player fighting (`world::combat`): with the weapon in hand (the
//! one equipped, else the first carried) or fists, the left mouse button
//! attacks along the view: guns as far as their projectile's range, each
//! pellet within the weapon's cone (its min spread, as the game fires
//! them), melee weapons and fists as far as their reach (× 128; 64
//! unarmed) between the bodies' edges. What's hit first takes it: a person
//! or creature, a scripted object (its `OnHit` blocks run), or a wall,
//! which stops it. Guns fire at their attack rate, use their ammunition
//! from the inventory, and need reloading (R) after a clip. Whoever is
//! hurt fights back (`ai`). A line at the bottom shows health, ammunition
//! and the player's crippled limbs.
//!
//! Where a hit lands (`world::body_parts`): shots must meet one of the
//! capsules the target's skeleton carries on its bones (the game's own hit
//! shapes: its ragdoll's bodies, posed as the actor is now), and land on
//! that bone's part; a melee hit reaches the target by its body's bounds
//! and lands where the view meets a capsule, else on the bone nearest the
//! view (a guess). Skeletons without bodies use their record's bounds and
//! the nearest bone. A person who drops their weapon has its model hidden.
//!
//! Not yet: projectiles in flight (the game's bullets are hitscan, others
//! fly), auto-aim (3° toward a target), scope sway, hits on a held weapon
//! outside V.A.T.S. (part 14: its collision isn't tested). V.A.T.S. is
//! `vats`, which shoots through [`first_met`] too.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use esm::FormId;
use world::body_parts::BodyPartData;
use world::combat::{self, Weapon};
use world::dialogue::PLAYER_REF;
use world::scripting::Runner;

use crate::actors::ActorRig;
use crate::ai::Walker;
use crate::dialogue::{Conversation, DialogueState, Talkers};
use crate::menus::Menus;
use crate::scripts::{CellScripts, Scripts};
use crate::sounds::SoundRequests;
use crate::walk::{game_point, CellCollision, Player};
use crate::{FlyCamera, GameFiles};

/// How far a shot can reach when nothing else says (units).
const SHOT_RANGE: f32 = 10_000.0;

/// The player's attacking: when the next one can come, rounds left in the
/// clip (`None`: a full one), and a reload under way.
#[derive(Resource, Default)]
pub struct PlayerAttack {
    next: f32,
    in_clip: Option<u32>,
    weapon: Option<FormId>,
    reloaded_at: f32,
    /// When the last attack and the reload under way started (for the
    /// first-person animations).
    pub fired_at: Option<f32>,
    pub reload_started: Option<f32>,
    /// How fast those animations play relative to the world (V.A.T.S.'s
    /// playback runs the player at its own multiplier, `vats`); `None`: as
    /// recorded.
    pub sped_up: Option<f32>,
    /// The rates the attack and the reload under way play at
    /// (`world::combat::attack_rate`, `reload_rate`: the weapon's speed
    /// and attack multiplier, Agility, and the perks), on top of
    /// `sped_up`; 0 before any.
    pub attack_rate: f32,
    pub reload_rate: f32,
    /// Whether the weapon (or fists) is out, as the game keeps it for the
    /// player (`process+0x135`, `IsWeaponOut`): a new game and every
    /// change of weapon start holstered (equipping into an empty hand and
    /// unequipping both put it away: `0088db20`, `0088d7d0`); the Ready
    /// Item key and attacking draw it (see [`player_attack`]).
    pub out: bool,
    /// When the weapon was last drawn (`true`) or put away (`false`), for
    /// the first-person animations (`viewmodel`).
    pub readied_at: Option<(f32, bool)>,
    /// Until when the drawing or putting away plays (the first-person view
    /// says, knowing the animation: [`Self::readying_until`]); the key and
    /// attacks wait for it, as the game waits for its queued weapon action
    /// (`FUN_008a7570() == -1`).
    busy_until: f32,
    /// The Ready Item key's press (`world::combat::ReadyKey`).
    ready: combat::ReadyKey,
    /// Bodies by base record: half width and height (from `OBND`).
    bodies: HashMap<FormId, (f32, f32)>,
    /// Body part data by person or creature (`world::body_parts`).
    parts: HashMap<FormId, Option<Arc<BodyPartData>>>,
}

impl PlayerAttack {
    /// Rounds in the clip of `weapon` (a full one to begin with), at most
    /// what's carried.
    pub(crate) fn clip(&self, weapon: &Weapon, carried: u32) -> u32 {
        let full = if self.weapon == Some(weapon.form_id) {
            self.in_clip.unwrap_or(weapon.clip)
        } else {
            weapon.clip
        };
        full.min(carried)
    }

    /// The clip of the weapon in hand now holds `rounds`.
    pub(crate) fn set_clip(&mut self, weapon: &Weapon, rounds: u32) {
        self.weapon = Some(weapon.form_id);
        self.in_clip = Some(rounds);
    }

    /// Draws (`true`) or puts away the weapon now, playing its animation.
    fn set_out(&mut self, out: bool, now: f32) {
        self.out = out;
        self.readied_at = Some((now, out));
        self.busy_until = now;
    }

    /// The weapon `weapon` (`None`: fists) in hand and out, without the
    /// animation: for `--weapon` pictures (the game starts holstered).
    pub fn draw_at_once(&mut self, weapon: Option<FormId>) {
        self.weapon = weapon;
        self.in_clip = None;
        self.out = true;
        self.readied_at = None;
        self.busy_until = 0.0;
    }

    /// The drawing or putting away that started at `since` plays for
    /// `seconds`.
    pub fn readying_until(&mut self, since: f32, seconds: f32) {
        if self.busy_until <= since {
            self.busy_until = since + seconds;
        }
    }

    /// Body part data for someone, read once.
    pub(crate) fn body_parts(
        &mut self,
        order: &esm::LoadOrder,
        who: FormId,
    ) -> Option<Arc<BodyPartData>> {
        self.parts
            .entry(who)
            .or_insert_with(|| BodyPartData::of(order, who).map(Arc::new))
            .clone()
    }
}

/// What a line from the eye meets first within its reach.
pub(crate) enum Met {
    /// Someone (and the body part it lands on) or a scripted object, this
    /// far along.
    Thing {
        distance: f32,
        reference: FormId,
        part: Option<u8>,
    },
    /// Nothing; `true` when a wall stopped it.
    Nothing(bool),
}

/// What a shot or blow along `(eye, dir)` meets first within `reach`
/// (see the module notes): the living people and creatures in `talkers`
/// (shots: their capsules; blows: their bounds), scripted objects, and
/// the cell's walls, which stop it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn first_met(
    order: &esm::LoadOrder,
    state: &world::scripting::GameState,
    caches: &mut PlayerAttack,
    talkers: &Talkers,
    cell_scripts: &CellScripts,
    collision: &CellCollision,
    rigs: &Query<(&Walker, &ActorRig)>,
    (eye, dir): ([f32; 3], [f32; 3]),
    reach: f32,
    melee: bool,
    now: f32,
) -> Met {
    let mut best: Option<(f32, FormId, Option<u8>)> = None;
    for t in &talkers.0 {
        if state.dead.contains(&t.reference) {
            continue;
        }
        // Shots pass a ghost by (`SetGhost`: the projectiles' target
        // searches skip ghosts, `00816f10`, `00817b90`).
        if !melee && world::more_functions::is_ghost(state, t.reference) {
            continue;
        }
        let (radius, height) = *caches
            .bodies
            .entry(t.base)
            .or_insert_with(|| body(order, t.base));
        let data = caches.body_parts(order, t.reference);
        let rig = rigs.iter().find(|(w, _)| w.reference == t.reference);
        let cylinder = ray_body(eye, dir, t.position, radius, height);
        let met = ray_actor(rig, data.as_deref(), (eye, dir), cylinder, melee, now);
        if let Some((d, bone)) = met {
            if d <= reach && best.is_none_or(|(bd, _, _)| d < bd) {
                let part =
                    bone.zip(rig)
                        .zip(data.as_deref())
                        .and_then(|((bone, (_, rig)), data)| {
                            data.part_of_bone(&rig.skeleton.bones, bone)
                        });
                best = Some((d, t.reference, part));
            }
        }
    }
    // Trigger volumes aren't solid: shots go through them (the player
    // standing in one caught every shot at 0 units before).
    for r in cell_scripts
        .refs
        .iter()
        .filter(|r| r.script.is_some() && r.trigger.is_none())
    {
        if let Some(d) = r.ray_hit(eye, dir) {
            if d <= reach && best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, r.reference, None));
            }
        }
    }
    let wall = collision.0.raycast(eye, dir, reach).map(|(d, _)| d);
    match best.filter(|(d, _, _)| wall.is_none_or(|w| w >= d - 5.0)) {
        Some((distance, reference, part)) => Met::Thing {
            distance,
            reference,
            part,
        },
        None => Met::Nothing(wall.is_some()),
    }
}

/// Says what a hit did, as the attacks print it.
pub(crate) fn tell_hit(
    order: &esm::LoadOrder,
    state: &world::scripting::GameState,
    caches: &mut PlayerAttack,
    target: FormId,
    distance: f32,
    hit: &world::combat::Hit,
) -> String {
    let left = combat::health(order, state, target).unwrap_or(0.0).max(0.0);
    let place = caches
        .body_parts(order, target)
        .zip(hit.part)
        .and_then(|(d, p)| d.part(p).map(|p| p.name.to_lowercase()))
        .map_or(String::new(), |name| format!(" in the {name}"));
    let mut said = format!(
        "Hit {target}{place} at {distance:.0} units for {:.1} ({left:.1} left)",
        hit.dealt
    );
    if hit.critical {
        said.push_str(", a critical");
    }
    if let Some(hurt) = &hit.hurt {
        if hurt.crippled {
            said.push_str(&format!("; {} crippled", hurt.name));
        }
        if hurt.dropped.is_some() {
            said.push_str("; dropped their weapon");
        }
    }
    said
}

/// Where a ray meets someone on screen: how far along it, and the bone it
/// lands on (`None`: they have no rig, or no bone was found).
fn ray_actor(
    rig: Option<(&Walker, &ActorRig)>,
    data: Option<&BodyPartData>,
    (eye, dir): ([f32; 3], [f32; 3]),
    cylinder: Option<f32>,
    melee: bool,
    now: f32,
) -> Option<(f32, Option<usize>)> {
    let Some((walker, rig)) = rig else {
        return cylinder.map(|d| (d, None));
    };
    let pose = rig.pose_now(now);
    let placement = walker.placement();
    let capsule = rig
        .skeleton
        .ragdoll
        .as_ref()
        .and_then(|r| r.ray_hit(&pose, &placement, eye, dir));
    let nearest = || {
        preview::ragdoll::nearest_bone(&pose, &placement, eye, dir, |i| {
            data.is_some_and(|d| d.part_of_bone(&rig.skeleton.bones, i).is_some())
        })
    };
    match (rig.skeleton.ragdoll.is_some(), melee) {
        // Shots meet the bodies or miss.
        (true, false) => capsule.map(|(d, bone)| (d, Some(bone))),
        // Melee reaches by the bounds and lands where the view meets a
        // body, else on the bone nearest the view.
        (true, true) => {
            let d = cylinder?;
            Some((d, capsule.map(|(_, b)| b).or_else(nearest)))
        }
        (false, _) => cylinder.map(|d| (d, nearest())),
    }
}

/// People who've dropped their weapon (`world::body_parts::hurt_part`)
/// have its model hidden.
pub fn show_dropped_weapons(state: Res<DialogueState>, mut rigs: Query<(&Walker, &mut ActorRig)>) {
    if state.0.dropped.is_empty() {
        return;
    }
    for (walker, mut rig) in &mut rigs {
        let disarmed = state.0.dropped.iter().any(|(w, _)| *w == walker.reference);
        if rig.disarmed != disarmed {
            rig.disarmed = disarmed;
        }
    }
}

impl PlayerAttack {
    /// Rounds left in the clip of the weapon in hand (`None`: a full one).
    pub fn in_clip(&self) -> Option<u32> {
        self.in_clip
    }
}

/// The health and ammunition line.
#[derive(Component)]
pub struct HudText;

pub fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.75, 0.3)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(16.0),
            ..default()
        },
        HudText,
    ));
}

/// A body's half width and height from its base record's bounds (people
/// without bounds: 25 and 130).
fn body(order: &esm::LoadOrder, base: FormId) -> (f32, f32) {
    order
        .get(base)
        .and_then(|r| r.record().ok())
        .and_then(|r| {
            let s = r
                .get(esm::FourCC::new(b"OBND"))
                .filter(|s| s.data.len() >= 12)?;
            let v = |i: usize| f32::from(i16::from_le_bytes([s.data[i * 2], s.data[i * 2 + 1]]));
            let half = ((v(3) - v(0)).max(v(4) - v(1)) * 0.5).max(10.0);
            let height = (v(5) - v(2)).max(20.0);
            Some((half, height))
        })
        .filter(|(h, t)| *h > 0.0 && *t > 0.0)
        .unwrap_or((25.0, 130.0))
}

/// Where a ray from the eye meets an upright cylinder around someone's
/// feet, if it does.
fn ray_body(eye: [f32; 3], dir: [f32; 3], feet: [f32; 3], radius: f32, height: f32) -> Option<f32> {
    let (ox, oy) = (eye[0] - feet[0], eye[1] - feet[1]);
    let a = dir[0] * dir[0] + dir[1] * dir[1];
    let b = 2.0 * (ox * dir[0] + oy * dir[1]);
    let c = ox * ox + oy * oy - radius * radius;
    let t = if a < 1e-9 {
        (c <= 0.0).then_some(0.0)?
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let near = (-b - disc.sqrt()) / (2.0 * a);
        let far = (-b + disc.sqrt()) / (2.0 * a);
        if far < 0.0 {
            return None;
        }
        near.max(0.0)
    };
    let z = eye[2] + dir[2] * t - feet[2];
    (0.0..=height).contains(&z).then_some(t)
}

/// Left click: an attack, if one is ready; R: reloading.
#[allow(clippy::too_many_arguments)]
pub fn player_attack(
    time: Res<Time>,
    game: Res<GameFiles>,
    scripts: Res<Scripts>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    (mut state, player, conversation, menus): (
        ResMut<DialogueState>,
        Res<Player>,
        Res<Conversation>,
        Res<Menus>,
    ),
    (talkers, cell_scripts, collision): (Res<Talkers>, Res<CellScripts>, Res<CellCollision>),
    mut attack: ResMut<PlayerAttack>,
    mut sounds: ResMut<SoundRequests>,
    mut hits: ResMut<crate::hiteffects::HitReports>,
    cameras: Query<&Transform, With<FlyCamera>>,
    mut hud: Query<&mut Text, With<HudText>>,
    rigs: Query<(&Walker, &ActorRig)>,
) {
    let order = &game.0.order;
    let now = time.elapsed_secs();
    let state = &mut state.0;
    let weapon = combat::weapon_in_hand(order, state, PLAYER_REF);
    let id = weapon.as_ref().map(|w| w.form_id);
    if attack.weapon != id {
        attack.weapon = id;
        attack.in_clip = None;
        // A change of weapon starts holstered (`0088db20`: equipping with
        // nothing in hand puts the player's weapon away; `0088d7d0`:
        // unequipping a weapon does too).
        attack.out = false;
        attack.readied_at = None;
    }
    if attack.out {
        state.weapon_out.insert(PLAYER_REF);
    } else {
        state.weapon_out.remove(&PLAYER_REF);
    }
    // The kind of ammunition in use, and how much of it is carried.
    let ammo_held = |state: &world::scripting::GameState, w: &Weapon| {
        w.ammo_in_use(order, state, PLAYER_REF)
            .map(|a| (a, state.item_count(order, PLAYER_REF, a).max(0) as u32))
    };
    // The HUD line.
    let health = combat::health(order, state, PLAYER_REF)
        .unwrap_or(0.0)
        .max(0.0);
    let full = combat::max_health(order, state, PLAYER_REF).unwrap_or(0.0);
    let mut line = format!(
        "HP {health:.0}/{full:.0}  AP {:.0}/{:.0}",
        world::vats::action_points(order, state),
        world::vats::max_action_points(order, state)
    );
    match &weapon {
        Some(w) => {
            line.push_str(&format!("    {}", w.name));
            if let Some((_, held)) = ammo_held(state, w) {
                let clip = attack.in_clip.unwrap_or(w.clip).min(held);
                line.push_str(&format!("  {clip}/{held}"));
            }
            if now < attack.reloaded_at {
                line.push_str("  (reloading)");
            }
        }
        None => line.push_str("    Fists"),
    }
    if !attack.out {
        line.push_str("  (holstered: R or attack draws)");
    }
    let crippled = world::body_parts::crippled_parts(order, state, PLAYER_REF);
    if !crippled.is_empty() {
        line.push_str(&format!("    Crippled: {}", crippled.join(", ")));
    }
    if state.dead.contains(&PLAYER_REF) {
        line = "You are dead. F9 loads the last quick save.".into();
    }
    for mut text in &mut hud {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
    let busy = !player.walking
        || !player.ready
        || conversation.0.is_some()
        || menus.is_open()
        || state.dead.contains(&PLAYER_REF)
        || state.controls_off[world::scripting::controls::FIGHTING];
    if busy {
        return;
    }
    // A reload takes the weapon's reload time at the reload rate
    // (`world::combat::reload_rate`: Agility and Rapid Reload).
    let start_reload =
        |attack: &mut PlayerAttack, state: &world::scripting::GameState, w: &Weapon| {
            let rate = combat::reload_rate(order, state, PLAYER_REF, Some(w)).max(1e-3);
            attack.in_clip = Some(w.clip);
            attack.reloaded_at = now + w.reload_time / rate;
            attack.reload_started = Some(now);
            attack.reload_rate = rate;
        };
    // The Ready Item key (R), as the game reads it: `world::combat::
    // ReadyKey`. "Reloadable": a gun with its ammunition.
    let reloadable = weapon
        .as_ref()
        .is_some_and(|w| ammo_held(state, w).is_some());
    let readying = now < attack.busy_until;
    let key = if keys.pressed(KeyCode::KeyR) {
        combat::KeyState::Held
    } else if keys.just_released(KeyCode::KeyR) {
        combat::KeyState::Released
    } else {
        combat::KeyState::Up
    };
    let (out, mut ready) = (attack.out, std::mem::take(&mut attack.ready));
    let action = ready.update(key, time.delta_secs(), out, reloadable, readying);
    attack.ready = ready;
    match action {
        combat::ReadyAction::Draw => attack.set_out(true, now),
        combat::ReadyAction::PutAway => attack.set_out(false, now),
        combat::ReadyAction::Reload => {
            if let Some(w) = weapon.as_ref() {
                start_reload(&mut attack, state, w);
            }
            return;
        }
        combat::ReadyAction::Nothing => {}
    }
    if !mouse.just_pressed(MouseButton::Left) || now < attack.next || now < attack.reloaded_at {
        return;
    }
    // Attacking with the weapon holstered draws it instead (`00948310`:
    // the attack control just pressed and the weapon not out).
    if !attack.out {
        if !readying {
            attack.set_out(true, now);
        }
        return;
    }
    if readying {
        return;
    }
    // Ammunition: each shot uses the weapon's ammo use from what's carried
    // (the clip counts down; an empty clip reloads first).
    if let Some(w) = weapon.as_ref() {
        if let Some((ammo, held)) = ammo_held(state, w) {
            let use_ = u32::from(w.ammo_use.max(1));
            if held < use_ {
                println!("Out of ammunition for the {}.", w.name);
                return;
            }
            let clip = attack.in_clip.unwrap_or(w.clip);
            if clip < use_ {
                start_reload(&mut attack, state, w);
                return;
            }
            attack.in_clip = Some(clip - use_);
            if let Some(n) = state.items.get_mut(&(PLAYER_REF, ammo)) {
                *n -= use_ as i32;
            }
            // The round's case or cell, some of the time (`world::combat::
            // ammo_item_recovered`: the ammunition's own chance, Hand
            // Loader's doubling).
            let roll = state.roll();
            if let Some(item) = combat::ammo_item_recovered(order, state, PLAYER_REF, w, ammo, roll)
            {
                *state.items.entry((PLAYER_REF, item)).or_insert(0) += 1;
            }
        }
    }
    // Every attack wears the weapon a little (`world::combat::attack_wear`,
    // through the perks' "Modify Item Damage").
    if let Some(w) = weapon.as_ref() {
        let ammo = w.ammo_in_use(order, state, PLAYER_REF);
        let wear = combat::attack_wear(order, ammo);
        combat::damage_weapon(order, state, PLAYER_REF, w, wear);
    }
    attack.next = now + weapon.as_ref().map_or(0.5, |w| w.shot_interval());
    attack.fired_at = Some(now);
    attack.attack_rate = combat::attack_rate(order, state, PLAYER_REF, weapon.as_ref());
    if let Some(s) = weapon.as_ref().and_then(|w| w.sound) {
        sounds.0.push(s);
    }
    // What the attack meets first along the view.
    let Ok(camera) = cameras.single() else {
        return;
    };
    let eye = game_point(camera.translation);
    let f = camera.forward().as_vec3();
    let view = [f.x, -f.z, f.y];
    let melee = weapon.as_ref().is_none_or(|w| w.is_melee());
    // Guns: the projectile's range, a shot's pellets each carrying an even
    // share of the damage, flying within the weapon's cone (`world::combat::
    // Weapon::shot`); melee: the weapon's reach between the bodies' edges
    // (from the eye's axis, so with the player's own radius added).
    let ammo = weapon
        .as_ref()
        .and_then(|w| w.ammo_in_use(order, state, PLAYER_REF));
    let (count, cone, reach) = match &weapon {
        Some(w) if !melee => {
            let (count, cone) = w.shot(order, ammo);
            (count, cone, w.range(order).unwrap_or(SHOT_RANGE))
        }
        _ => (
            1,
            0.0,
            Weapon::melee_reach(weapon.as_ref()) + physics::CharacterShape::PLAYER.radius,
        ),
    };
    let pellet = weapon.clone().map(|mut w| {
        w.damage /= count as f32;
        w
    });
    let heading = view[0].atan2(view[1]);
    let pitch = view[2].clamp(-1.0, 1.0).asin();
    for _ in 0..count {
        // Within the cone, uniform in the angle off the view (so shots
        // bunch toward the middle), as the game does: r = U(0, cone), θ =
        // U(0, 2π), added to heading and pitch.
        let unit = |v: u64| (v % 1_000_000) as f32 / 1_000_000.0;
        let r = cone * unit(state.roll());
        let theta = std::f32::consts::TAU * unit(state.roll());
        let (h, p) = (heading + r * theta.cos(), pitch + r * theta.sin());
        let dir = [h.sin() * p.cos(), h.cos() * p.cos(), p.sin()];
        // The nearest thing met: how far, who, and the body part.
        let met = first_met(
            order,
            state,
            &mut attack,
            &talkers,
            &cell_scripts,
            &collision,
            &rigs,
            (eye, dir),
            reach,
            melee,
            now,
        );
        // A shot striking the world: its impact (`hiteffects`).
        let struck = collision.0.raycast(eye, dir, reach);
        let gun = weapon.as_ref().filter(|w| !w.is_melee()).map(|w| w.form_id);
        let Met::Thing {
            distance: d,
            reference: target,
            part,
        } = met
        else {
            let wall = matches!(met, Met::Nothing(true));
            if let (Some(s), Some(g)) = (struck, gun) {
                hits.shot_on_world(&collision.0, (eye, dir), s, PLAYER_REF, g);
            }
            println!(
                "The attack hit nothing{}.",
                if wall { " but a wall" } else { "" }
            );
            continue;
        };
        let hit =
            Runner::new(order, &scripts.0, state).hit_at(PLAYER_REF, target, pellet.as_ref(), part);
        let Some(hit) = hit else {
            // An object (a scripted bottle): its impact where the shot
            // meets the cell's collision there.
            if let (Some(s), Some(g)) = (struck.filter(|(w, _)| (w - d).abs() < 5.0), gun) {
                hits.shot_on_world(&collision.0, (eye, dir), s, PLAYER_REF, g);
            }
            println!("Hit {target} at {d:.0} units.");
            continue;
        };
        hits.0.push(crate::hiteffects::HitReport {
            attacker: PLAYER_REF,
            target: Some(target),
            weapon: id,
            point: [0, 1, 2].map(|k| eye[k] + dir[k] * d),
            havok: None,
            damage: hit.dealt,
            killed: state.dead.contains(&target),
        });
        println!("{}.", tell_hit(order, state, &mut attack, target, d, &hit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_meet_a_body_of_its_own_size() {
        // A gecko-sized body (half width 35, height 100) 100 units ahead.
        let feet = [100.0, 0.0, 0.0];
        let d = ray_body([0.0, 0.0, 50.0], [1.0, 0.0, 0.0], feet, 35.0, 100.0).unwrap();
        assert!((d - 65.0).abs() < 1e-3);
        // Over its back.
        assert!(ray_body([0.0, 0.0, 120.0], [1.0, 0.0, 0.0], feet, 35.0, 100.0).is_none());
    }

    /// Someone standing at the origin facing north: `Bip01` (60 up) under
    /// the root, its spine (90), neck (100) and head (110) above, and a
    /// camera node (108) straight under the root; one body, a ball of
    /// radius 8 on the head; body part data with a head from the neck and
    /// a torso from `Bip01`.
    fn person(with_body: bool) -> (Walker, ActorRig, BodyPartData) {
        let at = |z: f32| nif::Transform {
            translation: [0.0, 0.0, z],
            ..nif::Transform::IDENTITY
        };
        let bone = |name: &str, parent, z| nif::Bone {
            name: name.into(),
            parent,
            local: at(z),
        };
        let bones = vec![
            bone("Scene Root", None, 0.0),
            bone("Bip01", Some(0), 60.0),
            bone("Bip01 Spine2", Some(1), 30.0),
            bone("Bip01 Neck1", Some(2), 10.0),
            bone("Bip01 Head", Some(3), 10.0),
            bone("Camera3rd", Some(0), 108.0),
        ];
        let ragdoll = with_body.then(|| {
            let head = nif::RagdollBody {
                bone: 4,
                bone_name: "Bip01 Head".into(),
                frame: at(110.0),
                mass: 3.0,
                center: [0.0; 3],
                inertia: [1.0; 3],
                linear_damping: 0.1,
                angular_damping: 0.05,
                friction: 0.3,
                restitution: 0.8,
                max_linear_speed: 7000.0,
                max_angular_speed: 30.0,
                capsule: Some(([0.0; 3], [0.0; 3], 8.0)),
                layer: 8,
                part: 1,
            };
            preview::ragdoll::RagdollRig::new(
                &bones,
                nif::Ragdoll {
                    bodies: vec![head],
                    joints: Vec::new(),
                },
            )
        });
        let skeleton = Arc::new(preview::cell::ActorSkeleton {
            bones,
            ragdoll,
            ..Default::default()
        });
        let actor = cellview::ActorData {
            skeleton: skeleton.clone(),
            transform: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            reference: 0x1234,
            base: 0x1235,
            position: [0.0; 3],
            female: false,
        };
        let rig = ActorRig::new(skeleton, 1.0, 0.0);
        let part = |name: &str, node: &str, kind: u8| {
            Some(world::body_parts::BodyPart {
                name: name.into(),
                node: node.into(),
                target: node.into(),
                damage_mult: 1.0,
                flags: 0,
                part_type: kind,
                health_percent: 50,
                actor_value: 25,
                to_hit_chance: 30,
                explode_chance: 0,
                limb_model: None,
            })
        };
        let mut parts = vec![None; 15];
        parts[0] = part("Torso", "Bip01", 0);
        parts[1] = part("Head", "Bip01 Neck1", 1);
        let data = BodyPartData {
            form_id: FormId(0x1D),
            editor_id: None,
            model: None,
            parts,
            ragdoll: None,
        };
        (Walker::new(&actor), rig, data)
    }

    #[test]
    fn shots_must_meet_a_body_and_land_on_its_part() {
        let (walker, actor_rig, data) = person(true);
        let bones = &actor_rig.skeleton.bones;
        let rig = Some((&walker, &actor_rig));
        let east = [1.0, 0.0, 0.0];
        // Through the head's ball: 8 short of its centre, on the head's bone.
        let cylinder = ray_body([-100.0, 0.0, 110.0], east, [0.0; 3], 25.0, 130.0);
        let met = ray_actor(
            rig,
            Some(&data),
            ([-100.0, 0.0, 110.0], east),
            cylinder,
            false,
            0.0,
        );
        let (d, bone) = met.unwrap();
        assert!((d - 92.0).abs() < 1e-3, "{d}");
        assert_eq!(data.part_of_bone(bones, bone.unwrap()), Some(1));
        // Through the bounds but no body: a shot misses ...
        let chest = ([-100.0, 0.0, 80.0], east);
        let cylinder = ray_body(chest.0, east, [0.0; 3], 25.0, 130.0);
        assert!(cylinder.is_some());
        assert!(ray_actor(rig, Some(&data), chest, cylinder, false, 0.0).is_none());
        // ... a blow lands on the nearest bone with a part: the spine (10
        // off), the torso's.
        let (_, bone) = ray_actor(rig, Some(&data), chest, cylinder, true, 0.0).unwrap();
        assert_eq!(bone, Some(2));
        assert_eq!(data.part_of_bone(bones, 2), Some(0));
    }

    #[test]
    fn without_bodies_the_bounds_and_the_nearest_bone_decide() {
        let (walker, rig, data) = person(false);
        let rig = Some((&walker, &rig));
        let east = [1.0, 0.0, 0.0];
        let eye = [-100.0, 0.0, 108.0];
        let cylinder = ray_body(eye, east, [0.0; 3], 25.0, 130.0);
        let (d, bone) = ray_actor(rig, Some(&data), (eye, east), cylinder, false, 0.0).unwrap();
        assert!((d - 75.0).abs() < 1e-3);
        // The camera node (108) is nearest but gives no part: the head's
        // bone (110).
        assert_eq!(bone, Some(4));
    }
}

/// Weapons placed objects fired (`FireWeapon`: shooter, weapon), for
/// [`object_shots`].
#[derive(Resource, Default)]
pub struct ObjectShots(pub Vec<(FormId, FormId)>);

/// Carries out `FireWeapon` (`00523150` for something that isn't an
/// actor): the shot leaves the object's projectile node (else its
/// position) along its facing (`world::more_functions::traps::shot_from`),
/// each pellet within the weapon's cone as the player's do, to the
/// projectile's range; the first person met (the player or someone about,
/// by their bounds) before a wall takes the hit. Not yet: projectiles in
/// flight, hit shapes (bounds only), the weapon's own node name, objects
/// made by `PlaceAtMe`.
#[allow(clippy::too_many_arguments)]
pub fn object_shots(
    game: Res<GameFiles>,
    scripts: Res<Scripts>,
    mut state: ResMut<DialogueState>,
    mut shots: ResMut<ObjectShots>,
    collision: Res<CellCollision>,
    mut sounds: ResMut<SoundRequests>,
    rigs: Query<(&Walker, &ActorRig)>,
    mut nodes: Local<HashMap<String, Option<nif::math::Transform>>>,
) {
    use world::more_functions::traps;
    if shots.0.is_empty() {
        return;
    }
    let order = &game.0.order;
    let state = &mut state.0;
    for (from, weapon) in std::mem::take(&mut shots.0) {
        let (Some(w), Some(p)) = (
            Weapon::load(order, weapon),
            world::placement_of(order, from),
        ) else {
            continue;
        };
        let node = p.model.as_ref().and_then(|m| {
            *nodes.entry(m.to_ascii_lowercase()).or_insert_with(|| {
                let bytes = game.0.assets.read(&format!("meshes\\{m}")).ok().flatten()?;
                let nif = nif::Nif::parse(bytes).ok()?;
                nif.placed_node(traps::PROJECTILE_NODE)
                    .or_else(|| nif.placed_node(traps::PROJECTILE_NODE_ALT))
            })
        });
        let (origin, aim) = traps::shot_from(p.position, p.rotation, p.scale, node);
        let (count, cone) = w.shot(order, None);
        let reach = w.range(order).unwrap_or(SHOT_RANGE);
        let pellet = {
            let mut w = w.clone();
            w.damage /= count.max(1) as f32;
            w
        };
        if let Some(s) = w.sound {
            sounds.0.push(s);
        }
        let heading = aim[0].atan2(aim[1]);
        let pitch = aim[2].clamp(-1.0, 1.0).asin();
        // Who can be met: the player and the people about, by their bounds.
        let mut bodies: Vec<(FormId, [f32; 3], f32, f32)> = Vec::new();
        if let Some(feet) = state.player_position {
            let shape = physics::CharacterShape::PLAYER;
            bodies.push((PLAYER_REF, feet, shape.radius, shape.height));
        }
        for (walker, _) in &rigs {
            if state.dead.contains(&walker.reference) {
                continue;
            }
            let base =
                world::scripting::base_of(order, walker.reference).unwrap_or(walker.reference);
            let (half, height) = body(order, base);
            bodies.push((
                walker.reference,
                walker.position,
                half * walker.scale,
                height * walker.scale,
            ));
        }
        for _ in 0..count {
            let unit = |v: u64| (v % 1_000_000) as f32 / 1_000_000.0;
            let r = cone * unit(state.roll());
            let theta = std::f32::consts::TAU * unit(state.roll());
            let (h, p) = (heading + r * theta.cos(), pitch + r * theta.sin());
            let dir = [h.sin() * p.cos(), h.cos() * p.cos(), p.sin()];
            let wall = collision
                .0
                .raycast(origin, dir, reach)
                .map_or(reach, |(d, _)| d);
            let met = bodies
                .iter()
                .filter_map(|(who, feet, radius, height)| {
                    ray_body(origin, dir, *feet, *radius, *height).map(|d| (d, *who))
                })
                .filter(|(d, _)| *d <= wall)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((d, target)) = met else {
                continue;
            };
            let hit =
                Runner::new(order, &scripts.0, state).hit_at(from, target, Some(&pellet), None);
            // An object isn't someone to fight back against.
            if state.combat.get(&target) == Some(&from) {
                state.combat.remove(&target);
            }
            if let Some(hit) = hit {
                println!("{from} shot {target} at {d:.0} units for {:.1}.", hit.dealt);
            }
        }
    }
}
