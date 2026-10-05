//! People and creatures noticing and fighting, as the game's combat AI
//! does (`world::combat_ai`, read from its code; this file only measures
//! and moves):
//!
//! - Each actor's detection run (every 0.3 s in combat, staggered up to
//!   5 s more out of it; none farther than 8192 units from the player)
//!   works out its detection value for the player and everyone else
//!   loaded. A value rising above −20 starts a fight where aggression and
//!   factions say so; allies join by their Assistance; the unaggressive run
//!   from those much stronger who'd attack them.
//! - Gunmen keep within their weapon's band, strafing every 2–5 s, and fire
//!   at the game's pace (semi-automatic: after the attack and a random
//!   delay; automatic: 1 s bursts, 1 s pauses), only at a target in sight
//!   within the aim arc. Melee fighters run in, fast-walk the last 64 units,
//!   and after each attack roll attack or hold by their combat style.
//! - A target unseen for 15 s is searched for; unseen for 30 s (never seen)
//!   or 60 s (and more than 4096 units away) it's given up, and they go
//!   back to their packages.
//!
//! Measured here (guesses where the game's way isn't traced): lines of
//! sight are rays through the cell's collision 60 units above the feet;
//! the spot in the band a gunman moves to is on the line to the target,
//! halfway into the band (the game searches the navmesh, `009d5000`); a
//! search walks to where the target was last seen, then to random spots
//! within the smallest search radius every `fCombatSearchAreaUpdateTime`;
//! someone fleeing runs `fCombatFleeNormalDistance` (2048) straight away
//! from the threat until they no longer notice it; paths toward a moving
//! target are made again every half second. Not done: crouching, dodging,
//! cover, blocking (no block animations are played, so the block score is 0
//! as for those without one), suppressive fire, reloads, grenades, spread
//! (every shot hits), the hit landing at the attack animation's hit key.

use std::collections::HashSet;

use esm::{FormId, LoadOrder};
use world::ai::NavMesh;
use world::combat::Weapon;
use world::combat_ai::{
    self, Approach, CombatStyle, Engage, EngageMove, Gait, MeleeChoice, MeleeSituation,
    RangedAttack, SettingCache, TargetMemory,
};
use world::dialogue::PLAYER_REF;
use world::scripting::{Facts, GameState, Runner, ScriptCache};

use crate::ai::{distance, step, Walker};

/// How often a path toward a moving target is made again, seconds (not
/// read from the game).
const REPATH_SECONDS: f32 = 0.5;

/// Someone others can notice, as the frame began.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Seen {
    pub reference: FormId,
    pub position: [f32; 3],
    pub moving: bool,
    pub running: bool,
    /// Attacking now (an attack under way).
    pub attacking: bool,
    /// Their collision radius.
    pub radius: f32,
}

/// What an actor fights with, read once: its combat style, a creature's
/// reach and type, its collision radius, how fast it walks and runs, and
/// how long its attack animation lasts (the weapon kind's for people, its
/// own for creatures).
#[derive(Debug, Clone)]
pub(crate) struct Kit {
    pub style: CombatStyle,
    pub creature: Option<(f32, u8)>,
    pub radius: f32,
    pub walk: f32,
    pub run: f32,
    pub attack_animation: f32,
}

impl Kit {
    /// Read from the records and the skeleton: people's radius
    /// [`combat_ai::PERSON_RADIUS`]; a creature's from its skeleton's bound
    /// (`combat_ai::creature_radius`), else the game's 25 × scale for a
    /// creature without one; walking at the game's speed
    /// (`world::animation::base_speed`: `fMoveBaseSpeed` × SpeedMult ÷
    /// 100, × the scale; the legs' condition is applied where they move)
    /// and running `fMoveRunMult` times that; the attack animation's
    /// length (1 s without one: a guess).
    pub fn read(
        order: &LoadOrder,
        state: &GameState,
        walker: &Walker,
        skeleton: &preview::cell::ActorSkeleton,
    ) -> Kit {
        let creature = world::combat::creature_reach(order, walker.reference);
        let radius = match (creature, skeleton.bound) {
            (Some(_), Some(b)) => combat_ai::creature_radius(b.half_extents, walker.scale),
            (Some(_), None) => 25.0 * walker.scale,
            (None, _) => combat_ai::PERSON_RADIUS,
        };
        let walk = world::animation::base_speed(order, state, walker.reference) * walker.scale;
        let run = walk * world::animation::run_mult(order);
        let attack_animation = skeleton
            .attack
            .as_ref()
            .map_or(1.0, |a| (a.stop - a.start).max(0.1));
        Kit {
            style: CombatStyle::of(order, walker.reference),
            creature,
            radius,
            walk,
            run,
            attack_animation,
        }
    }

    /// How long an attack lasts (`Weapon::attack_seconds`: a gun's 1 ÷
    /// its attack shots a second, a melee weapon's attack animation at its
    /// attack multiplier), else the attack animation.
    pub fn attack_seconds(&self, weapon: Option<&Weapon>) -> f32 {
        weapon.map_or(self.attack_animation, |w| {
            w.attack_seconds(self.attack_animation)
        })
    }
}

/// A move under way in a fight: its gait, whether they keep facing the
/// target meanwhile (strafing), for a chase the distance from the target
/// at which it ends, and whether it ends with the target in sight again.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Move {
    gait: Gait,
    facing: bool,
    chase: Option<f32>,
    until_seen: bool,
}

impl Move {
    /// A move to a spot.
    fn to_spot(gait: Gait, facing: bool) -> Move {
        Move {
            gait,
            facing,
            chase: None,
            until_seen: false,
        }
    }
}

/// A fight under way: what's known of the target, the gunman's and the
/// swordsman's timers, the move under way.
#[derive(Debug, Clone)]
pub(crate) struct Fight {
    pub memory: TargetMemory,
    engage: Engage,
    ranged: RangedAttack,
    /// When the attack under way ends; holding until when.
    attack_until: f32,
    holding: bool,
    hold_until: f32,
    /// The target's last detection value and whether it was in sight.
    in_sight: bool,
    moving: Option<Move>,
    repath_at: f32,
    /// Searching: whether the last known spot has been reached, and when the
    /// next spot is due.
    searched_spot: bool,
    search_at: f32,
}

impl Fight {
    fn new(target: FormId, now: f32, at: [f32; 3]) -> Fight {
        Fight {
            memory: TargetMemory::new(target, now, at),
            engage: Engage::default(),
            ranged: RangedAttack::default(),
            attack_until: f32::NEG_INFINITY,
            holding: false,
            hold_until: f32::NEG_INFINITY,
            in_sight: true,
            moving: None,
            repath_at: f32::NEG_INFINITY,
            searched_spot: false,
            search_at: f32::NEG_INFINITY,
        }
    }
}

/// Random numbers for a frame's choices, from the state's dice.
pub(crate) struct Dice(u64);

impl Dice {
    pub fn new(state: &mut GameState) -> Dice {
        Dice(state.roll().max(1))
    }

    pub fn roll(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    /// 0 up to (not including) 1.
    pub fn unit(&mut self) -> f32 {
        (self.roll() >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Whether nothing solid is between two people (from 60 units above their
/// feet: the game's eye points aren't traced).
pub(crate) fn clear_between(collision: &physics::Collider, from: [f32; 3], to: [f32; 3]) -> bool {
    let a = [from[0], from[1], from[2] + 60.0];
    let b = [to[0], to[1], to[2] + 60.0];
    let d = distance(a, b);
    if d < 1.0 {
        return true;
    }
    let dir = [(b[0] - a[0]) / d, (b[1] - a[1]) / d, (b[2] - a[2]) / d];
    collision
        .raycast(a, dir, d)
        .is_none_or(|(hit, _)| hit >= d - 10.0)
}

/// What noticing needs besides the state.
pub(crate) struct Noticing<'a> {
    pub order: &'a LoadOrder,
    pub settings: &'a SettingCache,
    pub collision: &'a physics::Collider,
    pub now: f32,
}

/// One actor's detection run (`008e40d0`, `008ff350`): its value for
/// everyone in `others` (the player first), what it knows of its target,
/// and, out of combat, a fight started (on noticing someone it would
/// attack, or a friend's enemy), or a flight.
pub(crate) fn detect(n: &Noticing, state: &mut GameState, walker: &mut Walker, others: &[Seen]) {
    let order = n.order;
    let me = walker.reference;
    let s = |name: &str, d: f32| n.settings.get(order, name, d);
    let max = s("fSneakMaxDistance", 1500.0)
        * if state.player_world.is_some() {
            s("fSneakExteriorDistanceMult", 2.0)
        } else {
            1.0
        };
    let mut values: Vec<(FormId, i32, bool, [f32; 3])> = Vec::new();
    {
        let facts = Facts {
            order,
            state,
            speaker: None,
        };
        for o in others {
            if o.reference == me || state.dead.contains(&o.reference) {
                continue;
            }
            let d = distance(walker.position, o.position);
            let sight = d < max && clear_between(n.collision, walker.position, o.position);
            let motion = (o.reference != PLAYER_REF).then_some((o.moving, o.running));
            if let Some(v) = combat_ai::detection_value(&facts, me, o.reference, sight, motion, &s)
            {
                values.push((o.reference, v, sight, o.position));
            }
        }
    }
    // Kept with the detection data, for `GetLineOfSight` (`008f6930`).
    for &(r, _, sight, _) in &values {
        world::more_functions::report_detection_sight(state, me, r, sight);
    }
    let noticed_min = s("fSneakNoticedMin", -20.0);
    let in_combat = state.combat.contains_key(&me);
    let mut start = None;
    for &(r, v, sight, at) in &values {
        if r == PLAYER_REF {
            walker.detected_player = v;
        }
        let noticed = v as f32 > noticed_min;
        let rising = noticed && walker.noticed.insert(r);
        if !noticed {
            walker.noticed.remove(&r);
            if walker.fleeing == Some(r) {
                walker.fleeing = None;
                walker.path.clear();
                walker.forget_package(n.now);
            }
        }
        if let Some(f) = walker.fight.as_mut().filter(|f| f.memory.target == r) {
            f.in_sight = sight;
            if v > 0 {
                f.memory.saw(n.now, at);
            }
        }
        if in_combat || start.is_some() {
            continue;
        }
        if rising && combat_ai::starts_combat(order, state, me, r, v, &s) {
            start = Some(r);
            continue;
        }
        if rising && walker.fleeing.is_none() && combat_ai::flees_on_sight(order, state, me, r, &s)
        {
            println!("{:.1} s: {me} runs from {r}.", n.now);
            walker.fleeing = Some(r);
            walker.path.clear();
        }
        if v > 0 {
            let value_of = |x: FormId| values.iter().find(|e| e.0 == x).map(|e| e.1);
            if let Some(enemy) = combat_ai::assists_against(order, state, me, r, v, value_of) {
                println!("{:.1} s: {me} helps {r} against {enemy}.", n.now);
                start = Some(enemy);
            }
        }
    }
    if let Some(t) = start {
        let value = values.iter().find(|e| e.0 == t).map_or(0, |e| e.1);
        println!(
            "{:.1} s: {me} notices {t} (detection {value}) and attacks.",
            n.now
        );
        state.combat.insert(me, t);
        walker.fleeing = None;
    }
}

/// What a fight needs besides the state and the fighter.
pub(crate) struct FightCtx<'a> {
    pub order: &'a LoadOrder,
    pub scripts: &'a ScriptCache,
    pub settings: &'a SettingCache,
    pub mesh: &'a NavMesh,
    pub sounds: &'a mut crate::sounds::SoundRequests,
    /// Hits made, for their sounds and effects (`hiteffects`).
    pub hits: &'a mut crate::hiteffects::HitReports,
    pub others: &'a [Seen],
    /// Turning settings (`world::movement`).
    pub moves: &'a world::movement::MoveSettings,
    pub now: f32,
    pub dt: f32,
}

/// What a fight frame did: moving (at what gait), and whether an attack
/// began.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FightFrame {
    pub gait: Option<Gait>,
    pub attacked: bool,
}

/// Reports a blow or shot that hurt `target` (`hiteffects`: its sounds,
/// heard within their distances of where it struck, and the hurt or death
/// cry). Where people's attacks meet a body isn't worked out here, so the
/// point is where the target stands.
fn report_hit(
    c: &mut FightCtx,
    state: &GameState,
    attacker: FormId,
    (target, at): (FormId, [f32; 3]),
    weapon: Option<FormId>,
    damage: f32,
) {
    c.hits.0.push(crate::hiteffects::HitReport {
        attacker,
        target: Some(target),
        weapon,
        point: at,
        havok: None,
        damage,
        killed: state.dead.contains(&target),
    });
}

/// Where someone is now.
fn position_of(order: &LoadOrder, state: &GameState, who: FormId) -> Option<[f32; 3]> {
    if who == PLAYER_REF {
        state.player_position
    } else {
        state.place(order, who).map(|p| p.2)
    }
}

/// One frame of a fight against `target`.
pub(crate) fn fight(
    c: &mut FightCtx,
    state: &mut GameState,
    walker: &mut Walker,
    kit: &Kit,
    target: FormId,
) -> FightFrame {
    let order = c.order;
    let me = walker.reference;
    let settings = c.settings;
    let s = |name: &str, d: f32| settings.get(order, name, d);
    let Some(goal) = position_of(order, state, target) else {
        return FightFrame::default();
    };
    let mut fight = match walker.fight.take() {
        Some(f) if f.memory.target == target => f,
        _ => {
            walker.path.clear();
            Fight::new(target, c.now, goal)
        }
    };
    let d = distance(walker.position, goal);
    if fight
        .memory
        .gives_up(c.now, d, state.dead.contains(&target), &s)
    {
        println!("{:.1} s: {me} gives up on {target}.", c.now);
        state.combat.remove(&me);
        end_fight(walker, c.now);
        return FightFrame::default();
    }
    let mut dice = Dice::new(state);
    let frame = if fight.memory.searching(c.now, &s) {
        search(c, state, walker, kit, &mut fight, &mut dice)
    } else {
        let weapon = world::combat::weapon_in_hand(order, state, me);
        match weapon.as_ref().filter(|w| !w.is_melee()) {
            Some(w) => ranged(
                c,
                state,
                walker,
                kit,
                &mut fight,
                (w, goal, target),
                &mut dice,
            ),
            None => melee(
                c,
                state,
                walker,
                kit,
                &mut fight,
                (weapon.as_ref(), goal, target),
                &mut dice,
            ),
        }
    };
    walker.fight = Some(fight);
    frame
}

/// A fight over: back to their packages at once.
pub(crate) fn end_fight(walker: &mut Walker, now: f32) {
    walker.fight = None;
    walker.path.clear();
    walker.next = 0;
    walker.forget_package(now);
}

/// Sets a path to `to` (over the navmesh, else `straight` allowing a
/// straight line); whether there is one.
fn go(mesh: &NavMesh, walker: &mut Walker, to: [f32; 3], straight: bool) -> bool {
    let path = match mesh.path(walker.position, to) {
        Some(p) => p,
        None if straight => vec![walker.position, to],
        None => {
            walker.clear_path();
            return false;
        }
    };
    // In a fight the walk starts at once (no turn in place first).
    walker.set_path(path, 0.0, false, &world::movement::MoveSettings::defaults());
    true
}

/// How far off to the side (radians, either way) and up or down `to` is
/// for someone at `from` facing `heading` (clockwise from north).
fn offsets(from: [f32; 3], heading: f32, to: [f32; 3]) -> (f32, f32) {
    let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
    let mut yaw = (dx.atan2(dy) - heading).rem_euclid(std::f32::consts::TAU);
    if yaw > std::f32::consts::PI {
        yaw -= std::f32::consts::TAU;
    }
    (yaw, dz.atan2(dx.hypot(dy)))
}

/// Turns to face `to`: in place, at the combat rate (225°/s for people,
/// `fAICombatTurnSpeedScale`; `ai::face`).
fn face(walker: &mut Walker, to: [f32; 3], c: &FightCtx) {
    let (dx, dy) = (to[0] - walker.position[0], to[1] - walker.position[1]);
    if dx.hypot(dy) > 1.0 {
        crate::ai::face(walker, dx.atan2(dy), c.dt, true, c.moves);
    }
}

/// Walks the move under way, at `legs` × the gait's speed (crippled legs:
/// `world::body_parts::leg_speed_mult`); whether they moved.
fn walk_move(
    walker: &mut Walker,
    fight: &Fight,
    kit: &Kit,
    (goal, legs): ([f32; 3], f32),
    dt: f32,
) -> bool {
    let Some(m) = fight.moving else {
        return false;
    };
    // Strafing, they keep facing the target (stepping sideways): the
    // walking turn aims at it, not along the path (`009e4450`).
    if m.facing {
        walker.face_point = Some(goal);
    }
    let walking = step(walker, m.gait.speed(kit.walk, kit.run) * legs, dt);
    walker.face_point = None;
    walking
}

/// A gunman's frame: keeping to the band (`Engage`), and shooting
/// (`RangedAttack`).
fn ranged(
    c: &mut FightCtx,
    state: &mut GameState,
    walker: &mut Walker,
    kit: &Kit,
    fight: &mut Fight,
    (w, goal, target): (&Weapon, [f32; 3], FormId),
    dice: &mut Dice,
) -> FightFrame {
    let order = c.order;
    let settings = c.settings;
    let s = |name: &str, d: f32| settings.get(order, name, d);
    let now = c.now;
    let reach = w
        .projectile
        .and_then(|p| world::combat::projectile_reach(order, p));
    let band = combat_ai::ranged_band(Some((w, reach)), &kit.style, &s);
    let d = distance(walker.position, goal);
    // A move ends when its path does, a chase within its distance, an
    // approach once the target is in sight again.
    if let Some(m) = fight.moving {
        let done = walker.next >= walker.path.len()
            || m.chase.is_some_and(|stop| d <= stop)
            || (m.until_seen && fight.memory.unseen_for(now) <= 0.5);
        if done {
            walker.path.clear();
            fight.moving = None;
            fight.engage.arrived(now, dice.unit());
        } else if m.chase.is_some() && now >= fight.repath_at {
            fight.repath_at = now + REPATH_SECONDS;
            go(c.mesh, walker, goal, true);
        }
    }
    let unseen = fight.memory.unseen_for(now);
    let decision = fight
        .engage
        .update(now, d, &band, fight.in_sight, unseen, &mut || dice.unit());
    let toward = {
        let (dx, dy) = (goal[0] - walker.position[0], goal[1] - walker.position[1]);
        let l = dx.hypot(dy).max(1e-3);
        [dx / l, dy / l]
    };
    let at = |p: [f32; 3], along: f32, side: f32| {
        [
            p[0] + toward[0] * along + toward[1] * side,
            p[1] + toward[1] * along - toward[0] * side,
            p[2],
        ]
    };
    let started = match decision {
        EngageMove::Stay => None,
        EngageMove::Strafe { distance, left } => {
            let side = if left { -distance } else { distance };
            let spot = at(walker.position, 0.0, side);
            go(c.mesh, walker, spot, false).then_some(Move::to_spot(Gait::FastWalk, true))
        }
        EngageMove::ToBand { run } => {
            // On the line to the target, halfway into the band, else just
            // inside its nearer edge (a guess for the game's navmesh
            // search for a spot in range).
            let edge = if d < band.min {
                band.min + 16.0
            } else {
                band.optimal - 16.0
            };
            let gait = if run { Gait::Run } else { Gait::FastWalk };
            [(band.min + band.optimal) * 0.5, edge]
                .into_iter()
                .any(|want| go(c.mesh, walker, at(goal, -want, 0.0), false))
                .then_some(Move::to_spot(gait, false))
        }
        EngageMove::Step { distance, closer } => {
            let spot = at(
                walker.position,
                if closer { distance } else { -distance },
                0.0,
            );
            go(c.mesh, walker, spot, false).then_some(Move::to_spot(Gait::FastWalk, false))
        }
        EngageMove::RunAt => {
            fight.repath_at = now + REPATH_SECONDS;
            go(c.mesh, walker, goal, true).then_some(Move {
                gait: Gait::Run,
                facing: false,
                chase: Some(128.0),
                until_seen: false,
            })
        }
        EngageMove::Approach => {
            fight.repath_at = now + REPATH_SECONDS;
            go(c.mesh, walker, goal, true).then_some(Move {
                gait: Gait::FastWalk,
                facing: false,
                chase: Some(kit.radius + combat_ai::PERSON_RADIUS),
                until_seen: true,
            })
        }
    };
    if decision != EngageMove::Stay {
        println!(
            "{now:.1} s: {} at {d:.0} units (band {:.0}–{:.0}): {decision:?}{}",
            walker.reference,
            band.min,
            band.optimal,
            if started.is_some() { "" } else { " (no path)" }
        );
        match started {
            Some(m) => fight.moving = Some(m),
            // Nowhere to go: the timer starts again.
            None => fight.engage.arrived(now, dice.unit()),
        }
    }
    let legs = world::body_parts::leg_speed_mult(order, state, walker.reference);
    let walking = walk_move(walker, fight, kit, (goal, legs), c.dt);
    if !walking {
        face(walker, goal, c);
    }
    // Shooting: in sight, within the style's targeting field of view, and
    // within the aim arc (or nearer than the band's minimum).
    let (yaw, pitch) = offsets(walker.position, walker.heading, goal);
    let aimed = fight.in_sight
        && combat_ai::within_targeting_fov(&kit.style, yaw)
        && (combat_ai::within_aim_arc(w.aim_arc, yaw, pitch) || d < band.min);
    let attack = kit.attack_seconds(Some(w));
    let shoots = fight
        .ranged
        .update(now, w, &kit.style, aimed, attack, dice.unit(), &s);
    if shoots {
        if let Some(sound) = w.sound {
            c.sounds.0.push(sound);
        }
        let dealt = Runner::new(order, c.scripts, state).hit(walker.reference, target, Some(w));
        if let Some(dmg) = dealt {
            report_hit(
                c,
                state,
                walker.reference,
                (target, goal),
                Some(w.form_id),
                dmg,
            );
            let left = world::combat::health(order, state, target).unwrap_or(0.0);
            println!(
                "{now:.1} s: {} shoots {target} from {d:.0} units for {dmg:.1} ({left:.1} left).",
                walker.reference
            );
        }
    }
    FightFrame {
        gait: walking.then(|| fight.moving.map_or(Gait::FastWalk, |m| m.gait)),
        attacked: shoots,
    }
}

/// A melee fighter's frame: closing in (`melee_approach`), then rolling
/// attack or hold after each attack (`melee_choice`).
fn melee(
    c: &mut FightCtx,
    state: &mut GameState,
    walker: &mut Walker,
    kit: &Kit,
    fight: &mut Fight,
    (weapon, goal, target): (Option<&Weapon>, [f32; 3], FormId),
    dice: &mut Dice,
) -> FightFrame {
    let order = c.order;
    let settings = c.settings;
    let s = |name: &str, d: f32| settings.get(order, name, d);
    let now = c.now;
    fight.moving = None;
    let creature = if weapon.is_none() { kit.creature } else { None };
    let reach = combat_ai::melee_reach(weapon, creature, walker.scale, &s);
    let them = c.others.iter().find(|o| o.reference == target);
    let their_radius = them.map_or(combat_ai::PERSON_RADIUS, |o| o.radius);
    // Out of sight for a moment: toward where they were last seen.
    let toward = if fight.memory.unseen_for(now) > 0.5 {
        fight.memory.last_known
    } else {
        goal
    };
    let gap = combat_ai::gap(distance(walker.position, toward), kit.radius, their_radius);
    let gait = match combat_ai::melee_approach(gap, reach, &s) {
        Approach::Run => Some(Gait::Run),
        Approach::FastWalk => Some(Gait::FastWalk),
        Approach::InReach => None,
    };
    if let Some(g) = gait {
        if now >= fight.repath_at || walker.next >= walker.path.len() {
            fight.repath_at = now + REPATH_SECONDS;
            go(c.mesh, walker, toward, true);
        }
        // Crippled legs slow them (`world::body_parts::leg_speed_mult`).
        let legs = world::body_parts::leg_speed_mult(order, state, walker.reference);
        let walking = step(walker, g.speed(kit.walk, kit.run) * legs, c.dt);
        if !walking {
            face(walker, toward, c);
        }
        return FightFrame {
            gait: walking.then_some(g),
            attacked: false,
        };
    }
    walker.path.clear();
    face(walker, goal, c);
    let due = now >= fight.attack_until && (!fight.holding || now >= fight.hold_until);
    if !due {
        return FightFrame::default();
    }
    let situation = MeleeSituation {
        skill: combat_ai::melee_skill(order, state, walker.reference, weapon),
        target_attacking: them.is_some_and(|o| o.attacking),
        blocking: false,
        target_recoiling: false,
        target_unconscious: state.unconscious.contains(&target),
        unarmed: weapon.is_none(),
        can_block: false,
        target_in_combat: state.combat.contains_key(&target),
        target_is_player: target == PLAYER_REF,
        holding: fight.holding,
    };
    let scores = combat_ai::melee_scores(&kit.style, &situation, &s);
    match combat_ai::melee_choice(&scores, dice.roll(), fight.holding) {
        MeleeChoice::Attack => {
            fight.holding = false;
            // Fatigue isn't kept: always full.
            let power = combat_ai::power_attack(
                &kit.style,
                situation.target_recoiling,
                situation.target_unconscious,
                1.0,
                dice.roll(),
            );
            fight.attack_until = now + kit.attack_seconds(weapon);
            if let Some(sound) = weapon.and_then(|w| w.sound) {
                c.sounds.0.push(sound);
            }
            let dealt = Runner::new(order, c.scripts, state).strike(
                walker.reference,
                target,
                weapon,
                power,
            );
            if let Some(dmg) = dealt {
                report_hit(
                    c,
                    state,
                    walker.reference,
                    (target, goal),
                    weapon.map(|w| w.form_id),
                    dmg,
                );
                let left = world::combat::health(order, state, target).unwrap_or(0.0);
                println!(
                    "{now:.1} s: {} {} {target} for {dmg:.1} ({left:.1} left).",
                    walker.reference,
                    if power { "power-attacks" } else { "hits" }
                );
            }
            FightFrame {
                gait: None,
                attacked: true,
            }
        }
        MeleeChoice::Hold => {
            fight.holding = true;
            fight.hold_until = now + combat_ai::hold_seconds(&kit.style, dice.unit());
            FightFrame::default()
        }
        MeleeChoice::Block | MeleeChoice::Nothing => {
            fight.holding = false;
            FightFrame::default()
        }
    }
}

/// Searching for a target unseen for 15 s: to where it was last seen, then
/// spots within the search radius around it (see the module notes).
fn search(
    c: &mut FightCtx,
    state: &mut GameState,
    walker: &mut Walker,
    kit: &Kit,
    fight: &mut Fight,
    dice: &mut Dice,
) -> FightFrame {
    let order = c.order;
    let settings = c.settings;
    let s = |name: &str, d: f32| settings.get(order, name, d);
    let now = c.now;
    let spot = fight.memory.last_known;
    let idle = walker.next >= walker.path.len();
    if idle {
        if !fight.searched_spot {
            fight.searched_spot = true;
            println!(
                "{now:.1} s: {} searches for {}.",
                walker.reference, fight.memory.target
            );
            go(c.mesh, walker, spot, false);
        } else if now >= fight.search_at {
            fight.search_at = now + s("fCombatSearchAreaUpdateTime", 5.0);
            let radius = combat_ai::search_radius(state.player_world.is_some(), &s);
            let angle = dice.unit() * std::f32::consts::TAU;
            let r = radius * dice.unit().sqrt();
            let to = [
                spot[0] + r * angle.sin(),
                spot[1] + r * angle.cos(),
                spot[2],
            ];
            go(c.mesh, walker, to, false);
        }
    }
    let walking = step(walker, kit.walk, c.dt);
    FightFrame {
        gait: walking.then_some(Gait::Walk),
        attacked: false,
    }
}

/// Someone running from `threat` (an unaggressive actor, see `detect`):
/// `fCombatFleeNormalDistance` (2048) straight away from it, else half
/// that, else a quarter (where the navmesh allows); whether they're
/// moving.
pub(crate) fn flee(
    order: &LoadOrder,
    settings: &SettingCache,
    mesh: &NavMesh,
    state: &GameState,
    walker: &mut Walker,
    kit: &Kit,
    dt: f32,
) -> bool {
    let Some(threat) = walker.fleeing else {
        return false;
    };
    let Some(from) = position_of(order, state, threat).filter(|_| !state.dead.contains(&threat))
    else {
        walker.fleeing = None;
        return false;
    };
    if walker.next >= walker.path.len() {
        let far = settings.get(order, "fCombatFleeNormalDistance", 2048.0);
        let (dx, dy) = (walker.position[0] - from[0], walker.position[1] - from[1]);
        let l = dx.hypot(dy).max(1e-3);
        for share in [1.0, 0.5, 0.25] {
            let to = [
                walker.position[0] + dx / l * far * share,
                walker.position[1] + dy / l * far * share,
                walker.position[2],
            ];
            if go(mesh, walker, to, false) {
                break;
            }
        }
    }
    step(walker, kit.run, dt)
}

/// Who's been noticed, kept per actor (see `detect`).
pub(crate) type Noticed = HashSet<FormId>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_measured_off_the_heading_both_ways() {
        use std::f32::consts::FRAC_PI_2;
        // Facing north: a target due east is a quarter turn to the right,
        // one due west a quarter to the left; one 100 up at 100 is 45° up.
        let (yaw, _) = offsets([0.0; 3], 0.0, [10.0, 0.0, 0.0]);
        assert!((yaw - FRAC_PI_2).abs() < 1e-5);
        let (yaw, _) = offsets([0.0; 3], 0.0, [-10.0, 0.0, 0.0]);
        assert!((yaw + FRAC_PI_2).abs() < 1e-5);
        // Facing east, a target just north of east is a little left.
        let (yaw, pitch) = offsets([0.0; 3], FRAC_PI_2, [100.0, 1.0, 100.0]);
        assert!(yaw < 0.0 && yaw > -0.02, "{yaw}");
        assert!((pitch - std::f32::consts::FRAC_PI_4).abs() < 1e-3);
    }

    #[test]
    fn dice_give_numbers_from_zero_to_one() {
        let mut dice = Dice(0x1234_5678_9ABC_DEF0);
        let draws: Vec<f32> = (0..1000).map(|_| dice.unit()).collect();
        assert!(draws.iter().all(|u| (0.0..1.0).contains(u)));
        // Spread over the range.
        assert!(draws.iter().any(|u| *u < 0.1) && draws.iter().any(|u| *u > 0.9));
    }
}
