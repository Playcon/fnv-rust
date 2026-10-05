//! The game's scripts in the viewer (`world::scripting`): quest scripts run
//! as time passes (not while talking, when the game is in a menu), and
//! what scripts make happen is shown: messages, journal entries and
//! objectives as notices at the top right; people starting to talk to the
//! player; placed objects shown or hidden by `Enable` / `Disable`. Also the
//! things the player can use where they are: E takes an item, takes what's
//! in a container, or runs an object's `OnActivate`.

use std::collections::HashSet;

use bevy::prelude::*;
use esm::FormId;
use world::dialogue::PLAYER_REF;
use world::scripting::{Event, GameState, Interactive, Runner, ScriptCache};

use crate::dialogue::{Conversation, DialogueState};
use crate::walk::game_point;
use crate::{FlyCamera, GameFiles};

/// Parsed scripts and quests, read once.
#[derive(Resource, Default)]
pub struct Scripts(pub ScriptCache);

/// The interior the player is in (a cell's form ID); outdoors it's worked
/// out from where the player stands.
#[derive(Resource, Default)]
pub struct Here(pub Option<u32>);

/// Which placed reference an entity draws (for `Enable` / `Disable`).
#[derive(Component, Clone, Copy)]
pub struct PlacedRef(pub u32);

/// Talking a script started: who speaks, the topic (`None`: their
/// greeting), and whether it's a conversation (`StartConversation`) or
/// just a line said (`SayTo`).
#[derive(Resource, Default)]
pub struct ScriptedTalk(pub Option<(FormId, Option<FormId>, bool)>);

/// Notices on screen, each with the time it appeared.
#[derive(Resource, Default)]
pub struct Notices(Vec<(String, f32)>);

impl Notices {
    /// Shows a notice from `now` (seconds since the viewer started).
    pub fn push(&mut self, text: String, now: f32) {
        self.0.push((text, now));
    }
}

/// `--stage`: a quest stage to set once the place has loaded.
#[derive(Resource, Default)]
pub struct StartStage(pub Option<(String, u16)>);

/// `--run`: script lines to run once the place has loaded, as the game's
/// console runs them.
#[derive(Resource, Default)]
pub struct StartCommands(pub Vec<String>);

/// The objects to use where the player is (the interior, or the outdoor
/// square stood in), and the triggers the player is inside.
#[derive(Resource, Default)]
pub struct CellScripts {
    cell: Option<FormId>,
    pub refs: Vec<Interactive>,
    /// (trigger, who) for everyone inside a trigger.
    inside: HashSet<(FormId, FormId)>,
    /// Where the player sat down, while they sit.
    seat: Option<[f32; 3]>,
}

/// World-space bounds of rendered placed objects in the loaded cell(s).
/// Some base records have an empty OBND despite having real geometry.
#[derive(Resource, Default)]
pub struct ObjectBounds(pub std::collections::HashMap<u32, ([f32; 3], [f32; 3])>);

impl ObjectBounds {
    pub fn from_scene(bounds: &[cellview::ObjectBounds]) -> Self {
        Self(bounds.iter().map(|b| (b.reference, (b.lo, b.hi))).collect())
    }

    fn hit(&self, reference: FormId, eye: [f32; 3], dir: [f32; 3]) -> Option<f32> {
        let (lo, hi) = self.0.get(&reference.0)?;
        ray_box(eye, dir, *lo, *hi)
    }
}

/// Where a ray enters an axis-aligned world-space box.
fn ray_box(origin: [f32; 3], dir: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> Option<f32> {
    let (mut near, mut far) = (0.0f32, f32::INFINITY);
    for axis in 0..3 {
        if dir[axis].abs() < 1e-9 {
            if origin[axis] < lo[axis] || origin[axis] > hi[axis] {
                return None;
            }
            continue;
        }
        let a = (lo[axis] - origin[axis]) / dir[axis];
        let b = (hi[axis] - origin[axis]) / dir[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (near <= far).then_some(near)
}

fn target_hit_distance(
    object: &Interactive,
    object_bounds: &ObjectBounds,
    eye: [f32; 3],
    dir: [f32; 3],
) -> Option<f32> {
    let empty_record_bounds = object
        .bounds
        .is_none_or(|(lo, hi)| lo == [0.0; 3] && hi == [0.0; 3]);
    if empty_record_bounds {
        object_bounds
            .hit(object.reference, eye, dir)
            .or_else(|| object.ray_hit(eye, dir))
    } else {
        object.ray_hit(eye, dir)
    }
}

fn hidden_by_surface(collision: &physics::Collider, eye: [f32; 3], dir: [f32; 3], d: f32) -> bool {
    collision
        .raycast(eye, dir, d)
        .is_some_and(|(wall, _)| wall < d - 10.0)
}

/// How far the player can move from where they sat down before they've
/// got up (they don't move onto the seat yet).
const LEAVE_SEAT: f32 = 40.0;

/// The object in view that E would use, and the prompt for it ("Take
/// Bottle Cap", "Open Chest", "Vit-o-matic Vigor Tester"), which
/// `walk::doors` shows when no door is in view.
#[derive(Resource, Default)]
pub struct Activatable(pub Option<(FormId, String)>);

/// E was pressed on [`Activatable`]'s object.
#[derive(Resource, Default)]
pub struct ActivateRequest(pub Option<FormId>);

/// Heights above the feet at which the player's body is tested against
/// trigger volumes (feet, middle, head; the game tests the character's
/// collision shape).
const BODY_HEIGHTS: [f32; 3] = [10.0, 64.0, 110.0];

/// Why something locked doesn't open now.
pub enum Locked {
    /// The lockpicking menu opens on it (`lockpick`).
    Pick,
    /// What to tell the player.
    Says(String),
}

/// The player tries something locked (`world::locks`): `None` when it
/// opens (it isn't locked, or they have its key), else the lockpicking
/// menu or a refusal in the game's own words: `sImpossibleLock` (only its
/// key opens it; no sound, `005180b0`) or `sLockpickSkillTooLow` with
/// `UIPopUpMessageGeneral` (`0078db00`).
pub fn locked(
    order: &esm::LoadOrder,
    state: &mut world::scripting::GameState,
    reference: FormId,
    label: &str,
) -> Option<Locked> {
    use world::locks::Opening;
    match world::locks::try_open(order, state, reference) {
        Opening::Open => None,
        Opening::WithKey => {
            println!("Unlocked {label} with its key.");
            None
        }
        Opening::Pick(_) => Some(Locked::Pick),
        Opening::NeedsKey => Some(Locked::Says(world::lockpick::message_text(
            order,
            "sImpossibleLock",
            "This lock cannot be picked. It requires a key to open.",
        ))),
        Opening::NeedsSkill(level) => {
            if let Some(s) = order.form_by_editor_id(crate::lockpick::POPUP_SOUND) {
                state.events.push(Event::Sound(s));
            }
            Some(Locked::Says(
                world::lockpick::message_text(
                    order,
                    "sLockpickSkillTooLow",
                    "You need a lockpick skill of %d to pick this lock.",
                )
                .replace("%d", &level.to_string()),
            ))
        }
    }
}

/// The player opens a door: a door a script has "destroyed" doesn't open;
/// a locked one opens the lockpicking menu (`pick` set to it) or says why
/// not; a door with an `OnActivate` script runs it and opens only if the
/// script calls `Activate` on it (`GSDocMitchellDoorScript` notes the
/// player left, then does); any other door opens.
pub fn door_opens(
    order: &esm::LoadOrder,
    cache: &ScriptCache,
    state: &mut world::scripting::GameState,
    door: FormId,
    pick: &mut Option<FormId>,
) -> bool {
    if state.destroyed.contains(&door) {
        println!("The door won't open.");
        return false;
    }
    match locked(order, state, door, "The door") {
        None => {}
        Some(Locked::Pick) => {
            *pick = Some(door);
            return false;
        }
        Some(Locked::Says(why)) => {
            println!("{why}");
            state.events.push(Event::Message {
                title: None,
                text: why,
                buttons: Vec::new(),
            });
            return false;
        }
    }
    let has_script = world::scripting::script_of(order, door)
        .and_then(|s| cache.script(order, s))
        .is_some_and(|s| s.blocks.iter().any(|b| b.kind == "onactivate"));
    if !has_script {
        return true;
    }
    let before = state.events.len();
    Runner::new(order, cache, state).run_event(door, "onactivate", PLAYER_REF);
    let opened = state.events[before..]
        .iter()
        .any(|e| matches!(e, Event::Activate { what, .. } if *what == door));
    state
        .events
        .retain(|e| !matches!(e, Event::Activate { what, .. } if *what == door));
    opened
}

/// The scripted objects where the player now is: a new place runs their
/// `OnLoad` blocks.
fn refresh_cell_scripts(
    order: &esm::LoadOrder,
    cache: &ScriptCache,
    state: &mut world::scripting::GameState,
    cell_scripts: &mut CellScripts,
) {
    if state.player_cell == cell_scripts.cell {
        return;
    }
    cell_scripts.cell = state.player_cell;
    cell_scripts.inside.clear();
    cell_scripts.refs = state
        .player_cell
        .map(|c| world::scripting::interactive_references(order, c))
        .unwrap_or_default();
    let mut runner = Runner::new(order, cache, state);
    for r in cell_scripts.refs.iter().filter(|r| r.script.is_some()) {
        runner.run_blocks(r.reference, Some(r.reference), "onload", |_| true);
    }
}

/// Whether an object's script has an `OnActivate` block.
fn has_on_activate(order: &esm::LoadOrder, cache: &ScriptCache, r: &Interactive) -> bool {
    r.script
        .and_then(|s| cache.script(order, s))
        .is_some_and(|s| s.blocks.iter().any(|b| b.kind == "onactivate"))
}

/// What using an object did.
enum Used {
    /// Something to tell the player.
    Notice(String),
    /// A container to open on screen: its reference and name.
    Container(FormId, String),
    /// A terminal's screen: its record and the placed terminal.
    Terminal(FormId, FormId),
    /// The lockpicking menu, on a locked container.
    Lockpick(FormId),
    /// A bed's sleep menu (the game's checks passed).
    Sleep,
}

/// E on an object: its `OnActivate` script runs, if it has one, and its
/// usual action happens (an item is taken, a container opens) unless the
/// script has one and doesn't call `Activate`, as for doors.
fn use_object(
    order: &esm::LoadOrder,
    cache: &ScriptCache,
    state: &mut GameState,
    r: &Interactive,
) -> Option<Used> {
    if has_on_activate(order, cache, r) {
        let before = state.events.len();
        Runner::new(order, cache, state).run_event(r.reference, "onactivate", PLAYER_REF);
        let went_on = state.events[before..]
            .iter()
            .any(|e| matches!(e, Event::Activate { what, .. } if *what == r.reference));
        state
            .events
            .retain(|e| !matches!(e, Event::Activate { what, .. } if *what == r.reference));
        if !went_on {
            return None;
        }
    }
    let name = |id: FormId| {
        order
            .get(id)
            .and_then(|rr| rr.record().ok())
            .and_then(|rec| rec.full_name())
            .unwrap_or_else(|| id.to_string())
    };
    let counted = |item: FormId, n: i32| {
        if n > 1 {
            format!("{} ({n})", name(item))
        } else {
            name(item)
        }
    };
    if r.is_item() {
        // Someone else's: stealing (`world::crime`).
        let owner = world::crime::owner_of(order, state, r.reference);
        state.pick_up(order, r.reference, r.base, r.count);
        if let Some(owner) = owner.filter(|_| !world::crime::may_take(order, state, owner)) {
            if world::crime::steal(order, state, r.reference, owner) {
                println!("Seen stealing {}.", counted(r.base, r.count));
            }
        }
        return Some(Used::Notice(format!("{} added", counted(r.base, r.count))));
    }
    if r.is_container() {
        let label = r.name.clone().unwrap_or_else(|| "Container".into());
        return Some(match locked(order, state, r.reference, &label) {
            None => Used::Container(r.reference, label),
            Some(Locked::Pick) => Used::Lockpick(r.reference),
            Some(Locked::Says(why)) => Used::Notice(why),
        });
    }
    // A bed: the game's checks (`world::living::sleep::may_sleep_in`; the
    // player doesn't lie down), then its sleep menu (the game's,
    // `game_menus::sleepwait`).
    if r.is_furniture() && world::living::sleep::is_bed(order, r.reference) {
        return match world::living::sleep::may_sleep_in(order, state, r.reference) {
            Ok(()) => Some(Used::Sleep),
            Err(why) => Some(Used::Notice(why)),
        };
    }
    if r.is_furniture() {
        // Sitting: scripts see it (`IsCurrentFurnitureRef`,
        // `GetSitting`); the player stays where they are (no sitting
        // animation or seat position yet).
        state.sit(PLAYER_REF, r.reference);
    }
    if r.is_terminal() {
        return Some(Used::Terminal(r.base, r.reference));
    }
    None
}

/// Each frame in the game (not in a menu): the objects' `GameMode`
/// blocks, and triggers that the player's body, or a person, enters, stays
/// in or leaves (`people`: reference and feet).
fn run_cell_scripts(
    order: &esm::LoadOrder,
    cache: &ScriptCache,
    state: &mut world::scripting::GameState,
    cell_scripts: &mut CellScripts,
    people: &[(FormId, [f32; 3])],
    seconds: f32,
    sight: Option<&dyn world::sight::Sight>,
) {
    let mut runner = Runner::new(order, cache, state);
    runner.sight = sight;
    runner.seconds_passed = seconds;
    for r in cell_scripts.refs.iter().filter(|r| r.script.is_some()) {
        if !world::enabled_now(order, r.reference, &runner.state.disabled) {
            continue;
        }
        runner.run_blocks(r.reference, Some(r.reference), "gamemode", |_| true);
        if r.trigger.is_none() {
            continue;
        }
        for &(who, feet) in people {
            let inside = BODY_HEIGHTS
                .iter()
                .any(|h| r.contains([feet[0], feet[1], feet[2] + h]));
            let key = (r.reference, who);
            let was = cell_scripts.inside.contains(&key);
            match (was, inside) {
                (false, true) => {
                    cell_scripts.inside.insert(key);
                    runner.run_event(r.reference, "ontriggerenter", who);
                    runner.run_event(r.reference, "ontrigger", who);
                }
                (true, true) => runner.run_event(r.reference, "ontrigger", who),
                (true, false) => {
                    cell_scripts.inside.remove(&key);
                    runner.run_event(r.reference, "ontriggerleave", who);
                }
                (false, false) => {}
            }
        }
    }
}

/// What the player looks at within reach that E would use: an item, a
/// container, or an object whose script has an `OnActivate` block (people
/// and doors have their own handling), with the prompt for it.
fn object_in_view(
    cache: &ScriptCache,
    order: &esm::LoadOrder,
    state: &GameState,
    refs: &[Interactive],
    object_bounds: &ObjectBounds,
    collision: &physics::Collider,
    view: ([f32; 3], [f32; 3]),
) -> Option<(FormId, String)> {
    let (eye, dir) = view;
    let mut best: Option<(f32, &Interactive)> = None;
    for r in refs {
        if r.trigger.is_some() || [*b"NPC_", *b"CREA", *b"DOOR"].contains(r.kind.as_bytes()) {
            continue;
        }
        let Some(d) = target_hit_distance(r, object_bounds, eye, dir) else {
            continue;
        };
        if d > cellview::ACTIVATE_REACH || best.is_some_and(|(bd, _)| d >= bd) {
            continue;
        }
        let usable =
            r.is_item() || r.is_container() || r.is_furniture() || has_on_activate(order, cache, r);
        if usable && world::enabled_now(order, r.reference, &state.disabled) {
            best = Some((d, r));
        }
    }
    let (d, r) = best?;
    // Walls in the way hide it.
    if hidden_by_surface(collision, eye, dir, d) {
        return None;
    }
    let name = r.name.clone().unwrap_or_else(|| r.reference.to_string());
    // Someone else's things: the game's "Steal" (`sSteal`, `00579690`).
    let owned = (r.is_item() || r.is_container())
        && !world::crime::may_take(
            order,
            state,
            world::crime::owner_of(order, state, r.reference),
        );
    let steal =
        || world::scripting::game_setting_text(order, "sSteal").unwrap_or_else(|| "Steal".into());
    let prompt = if r.is_item() {
        let verb = if owned { steal() } else { "Take".into() };
        if r.count > 1 {
            format!("{verb} {name} ({})", r.count)
        } else {
            format!("{verb} {name}")
        }
    } else if r.is_container() {
        let verb = if owned { steal() } else { "Open".into() };
        format!("{verb} {name}")
    } else if r.is_furniture() {
        if state.furniture.get(&PLAYER_REF) == Some(&r.reference) {
            return None;
        }
        format!("Sit ({name})")
    } else if r.is_terminal() {
        format!("Access {name}")
    } else {
        name
    };
    Some((r.reference, prompt))
}

/// A placed reference's base record (the reference itself if it has none).
fn base_or(order: &esm::LoadOrder, r: FormId) -> FormId {
    world::scripting::base_of(order, r).unwrap_or(r)
}

/// `MoveTo` on the player: go to where `to` stands, loading its place.
fn move_player(
    game: &cellview::Game,
    disabled: &world::Disabled,
    to: FormId,
    pending: &mut crate::PendingScene,
    pending_exterior: &mut crate::exterior::PendingExterior,
) -> Result<(), String> {
    let order = &game.order;
    let w = world::scripting::whereabouts(order, to)
        .ok_or_else(|| format!("{to} isn't a placed object"))?;
    let info = world::cell_info(order, w.cell).map_err(|e| e.to_string())?;
    if !info.interior {
        let world = w
            .world
            .or(info.world)
            .ok_or_else(|| "it's outdoors but in no worldspace".to_string())?;
        let grid = world::WorldGrid::load(order, world).map_err(|e| e.to_string())?;
        pending_exterior.0 = Some(crate::exterior::ExteriorStart {
            grid,
            feet: [w.position[0], w.position[1]],
            height: Some(w.position[2]),
            heading: w.heading,
        });
        return Ok(());
    }
    let mut scene = game.load_cell_now(w.cell, disabled).map_err(|e| e.0)?;
    let [x, y, z] = w.position;
    scene.start = cellview::Start {
        eye: [x, y, z + cellview::EYE_HEIGHT],
        heading: w.heading,
        via: "a script (MoveTo)",
    };
    pending.0 = Some(scene);
    Ok(())
}

/// The game writes keys in messages as `&sUActnForward;` (or
/// `&-sUActnForward;`), filled in with the key bound to that action; here
/// they're the viewer's keys.
fn fill_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find(';') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let action = tail[..end].trim_start_matches('-');
        let key = match action.strip_prefix("sUActn") {
            Some(a) => match a.to_ascii_lowercase().as_str() {
                "forward" => "W".to_string(),
                "back" => "S".to_string(),
                "sldleft" => "A".to_string(),
                "sldright" => "D".to_string(),
                "jump" => "Space".to_string(),
                "activate" => "E".to_string(),
                "sneak" => "Ctrl".to_string(),
                "togglerun" | "run" => "Shift".to_string(),
                "attack" => "the left mouse button".to_string(),
                "reload" => "R".to_string(),
                "menumode" | "pipboy" => "Tab".to_string(),
                "quicksave" => "F5".to_string(),
                "quickload" => "F9".to_string(),
                "vats" => "V".to_string(),
                other => format!("[{other}]"),
            },
            None => format!("&{};", &tail[..end]),
        };
        out.push_str(&key);
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A record's name (its `FULL`), else its editor ID.
fn record_name(order: &esm::LoadOrder, id: FormId) -> String {
    order
        .get(id)
        .and_then(|r| r.record().ok())
        .and_then(|r| r.full_name().or_else(|| r.editor_id()))
        .unwrap_or_else(|| id.to_string())
}

/// Where F5 saves and F9 loads (in the folder the viewer runs in).
const QUICKSAVE: &str = "nv-rs-quicksave.txt";

/// A loaded world cannot retain trigger occupancy or a seat from the world
/// it replaces, even when the saved player is in the same cell.
fn restore_script_state(state: &mut GameState, cells: &mut CellScripts, loaded: GameState) {
    *state = loaded;
    *cells = CellScripts::default();
}

/// F5 saves the game (its state and where the player stands); F9 loads
/// the last save and puts the player back there.
#[allow(clippy::too_many_arguments)]
pub fn save_and_load(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    game: Res<GameFiles>,
    mut state: ResMut<DialogueState>,
    mut notices: ResMut<Notices>,
    mut pending: ResMut<crate::PendingScene>,
    mut pending_exterior: ResMut<crate::exterior::PendingExterior>,
    cameras: Query<(&Transform, &FlyCamera)>,
    player: Res<crate::walk::Player>,
    mut player_idle: ResMut<crate::player_idle::PlayerIdle>,
    mut cell_scripts: ResMut<CellScripts>,
) {
    let now = time.elapsed_secs();
    let mut say = |text: String| {
        println!("{text}");
        notices.0.push((text, now));
    };
    if keys.just_pressed(KeyCode::F5) {
        let place = cameras.single().ok().and_then(|(t, input)| {
            let eye = game_point(t.translation);
            let f = t.forward().as_vec3();
            Some(world::save::PlayerPlace {
                cell: state.0.player_cell?,
                world: state.0.player_world,
                position: player.position_for_view(eye),
                heading: if player.walking {
                    -input.yaw
                } else {
                    f.x.atan2(-f.z)
                },
            })
        });
        let text = world::save::save(&state.0, place);
        match std::fs::write(QUICKSAVE, text) {
            Ok(()) => say(format!("Saved ({QUICKSAVE}).")),
            Err(e) => say(format!("Couldn't save: {e}")),
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        let loaded = std::fs::read_to_string(QUICKSAVE)
            .map_err(|e| e.to_string())
            .and_then(|text| world::save::load(&text));
        let (loaded, place) = match loaded {
            Ok(l) => l,
            Err(e) => {
                say(format!("Couldn't load {QUICKSAVE}: {e}"));
                return;
            }
        };
        restore_script_state(&mut state.0, &mut cell_scripts, loaded);
        *player_idle = crate::player_idle::PlayerIdle::default();
        let Some(place) = place else {
            say("Loaded (the save doesn't say where the player was).".into());
            return;
        };
        let order = &game.0.order;
        let result = match place.world {
            Some(world) => world::WorldGrid::load(order, world)
                .map(|grid| {
                    pending_exterior.0 = Some(crate::exterior::ExteriorStart {
                        grid,
                        feet: [place.position[0], place.position[1]],
                        height: Some(place.position[2]),
                        heading: place.heading,
                    });
                })
                .map_err(|e| e.to_string()),
            None => game
                .0
                .load_cell_now(place.cell, &state.0.disabled)
                .map(|mut scene| {
                    let [x, y, z] = place.position;
                    scene.start = cellview::Start {
                        eye: [x, y, z + cellview::EYE_HEIGHT],
                        heading: place.heading,
                        via: "a saved game",
                    };
                    pending.0 = Some(scene);
                })
                .map_err(|e| e.0),
        };
        match result {
            Ok(()) => say("Loaded.".into()),
            Err(e) => say(format!("Loaded, but couldn't go back there: {e}")),
        }
    }
}

/// How long a notice stays up, in seconds.
const NOTICE_SECONDS: f32 = 6.0;

#[derive(Component)]
pub struct NoticeText;

pub fn setup_notices(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.85, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(8.0),
            max_width: Val::Percent(40.0),
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Visibility::Hidden,
        NoticeText,
    ));
}

/// The notice panel's text and whether it shows.
type NoticePanel = (&'static mut Text, &'static mut Visibility);

/// What's around the player that they can use, and who else is there.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HereNow<'w> {
    cell_scripts: ResMut<'w, CellScripts>,
    activatable: ResMut<'w, Activatable>,
    activate_request: ResMut<'w, ActivateRequest>,
    collision: Res<'w, crate::walk::CellCollision>,
    talkers: Res<'w, crate::dialogue::Talkers>,
    sound_requests: ResMut<'w, crate::sounds::SoundRequests>,
    music_requests: ResMut<'w, crate::music::MusicRequests>,
    moved: ResMut<'w, crate::ai::Moved>,
    starts: ResMut<'w, crate::ai::Starts>,
    menus: ResMut<'w, crate::menus::Menus>,
    hud_messages: ResMut<'w, crate::hud::HudMessages>,
    groups: ResMut<'w, crate::PlayingGroups>,
    lockpicking: ResMut<'w, crate::lockpick::Lockpicking>,
    /// The game's clock speed (`SetGlobalTimeMultiplier`).
    virtual_time: ResMut<'w, Time<Virtual>>,
    /// The game's own menus open (their classes), for `MenuMode` blocks.
    menu_draw: Res<'w, crate::game_menus::MenuDraw>,
    player_idle: ResMut<'w, crate::player_idle::PlayerIdle>,
    object_bounds: Option<Res<'w, ObjectBounds>>,
}

/// Runs the scripts for this frame and carries out what they asked for.
#[allow(clippy::too_many_arguments)]
pub fn run_scripts(
    time: Res<Time>,
    game: Res<GameFiles>,
    scripts: Res<Scripts>,
    mut state: ResMut<DialogueState>,
    here: Res<Here>,
    exterior: Option<Res<crate::exterior::Exterior>>,
    conversation: Res<Conversation>,
    mut talk: ResMut<ScriptedTalk>,
    mut notices: ResMut<Notices>,
    (mut start_stage, player, mut pending, mut pending_exterior): (
        ResMut<StartStage>,
        Res<crate::walk::Player>,
        ResMut<crate::PendingScene>,
        ResMut<crate::exterior::PendingExterior>,
    ),
    mut start_commands: ResMut<StartCommands>,
    here_now: HereNow,
    cameras: Query<(&Transform, &FlyCamera, Option<&Projection>)>,
    mut placed: Query<(&PlacedRef, &mut Visibility)>,
    mut text: Query<NoticePanel, (With<NoticeText>, Without<PlacedRef>)>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let HereNow {
        mut cell_scripts,
        mut activatable,
        mut activate_request,
        collision,
        talkers,
        mut sound_requests,
        mut music_requests,
        mut moved,
        mut starts,
        menus: mut waiting,
        mut hud_messages,
        mut groups,
        mut lockpicking,
        mut virtual_time,
        menu_draw,
        mut player_idle,
        object_bounds,
    } = here_now;
    let order = &game.0.order;
    let now = time.elapsed_secs();
    // What the game announces goes to the HUD's message corner while it's
    // drawn, else to the notice panel.
    let mut announce = |n: String, notices: &mut Notices| {
        println!("{n}");
        if hud_messages.on {
            hud_messages.queue.push(n);
        } else {
            notices.0.push((n, now));
        }
    };
    // Tab, I and J open the Pip-Boy (`pipboy`).
    // T (the Rest control): the wait menu, unless the game refuses first
    // (`world::living::sleep`).
    if keys.just_pressed(KeyCode::KeyT) && conversation.0.is_none() && !waiting.is_open() {
        match world::living::sleep::may_wait(order, &state.0) {
            Ok(()) => waiting.push(crate::menus::Menu::SleepWait { sleep: false }),
            Err(why) => {
                println!("{why}");
                notices.0.push((why, now));
            }
        }
    }
    // `--stage`, once the place is up.
    if player.ready {
        if let Some((quest, n)) = start_stage.0.take() {
            match order.form_by_editor_id(&quest) {
                Some(q) => {
                    let set = Runner::new(order, &scripts.0, &mut state.0).set_stage(q, n);
                    println!(
                        "SetStage {quest} {n}: {}",
                        if set {
                            "set"
                        } else {
                            "not set (no such stage, or done)"
                        }
                    );
                }
                None => println!("--stage: there's no quest called {quest}"),
            }
        }
    }
    // Where the player is, for GetInCell and GetInWorldspace.
    let state = &mut state.0;
    let (eye, dir, heading) = cameras
        .single()
        .map(|(t, input, _)| {
            let f = t.forward().as_vec3();
            (
                game_point(t.translation),
                [f.x, -f.z, f.y],
                if player.walking {
                    -input.yaw
                } else {
                    f.x.atan2(-f.z)
                },
            )
        })
        .unwrap_or_default();
    // The camera's up and view angle, for `GetLineOfSight`.
    let (up, tan_half_fov, aspect) = cameras
        .single()
        .map(|(t, _, projection)| {
            let u = t.up().as_vec3();
            let (fov, aspect) = match projection {
                Some(Projection::Perspective(p)) => (p.fov, p.aspect_ratio),
                _ => (
                    cellview::vertical_fov(cellview::GAME_FOV_DEGREES),
                    16.0 / 9.0,
                ),
            };
            ([u.x, -u.z, u.y], (fov * 0.5).tan(), aspect)
        })
        .unwrap_or(([0.0, 0.0, 1.0], 1.0, 1.0));
    // Animation and V.A.T.S. can move the view independently of the body.
    // Quest positions, triggers and saves must follow the collision capsule.
    let feet = player.position_for_view(eye);
    state.player_position = Some(feet);
    // What has 3D now (`PlayMagicShaderVisuals` needs it): the rendered
    // placed objects, the people about and the player.
    let mut loaded: std::collections::HashSet<FormId> = object_bounds
        .as_deref()
        .map(|b| b.0.keys().map(|&r| FormId(r)).collect())
        .unwrap_or_default();
    loaded.insert(PLAYER_REF);
    loaded.extend(talkers.0.iter().map(|t| t.reference));
    world::more_functions::report_loaded(state, loaded);
    match &exterior {
        Some(e) => {
            state.player_world = Some(e.grid.world.form_id);
            state.player_cell = e.grid.cells.get(&world::square_of(feet)).copied();
        }
        None => {
            state.player_world = None;
            state.player_cell = here.0.map(FormId);
        }
    }
    // `--run`: each line as the console would run it, once the place is
    // up and the player is in it.
    if player.ready {
        for line in std::mem::take(&mut start_commands.0) {
            let flow = Runner::new(order, &scripts.0, state).run_source(&line, None, None);
            println!("{line}: {flow:?}");
        }
    }
    refresh_cell_scripts(order, &scripts.0, state, &mut cell_scripts);
    // Walking away from a seat gets the player up.
    if let Some(seat) = cell_scripts.seat {
        let d = ((feet[0] - seat[0]).powi(2) + (feet[1] - seat[1]).powi(2)).sqrt();
        if d > LEAVE_SEAT || !state.furniture.contains_key(&PLAYER_REF) {
            state.stand(PLAYER_REF);
            cell_scripts.seat = None;
        }
    }
    // Time stands still in the dialogue menu (not while a line is said)
    // and in the other menus.
    let dt = time.delta_secs();
    if conversation.0.as_ref().is_none_or(|t| t.is_line_only()) && !waiting.is_open() {
        let warner = state.living.trespass.as_ref().map(|w| w.warner);
        let mut people: Vec<(FormId, [f32; 3])> = vec![(PLAYER_REF, feet)];
        people.extend(talkers.0.iter().map(|t| (t.reference, t.position)));
        let sight = crate::sight::ViewerSight {
            collision: &collision.0,
            bounds: object_bounds.as_deref(),
            people: &people,
            eye,
            forward: dir,
            up,
            tan_half_fov,
            aspect,
        };
        Runner::new(order, &scripts.0, state)
            .with_sight(&sight)
            .update(dt);
        // Someone coming to warn the trespassing player off
        // (`world::living::trespass`).
        let now_warner = state.living.trespass.as_ref().map(|w| w.warner);
        if now_warner != warner {
            match now_warner {
                Some(w) => println!(
                    "{} comes to warn you off (trespassing).",
                    record_name(order, base_or(order, w))
                ),
                None => println!("The trespass warning is over."),
            }
        }
        if player.ready {
            run_cell_scripts(
                order,
                &scripts.0,
                state,
                &mut cell_scripts,
                &people,
                dt,
                Some(&sight),
            );
        }
        // The sleep/wait menu open: the scripts' `MenuMode 1012` blocks
        // run (`PlayerBedSCRIPT` notes `IsPCSleeping` there).
        if menu_draw
            .1
            .contains(&i32::from(world::living::sleep::SLEEP_WAIT_MENU))
        {
            let refs: Vec<FormId> = cell_scripts
                .refs
                .iter()
                .filter(|r| r.script.is_some())
                .map(|r| r.reference)
                .collect();
            world::living::sleep::menu_mode(&mut Runner::new(order, &scripts.0, state), &refs);
        }
        if let Some(r) = activate_request.0.take() {
            let object = cell_scripts.refs.iter().find(|o| o.reference == r);
            match object.and_then(|o| use_object(order, &scripts.0, state, o)) {
                Some(Used::Notice(n)) => announce(n, &mut notices),
                Some(Used::Container(c, name)) => {
                    waiting.push(crate::menus::Menu::Container(c, name));
                }
                Some(Used::Terminal(t, r)) => {
                    waiting.push(crate::menus::Menu::Terminal(t, r));
                }
                Some(Used::Lockpick(r)) => lockpicking.request = Some(r),
                Some(Used::Sleep) => {
                    waiting.push(crate::menus::Menu::SleepWait { sleep: true });
                }
                None => {}
            }
            // A taken item's pick-up sound.
            if let Some(o) = object.filter(|o| o.is_item()) {
                if !world::enabled_now(order, o.reference, &state.disabled) {
                    if let Some(s) = world::sound::pickup_sound(order, o.base) {
                        sound_requests.0.push(s);
                    }
                }
            }
            if state.furniture.contains_key(&PLAYER_REF) && cell_scripts.seat.is_none() {
                cell_scripts.seat = Some(feet);
            }
        }
    }
    // Scripts can turn the crosshair's roll-over text (and with it using
    // things) off (`DisablePlayerControls`).
    let rollover = !state.controls_off[world::scripting::controls::ROLLOVER];
    activatable.0 = if conversation.0.is_none() && rollover {
        object_in_view(
            &scripts.0,
            order,
            state,
            &cell_scripts.refs,
            object_bounds.as_deref().unwrap_or(&ObjectBounds::default()),
            &collision.0,
            (eye, dir),
        )
    } else {
        None
    };

    // Menus scripts opened. None are drawn yet: each counts as opened and
    // closed at once, with nothing changed, so its MenuMode blocks run once.
    let mut menus = Vec::new();
    for event in std::mem::take(&mut state.events) {
        let name = |id: FormId| {
            order
                .get(id)
                .and_then(|r| r.record().ok())
                .and_then(|r| r.full_name().or_else(|| r.editor_id()))
                .unwrap_or_else(|| id.to_string())
        };
        let notice = match event {
            // A message box waits for an answer.
            Event::Message {
                title,
                text,
                buttons,
            } if !buttons.is_empty() => {
                waiting.push(crate::menus::Menu::Message {
                    title,
                    text: fill_keys(&text),
                    buttons,
                });
                None
            }
            Event::Message { title, text, .. } => Some(fill_keys(&match title {
                Some(t) => format!("{t}\n{text}"),
                None => text,
            })),
            Event::CharacterMenu(m) => {
                waiting.push(crate::menus::Menu::Character(m));
                None
            }
            Event::Barter(merchant) => {
                waiting.push(crate::menus::Menu::Barter(merchant));
                None
            }
            // `ShowSleepWaitMenu` (its refusals already given): the game's
            // sleep/wait menu (`game_menus::sleepwait`).
            Event::SleepWaitMenu { sleep } => {
                waiting.push(crate::menus::Menu::SleepWait { sleep });
                None
            }
            Event::Died { who, by } => Some(if who == PLAYER_REF {
                "You died.".to_string()
            } else {
                format!("{} was killed by {}.", name(who), name(by))
            }),
            Event::Journal { quest, text } => Some(format!("{}: {text}", name(quest))),
            Event::Objective {
                text, completed, ..
            } => Some(if completed {
                format!("Completed: {text}")
            } else {
                text
            }),
            Event::Talk {
                speaker,
                to,
                topic,
                conversation,
            } => {
                // `StartConversation` by someone else: they come up first
                // (the conversation package, `ai::Starts`).
                if conversation && speaker != PLAYER_REF && to.0 != 0 {
                    starts.0.push((speaker, to, topic));
                } else if (to == PLAYER_REF || to.0 == 0) && talk.0.is_none() {
                    // `Say` speaks to no one in particular (`to` 0): the
                    // player hears it as a line said to them.
                    talk.0 = Some((speaker, topic, conversation));
                }
                None
            }
            Event::PackageAction { who, package, kind } => {
                if let Some(p) = world::ai::Package::load(order, package) {
                    use world::scripting::PackageActionKind;
                    let action = match kind {
                        PackageActionKind::Begin => &p.actions.begin,
                        PackageActionKind::Change => &p.actions.change,
                    };
                    // Camera-bearing player idles with no script or topic
                    // can now use the actual KF. Other callback payloads
                    // still require their own synchronous dispatcher.
                    if who == PLAYER_REF
                        && action.topic.is_none()
                        && action.bytecode().is_empty()
                        && action.source().is_none_or(|s| s.trim().is_empty())
                    {
                        if let Some(idle) = action.idle {
                            player_idle.requests.push(idle);
                        }
                        continue;
                    }
                    if action.idle.is_some()
                        || action.topic.is_some()
                        || !action.bytecode().is_empty()
                    {
                        println!(
                            "{}: package {} {kind:?} action requested (playback pending).",
                            name(who),
                            name(package)
                        );
                    }
                }
                None
            }
            Event::PlayIdle { who, idle } => {
                if who != PLAYER_REF {
                    player_idle.npc_requests.insert(who, idle);
                } else {
                    println!("Player PlayIdle {}: scripted dispatch pending.", name(idle));
                }
                None
            }
            Event::SwapTexture {
                what,
                node,
                texture,
            } => {
                println!(
                    "A script gives {}'s {node} the texture {texture} (not shown here).",
                    name(what)
                );
                None
            }
            Event::Enable(r, on) => {
                let mut found = false;
                for (p, mut visibility) in &mut placed {
                    if p.0 == r.0 {
                        *visibility = if on {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        };
                        found = true;
                    }
                }
                if on && !found {
                    println!(
                        "A script enabled {} ({}), which isn't loaded here.",
                        name(r),
                        r
                    );
                }
                None
            }
            Event::PlayerControls(_) => {
                let names = [
                    "movement",
                    "Pip-Boy",
                    "fighting",
                    "view",
                    "looking",
                    "roll-over",
                    "sneaking",
                ];
                let off: Vec<&str> = names
                    .iter()
                    .zip(state.controls_off)
                    .filter(|(_, off)| *off)
                    .map(|(n, _)| *n)
                    .collect();
                println!(
                    "A script set the player's controls: {}.",
                    if off.is_empty() {
                        "all on".to_string()
                    } else {
                        format!("off: {}", off.join(", "))
                    }
                );
                None
            }
            Event::MoveTo { what, to } if what == PLAYER_REF => {
                println!("A script moves the player to {}.", name(to));
                if let Err(e) = move_player(
                    &game.0,
                    &state.disabled,
                    to,
                    &mut pending,
                    &mut pending_exterior,
                ) {
                    println!("  couldn't: {e}");
                }
                None
            }
            // Someone loaded here goes to the marker (or out of sight, if
            // it's elsewhere); someone moved in from elsewhere shows up
            // only once the place is loaded again.
            Event::MoveTo { what, to } => {
                println!("A script moved {} to {}.", name(what), name(to));
                moved.0.push(what);
                None
            }
            // Played by `effects::play_effects` from the state's list.
            Event::ImageSpace(m, on) => {
                println!(
                    "A script {} the screen effect {}.",
                    if on { "applied" } else { "removed" },
                    name(m)
                );
                None
            }
            // Not played yet: said, so it's clear what scripts did.
            Event::Video(file) => {
                println!("A script plays the video {file} (Bink videos aren't played here).");
                None
            }
            // Played by `music::play_music`.
            Event::Music(m) => {
                println!("A script plays the music {}.", name(m));
                music_requests.0.push(m);
                None
            }
            Event::Weather(w) => {
                match w {
                    Some(w) => println!("A script forces the weather {}.", name(w)),
                    None => println!("A script releases the forced weather."),
                }
                None
            }
            Event::Menu(menu) => {
                menus.push(menu);
                Some(match menu {
                    world::scripting::RACE_SEX_MENU => {
                        "(The face and body menu would open here; kept as it is.)".to_string()
                    }
                    _ => format!("(Menu {menu} would open here.)"),
                })
            }
            Event::Sound(sound) => {
                sound_requests.0.push(sound);
                None
            }
            // A placed object's model plays a sequence from now on
            // (`move_pieces`). The script's flags (a blend-in time on
            // objects' controllers, `005c0df0`) aren't used yet.
            Event::PlayGroup {
                who,
                group,
                flags: _,
            } => {
                groups.0.insert(who.0, (group, now));
                None
            }
            // A script activating a door that swings where it stands opens
            // or closes it, with its sound (`world::doors`).
            Event::Activate { what, by } if world::doors::is_swing_door(order, what) => {
                crate::doors::activate(
                    order,
                    state,
                    &mut sound_requests,
                    what,
                    by.filter(|b| b.0 != 0),
                );
                None
            }
            // A script having the player use a terminal or a container
            // opens it, as the player's own E would (a locked container
            // its lockpicking menu, or says why not).
            Event::Activate {
                what,
                by: Some(PLAYER_REF),
            } => {
                let base = world::scripting::base_of(order, what);
                match base.and_then(|b| order.get(b)).map(|r| r.entry.header.kind) {
                    Some(k) if k.as_bytes() == b"TERM" => {
                        waiting.push(crate::menus::Menu::Terminal(base.unwrap_or(what), what));
                        None
                    }
                    Some(k) if k.as_bytes() == b"CONT" => {
                        match locked(order, state, what, &name(what)) {
                            None => {
                                waiting.push(crate::menus::Menu::Container(what, name(what)));
                                None
                            }
                            Some(Locked::Pick) => {
                                lockpicking.request = Some(what);
                                None
                            }
                            Some(Locked::Says(why)) => Some(why),
                        }
                    }
                    _ => None,
                }
            }
            Event::Activate { .. } => None,
            Event::More(shown) => {
                println!("{}", world::more_functions::describe(order, state, &shown));
                match shown {
                    // The game's save manager is asked for a save
                    // (`Autosave`, `ForceSave`, `SystemSave`): here this
                    // project's own format, as F5's, to a file of its own.
                    world::more_functions::Shown::Save(kind) => {
                        let file = match kind {
                            world::more_functions::SaveKind::Autosave => "nv-rs-autosave.txt",
                            world::more_functions::SaveKind::Force => "nv-rs-forcesave.txt",
                            world::more_functions::SaveKind::System => "nv-rs-systemsave.txt",
                        };
                        let place = state.player_cell.map(|cell| world::save::PlayerPlace {
                            cell,
                            world: state.player_world,
                            position: feet,
                            heading,
                        });
                        match std::fs::write(file, world::save::save(state, place)) {
                            Ok(()) => println!("Saved ({file})."),
                            Err(e) => println!("Couldn't save {file}: {e}"),
                        }
                    }
                    // Everything runs at the multiplier's speed.
                    world::more_functions::Shown::TimeMultiplier(m) => {
                        virtual_time.set_relative_speed(m.max(0.0));
                    }
                    _ => {}
                }
                None
            }
        };
        if let Some(n) = notice {
            announce(n, &mut notices);
        }
    }
    for menu in menus {
        Runner::new(order, &scripts.0, state).menu_mode(menu);
    }

    notices.0.retain(|(_, at)| now - at < NOTICE_SECONDS);
    let shown = notices
        .0
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    for (mut t, mut visibility) in &mut text {
        if t.0 != shown {
            t.0 = shown.clone();
        }
        *visibility = if shown.is_empty() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_collider(lo: [f32; 3], hi: [f32; 3]) -> physics::Collider {
        let [x0, y0, z0] = lo;
        let [x1, y1, z1] = hi;
        let vertices = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        let triangles = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        let mut collider = physics::Collider::new();
        collider.add(&vertices, &triangles);
        collider
    }

    fn vigor_tester_ref() -> Interactive {
        Interactive {
            reference: FormId(0x0010_4c13),
            base: FormId(0x0010_4c0a),
            script: Some(FormId(0x0010_4c03)),
            count: 1,
            position: [1883.0, 1763.0, 7360.0],
            rotation: [0.0; 3],
            scale: 1.0,
            trigger: None,
            // FalloutNV.esm's VCG01VigorTester ACTI has empty OBND.
            bounds: Some(([0.0; 3], [0.0; 3])),
            name: Some("Vit-o-matic Vigor Tester".into()),
            kind: esm::FourCC::new(b"ACTI"),
        }
    }

    #[test]
    fn mesh_bounds_do_not_expand_targets_with_real_record_bounds() {
        let mut object = vigor_tester_ref();
        object.bounds = Some(([-1.0; 3], [1.0; 3]));
        let mesh_bounds = ObjectBounds::from_scene(&[cellview::ObjectBounds {
            reference: object.reference.0,
            lo: [1842.0, 1746.0, 7360.0],
            hi: [1924.0, 1782.0, 7522.0],
        }]);
        assert!(target_hit_distance(
            &object,
            &mesh_bounds,
            [1883.0, 1600.0, 7440.0],
            [0.0, 1.0, 0.0],
        )
        .is_none());
    }

    #[test]
    fn same_cell_reload_reenters_triggers_from_the_loaded_state() {
        use esm::{ActivePlugins, LoadOrder};
        use testdata::functions::ids::{HOUSE, VIGOR_QUEST};

        let data = testdata::functions::functions("vigor-reload");
        let order = LoadOrder::from_data_dir(data.path(), &ActivePlugins::OfficialOnly).unwrap();
        let cache = ScriptCache::default();
        let mut state = GameState::new(&order);
        state.player_cell = Some(FormId(HOUSE));
        let mut cells = CellScripts::default();
        refresh_cell_scripts(&order, &cache, &mut state, &mut cells);
        let people = [(PLAYER_REF, [1888.0, 1835.0, 7360.0])];
        // Enter before the quest permits the instruction, then load a save
        // at stage55 in that same volume. Old occupancy must not suppress it.
        run_cell_scripts(&order, &cache, &mut state, &mut cells, &people, 0.016, None);
        assert!(!cells.inside.is_empty());
        cells.seat = Some([1.0; 3]);
        let mut loaded = GameState::new(&order);
        loaded.player_cell = Some(FormId(HOUSE));
        loaded.stages.insert(FormId(VIGOR_QUEST), 55);
        restore_script_state(&mut state, &mut cells, loaded);
        refresh_cell_scripts(&order, &cache, &mut state, &mut cells);
        run_cell_scripts(&order, &cache, &mut state, &mut cells, &people, 0.016, None);
        assert_eq!(state.stages.get(&FormId(VIGOR_QUEST)), Some(&60));
        assert!(cells.seat.is_none());
    }

    #[test]
    fn authored_vigor_activation_opens_special_at_stage_60_and_advances_to_65() {
        use esm::{ActivePlugins, LoadOrder};
        use testdata::functions::ids::{HOUSE, VIGOR_QUEST, VIGOR_TESTER_REF};

        let data = testdata::functions::functions("vigor-activation");
        let order = LoadOrder::from_data_dir(data.path(), &ActivePlugins::OfficialOnly).unwrap();
        let cache = ScriptCache::default();
        let refs = world::scripting::interactive_references(&order, FormId(HOUSE));
        let tester = refs
            .iter()
            .find(|r| r.reference == FormId(VIGOR_TESTER_REF))
            .cloned()
            .expect("fixture placed the scripted tester");

        let mut state = GameState::new(&order);
        state.stages.insert(FormId(VIGOR_QUEST), 55);
        Runner::new(&order, &cache, &mut state).run_source(
            "SetObjectiveDisplayed VCG01 10 1",
            None,
            None,
        );
        assert!(use_object(&order, &cache, &mut state, &tester).is_none());
        assert!(!state
            .events
            .iter()
            .any(|e| matches!(e, Event::CharacterMenu(_))));
        assert_eq!(state.stages.get(&FormId(VIGOR_QUEST)), Some(&55));
        let mut cell_scripts = CellScripts {
            refs,
            ..Default::default()
        };
        run_cell_scripts(
            &order,
            &cache,
            &mut state,
            &mut cell_scripts,
            &[(PLAYER_REF, [1888.0, 1835.0, 7360.0])],
            1.0 / 60.0,
            None,
        );
        assert_eq!(state.stages.get(&FormId(VIGOR_QUEST)), Some(&60));
        assert!(!state.objectives.contains_key(&(FormId(VIGOR_QUEST), 30)));
        assert!(state.objectives.contains_key(&(FormId(VIGOR_QUEST), 10)));
        assert!(use_object(&order, &cache, &mut state, &tester).is_none());
        assert!(!state
            .events
            .iter()
            .any(|e| matches!(e, Event::CharacterMenu(_))));

        // This is the actual installed INFO 001074A2 result script. The
        // headless fixture simulates its execution, not topic selection or
        // Doc's authored dialogue delivery.
        Runner::new(&order, &cache, &mut state).run_source(
            "SetObjectiveDisplayed VCG01 30 1",
            None,
            None,
        );
        assert!(state.objectives.contains_key(&(FormId(VIGOR_QUEST), 30)));
        assert!(use_object(&order, &cache, &mut state, &tester).is_none());
        assert_eq!(state.stages.get(&FormId(VIGOR_QUEST)), Some(&65));
        assert!(state.events.contains(&Event::CharacterMenu(
            world::chargen::CharacterMenu::Special { points: 40 }
        )));

        // Both authored gate conditions matter: stage 60 with objective 30
        // absent leaves the machine closed and the stage unchanged.
        let mut gated = GameState::new(&order);
        gated.stages.insert(FormId(VIGOR_QUEST), 60);
        assert!(use_object(&order, &cache, &mut gated, &tester).is_none());
        assert_eq!(gated.stages.get(&FormId(VIGOR_QUEST)), Some(&60));
        assert!(!gated
            .events
            .iter()
            .any(|e| matches!(e, Event::CharacterMenu(_))));
    }

    #[test]
    fn empty_obnd_uses_vigor_testers_real_mesh_bounds_and_keeps_wall_occlusion() {
        // nvinspect on the installed NV_VitoMaticVigorTester_Cabinet02.NIF
        // reports mesh and Havok hull extents x[-41,41], y[-17,19], z[0,162].
        // The placed reference is at (1883,1763,7360), rotation 0, scale 1.
        let object = vigor_tester_ref();
        let eye = [1883.0, 1600.0, 7440.0];
        let dir = [0.0, 1.0, 0.0];
        assert!(object.ray_hit(eye, dir).is_none());

        let object_bounds = ObjectBounds::from_scene(&[cellview::ObjectBounds {
            reference: object.reference.0,
            lo: [1842.0, 1746.0, 7360.0],
            hi: [1924.0, 1782.0, 7522.0],
        }]);
        let hit = target_hit_distance(&object, &object_bounds, eye, dir).unwrap();
        assert!((hit - 146.0).abs() < 1e-5, "{hit}");

        // The same NIF's collision hull reaches the same front plane, so it
        // is not mistaken for a blocking wall before the rendered target.
        let cabinet = box_collider([1842.0, 1746.0, 7360.0], [1924.0, 1782.0, 7522.0]);
        assert!(!hidden_by_surface(&cabinet, eye, dir, hit));

        // A separate nearer wall still hides the tester.
        let wall = box_collider([1800.0, 1699.0, 7300.0], [1960.0, 1701.0, 7600.0]);
        assert!(hidden_by_surface(&wall, eye, dir, hit));
    }

    #[test]
    fn keys_in_messages_become_the_viewers() {
        assert_eq!(
            fill_keys(
                "Use &-sUActnForward;&-sUActnSldleft;&-sUActnBack;&-sUActnSldright; to move."
            ),
            "Use WASD to move."
        );
        assert_eq!(
            fill_keys("To toggle between run and walk, &sUActnToggleRun;."),
            "To toggle between run and walk, Shift."
        );
        assert_eq!(fill_keys("Grab with &sUActnGrab;"), "Grab with [grab]");
        assert_eq!(
            fill_keys("To enter VATS, &sUActnVATS;."),
            "To enter VATS, V."
        );
        assert_eq!(fill_keys("Rock & roll; no key"), "Rock & roll; no key");
    }
}
