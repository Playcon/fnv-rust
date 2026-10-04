//! People following their AI packages (`world::ai`) by the game's rules
//! (`world::movement`, `world::social`, read from its code). Packages are
//! looked at again every 20 s, on each game-hour change and when forced
//! (`world::movement::PackageClock`); one that sends someone somewhere
//! gets a path over the navmesh (the interior's, or outdoors the 3 × 3
//! squares around the player, joined): first an in-place turn toward it,
//! then the walk at their walk animation's own speed, turning by the
//! walking rule, the body moving toward the steering point while the
//! facing catches up; others in the way are waited for or gone round
//! (`world::movement::Avoidance`). Dialogue packages walk up to their
//! target and talk; people greet the player, chatter and talk with each
//! other (`chatter` says the lines). Furniture, idles and sandbox packages
//! are `sitting`'s; noticing others, fighting and running away are
//! `fighting`'s. People out of sight walk their paths in game-time steps
//! ([`move_offstage`]).

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use cellview::{space, ActorData};
use esm::FormId;
use world::ai::{current_package, destination, DialogueStep, NavMesh};
use world::dialogue::{Speaker, PLAYER_REF};
use world::furniture::SitState;
use world::head_track::{Candidate, HeadTrack, Slot};
use world::movement::{
    self as mv, AvoidStep, Avoidance, MoveSettings, Obstacle, PackageClock, ProcessLevel, Turn,
    TurnSide,
};
use world::social::{Social, SocialSettings};

use crate::actors::ActorRig;
use crate::chatter::Lines;
use crate::dialogue::{Conversation, DialogueState, Talkers};
use crate::sitting::{Ctx, Life, Seats};
use crate::GameFiles;

/// The settings people's moving and talking read, once.
#[derive(Resource)]
pub struct Moves {
    pub settings: MoveSettings,
    pub social: SocialSettings,
    /// `fAIMoveDistanceToRecalcFollowPath` (300): a target that moved this
    /// far since the path was made gets a new one.
    pub recalc_follow: f32,
    /// Head tracking's settings (`world::head_track`): game settings and
    /// `[HeadTracking]` in the INI.
    pub head_track: world::head_track::Settings,
}

impl Moves {
    pub fn new(game: &cellview::Game) -> Moves {
        let order = &game.order;
        let g = |name: &str, d: f32| world::scripting::game_setting(order, name).unwrap_or(d);
        Moves {
            settings: MoveSettings::read(order, &|section, key| game.settings.float(section, key)),
            social: SocialSettings::read(order),
            recalc_follow: g("fAIMoveDistanceToRecalcFollowPath", 300.0),
            head_track: world::head_track::Settings::read(
                |name| world::scripting::game_setting(order, name),
                |section, key| game.settings.float(section, key),
            ),
        }
    }
}

/// A conversation between two people (`StartConversation`, the type-0x1c
/// package `008b2170`): the one who started walks up to within `reach` of
/// the other (90; 200 when seated), who waits facing them; then the lines,
/// worked out as it began (`world::social::conversation`), each said once
/// the last is over and at least 2 s after it began (the playback's pause
/// timer, `009ee0a0`; which of its 2/10/50 s pauses applies when isn't
/// traced, so the line's own length and 2 s are taken).
#[derive(Debug, Clone)]
pub struct Chat {
    pub with: FormId,
    pub starter: bool,
    pub reach: f32,
    pub lines: Vec<world::social::Line>,
    pub next: usize,
    /// When the last line began; whether its speaker is still saying it.
    pub last_line: f32,
    pub talking: bool,
}

/// Conversations between people, by each of the two.
#[derive(Resource, Default)]
pub struct Chats(pub HashMap<FormId, Chat>);

/// `StartConversation` asked of someone by a script (`005c8740` →
/// `008b2170`): who, with whom, about what. The one asked comes up first
/// (the conversation package, type 0x1c): to within 90 of the other (200
/// when seated), then the dialogue menu with the player, or a conversation
/// with anyone else (`start_chat`).
#[derive(Resource, Default)]
pub struct Starts(pub Vec<(FormId, FormId, Option<FormId>)>);

/// A person's place and what they're doing.
#[derive(Component)]
pub struct Walker {
    pub reference: FormId,
    pub position: [f32; 3],
    /// Radians clockwise from north.
    pub heading: f32,
    pub scale: f32,
    pub(crate) package: Option<FormId>,
    pub(crate) package_kind: Option<u8>,
    pub(crate) target: Option<[f32; 3]>,
    /// The reference the package sends them to, if any, and where it stood
    /// when the path was made.
    target_ref: Option<FormId>,
    path_target: Option<[f32; 3]>,
    pub(crate) path: Vec<[f32; 3]>,
    /// The point walked toward (the steering point's segment end).
    pub(crate) next: usize,
    /// Path progress: point index and fraction (`009e3d50`).
    pub(crate) progress: f32,
    /// A point to keep facing while walking (strafing in a fight).
    pub(crate) face_point: Option<[f32; 3]>,
    /// The turn in place under way, their turning speed (degrees a second)
    /// and in-place rates (radians a second, out of and in combat), and the
    /// side turned this frame (for the turn animation).
    pub(crate) turn: Turn,
    pub(crate) turn_speed: f32,
    pub(crate) rates: [f32; 2],
    pub(crate) turning: Option<TurnSide>,
    /// When the package is looked at again; forced when set.
    clock: PackageClock,
    pub(crate) evaluate: bool,
    /// Placed (or moved by a script) since the last frame: start from the
    /// state's position.
    fresh: bool,
    avoidance: Avoidance,
    /// Units a second, as last moved.
    pub(crate) velocity: [f32; 3],
    /// The player's detection value at their last detection run.
    pub(crate) detected_player: i32,
    pub(crate) social: Option<Social>,
    /// A dialogue package's talk (or line) given: nothing more until the
    /// package is looked at again.
    dialogue_done: bool,
    /// A dialogue package's travel step is over: its procedures go on from
    /// there (walking up to the target may take them off the place; the
    /// game doesn't go back to the travel, only a restart of the package
    /// does).
    travelled: bool,
    /// Whom they look at (and so turn the body to, `008a3100`): the head-
    /// track target slots (`world::head_track`).
    pub(crate) head_track: HeadTrack,
    /// `bDisableHeadTracking:HeadTracking`: the head doesn't follow.
    head_tracking_off: bool,
    /// How they fight, once read (`fighting::Kit`), and the fight under way.
    pub(crate) kit: Option<crate::fighting::Kit>,
    pub(crate) fight: Option<crate::fighting::Fight>,
    /// When their next detection run is due, and whom they've noticed
    /// (above −20) since that last dropped (`fighting::detect`).
    pub(crate) detect_at: f32,
    pub(crate) noticed: crate::fighting::Noticed,
    /// Running from someone (`fighting::flee`).
    pub(crate) fleeing: Option<FormId>,
    /// Fallen (dead).
    fallen: bool,
    /// Held still while the dialogue menu stops the world.
    paused: bool,
    /// A woman (for the idle tree's women's idles).
    pub(crate) female: bool,
    /// The load door they're walking to, toward a package's place
    /// elsewhere.
    door: Option<world::ai::DoorWay>,
    /// The doors their path goes through that they haven't passed yet,
    /// with each one's portal (`doors::walker_at_door`).
    pub(crate) doors_ahead: Vec<(FormId, [f32; 3])>,
    /// How near the target counts as there: the path request's radius.
    pub(crate) radius: f32,
    /// The heading to turn to when the walk ends (the end of the travel
    /// procedure: an `XMarkerHeading`'s, `world::ai::arrival_heading`), and
    /// the one being turned to now, standing.
    pub(crate) arrival: Option<f32>,
    pub(crate) facing: Option<f32>,
    /// Where they stood when their package began (the middle of a wander
    /// "near the current location").
    pub(crate) home: [f32; 3],
    /// A script's `StartConversation` to carry out ([`Starts`]), and the
    /// walk up to the player it became: the topic and the reach.
    start: Option<(FormId, Option<FormId>)>,
    talk_to: Option<(Option<FormId>, f32)>,
}

/// People turn 135° a second in place until their kit is read.
const PEOPLE_RATES: [f32; 2] = [90.0 * 1.5 * mv::ONE_DEGREE, 90.0 * 2.5 * mv::ONE_DEGREE];

impl Walker {
    /// Discard the running procedure for ResetAI (008a6ce0 / 00923c60),
    /// keeping physical placement. A new package is chosen this frame.
    fn reset_procedure(&mut self) {
        self.clear_path();
        self.package = None;
        self.package_kind = None;
        self.target = None;
        self.target_ref = None;
        self.path_target = None;
        self.dialogue_done = false;
        self.travelled = false;
        self.door = None;
        self.evaluate = true;
    }

    pub fn new(actor: &ActorData) -> Walker {
        let m = &actor.transform;
        // Column 1 is the actor's forward (+y) times its scale.
        let (fx, fy) = (m[4], m[5]);
        let scale = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-3);
        Walker::at(
            FormId(actor.reference),
            actor.position,
            fx.atan2(fy),
            scale,
            actor.female,
        )
    }

    pub(crate) fn at(
        reference: FormId,
        position: [f32; 3],
        heading: f32,
        scale: f32,
        female: bool,
    ) -> Walker {
        Walker {
            reference,
            position,
            heading,
            scale,
            package: None,
            package_kind: None,
            target: None,
            target_ref: None,
            path_target: None,
            path: Vec::new(),
            next: 0,
            progress: 0.0,
            face_point: None,
            turn: Turn::default(),
            turn_speed: 90.0,
            rates: PEOPLE_RATES,
            turning: None,
            clock: PackageClock::default(),
            evaluate: true,
            fresh: true,
            avoidance: Avoidance::default(),
            velocity: [0.0; 3],
            detected_player: i32::MIN,
            social: None,
            dialogue_done: false,
            travelled: false,
            head_track: HeadTrack::default(),
            head_tracking_off: false,
            kit: None,
            fight: None,
            detect_at: 0.0,
            noticed: Default::default(),
            fleeing: None,
            fallen: false,
            paused: false,
            female,
            door: None,
            doors_ahead: Vec::new(),
            radius: 0.0,
            arrival: None,
            facing: None,
            home: position,
            start: None,
            talk_to: None,
        }
    }

    /// Their package is looked at afresh at once (after a fight).
    pub(crate) fn forget_package(&mut self, _now: f32) {
        self.package = None;
        self.target = None;
        self.evaluate = true;
    }

    /// Whether they're on a path (walking, or turning to start it).
    pub(crate) fn on_path(&self) -> bool {
        self.next < self.path.len()
    }

    /// Whether they're walking this frame (on a path, not turning in place
    /// first).
    pub(crate) fn walking_now(&self) -> bool {
        self.on_path() && !self.turn.active
    }

    /// A new path (the path handler made for a request, `009dbdc0`): from
    /// its first point, the walk over within `radius` of its last. With
    /// `turn_first` they first turn in place toward it when more than a
    /// degree off (`009e0470` called with no time: a turn request). The
    /// doors of the old path are forgotten (a path made with its doors sets
    /// them after, `go_to`).
    pub(crate) fn set_path(
        &mut self,
        path: Vec<[f32; 3]>,
        radius: f32,
        turn_first: bool,
        settings: &MoveSettings,
    ) {
        self.target = path.last().copied();
        self.path = path;
        self.next = 1;
        self.progress = 0.0;
        self.radius = radius;
        self.avoidance = Avoidance::default();
        self.arrival = None;
        self.facing = None;
        self.doors_ahead.clear();
        if turn_first {
            if let Some(&first) = self.path.get(1) {
                let flat = (first[0] - self.position[0]).hypot(first[1] - self.position[1]);
                if flat > 1e-3 {
                    self.turn
                        .request(self.heading, mv::heading_to(self.position, first), settings);
                }
            }
        }
    }

    /// Off the path.
    pub(crate) fn clear_path(&mut self) {
        self.path.clear();
        self.next = 0;
        self.progress = 0.0;
        self.doors_ahead.clear();
    }

    /// Whom it looks at now (and turns its body to), if anyone.
    /// The head follows only while head tracking is on (`008a3100`: with
    /// `bDisableHeadTracking` the look eases out).
    pub fn looking_at(&self) -> Option<FormId> {
        self.head_track
            .current()
            .filter(|_| !self.head_tracking_off)
    }

    #[cfg(test)]
    pub(crate) fn look_at_for_test(&mut self, who: FormId) {
        self.head_track.set(Slot::Action, Some(who));
    }

    /// Where its skeleton stands in the world (game axes): turned by its
    /// heading (clockwise from north), at its scale.
    pub fn placement(&self) -> nif::Transform {
        let (s, c) = self.heading.sin_cos();
        nif::Transform {
            rotation: [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]],
            translation: self.position,
            scale: self.scale,
        }
    }

    /// Where it stands, as a transform in the game's axes (column-major):
    /// turned by its heading, at its scale.
    fn game_matrix(&self) -> [f32; 16] {
        let (s, c) = self.heading.sin_cos();
        let k = self.scale;
        let [x, y, z] = self.position;
        [
            c * k,
            -s * k,
            0.0,
            0.0,
            s * k,
            c * k,
            0.0,
            0.0,
            0.0,
            0.0,
            k,
            0.0,
            x,
            y,
            z,
            1.0,
        ]
    }
}

/// People scripts moved (`MoveTo`) since the last frame: they go where the
/// state now has them.
#[derive(Resource, Default)]
pub struct Moved(pub Vec<FormId>);

/// `--freeze-ai`: nobody's AI runs (the game's console command `tai`,
/// toggle AI): people and creatures stay where they stand, play their idle
/// and don't start conversations, fights or packages.
#[derive(Resource, Default)]
pub struct FrozenAi(pub bool);

/// The navmesh where the player is: the interior's, or outdoors the 3 × 3
/// squares around the player's, joined.
#[derive(Resource, Default)]
pub struct CellNav {
    /// The interior, or the worldspace and square, it was loaded for.
    key: Option<(FormId, Option<(i32, i32)>)>,
    mesh: NavMesh,
}

/// What people's moving uses besides the state: scripts, collision, sounds
/// to play, seats (`sitting`), the settings, lines said and conversations.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Around<'w> {
    scripts: Res<'w, crate::scripts::Scripts>,
    collision: Res<'w, crate::walk::CellCollision>,
    sounds: ResMut<'w, crate::sounds::SoundRequests>,
    seats: ResMut<'w, Seats>,
    settings: Res<'w, CombatSettings>,
    frozen: Res<'w, FrozenAi>,
    attack: Res<'w, crate::combat::PlayerAttack>,
    hits: ResMut<'w, crate::hiteffects::HitReports>,
    moves: Res<'w, Moves>,
    lines: ResMut<'w, Lines>,
    chats: ResMut<'w, Chats>,
    starts: ResMut<'w, Starts>,
    talk: ResMut<'w, crate::scripts::ScriptedTalk>,
}

/// The game settings fights ask for, looked up once
/// (`world::combat_ai::SettingCache`).
#[derive(Resource, Default)]
pub struct CombatSettings(pub world::combat_ai::SettingCache);

/// A person on screen: where and what they're doing.
type Person<'a> = (
    &'a mut Walker,
    &'a mut Life,
    &'a mut ActorRig,
    &'a mut Transform,
    &'a mut Visibility,
);

/// What the last frame knew: the dialogue menu's speaker and the player's
/// feet (for the player's speed).
#[derive(Default)]
pub struct LastFrame {
    speaker: Option<FormId>,
    player: Option<[f32; 3]>,
    /// Who came to warn the trespassing player.
    warner: Option<FormId>,
}

/// Every frame: people look at their package again when due and walk their
/// paths, use furniture, play idles and sandbox (`sitting`), greet, chatter
/// and talk. Time stands still in the dialogue menu, except that the
/// speaker turns to face the player.
#[allow(clippy::too_many_arguments)]
pub fn move_actors(
    time: Res<Time>,
    game: Res<GameFiles>,
    mut state: ResMut<DialogueState>,
    conversation: Res<Conversation>,
    mut nav: ResMut<CellNav>,
    mut talkers: ResMut<Talkers>,
    exterior: Option<Res<crate::exterior::Exterior>>,
    mut moved: ResMut<Moved>,
    mut last: Local<LastFrame>,
    mut commands: Commands,
    around: Around,
    mut actors: Query<Person>,
) {
    let Around {
        scripts,
        collision,
        mut sounds,
        mut seats,
        settings,
        attack,
        mut hits,
        frozen: frozen_ai,
        moves,
        mut lines,
        mut chats,
        mut starts,
        mut talk,
    } = around;
    let order = &game.0.order;
    let state = &mut state.0;
    let settings = &settings.0;
    let moves = &*moves;
    let moved = std::mem::take(&mut moved.0);
    // Someone sent to warn the trespassing player off (or no longer): their
    // package changed, so it's looked at again at once.
    let warner = state.living.trespass.as_ref().map(|w| w.warner);
    if warner != last.warner {
        state.evaluate.extend(warner.into_iter().chain(last.warner));
        last.warner = warner;
    }
    // In the dialogue menu time stands still, except that the speaker
    // turns to face the player. With the AI held still (`--freeze-ai`)
    // people only take the places scripts put them in.
    let in_menu = conversation.0.as_ref().is_some_and(|t| !t.is_line_only());
    let frozen = frozen_ai.0 || in_menu;
    let speaker = conversation.0.as_ref().map(|t| t.speaker());
    let menu_speaker = speaker.filter(|_| in_menu);
    // The dialogue menu closed: the speaker's package is looked at again at
    // once, and they don't greet for `fAIGreetingTimer` (`00762160`).
    let ended = last.speaker.filter(|s| Some(*s) != menu_speaker);
    last.speaker = menu_speaker;
    let key = match (state.player_world, state.player_cell, &exterior) {
        (None, Some(c), _) => (c, None),
        (Some(w), _, Some(_)) => {
            let feet = state.player_position.unwrap_or_default();
            (w, Some(world::square_of(feet)))
        }
        _ => return,
    };
    if nav.key != Some(key) {
        nav.key = Some(key);
        nav.mesh = match (key.1, &exterior) {
            (None, _) => NavMesh::load(order, key.0),
            (Some((x, y)), Some(e)) => {
                let cells: Vec<FormId> = (-1..=1)
                    .flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
                    .filter_map(|s| e.grid.cells.get(&s).copied())
                    .collect();
                NavMesh::load_cells(order, &cells)
            }
            _ => NavMesh::default(),
        };
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let game_hour = state.global(order, "GameHour").unwrap_or(12.0);
    let player_velocity = match (state.player_position, last.player) {
        (Some(p), Some(q)) if dt > 0.0 => [0, 1, 2].map(|k| (p[k] - q[k]) / dt),
        _ => [0.0; 3],
    };
    last.player = state.player_position;
    let (others, obstacles) = seen(order, state, &attack, now, &actors, player_velocity);
    let interior = state.player_world.is_none();
    let mut starts = std::mem::take(&mut starts.0);
    // How many chose whom to look at this frame (`011df674`).
    let mut head_track_choices = 0;
    for (mut walker, mut life, mut rig, mut transform, mut visibility) in &mut actors {
        let walker = &mut *walker;
        let me = walker.reference;
        // The turn in place under way (as the last frame left it) plays the
        // turn animation, while the turn state lasts (at least
        // `fActorTurnAnimMinTime`).
        rig.turning = turning_of(walker, rig.fighting);
        if Some(me) == ended {
            walker.evaluate = true;
            if let Some(s) = walker.social.as_mut() {
                s.greeted(&moves.social);
            }
        }
        // Moved by a script: start again from there, or leave if it's
        // another place.
        if moved.contains(&me) {
            walker.fresh = true;
            walker.clear_path();
            walker.target = None;
            walker.package = None;
        }
        if walker.fresh {
            let away = state
                .spaces
                .get(&me)
                .is_some_and(|(space, _)| *space != key.0);
            if away {
                *visibility = Visibility::Hidden;
                talkers.0.retain(|t| t.reference != me);
            } else if moved.contains(&me) {
                *visibility = Visibility::Inherited;
            }
        }
        if *visibility == Visibility::Hidden {
            continue;
        }
        // ResetAI is consumed before furniture's early return. It releases
        // furniture directly (0088d640), unlike EvaluatePackage or the
        // ordinary animated stand-up procedure. Never snap to a marker.
        if state.take_ai_reset(me) {
            walker.reset_procedure();
            life.idles.stop();
            life.base_idle = None;
            life.sandbox = None;
            life.activity = None;
            life.getting_up = false;
            rig.dynamic_idle = None;
            rig.overlay = None;
            rig.player.free_special_idle();
            rig.scripted_idle = None;
            println!("{now:.1} s: {me}: ResetAI released furniture and restarted its procedure.");
        }
        // A script's `StartConversation` for them.
        if let Some(i) = starts.iter().position(|s| s.0 == me) {
            let (_, to, topic) = starts.remove(i);
            walker.start = Some((to, topic));
        }
        // Someone who had moved before this place loaded (or in a loaded
        // game) starts from where they got to.
        if walker.fresh {
            walker.fresh = false;
            walker.evaluate = true;
            if let Some(&(p, h)) = state.positions.get(&me) {
                walker.position = p;
                walker.heading = h;
                *transform = Transform::from_matrix(Mat4::from_cols_array(&space::matrix(
                    &walker.game_matrix(),
                )));
                for t in talkers.0.iter_mut() {
                    if t.reference == me {
                        t.position = p;
                    }
                }
            }
        }
        // What asked them to look at someone holds while it lasts and then
        // lets go (`world::head_track`).
        head_track_asks(walker, speaker, in_menu, &lines, &chats, &moves.head_track);
        let before = walker.position;
        walker.turning = None;
        // Talking to the player in the dialogue menu: they stop where they
        // are (the walk held, not dropped) and turn in place to face the
        // player, unless seated or sitting down (who can't turn, `008843a0`):
        // the conversation's rule, the target facing the one who started it
        // every frame (`008ec460`; that the player's menu runs it is
        // inferred).
        let seated = state.furniture.contains_key(&me) || state.sitters.contains_key(&me);
        if menu_speaker == Some(me) {
            // A line of their own (chatter, a conversation) gives way.
            if lines.is_saying(me) {
                crate::chatter::hush(&mut commands, &mut lines, me);
            }
            end_chat(&mut chats, me);
            if walker.paused {
                walker.paused = false;
                rig.still = false;
            }
            rig.walking = false;
            rig.speed = 0.0;
            if !seated {
                if let Some(p) = state.player_position {
                    let flat = (p[0] - walker.position[0]).hypot(p[1] - walker.position[1]);
                    if flat > 1.0 {
                        face(
                            walker,
                            mv::heading_to(before, p),
                            dt,
                            false,
                            &moves.settings,
                        );
                    }
                }
            }
            place(walker, &mut transform, state, &mut talkers);
            continue;
        }
        if frozen {
            // The game stops the world in the dialogue menu: everyone else
            // holds still where they are, mid-stride included.
            if in_menu && !walker.paused && !state.dead.contains(&me) {
                walker.paused = true;
                rig.still = true;
            }
            continue;
        }
        if walker.paused {
            walker.paused = false;
            if !state.dead.contains(&me) {
                rig.still = false;
            }
        }
        // The dead go limp (their skeleton's ragdoll, thrown by the killing
        // blow) and do nothing more; without a ragdoll they tip over.
        if state.dead.contains(&me) {
            if !walker.fallen {
                fall(
                    walker,
                    &mut life,
                    &mut rig,
                    &mut transform,
                    state,
                    order,
                    &collision,
                    now,
                );
                crate::chatter::hush(&mut commands, &mut lines, me);
                end_chat(&mut chats, me);
            }
            continue;
        }
        if walker.kit.is_none() {
            let kit = crate::fighting::Kit::read(order, state, walker, &rig.skeleton);
            // Their turning speed and in-place rates (`world::movement`).
            let speed = mv::turning_speed(order, me, &moves.settings);
            let creature = kit.creature.is_some();
            walker.turn_speed = speed;
            walker.rates = [
                moves.settings.in_place_rate(speed, creature, false),
                moves.settings.in_place_rate(speed, creature, true),
            ];
            walker.kit = Some(kit);
        }
        // A combat style a script gave them (`SetCombatStyle`) is theirs at
        // once, in a fight under way too (`008a8010`).
        if let Some(&style) = state.more.combat_styles.get(&me) {
            if let Some(kit) = walker
                .kit
                .as_mut()
                .filter(|k| k.style.form_id != Some(style))
            {
                kit.style = world::more_functions::combat_style(order, state, me);
            }
        }
        if walker.social.is_none() {
            let mut dice = crate::fighting::Dice::new(state);
            walker.social = Some(Social::new(&moves.social, &mut || dice.unit()));
        }
        // Noticing (`fighting::detect`): each actor's detection run, every
        // 0.3 s in combat, staggered out of it; none for actors 8192 or
        // more from the player.
        let near_player = state
            .player_position
            .is_some_and(|p| distance(p, walker.position) < world::combat_ai::DETECTION_RANGE);
        if now >= walker.detect_at && near_player {
            let in_combat = state.combat.contains_key(&me);
            let unit = crate::fighting::Dice::new(state).unit();
            let s = |n: &str, d: f32| settings.get(order, n, d);
            walker.detect_at =
                now + world::combat_ai::detection_interval(in_combat, dt, others.len(), unit, &s);
            let noticing = crate::fighting::Noticing {
                order,
                settings,
                collision: &collision.0,
                now,
            };
            crate::fighting::detect(&noticing, state, walker, &others);
        }
        // Whom they look at of their own accord (`008a3100`), seated too.
        choose_head_track(
            walker,
            state,
            &others,
            &collision.0,
            &moves.head_track,
            &mut head_track_choices,
            dt,
        );
        // A fight over (its target dead, or a script's `StopCombat`): back
        // to their package.
        if !state.combat.contains_key(&me) && walker.fight.is_some() {
            crate::fighting::end_fight(walker, now);
        }
        rig.fighting = state.combat.contains_key(&me);
        if rig.fighting {
            end_chat(&mut chats, me);
        }
        if let Some(s) = walker.social.as_mut() {
            s.tick(dt);
        }
        // Furniture first: sitting down, seated, getting up (a fight gets
        // them up with the fast exit); nothing else moves them meanwhile.
        let talking = lines.is_saying(me);
        let mut ctx = Ctx {
            game: &game.0,
            state: &mut *state,
            seats: &mut seats,
            mesh: &nav.mesh,
            moves: &moves.settings,
            now,
            dt,
            fighting: rig.fighting,
            talking,
        };
        // The game's package check runs while sit state is 0, 4 or 9
        // (`008da670`). A forced EVP must therefore be honored while a
        // settled actor is still in furniture; otherwise this early-return
        // path prevents the package change from requesting the stand-up.
        let package_checked_before_furniture =
            rethink_queued_package_before_furniture(&mut ctx, walker, &mut life, game_hour);
        let in_furniture = crate::sitting::furniture_frame(&mut ctx, walker, &mut life, &mut rig);
        if in_furniture {
            rig.walking = false;
            rig.speed = 0.0;
            begin_conversation(&mut ctx, walker, &mut chats);
            let chatting = chats.0.contains_key(&me);
            if chatting {
                chat_frame(&mut ctx, walker, &mut chats, &mut lines, moves);
            } else if walker.talk_to.is_some() {
                talk_frame(&mut ctx, walker, &mut talk, moves);
            } else if life.sandbox.is_some() && !rig.fighting {
                crate::sitting::sandbox_frame(
                    &mut ctx,
                    walker,
                    &mut life,
                    &mut chats,
                    &moves.social,
                );
            }
            crate::sitting::idles_frame(&mut ctx, walker, &mut life, &mut rig);
            if !rig.fighting {
                social_frame(
                    &mut ctx, walker, &mut chats, &mut lines, moves, &others, interior,
                );
            }
            place(walker, &mut transform, state, &mut talkers);
            walker.velocity = [0.0; 3];
            continue;
        }
        if let Some(&target) = state.combat.get(&me) {
            life.activity = None;
            life.idles.stop();
            rig.overlay = None;
            let kit = walker.kit.clone().expect("read above");
            let mut ctx = crate::fighting::FightCtx {
                order,
                scripts: &scripts.0,
                settings,
                mesh: &nav.mesh,
                sounds: &mut sounds,
                hits: &mut hits,
                others: &others,
                moves: &moves.settings,
                now,
                dt,
            };
            let frame = crate::fighting::fight(&mut ctx, state, walker, &kit, target);
            rig.walking = frame.gait.is_some();
            rig.running = frame.gait == Some(world::combat_ai::Gait::Run);
            rig.speed = frame.gait.map_or(0.0, |g| {
                g.speed(kit.walk, kit.run) * world::body_parts::leg_speed_mult(order, state, me)
            });
            if frame.attacked {
                rig.attack_at = Some(now);
            }
            place(walker, &mut transform, state, &mut talkers);
            walker.velocity = velocity(before, walker.position, dt);
            continue;
        }
        // Running from someone they won't fight.
        if walker.fleeing.is_some() {
            life.activity = None;
            life.idles.stop();
            rig.overlay = None;
            let kit = walker.kit.clone().expect("read above");
            let moving = crate::fighting::flee(order, settings, &nav.mesh, state, walker, &kit, dt);
            rig.walking = moving && walker.walking_now();
            rig.running = moving;
            rig.speed = if moving { kit.run } else { 0.0 };
            if walker.fleeing.is_none() {
                walker.forget_package(now);
            }
            place(walker, &mut transform, state, &mut talkers);
            walker.velocity = velocity(before, walker.position, dt);
            continue;
        }
        rig.running = false;
        let mut ctx = Ctx {
            game: &game.0,
            state: &mut *state,
            seats: &mut seats,
            mesh: &nav.mesh,
            moves: &moves.settings,
            now,
            dt,
            fighting: false,
            talking,
        };
        // A conversation with someone (or coming up to the player for one):
        // it runs instead of the package.
        begin_conversation(&mut ctx, walker, &mut chats);
        let chatting = chats.0.contains_key(&me);
        if chatting {
            chat_frame(&mut ctx, walker, &mut chats, &mut lines, moves);
        } else if walker.talk_to.is_some() {
            talk_frame(&mut ctx, walker, &mut talk, moves);
        } else {
            // The package, looked at again when due (`008da670`): forced,
            // none, every 20 s, a new game hour; only in sit states 0, 4, 9.
            // A settled actor may have been checked immediately before the
            // furniture procedure. Leave any new request raised by a
            // procedure that ended this frame for the next frame; don't tick
            // the clock or evaluate twice.
            let forced = if package_checked_before_furniture {
                false
            } else {
                take_forced_package_evaluation(walker, ctx.state, me)
            };
            let due = !package_checked_before_furniture
                && walker
                    .clock
                    .due(dt, game_hour, forced, walker.package.is_some());
            if due && !life.getting_up {
                rethink(&mut ctx, walker, &mut life, forced);
            }
            // A dialogue package: walk up and talk (`008e8600`).
            if walker.package_kind == Some(world::ai::kinds::DIALOGUE) && !walker.dialogue_done {
                dialogue_frame(&mut ctx, walker, (&mut chats, &mut lines, &mut talk), moves);
            }
            // A sandbox: the game's choices (`sitting`).
            if life.sandbox.is_some() {
                crate::sitting::sandbox_frame(
                    &mut ctx,
                    walker,
                    &mut life,
                    &mut chats,
                    &moves.social,
                );
            }
            // Following someone, or walking to a reference that moves: a new
            // path when it moved `fAIMoveDistanceToRecalcFollowPath` since the
            // last, or they stand farther than the radius from it.
            follow_target(&mut ctx, walker, moves);
            // A wander package at its place: the wander procedure.
            if walker.package_kind == Some(world::ai::kinds::WANDER) {
                crate::sitting::wander_package_frame(&mut ctx, walker, &mut life);
            }
        }
        // The game's walking speed (`world::animation::walk_speed`:
        // `fMoveBaseSpeed` 77 × SpeedMult ÷ 100 × the legs' condition),
        // times their scale; the walk animation is played at the rate that
        // makes its root travel this (`actors`).
        let speed = world::animation::walk_speed(order, ctx.state, me) * walker.scale;
        let was_on_path = walker.on_path();
        // Others in the way: wait, or a way round (`009e5ae0`).
        let mut blocked = false;
        if walker.walking_now() {
            let ahead = [walker.heading.sin(), walker.heading.cos()];
            let running = rig.running;
            let target_ref = walker.target_ref;
            let mine: Vec<Obstacle> = obstacles.iter().filter(|o| o.who != me).copied().collect();
            let step = walker.avoidance.update(
                walker.position,
                mv::REQUEST_RADIUS,
                ahead,
                walker.velocity,
                running,
                target_ref,
                &mine,
                &moves.settings.avoidance,
                dt,
            );
            match step {
                AvoidStep::Clear => {}
                AvoidStep::Wait => blocked = true,
                AvoidStep::Repath(nodes) => {
                    if let Some(goal) = walker.path.last().copied() {
                        if let Some(path) = nav.mesh.path_avoiding(walker.position, goal, &nodes) {
                            // The same walk, round them: its radius, end
                            // heading and the doors still ahead kept.
                            let keep = std::mem::take(&mut walker.avoidance);
                            let doors = std::mem::take(&mut walker.doors_ahead);
                            let (radius, arrival) = (walker.radius, walker.arrival);
                            walker.set_path(path, radius, false, &moves.settings);
                            walker.avoidance = keep;
                            walker.arrival = arrival;
                            walker.doors_ahead = doors;
                            println!("{:.1} s: {me} goes round {} in the way.", now, nodes.len());
                        }
                    }
                }
            }
        }
        // A closed door across the path: they open it and wait while it
        // swings (`doors::walker_at_door`, the game's `009e20c0`), standing.
        let at = walker.position;
        if walker.next >= walker.path.len() {
            walker.doors_ahead.clear();
        }
        let waiting = !walker.doors_ahead.is_empty()
            && crate::doors::walker_at_door(
                order,
                ctx.state,
                &mut sounds,
                me,
                at,
                &mut walker.doors_ahead,
            );
        let on_path = if blocked || waiting {
            walker.on_path()
        } else {
            step(walker, speed, dt)
        };
        // At a load door toward somewhere else: through it.
        if was_on_path && !on_path && walker.door.is_some() {
            go_through(order, ctx.state, walker);
            life.activity = None;
            continue;
        }
        // The walk over: they turn in place to the travel's end heading
        // (`008e5e90` → `008bb5c0`), not while using furniture.
        if was_on_path && !on_path {
            walker.facing = walker.arrival.take();
        }
        if let Some(h) = walker.facing.filter(|_| !on_path) {
            let using =
                ctx.state.furniture.contains_key(&me) || ctx.state.sitters.contains_key(&me);
            if using || !face(walker, h, dt, false, &moves.settings) {
                walker.facing = None;
            }
        }
        // Walking: the walk plays at the rate that makes its root travel
        // their speed (`actors`); waiting for others or a door, turning in
        // place first or after: not walking.
        let walking = on_path && walker.walking_now() && !blocked && !waiting;
        rig.walking = walking;
        rig.speed = if walking { speed } else { 0.0 };
        // Greeting the player, idle chatter, starting to talk with others.
        social_frame(
            &mut ctx, walker, &mut chats, &mut lines, moves, &others, interior,
        );
        // Looking at whom they spoke to: the body turns past 80° off
        // (`008a3100`).
        look_frame(&mut ctx, walker, rig.fighting, moves);
        // Idles (`sitting`): once a second of free time, the idle tree.
        crate::sitting::idles_frame(&mut ctx, walker, &mut life, &mut rig);
        // Turned in place or walked.
        place(walker, &mut transform, state, &mut talkers);
        walker.velocity = velocity(before, walker.position, dt);
    }
    for (speaker, ..) in starts {
        println!("A script has {speaker} start a conversation, but they aren't loaded here.");
    }
}

/// The side someone turns in place, and the turn animation's rate (the
/// in-place scale: 1.5, in combat 2.5; creatures 1.25), while they're
/// turning.
fn turning_of(walker: &Walker, combat: bool) -> Option<(TurnSide, f32)> {
    if !walker.turn.active {
        return None;
    }
    let side = if walker.turn.right {
        TurnSide::Right
    } else {
        TurnSide::Left
    };
    let base = (walker.turn_speed * mv::ONE_DEGREE).max(1e-6);
    Some((side, walker.rates[usize::from(combat)] / base))
}

fn velocity(before: [f32; 3], after: [f32; 3], dt: f32) -> [f32; 3] {
    if dt <= 0.0 {
        return [0.0; 3];
    }
    [0, 1, 2].map(|k| (after[k] - before[k]) / dt)
}

/// Someone has just died: limp (their skeleton's ragdoll, thrown as the
/// game throws the dead), else tipped over.
#[allow(clippy::too_many_arguments)]
fn fall(
    walker: &mut Walker,
    life: &mut Life,
    rig: &mut ActorRig,
    transform: &mut Transform,
    state: &mut world::scripting::GameState,
    order: &esm::LoadOrder,
    collision: &crate::walk::CellCollision,
    now: f32,
) {
    walker.fallen = true;
    walker.clear_path();
    rig.walking = false;
    // Thrown as the game throws the dead: by the killer's weapon (or the
    // damage, without one), away from a point 2048 units back along the
    // blow from where it struck (the chest).
    let push = state
        .last_blow
        .get(&walker.reference)
        .and_then(|&(by, damage)| {
            let from = if by == PLAYER_REF {
                state.player_position
            } else {
                state.place(order, by).map(|p| p.2)
            }?;
            let struck = [
                walker.position[0],
                walker.position[1],
                walker.position[2] + 90.0,
            ];
            let direction = [
                struck[0] - from[0],
                struck[1] - from[1],
                struck[2] - (from[2] + 90.0),
            ];
            let weapon = world::combat::weapon_in_hand(order, state, by);
            let ranged = weapon.as_ref().is_some_and(|w| !w.is_melee());
            let across = (direction[0]).hypot(direction[1]);
            let speed = world::combat::death_push(order, weapon.as_ref(), damage, ranged, across);
            Some((world::combat::death_push_origin(struck, direction), speed))
        });
    let fresh = push.is_some();
    // From the pose they're in (seated, if sitting).
    let limp = rig.go_limp(now, walker.placement(), push);
    rig.dynamic_idle = None;
    rig.overlay = None;
    state.stand(walker.reference);
    life.idles.stop();
    if limp {
        // Dead since before this place loaded: already lying where they
        // fell.
        if !fresh {
            if let Some(dead) = rig.ragdoll.as_mut() {
                for _ in 0..(4.0 / physics::ragdoll::STEP) as usize {
                    dead.sim.step(&collision.0);
                    if dead.sim.asleep {
                        break;
                    }
                }
            }
        }
    } else {
        rig.still = true;
        *transform = fallen_transform(walker);
    }
}

/// Puts someone's root where they are, and tells the state and the
/// talkers.
fn place(
    walker: &Walker,
    transform: &mut Transform,
    state: &mut world::scripting::GameState,
    talkers: &mut Talkers,
) {
    let new = Transform::from_matrix(Mat4::from_cols_array(&space::matrix(&walker.game_matrix())));
    if *transform == new {
        return;
    }
    *transform = new;
    state
        .positions
        .insert(walker.reference, (walker.position, walker.heading));
    for t in talkers.0.iter_mut() {
        if t.reference == walker.reference {
            t.position = walker.position;
        }
    }
}

/// Looks at the person's package again: a new place to go gets a new path
/// (getting up from any seat first: the game's `StandUp`, and this again
/// once they're up). A travel to furniture uses it; a sandbox package gets
/// its area (`sitting`). The same package goes on as it was, unless
/// `restart` (forced: a dialogue package then starts again).
fn rethink(ctx: &mut Ctx, walker: &mut Walker, life: &mut Life, restart: bool) {
    let game = ctx.game;
    let order = &game.order;
    let me = walker.reference;
    let state = &mut *ctx.state;
    let package = current_package(order, state, me);
    let near = package
        .as_ref()
        .and_then(|p| destination(order, state, me, p));
    // Somewhere else: the load door that leads there.
    let here = state.place(order, me).map(|p| p.0);
    let way = match (&package, near) {
        (Some(p), None) => world::ai::target_place(order, state, p)
            .filter(|(space, _)| Some(*space) != here)
            .and_then(|(space, _)| world::ai::door_toward(order, state, me, space)),
        _ => None,
    };
    let goal = near.or(way.map(|w| (w.at, 0.0)));
    let package_id = package.as_ref().map(|p| p.form_id);
    let target_ref = package
        .as_ref()
        .and_then(|p| p.location)
        .filter(|l| l.kind == 0)
        .map(|l| l.form)
        .or_else(|| package.as_ref().and_then(world::ai::followed).map(|f| f.0));
    // The same package: carry on (a new walk only if its place moved off
    // while they were idle, which `follow_target` sees to).
    if package_id == walker.package
        && !(restart && walker.package_kind == Some(world::ai::kinds::DIALOGUE))
    {
        return;
    }
    walker.dialogue_done = false;
    walker.travelled = false;
    // Something new: up first if seated (or sitting down).
    let using = state.furniture.get(&me).copied();
    if let Some(sitter) = state.sitters.get_mut(&me) {
        if sitter.state != SitState::Normal {
            if using != target_ref || target_ref.is_none() {
                sitter.stand_up();
                life.getting_up = true;
            }
            return;
        }
    }
    if using.is_some() && using != target_ref {
        state.stand(me);
    }
    walker.door = way;
    walker.package = package_id;
    walker.package_kind = package.as_ref().map(|p| p.kind);
    walker.target = goal.map(|g| g.0);
    walker.target_ref = target_ref;
    walker.path_target = None;
    walker.clear_path();
    walker.radius = goal.map_or(0.0, |(_, r)| r);
    walker.home = walker.position;
    walker.arrival = None;
    walker.facing = None;
    life.activity = None;
    // A sandbox: its area around where the package puts it (near a
    // reference, the editor location), else where they are now ("near
    // the current location", and "in a cell": the latter's centre isn't
    // traced); its radius the package's, else the search radius.
    life.sandbox = package
        .as_ref()
        .filter(|p| p.kind == world::ai::kinds::SANDBOX)
        .map(|p| {
            let center = match p.location.map(|l| l.kind) {
                Some(0) | Some(3) | Some(6) => near.map_or(walker.position, |(c, _)| c),
                _ => walker.position,
            };
            let radius = p.location.map_or(0, |l| l.radius);
            let s = world::sandbox::Sandbox::new(
                p.form_id,
                center,
                radius,
                world::sandbox::package_flags(order, p.form_id),
                world::sandbox::energy(order, me),
                &ctx.seats.sandbox,
            );
            println!(
                "{me} sandboxes ({}) within {:.0} of {:.0},{:.0},{:.0}",
                p.editor_id.clone().unwrap_or_default(),
                s.radius,
                center[0],
                center[1],
                center[2]
            );
            s
        });
    if life.sandbox.is_some() {
        return;
    }
    // A travel to a piece of furniture: to use it (the travel procedure's
    // furniture case, `00915f10`).
    if let Some(f) = target_ref.filter(|f| world::scripting::GameState::is_furniture(order, *f)) {
        if state.furniture.get(&me) == Some(&f) {
            return;
        }
        if crate::sitting::begin_use(ctx, walker, f) {
            return;
        }
    }
    let state = &mut *ctx.state;
    // A dialogue package walks its own way (`dialogue_frame`).
    if walker.package_kind == Some(world::ai::kinds::DIALOGUE) {
        return;
    }
    let Some((to, radius)) = goal else {
        return;
    };
    // At the travel's end they face an `XMarkerHeading`'s heading (or
    // their editor heading near the editor location; `008e5e90`).
    let arrival = package
        .as_ref()
        .filter(|_| way.is_none())
        .and_then(|p| world::ai::arrival_heading(order, state, me, p));
    if way.is_none() && mv::arrived(walker.position, to, radius) {
        walker.facing = arrival;
        return;
    }
    if way.is_some() && distance(walker.position, to) <= radius.max(1.0) {
        go_through(order, state, walker);
        return;
    }
    if let Some((path, doors)) = ctx.mesh.path_with_doors(walker.position, to) {
        let length: f32 = path.windows(2).map(|w| distance(w[0], w[1])).sum();
        println!(
            "{me} walks {length:.0} units ({})",
            package
                .as_ref()
                .and_then(|p| p.editor_id.clone())
                .unwrap_or_default()
        );
        walker.path_target = target_ref.and_then(|t| state.place(order, t)).map(|p| p.2);
        walker.set_path(path, radius, true, ctx.moves);
        walker.arrival = arrival;
        walker.doors_ahead = doors;
    }
}

/// Consume both queued force sources without short-circuiting. `EvaluatePackage`
/// sets state.evaluate; other viewer transitions can set walker.evaluate.
fn take_forced_package_evaluation(
    walker: &mut Walker,
    state: &mut world::scripting::GameState,
    me: FormId,
) -> bool {
    let walker_forced = std::mem::take(&mut walker.evaluate);
    let state_forced = state.evaluate.remove(&me);
    walker_forced || state_forced
}

/// Run a queued forced package check before the furniture path returns early
/// for a settled sitter. Entry and exit states are left to finish first, as
/// `008da670` only permits package evaluation in sit states 0, 4 and 9.
fn rethink_queued_package_before_furniture(
    ctx: &mut Ctx,
    walker: &mut Walker,
    life: &mut Life,
    game_hour: f32,
) -> bool {
    let me = walker.reference;
    let settled = ctx
        .state
        .sitters
        .get(&me)
        .is_some_and(|sitter| sitter.state.is_settled());
    if !settled {
        return false;
    }
    if !take_forced_package_evaluation(walker, ctx.state, me) {
        return false;
    }
    let due = walker
        .clock
        .due(ctx.dt, game_hour, true, walker.package.is_some());
    if due {
        rethink(ctx, walker, life, true);
    }
    due
}

/// A walk to a reference that moves (whom they follow, a package's
/// reference): a new path when it has moved more than
/// `fAIMoveDistanceToRecalcFollowPath` (300) since the path was made, or
/// when they stand idle farther than the radius from it (`009e0a00` and
/// the follow procedure, as `findings\ai_rules.md` §3 reads them).
fn follow_target(ctx: &mut Ctx, walker: &mut Walker, moves: &Moves) {
    let order = &ctx.game.order;
    let me = walker.reference;
    // Dialogue, sandbox and wander packages keep to their place their own
    // way.
    if walker.door.is_some()
        || walker.package_kind == Some(world::ai::kinds::DIALOGUE)
        || walker.package_kind == Some(world::ai::kinds::SANDBOX)
        || walker.package_kind == Some(world::ai::kinds::WANDER)
        || ctx.state.furniture.contains_key(&me)
    {
        return;
    }
    let Some(t) = walker.target_ref else {
        return;
    };
    let Some((there, _, at, _)) = ctx.state.place(order, t) else {
        return;
    };
    if ctx.state.place(order, me).map(|p| p.0) != Some(there) {
        return;
    }
    let moved_far = walker
        .path_target
        .is_some_and(|p| distance(p, at) > moves.recalc_follow);
    let idle_far = !walker.on_path() && !mv::arrived(walker.position, at, walker.radius);
    if !(moved_far || idle_far) {
        return;
    }
    // Look again at where the package sends them (its radius).
    let Some(package) = walker
        .package
        .and_then(|p| world::ai::Package::load(order, p))
        .or_else(|| current_package(order, ctx.state, me))
    else {
        return;
    };
    let Some((to, radius)) = destination(order, ctx.state, me, &package) else {
        return;
    };
    if mv::arrived(walker.position, to, radius) {
        return;
    }
    if let Some(path) = ctx.mesh.path(walker.position, to) {
        walker.path_target = Some(at);
        let turn_first = !walker.on_path();
        walker.set_path(path, radius, turn_first, ctx.moves);
        walker.arrival = world::ai::arrival_heading(order, ctx.state, me, &package);
    } else {
        walker.path_target = Some(at);
    }
}

/// One frame of a dialogue package (`world::ai::dialogue_step`): travel
/// first, then wait for the target at its second location, walk up to it,
/// and say the line ("Say To") or talk: the dialogue menu with the player,
/// a conversation with anyone else.
fn dialogue_frame(
    ctx: &mut Ctx,
    walker: &mut Walker,
    (chats, lines, talk): (&mut Chats, &mut Lines, &mut crate::scripts::ScriptedTalk),
    moves: &Moves,
) {
    let order = &ctx.game.order;
    let me = walker.reference;
    let Some(package) = walker
        .package
        .and_then(|p| world::ai::Package::load(order, p))
    else {
        return;
    };
    // The travel step, until it's over (then not again: the procedures
    // only go on, `008e8600`'s list).
    let at_place = walker.travelled
        || match destination(order, ctx.state, me, &package) {
            Some((to, radius)) => mv::arrived(walker.position, to, radius),
            None => true,
        };
    walker.travelled = at_place;
    if !at_place {
        // The travel: walking there (set up by `rethink`).
        if !walker.on_path() {
            if let Some((to, radius)) = destination(order, ctx.state, me, &package) {
                if let Some(path) = ctx.mesh.path(walker.position, to) {
                    walker.set_path(path, radius, true, ctx.moves);
                    walker.arrival = world::ai::arrival_heading(order, ctx.state, me, &package);
                }
            }
        }
        return;
    }
    let my_radius = walker
        .kit
        .as_ref()
        .map_or(world::combat_ai::PERSON_RADIUS, |k| k.radius);
    let Some(step) = world::ai::dialogue_step(order, ctx.state, me, &package, true, my_radius)
    else {
        return;
    };
    let Some((_, target, _)) = package.target else {
        return;
    };
    let data = world::ai::dialogue_data(order, package.form_id);
    let topic = data.and_then(|d| d.topic);
    match step {
        DialogueStep::Travel | DialogueStep::Wait => {
            if walker.on_path() && walker.door.is_none() && at_place {
                walker.clear_path();
            }
        }
        DialogueStep::Approach { reach } => {
            let Some((_, _, at, _)) = ctx.state.place(order, target) else {
                return;
            };
            let repath = !walker.on_path()
                || walker
                    .path_target
                    .is_some_and(|p| distance(p, at) > moves.recalc_follow);
            if repath {
                if let Some(path) = ctx.mesh.path(walker.position, at) {
                    walker.path_target = Some(at);
                    let turn_first = !walker.on_path();
                    walker.set_path(path, reach, turn_first, ctx.moves);
                }
            }
        }
        DialogueStep::Say => {
            walker.clear_path();
            if lines.is_saying(me) {
                return;
            }
            // The topic's line (else `HELLO`) through the GREET procedure: a
            // line said to the target, no dialogue menu; done after it
            // (`008dbe30` ends a "Say To" dialogue package, step 3).
            let topic = topic.unwrap_or(world::social::topics::HELLO);
            if let Some(info) = pick_line(order, ctx.state, me, target, topic) {
                lines.say(me, target, info);
                walker.head_track.set(Slot::Action, Some(target));
            }
            if let Some(s) = walker.social.as_mut() {
                s.greeted(&moves.social);
            }
            walker.dialogue_done = true;
        }
        DialogueStep::Talk => {
            walker.clear_path();
            walker.dialogue_done = true;
            if target == PLAYER_REF {
                // The player is activated: the dialogue menu, about the
                // package's topic.
                if talk.0.is_some() {
                    // Another's talk opens this frame: try again.
                    walker.dialogue_done = false;
                    return;
                }
                println!("{:.1} s: {me} starts talking to the player.", ctx.now);
                talk.0 = Some((me, topic, true));
            } else {
                start_chat(ctx, walker, chats, target, topic, false);
            }
        }
    }
}

/// The first line `who` can say on a topic to `listener`.
fn pick_line(
    order: &esm::LoadOrder,
    state: &world::scripting::GameState,
    who: FormId,
    listener: FormId,
    topic: FormId,
) -> Option<world::dialogue::Info> {
    let speaker = Speaker::load(order, who, world::scripting::base_of(order, who)?)?;
    world::social::pick_for(order, topic, &speaker, listener, state, &[])
}

/// Starts a conversation between `walker` and `other` (`008b2170`): its
/// lines worked out now (none: no conversation); the starter walks up
/// within 90 (200 when seated), the other waits.
pub(crate) fn start_chat(
    ctx: &mut Ctx,
    walker: &mut Walker,
    chats: &mut Chats,
    other: FormId,
    topic: Option<FormId>,
    seated: bool,
) -> bool {
    let order = &ctx.game.order;
    let me = walker.reference;
    if chats.0.contains_key(&other) || chats.0.contains_key(&me) {
        return false;
    }
    let (Some(a), Some(b)) = (
        world::scripting::base_of(order, me).and_then(|base| Speaker::load(order, me, base)),
        world::scripting::base_of(order, other).and_then(|base| Speaker::load(order, other, base)),
    ) else {
        return false;
    };
    let mut dice = crate::fighting::Dice::new(ctx.state);
    let conversation = world::social::conversation(order, ctx.state, &a, &b, topic, &mut || {
        u64::from(dice.roll())
    });
    if conversation.is_empty() {
        return false;
    }
    println!(
        "{:.1} s: {me} starts a conversation with {other} ({} lines).",
        ctx.now,
        conversation.len()
    );
    let reach = mv::conversation_reach(seated);
    // Scripts start some with the speaker themselves (a performance, the
    // Tops' `TopsPerformerActivatorSCRIPT`): one side only.
    if other != me {
        chats.0.insert(
            other,
            Chat {
                with: me,
                starter: false,
                reach,
                lines: Vec::new(),
                next: 0,
                last_line: f32::NEG_INFINITY,
                talking: false,
            },
        );
    }
    chats.0.insert(
        me,
        Chat {
            with: other,
            starter: true,
            reach,
            lines: conversation,
            next: 0,
            last_line: f32::NEG_INFINITY,
            talking: false,
        },
    );
    true
}

/// A script's `StartConversation` begins (`008b2170`): with the player, the
/// walk up ([`talk_frame`]); with anyone else, a conversation
/// ([`start_chat`], its lines from the topic given).
fn begin_conversation(ctx: &mut Ctx, walker: &mut Walker, chats: &mut Chats) {
    let Some((to, topic)) = walker.start.take() else {
        return;
    };
    let seated = ctx.state.sitters.contains_key(&walker.reference);
    if to == PLAYER_REF {
        walker.talk_to = Some((topic, mv::conversation_reach(seated)));
    } else if !start_chat(ctx, walker, chats, to, topic, seated) {
        println!(
            "{} can't start a conversation with {to} (nothing to say).",
            walker.reference
        );
    }
}

/// Coming up to the player for a script's `StartConversation` (the
/// conversation package's activate step, `008e9640`): within the reach
/// (90, 200 seated), measured as `IsWithinDistance` adding their radius (at
/// least 32), the player is activated: the dialogue menu. Farther, they walk
/// up; seated, they don't (they wait for the player to come).
fn talk_frame(
    ctx: &mut Ctx,
    walker: &mut Walker,
    talk: &mut crate::scripts::ScriptedTalk,
    moves: &Moves,
) {
    let me = walker.reference;
    let Some((topic, reach)) = walker.talk_to else {
        return;
    };
    let Some(at) = ctx.state.player_position else {
        return;
    };
    let my_radius = walker
        .kit
        .as_ref()
        .map_or(world::combat_ai::PERSON_RADIUS, |k| k.radius);
    if mv::within_distance(walker.position, 128.0, Some(my_radius), at, reach, true) {
        if talk.0.is_none() {
            walker.clear_path();
            walker.talk_to = None;
            println!("{:.1} s: {me} starts talking to the player.", ctx.now);
            talk.0 = Some((me, topic, true));
        }
        return;
    }
    let seated = ctx.state.sitters.contains_key(&me) || ctx.state.furniture.contains_key(&me);
    if seated {
        return;
    }
    let repath = !walker.on_path()
        || walker
            .path_target
            .is_some_and(|p| distance(p, at) > moves.recalc_follow);
    if repath {
        if let Some(path) = ctx.mesh.path(walker.position, at) {
            walker.path_target = Some(at);
            let turn_first = !walker.on_path();
            walker.set_path(path, reach, turn_first, ctx.moves);
        }
    }
}

/// A conversation is over for both.
fn end_chat(chats: &mut Chats, who: FormId) {
    if let Some(c) = chats.0.remove(&who) {
        chats.0.remove(&c.with);
    }
}

/// The least time between two lines of a conversation (the playback's
/// pause timer, `009ee0a0` sets 2 s).
const LINE_PAUSE: f32 = 2.0;

/// One frame of a conversation between two people. The starter walks up
/// within reach (as `IsWithinDistance`, adding their radius), then says
/// the lines in turn (each by its speaker, through `chatter`); the other
/// turns in place to face the starter every frame (unless seated); the
/// starter turns to face the other only when the other faces more than
/// `iActorTurnDegree` (100°) away from them and it isn't turning already
/// (`008ec460`, as written there). Over: both look at their packages again
/// (`008ec460` zeroes the timer).
fn chat_frame(
    ctx: &mut Ctx,
    walker: &mut Walker,
    chats: &mut Chats,
    lines: &mut Lines,
    moves: &Moves,
) {
    let order = &ctx.game.order;
    let me = walker.reference;
    let Some(chat) = chats.0.get(&me).cloned() else {
        return;
    };
    let other = chat.with;
    let alone = other == me;
    let gone = ctx.state.dead.contains(&other)
        || (!alone && ctx.state.combat.contains_key(&other))
        || ctx.state.place(order, other).map(|p| p.0) != ctx.state.place(order, me).map(|p| p.0);
    let Some((_, _, at, their_heading)) = ctx.state.place(order, other).filter(|_| !gone) else {
        end_chat(chats, me);
        walker.evaluate = true;
        return;
    };
    let seated = ctx.state.sitters.contains_key(&me);
    let toward = mv::heading_to(walker.position, at);
    if !chat.starter {
        // Waiting for them, facing them.
        walker.clear_path();
        if !seated && (at[0] - walker.position[0]).hypot(at[1] - walker.position[1]) > 1.0 {
            face(walker, toward, ctx.dt, false, ctx.moves);
        }
        walker.head_track.set(Slot::Dialog, Some(other));
        return;
    }
    let my_radius = walker
        .kit
        .as_ref()
        .map_or(world::combat_ai::PERSON_RADIUS, |k| k.radius);
    let in_reach = alone
        || mv::within_distance(
            walker.position,
            128.0,
            Some(my_radius),
            at,
            chat.reach,
            true,
        );
    if chat.next == 0 && !chat.talking && !in_reach {
        if seated {
            // Seated, they don't walk: too far, no conversation.
            end_chat(chats, me);
            return;
        }
        let repath = !walker.on_path()
            || walker
                .path_target
                .is_some_and(|p| distance(p, at) > moves.recalc_follow);
        if repath {
            match ctx.mesh.path(walker.position, at) {
                Some(path) => {
                    walker.path_target = Some(at);
                    let turn_first = !walker.on_path();
                    walker.set_path(path, chat.reach, turn_first, ctx.moves);
                }
                None => {
                    end_chat(chats, me);
                }
            }
        }
        return;
    }
    walker.clear_path();
    if !alone {
        walker.head_track.set(Slot::Dialog, Some(other));
    }
    // The starter's turn: only when the other faces well away.
    let their_off = mv::wrap_pi(mv::heading_to(at, walker.position) - their_heading).abs();
    if !seated
        && !alone
        && (walker.turn.active
            || (their_off > moves.settings.turn_degree * mv::ONE_DEGREE && !walker.turn.active))
    {
        face(walker, toward, ctx.dt, false, ctx.moves);
    }
    // The lines.
    let saying = chat.lines.iter().any(|l| lines.is_saying(l.speaker));
    let mut chat = chat;
    if saying || ctx.now - chat.last_line < LINE_PAUSE {
        chats.0.insert(me, chat);
        return;
    }
    if chat.next >= chat.lines.len() {
        println!(
            "{:.1} s: {me}'s conversation with {other} is over.",
            ctx.now
        );
        end_chat(chats, me);
        walker.evaluate = true;
        return;
    }
    let line = chat.lines[chat.next].clone();
    lines.say(line.speaker, line.listener, line.info);
    chat.next += 1;
    chat.last_line = ctx.now;
    chat.talking = true;
    chats.0.insert(me, chat);
}

/// Greeting the player, idle chatter, and starting conversations with
/// others (`008eeec0`, `00904800`; `world::social`), for someone free to
/// (not fighting, not in a conversation or saying a line).
fn social_frame(
    ctx: &mut Ctx,
    walker: &mut Walker,
    chats: &mut Chats,
    lines: &mut Lines,
    moves: &Moves,
    others: &[crate::fighting::Seen],
    interior: bool,
) {
    let order = &ctx.game.order;
    let me = walker.reference;
    if chats.0.contains_key(&me) || lines.is_saying(me) || world::combat::is_creature(order, me) {
        return;
    }
    let Some(mut social) = walker.social.take() else {
        return;
    };
    let package_kind = walker.package_kind;
    let player_distance = ctx
        .state
        .player_position
        .map(|p| distance(p, walker.position));
    let detected = walker.detected_player;
    let near_player =
        player_distance.is_some_and(|d| detected > 0 && d <= moves.social.greeting_distance);
    if near_player {
        // A greeting: `HELLO`, said to the player, no dialogue menu.
        if social.greets(
            detected,
            true,
            player_distance.unwrap_or(f32::MAX),
            &moves.social,
        ) {
            social.greeted(&moves.social);
            let line = pick_line(
                order,
                ctx.state,
                me,
                PLAYER_REF,
                world::social::topics::HELLO,
            );
            println!(
                "{:.1} s: {me} greets the player{}.",
                ctx.now,
                if line.is_some() {
                    ""
                } else {
                    " (no HELLO line for them)"
                }
            );
            if let Some(info) = line {
                lines.say(me, PLAYER_REF, info);
                walker.head_track.set(Slot::Action, Some(PLAYER_REF));
                // They turn to the player standing, unless their package is
                // one that keeps them busy (GREET, `008dbe30`).
                let busy = matches!(
                    package_kind,
                    Some(k) if [
                        world::ai::kinds::TRAVEL,
                        world::ai::kinds::ESCORT,
                        world::ai::kinds::FOLLOW,
                        world::ai::kinds::ACCOMPANY,
                        world::ai::kinds::PATROL,
                        world::ai::kinds::SANDBOX,
                        8,
                        16,
                        0,
                    ]
                    .contains(&k)
                );
                if !busy && !walker.on_path() && !ctx.state.sitters.contains_key(&me) {
                    if let Some(p) = ctx.state.player_position {
                        walker.turn.request(
                            walker.heading,
                            mv::heading_to(walker.position, p),
                            ctx.moves,
                        );
                    }
                }
            }
        }
    } else {
        let mut dice = crate::fighting::Dice::new(ctx.state);
        if social.chatter_due(ctx.dt, package_kind, &moves.social, &mut || dice.unit()) {
            if let Some(info) = pick_line(
                order,
                ctx.state,
                me,
                PLAYER_REF,
                world::social::topics::IDLE_CHATTER,
            ) {
                lines.say(me, PLAYER_REF, info);
            }
        }
    }
    // Conversations with others (`00904800`): not while asleep or in sleep,
    // use item at, ambush, guard, dialogue or use weapon packages; a
    // sandbox only if it allows them; follow, escort and accompany only
    // with their target.
    let allowed = match package_kind {
        Some(4) | Some(8) | Some(9) | Some(14) | Some(15) | Some(16) => false,
        Some(12) => walker.package.is_some_and(|p| {
            world::sandbox::package_flags(order, p) & world::sandbox::flags::NO_CONVERSATION == 0
        }),
        _ => walker.package.is_some(),
    };
    let only = match package_kind {
        Some(1) | Some(2) | Some(7) => walker
            .package
            .and_then(|p| world::ai::Package::load(order, p))
            .and_then(|p| world::ai::followed(&p).map(|f| f.0)),
        _ => None,
    };
    let candidates: Vec<(FormId, f32)> = others
        .iter()
        .filter(|o| o.reference != me && o.reference != PLAYER_REF)
        .filter(|o| {
            !chats.0.contains_key(&o.reference) && !ctx.state.combat.contains_key(&o.reference)
        })
        .filter(|o| !world::combat::is_creature(order, o.reference))
        .filter(|o| {
            // Someone sandboxing talks only if their sandbox allows it.
            current_package(order, ctx.state, o.reference).is_none_or(|p| {
                p.kind != world::ai::kinds::SANDBOX
                    || world::sandbox::package_flags(order, p.form_id)
                        & world::sandbox::flags::NO_CONVERSATION
                        == 0
            })
        })
        .map(|o| (o.reference, distance(o.position, walker.position)))
        .collect();
    let mut dice = crate::fighting::Dice::new(ctx.state);
    let mut dice2 = crate::fighting::Dice::new(ctx.state);
    let started = social.start_conversation(
        me,
        allowed,
        only,
        &candidates,
        interior,
        ctx.dt,
        &moves.social,
        &mut || u64::from(dice.roll()),
        &mut || dice2.unit(),
    );
    walker.social = Some(social);
    if let Some(other) = started {
        let seated = ctx.state.sitters.contains_key(&me);
        start_chat(ctx, walker, chats, other, None, seated);
    }
}

/// Holds or lets go of the head-track slots others asked for, the viewer's
/// stand-in for where the game sets and clears them
/// (`docs/HEAD_TRACK_TARGET.md`):
///
/// * ACTION (a line said to someone: the "Say To" package, a greeting, and
///   a script's `SayTo` to the player, `005c9100`) holds while they say
///   it, then is cleared with demote, as the "Say To" package does when it
///   ends (`008dbe30`). That it ends with the line is inferred.
/// * DIALOG (a conversation: with someone, `00935480`; the dialogue menu
///   with the player) holds while it lasts, then is cleared with demote
///   (`00933d20`).
///
/// The speaker in a conversation with the player has the player put in
/// the slot here; the other sites put their target in when they start.
fn head_track_asks(
    walker: &mut Walker,
    speaker: Option<FormId>,
    in_menu: bool,
    lines: &Lines,
    chats: &Chats,
    settings: &world::head_track::Settings,
) {
    let me = walker.reference;
    walker.head_tracking_off = settings.disabled;
    let with_player = speaker == Some(me);
    if with_player {
        let slot = if in_menu { Slot::Dialog } else { Slot::Action };
        walker.head_track.set(slot, Some(PLAYER_REF));
    }
    let saying = lines.is_saying(me) || (with_player && !in_menu);
    if walker.head_track.in_slot(Slot::Action).is_some() && !saying {
        walker.head_track.clear(Slot::Action, true, settings);
    }
    let talking = chats.0.contains_key(&me) || (with_player && in_menu);
    if walker.head_track.in_slot(Slot::Dialog).is_some() && !talking {
        walker.head_track.clear(Slot::Dialog, true, settings);
    }
}

/// One head-track update of an actor (`008a3100`, `world::head_track`):
/// none farther than `fAIMaxHeadTrackDistanceFromPC` from the player; the
/// player forgotten as a target once no longer noticed; a target no longer
/// among the people here forgotten; then the timers, and on them a choice
/// among the people here.
///
/// The candidates' facts come from the viewer: distance and angle from
/// positions, detection from its detection run ("noticed", standing for the
/// game's level ≥ 1, an inference), line of sight from its ray cast between
/// the two (`fighting::clear_between`). The dead aren't among the people
/// here, so the game's halving for someone down never applies.
fn choose_head_track(
    walker: &mut Walker,
    state: &mut world::scripting::GameState,
    others: &[crate::fighting::Seen],
    collision: &physics::Collider,
    settings: &world::head_track::Settings,
    chosen_this_frame: &mut u32,
    dt: f32,
) {
    let me = walker.reference;
    let Some(player) = state.player_position else {
        return;
    };
    if distance(player, walker.position) > settings.max_distance_from_player {
        return;
    }
    if walker.head_track.current() == Some(PLAYER_REF) && !walker.noticed.contains(&PLAYER_REF) {
        walker.head_track.clear_all();
    }
    if let Some(t) = walker.head_track.current() {
        if !others.iter().any(|o| o.reference == t) {
            walker.head_track.clear_all();
        }
    }
    let mut dice = crate::fighting::Dice::new(state);
    let (position, heading) = (walker.position, walker.heading);
    let noticed = &walker.noticed;
    let candidates = || -> Vec<Candidate> {
        others
            .iter()
            .filter(|o| o.reference != me)
            .map(|o| Candidate {
                reference: o.reference,
                is_player: o.reference == PLAYER_REF,
                distance: distance(position, o.position),
                off_heading: mv::wrap_pi(mv::heading_to(position, o.position) - heading),
                detected: noticed.contains(&o.reference),
                in_sight: crate::fighting::clear_between(collision, position, o.position),
                down: false,
            })
            .collect()
    };
    walker.head_track.update(
        dt,
        settings,
        chosen_this_frame,
        &mut || dice.unit(),
        |current, timer| world::head_track::choose(&candidates(), current, timer, settings),
    );
}

/// The body turns to whom they look at (`008a3100`): standing, not
/// walking, not fighting, not seated, out of dialogue and "use" packages,
/// when that one is more than 80° off (8° while turning), in place.
fn look_frame(ctx: &mut Ctx, walker: &mut Walker, fighting: bool, moves: &Moves) {
    let order = &ctx.game.order;
    let Some(who) = walker.head_track.current() else {
        return;
    };
    let blocked = fighting
        || walker.on_path()
        || ctx.state.sitters.contains_key(&walker.reference)
        || matches!(walker.package_kind, Some(8) | Some(15) | Some(16));
    if blocked {
        return;
    }
    let Some((_, _, at, _)) = ctx.state.place(order, who) else {
        return;
    };
    if (at[0] - walker.position[0]).hypot(at[1] - walker.position[1]) < 1.0 {
        return;
    }
    let toward = mv::heading_to(walker.position, at);
    if walker.turn.active
        || walker
            .turn
            .should_face(walker.heading, toward, &moves.settings)
    {
        face(walker, toward, ctx.dt, false, ctx.moves);
    }
}

/// The cells the game keeps in memory after the player leaves them (its
/// cell buffers, `[General] uInterior Cell Buffer` 3 and `uExterior Cell
/// Buffer` 36 in this install's INI): the most recent interiors and outdoor
/// squares the player has been in. Which ones the game keeps (most recently
/// used here) isn't traced.
#[derive(Resource)]
pub struct CellBuffer {
    interiors: VecDeque<FormId>,
    squares: VecDeque<FormId>,
    interior_size: usize,
    exterior_size: usize,
}

impl CellBuffer {
    pub fn new(game: &cellview::Game) -> CellBuffer {
        let u =
            |key: &str, d: f32| game.settings.float("General", key).unwrap_or(d).max(0.0) as usize;
        CellBuffer {
            interiors: VecDeque::new(),
            squares: VecDeque::new(),
            interior_size: u("uInterior Cell Buffer", 3.0),
            exterior_size: u("uExterior Cell Buffer", 36.0),
        }
    }

    fn visit(&mut self, cell: FormId, interior: bool) {
        let (list, size) = if interior {
            (&mut self.interiors, self.interior_size)
        } else {
            (&mut self.squares, self.exterior_size)
        };
        if list.front() == Some(&cell) {
            return;
        }
        list.retain(|c| *c != cell);
        list.push_front(cell);
        list.truncate(size);
    }

    fn holds(&self, cell: FormId) -> bool {
        self.interiors.contains(&cell) || self.squares.contains(&cell)
    }
}

/// Who moves out of sight, and when each last moved.
#[derive(Default)]
pub struct Offstage {
    /// Persistent people (always in memory), found once.
    persistent: Vec<FormId>,
    found: bool,
    /// When each last moved, game hours since the game began.
    last: HashMap<FormId, f64>,
    /// Where the low list's round stopped.
    cursor: usize,
    navs: world::ai::NavCache,
}

/// People out of sight walk on (`world::ai::move_offstage`), as the game's
/// lower process levels move them (`009334b0`, `0096b810`/`0096b470`/
/// `0096b050`, `009ea8a0`): the persistent people (who stay in memory) and
/// anyone in a cell the game still holds (its cell buffers), not on
/// screen. Someone in a held cell is moved every 0.3 game hours (middle-low;
/// the middle-high 0.15 is for cells loading or detaching, which the viewer
/// doesn't have), anyone else every game hour (low), the low ones within
/// `iLowProcessingMilliseconds` (2 ms) a frame, resuming where the last
/// frame stopped. Each update covers the game time since their last at
/// their walking speed (`fMoveBaseSpeed` × SpeedMult; running in a fight).
#[allow(clippy::too_many_arguments)]
pub fn move_offstage(
    game: Res<GameFiles>,
    mut state: ResMut<DialogueState>,
    shown: Query<(&Walker, &Visibility)>,
    mut buffer: ResMut<CellBuffer>,
    moves: Res<Moves>,
    frozen: Res<FrozenAi>,
    conversation: Res<Conversation>,
    mut off: Local<Offstage>,
) {
    let order = &game.0.order;
    let state = &mut state.0;
    // The player's cell goes into the buffer as they enter it.
    if let Some(cell) = state.player_cell {
        buffer.visit(cell, state.player_world.is_none());
    }
    if frozen.0 || conversation.0.as_ref().is_some_and(|t| !t.is_line_only()) {
        return;
    }
    let Some(days) = state.global(order, "GameDaysPassed") else {
        return;
    };
    let now = f64::from(days) * 24.0;
    let time_scale = state.global(order, "TimeScale").unwrap_or(30.0);
    if !off.found {
        off.found = true;
        let mut people: Vec<FormId> = [esm::sig::ACHR, esm::sig::ACRE]
            .into_iter()
            .flat_map(|k| order.records_of_type(k))
            .filter(|rr| rr.entry.header.flags & 0x400 != 0 && !rr.entry.header.is_deleted())
            .map(|rr| rr.form_id)
            .collect();
        people.sort();
        off.persistent = people;
    }
    let on_screen: HashSet<FormId> = shown
        .iter()
        .filter(|(_, v)| **v != Visibility::Hidden)
        .map(|(w, _)| w.reference)
        .collect();
    let here = state.player_world.or(state.player_cell);
    // Those in held cells: the people the state has moved there, and the
    // persistent ones.
    let mut middle: Vec<FormId> = state
        .positions
        .keys()
        .chain(state.spaces.keys())
        .copied()
        .filter(|r| *r != PLAYER_REF)
        .collect();
    middle.sort();
    middle.dedup();
    let level_of = |state: &world::scripting::GameState, who: FormId| {
        let (_, cell, ..) = state.place(order, who)?;
        Some(if buffer.holds(cell) {
            ProcessLevel::MiddleLow
        } else {
            ProcessLevel::Low
        })
    };
    let eligible = |state: &world::scripting::GameState, who: FormId| {
        who != PLAYER_REF
            && !on_screen.contains(&who)
            && !state.dead.contains(&who)
            && world::enabled_now(order, who, &state.disabled)
            && !(state.place(order, who).map(|p| p.0) == here && here.is_some() && {
                // In the loaded place but not drawn: outdoors beyond the
                // loaded squares only.
                state.player_world.is_none()
            })
    };
    let step = |state: &mut world::scripting::GameState,
                off: &mut Offstage,
                who: FormId,
                level: ProcessLevel| {
        let last = off.last.get(&who).copied();
        if !level.due(last, now) {
            return;
        }
        off.last.insert(who, now);
        let hours = last.map(|t| (now - t) as f32);
        let in_combat = state.combat.contains_key(&who);
        let seconds = mv::offstage_seconds(hours, time_scale, in_combat, 0.016);
        let facts = world::scripting::Facts {
            order,
            state,
            speaker: None,
        };
        let speed_mult = facts.current_actor_value(who, 21).unwrap_or(100.0) as f32;
        let legs = [29u16, 30]
            .iter()
            .filter(|&&av| facts.current_actor_value(who, av).is_some_and(|v| v <= 0.0))
            .count() as u8;
        let speed = mv::offstage_speed(&moves.settings, speed_mult, legs, in_combat);
        let before = state.place(order, who).map(|p| p.0);
        let result = world::ai::move_offstage(order, state, who, speed * seconds, &mut off.navs);
        if result != world::ai::Offstage::Stayed {
            let after = state.place(order, who).map(|p| p.0);
            if before != after {
                println!(
                    "{who} (out of sight) walks on into {}",
                    after.unwrap_or_default()
                );
            }
        }
    };
    for who in middle {
        if !eligible(state, who) || off.persistent.binary_search(&who).is_ok() {
            continue;
        }
        if let Some(level) = level_of(state, who) {
            step(state, &mut off, who, level);
        }
    }
    // The persistent people, a share each frame within the time allowed.
    let budget = std::time::Duration::from_secs_f32(
        world::scripting::game_setting(order, "iLowProcessingMilliseconds").unwrap_or(2.0) / 1000.0,
    );
    let started = std::time::Instant::now();
    let count = off.persistent.len();
    let mut done = 0;
    while done < count {
        let who = off.persistent[off.cursor % count];
        off.cursor = (off.cursor + 1) % count;
        done += 1;
        if eligible(state, who) {
            if let Some(level) = level_of(state, who) {
                step(state, &mut off, who, level);
            }
        }
        // The game looks at the clock every 5 people.
        if done % 5 == 0 && started.elapsed() >= budget {
            break;
        }
    }
}

/// At the load door they were walking to: through it, to its far side
/// (where the game puts the player too), in the other place. They're then
/// looked at again: gone from here, or (`bring_in_people`) on screen there.
fn go_through(
    order: &esm::LoadOrder,
    state: &mut world::scripting::GameState,
    walker: &mut Walker,
) {
    let Some(d) = walker.door.take() else {
        return;
    };
    println!(
        "{} goes through {} to {}",
        walker.reference,
        d.door,
        order
            .get(d.to_space)
            .and_then(|r| r.record().ok())
            .and_then(|r| r.editor_id())
            .unwrap_or_default()
    );
    state.stand(walker.reference);
    state
        .spaces
        .insert(walker.reference, (d.to_space, d.to_cell));
    state.positions.insert(walker.reference, (d.to, d.heading));
    walker.clear_path();
    walker.target = None;
    walker.package = None;
    walker.fresh = true;
}

/// Everyone who can be noticed as this frame begins (`fighting::Seen`):
/// the player (moving and attacking as the state and the player's attack
/// say), then the people on screen and alive, moving as their rigs show
/// and attacking while their attack lasts; and the same as obstacles for
/// walkers (`world::movement::Obstacle`).
fn seen(
    order: &esm::LoadOrder,
    state: &world::scripting::GameState,
    attack: &crate::combat::PlayerAttack,
    now: f32,
    actors: &Query<Person>,
    player_velocity: [f32; 3],
) -> (Vec<crate::fighting::Seen>, Vec<Obstacle>) {
    let mut out = Vec::new();
    let mut obstacles = Vec::new();
    if let Some(p) = state
        .player_position
        .filter(|_| !state.dead.contains(&PLAYER_REF))
    {
        let weapon = world::combat::weapon_in_hand(order, state, PLAYER_REF);
        let attack_time = weapon.as_ref().map_or(0.5, |w| w.shot_interval());
        out.push(crate::fighting::Seen {
            reference: PLAYER_REF,
            position: p,
            moving: state.player_moving,
            running: state.player_running,
            attacking: attack.fired_at.is_some_and(|t| now - t < attack_time),
            radius: world::combat_ai::PERSON_RADIUS,
        });
        obstacles.push(Obstacle {
            who: PLAYER_REF,
            position: p,
            velocity: player_velocity,
            radius: world::combat_ai::PERSON_RADIUS,
            is_player: true,
            seated: false,
        });
    }
    for (walker, _, rig, _, visibility) in actors.iter() {
        if *visibility == Visibility::Hidden || state.dead.contains(&walker.reference) {
            continue;
        }
        let kit = walker.kit.as_ref();
        let attack_time = kit.map_or(1.0, |k| k.attack_animation);
        let radius = kit.map_or(world::combat_ai::PERSON_RADIUS, |k| k.radius);
        out.push(crate::fighting::Seen {
            reference: walker.reference,
            position: walker.position,
            moving: rig.walking,
            running: rig.running,
            attacking: rig.attack_at.is_some_and(|t| now - t < attack_time),
            radius,
        });
        obstacles.push(Obstacle {
            who: walker.reference,
            position: walker.position,
            velocity: walker.velocity,
            radius,
            is_player: false,
            seated: state.sitters.contains_key(&walker.reference),
        });
    }
    (out, obstacles)
}

/// Lying where they fell: turned a quarter onto their side.
fn fallen_transform(walker: &Walker) -> Transform {
    let side = Mat4::from_cols(
        Vec4::new(0.0, 0.0, -1.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::new(1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 0.0, 1.0),
    );
    let game = Mat4::from_cols_array(&walker.game_matrix()) * side;
    Transform::from_matrix(Mat4::from_cols_array(&space::matrix(&game.to_cols_array())))
}

/// Turns someone in place toward a heading, the game's way (a turn
/// request when more than a degree off, then the in-place rate: 135°/s for
/// people, 225°/s in combat; `009e7610`, `009e7d70`). Whether they're
/// turning.
pub(crate) fn face(
    walker: &mut Walker,
    want: f32,
    dt: f32,
    combat: bool,
    settings: &MoveSettings,
) -> bool {
    let target = want.rem_euclid(std::f32::consts::TAU);
    if !walker.turn.active || mv::wrap_pi(walker.turn.target - target).abs() > mv::ONE_DEGREE {
        walker.turn.request(walker.heading, target, settings);
    }
    let rate = walker.rates[usize::from(combat)];
    let side = walker.turn.update(&mut walker.heading, dt, rate);
    if side.is_some() {
        walker.turning = side;
    }
    walker.turn.active
}

/// Walks along the path for `dt` seconds (`009e0a00`); whether they're
/// still on it. A turn in place under way comes first (the walk waits for
/// it). The walk ends within the path's radius of its end
/// (`world::movement::arrived`). The steering point moves on in tenths
/// (`advance_progress`); the facing turns toward the point a tenth further
/// by the walking rule (`walking_turn`, or toward `face_point` when set),
/// the forward speed cut on sharp turns; the body moves toward the steering
/// point whatever the facing (`009e3560`).
pub(crate) fn step(walker: &mut Walker, speed: f32, dt: f32) -> bool {
    let n = walker.path.len();
    if walker.next >= n || n == 0 {
        return false;
    }
    if walker.turn.active {
        let side = walker.turn.update(&mut walker.heading, dt, walker.rates[0]);
        if side.is_some() {
            walker.turning = side;
        }
        return true;
    }
    let pos = walker.position;
    let goal = walker.path[n - 1];
    if mv::arrived(pos, goal, walker.radius) {
        walker.clear_path();
        return false;
    }
    let frame_move = speed * dt;
    walker.progress = mv::advance_progress(&walker.path, walker.progress, pos, frame_move);
    let steer = mv::path_point(&walker.path, walker.progress);
    let look = match walker.face_point {
        Some(p) => p,
        None => mv::path_point(&walker.path, walker.progress + mv::PATH_STEP),
    };
    let flat_look = (look[0] - pos[0]).hypot(look[1] - pos[1]);
    let desired = if flat_look > 1e-3 {
        mv::heading_to(pos, look)
    } else {
        walker.heading
    };
    let ahead = mv::look_ahead_point(&walker.path, walker.progress, pos, speed);
    let factor = mv::walking_turn_factor(
        walker.turn_speed,
        walker.face_point.is_none(),
        mv::heading_to(pos, ahead) - walker.heading,
    );
    let w = mv::walking_turn(walker.heading, desired, walker.turn_speed, dt, factor);
    walker.heading = w.heading;
    let (dx, dy, dz) = (steer[0] - pos[0], steer[1] - pos[1], steer[2] - pos[2]);
    let flat = dx.hypot(dy);
    if flat > 1e-4 {
        let go = (frame_move * w.forward).min(flat);
        walker.position = [
            pos[0] + dx / flat * go,
            pos[1] + dy / flat * go,
            pos[2] + dz * (go / flat),
        ];
    } else if (walker.progress - (n - 1) as f32).abs() < 1e-4 {
        walker.position = goal;
    }
    walker.next = ((walker.progress.floor() as usize) + 1).min(n - 1);
    true
}

pub(crate) fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walker() -> Walker {
        let mut w = Walker::at(FormId(1), [0.0, 0.0, 0.0], 0.0, 1.0, false);
        w.set_path(
            vec![[0.0, 0.0, 0.0], [0.0, 100.0, 0.0], [100.0, 100.0, 0.0]],
            0.0,
            false,
            &MoveSettings::defaults(),
        );
        w
    }

    #[test]
    fn a_scripted_say_to_has_the_speaker_look_at_the_player_then_hold_it() {
        let s = world::head_track::Settings::default();
        let (lines, chats) = (Lines::default(), Chats::default());
        let mut doc = walker();
        let me = doc.reference;
        // An earlier choice of their own (no one) set the own slot.
        doc.head_track.set(Slot::Default, None);
        // `SayTo Player`: a line said to the player, no menu.
        head_track_asks(&mut doc, Some(me), false, &lines, &chats, &s);
        assert_eq!(doc.looking_at(), Some(PLAYER_REF));
        assert_eq!(doc.head_track.current_slot(), Some(Slot::Action));
        assert!(!doc.head_track.may_choose());
        // The line over: kept as their own choice for 10 s.
        head_track_asks(&mut doc, None, false, &lines, &chats, &s);
        assert_eq!(doc.looking_at(), Some(PLAYER_REF));
        assert_eq!(doc.head_track.current_slot(), Some(Slot::Default));
        assert!(!doc.head_track.may_choose());
        // The dialogue menu: the DIALOG slot, let go when it closes.
        head_track_asks(&mut doc, Some(me), true, &lines, &chats, &s);
        assert_eq!(doc.head_track.current_slot(), Some(Slot::Dialog));
        head_track_asks(&mut doc, None, false, &lines, &chats, &s);
        assert_eq!(doc.head_track.current_slot(), Some(Slot::Default));
        // `bDisableHeadTracking`: the head doesn't follow.
        let off = world::head_track::Settings {
            disabled: true,
            ..s
        };
        head_track_asks(&mut doc, None, false, &lines, &chats, &off);
        assert_eq!(doc.looking_at(), None);
        assert_eq!(doc.head_track.current(), Some(PLAYER_REF));
    }

    #[test]
    fn walkers_follow_their_path_at_their_speed_and_turn_by_the_walking_rule() {
        let mut w = walker();
        // North at 50 a second for one second (in small frames).
        for _ in 0..10 {
            assert!(step(&mut w, 50.0, 0.1));
        }
        assert!((w.position[1] - 50.0).abs() < 0.5, "{:?}", w.position);
        assert!(w.heading.abs() < 1e-3);
        // On round the corner: the body heads east while the facing
        // catches up at up to 270°/s.
        for _ in 0..40 {
            step(&mut w, 50.0, 0.05);
        }
        assert!(w.position[0] > 20.0, "{:?}", w.position);
        assert!(
            (w.heading - std::f32::consts::FRAC_PI_2).abs() < 0.2,
            "{}",
            w.heading
        );
        for _ in 0..200 {
            if !step(&mut w, 50.0, 0.05) {
                break;
            }
        }
        assert!(!w.on_path());
        assert!((w.position[0] - 100.0).abs() < 1.0, "{:?}", w.position);
    }

    #[test]
    fn a_new_path_starts_with_a_turn_in_place() {
        let s = MoveSettings::defaults();
        let mut w = Walker::at(FormId(1), [0.0, 0.0, 0.0], 0.0, 1.0, false);
        // Due east: a quarter turn first, in place, at 135°/s.
        w.set_path(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], 0.0, true, &s);
        assert!(w.turn.active);
        let mut t = 0.0f32;
        while w.turn.active {
            assert!(step(&mut w, 85.0, 0.01));
            assert_eq!(w.position, [0.0, 0.0, 0.0]);
            t += 0.01;
        }
        assert!((t - 0.67).abs() < 0.03, "{t}");
        assert!(step(&mut w, 85.0, 0.1));
        assert!(w.position[0] > 8.0);
    }

    #[test]
    fn facing_someone_turns_in_place_at_the_game_rate() {
        let s = MoveSettings::defaults();
        let mut w = Walker::at(FormId(1), [0.0, 0.0, 0.0], 0.0, 1.0, false);
        // Half a second at 135°/s: 67.5° of the 90°.
        for _ in 0..50 {
            face(&mut w, std::f32::consts::FRAC_PI_2, 0.01, false, &s);
        }
        assert!(
            (w.heading.to_degrees() - 67.5).abs() < 1.5,
            "{}",
            w.heading.to_degrees()
        );
        assert_eq!(w.turning, Some(TurnSide::Right));
    }

    #[test]
    fn the_game_matrix_faces_the_heading() {
        let mut w = walker();
        w.heading = std::f32::consts::FRAC_PI_2;
        let m = w.game_matrix();
        // Forward (+y) points east.
        assert!((m[4] - 1.0).abs() < 1e-6 && m[5].abs() < 1e-6);
    }

    #[test]
    fn reset_ai_discards_the_old_path_without_moving_the_actor() {
        let mut w = walker();
        let position = w.position;
        let heading = w.heading;
        w.package = Some(FormId(42));
        w.package_kind = Some(world::ai::kinds::TRAVEL);
        w.set_path(
            vec![position, [1000.0, 0.0, 0.0]],
            0.0,
            true,
            &MoveSettings::defaults(),
        );
        w.reset_procedure();
        assert!(!w.on_path());
        assert!(w.package.is_none());
        assert!(w.target.is_none());
        assert!(w.evaluate);
        assert_eq!(w.position, position);
        assert_eq!(w.heading, heading);
    }

    #[test]
    fn queued_package_change_rethinks_a_seated_actor_before_furniture_returns() {
        use testdata::ai::ids as fixture;

        let data = testdata::ai::world("seated-package-rethink");
        let game = cellview::Game::open(
            data.path(),
            &cellview::Options {
                official: true,
                ..default()
            },
        )
        .unwrap();
        let actor = FormId(fixture::TALKER_REF);
        let chair = FormId(fixture::CHAIR_REF);
        let mut state = world::scripting::GameState::default();
        state
            .script_packages
            .insert(actor, FormId(fixture::TO_MARKER));
        state.evaluate.insert(actor);
        state.furniture.insert(actor, chair);
        let mut pick = |_, _, _| None;
        let marker = world::furniture::PlacedMarker {
            index: 2,
            number: 14,
            position: [0.0, 0.0, 0.0],
            heading: 0.0,
        };
        state.sitters.insert(
            actor,
            world::furniture::Sitter::seated(
                chair,
                marker,
                world::furniture::MarkerSettings::default(),
                1.0,
                &mut pick,
            ),
        );
        let mut walker = Walker::at(actor, [0.0; 3], 0.0, 1.0, false);
        walker.package = Some(FormId(fixture::TO_CHAIR));
        walker.package_kind = Some(world::ai::kinds::TRAVEL);
        let mut life = Life::default();
        let mut seats = Seats::new(&game.order);
        let mesh = world::ai::NavMesh::load(&game.order, FormId(fixture::CELL));
        let moves = MoveSettings::defaults();
        let mut ctx = Ctx {
            game: &game,
            state: &mut state,
            seats: &mut seats,
            mesh: &mesh,
            moves: &moves,
            now: 1.0,
            dt: 0.1,
            fighting: false,
            talking: true,
        };

        assert!(rethink_queued_package_before_furniture(
            &mut ctx,
            &mut walker,
            &mut life,
            12.0,
        ));
        let sitter = ctx.state.sitters.get(&actor).unwrap();
        assert!(sitter.stand_requested);
        assert!(life.getting_up);
        assert_eq!(walker.package, Some(FormId(fixture::TO_CHAIR)));
        assert!(!walker.evaluate);
        assert!(!ctx.state.evaluate.contains(&actor));
        assert!((walker.clock.timer - 19.9).abs() < 1e-5);

        // The still-pending chair exit must not cause another package check
        // or tick this frame after the furniture procedure releases the actor.
        assert!(!rethink_queued_package_before_furniture(
            &mut ctx,
            &mut walker,
            &mut life,
            12.0,
        ));
        assert!((walker.clock.timer - 19.9).abs() < 1e-5);
    }

    #[test]
    fn queued_package_change_waits_for_furniture_entry_or_exit_to_finish() {
        use testdata::ai::ids as fixture;

        let data = testdata::ai::world("seated-package-rethink-gate");
        let game = cellview::Game::open(
            data.path(),
            &cellview::Options {
                official: true,
                ..default()
            },
        )
        .unwrap();
        let actor = FormId(fixture::TALKER_REF);
        let chair = FormId(fixture::CHAIR_REF);
        let mut state = world::scripting::GameState::default();
        state
            .script_packages
            .insert(actor, FormId(fixture::TO_MARKER));
        state.evaluate.insert(actor);
        state.furniture.insert(actor, chair);
        let mut pick = |_, _, _| None;
        let marker = world::furniture::PlacedMarker {
            index: 2,
            number: 14,
            position: [0.0, 0.0, 0.0],
            heading: 0.0,
        };
        let mut sitter = world::furniture::Sitter::seated(
            chair,
            marker,
            world::furniture::MarkerSettings::default(),
            1.0,
            &mut pick,
        );
        sitter.state = SitState::WantToStand;
        state.sitters.insert(actor, sitter);
        let mut walker = Walker::at(actor, [0.0; 3], 0.0, 1.0, false);
        walker.package = Some(FormId(fixture::TO_CHAIR));
        walker.package_kind = Some(world::ai::kinds::TRAVEL);
        let mut life = Life::default();
        let mut seats = Seats::new(&game.order);
        let mesh = world::ai::NavMesh::load(&game.order, FormId(fixture::CELL));
        let moves = MoveSettings::defaults();
        let mut ctx = Ctx {
            game: &game,
            state: &mut state,
            seats: &mut seats,
            mesh: &mesh,
            moves: &moves,
            now: 1.0,
            dt: 0.1,
            fighting: false,
            talking: false,
        };

        assert!(!rethink_queued_package_before_furniture(
            &mut ctx,
            &mut walker,
            &mut life,
            12.0,
        ));
        assert!(ctx.state.evaluate.contains(&actor));
        assert!(!ctx.state.sitters.get(&actor).unwrap().stand_requested);
        assert_eq!(walker.clock, PackageClock::default());
    }
}
