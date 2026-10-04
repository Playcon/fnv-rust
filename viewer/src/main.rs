//! nv-viewer: walk around a Fallout: New Vegas cell in real time.
//!
//! The game-specific work (reading the cell, placing models, textures and
//! materials) happens in the `cellview` crate; this file only hands the
//! result to Bevy and flies a camera around it. Written against Bevy 0.16.

mod actors;
mod ai;
mod args;
mod chatter;
mod combat;
mod daylight;
mod dialogue;
mod doors;
mod effects;
mod emittance;
mod exterior;
mod faces;
mod fighting;
mod game_menus;
mod grade;
mod grass;
mod hiteffects;
mod hud;
mod lighting;
mod lockpick;
mod lod;
mod lod_objects;
mod look;
mod map;
mod menus;
mod music;
mod particles;
mod pipboy;
mod player_idle;
mod report;
mod scripts;
mod sitting;
mod sounds;
mod terrain;
mod trees;
mod vats;
mod viewmodel;
mod walk;
mod water;
mod weather;

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::audio::AddAudioSource;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::system::SystemParam;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::render::camera::{CameraOutputMode, Exposure};
use bevy::render::mesh::PrimitiveTopology;
use bevy::render::render_resource::{
    Extent3d, Face, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, TextureViewDimension, WgpuFeatures,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::window::WindowResolution;
use cellview::{space, Blend, Game, GpuFormat, TextureData, ViewerScene};
use exterior::{ExteriorStart, PendingExterior};
use grade::{GradePlugin, ImageSpaceGrade};
use lighting::{
    DrawKey, GameLight, GameLighting, GameLightingPlugin, GameLit, GameLitMaterial, MAX_LIGHTS,
};
use lod::{LodLandMaterial, LodLandParams, LodPlugin};
use terrain::{Terrain, TerrainMaterial, TerrainPlugin};

/// Starting exposure (EV100); lower is brighter. At this exposure lit
/// surfaces show exactly the brightness the game's lighting gives them.
const START_EV100: f32 = 5.5;

/// The luminance (cd/m²) that shows at full brightness at the starting
/// exposure (Bevy scales what the camera sees by 1 / (1.2 × 2^EV100)). The
/// lighting shader scales its result by it, so the exposure keys still
/// work.
fn full_brightness_nits() -> f32 {
    1.2 * 2f32.powf(START_EV100)
}
const LOOK_SPEED: f32 = 0.003;

/// The camera's near plane, in meters.
const CAMERA_NEAR_METERS: f32 = 0.05;

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match args::parse(&raw) {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{}", args::USAGE);
            return;
        }
        Err(message) => {
            eprintln!("error: {message}\n\n{}", args::USAGE);
            std::process::exit(2);
        }
    };
    let data = match cellview::find_data_folder(&args.data) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    println!("Loading {} from {} ...", args.cell, data.display());
    let started = std::time::Instant::now();
    let game = match Game::open(&data, &args.options) {
        Ok(game) => Arc::new(game),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let (scene, outdoors) = match open_place(&game, &args.cell, args.at) {
        Ok(place) => place,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let title = match (&scene, &outdoors) {
        (Some(scene), _) => {
            for note in &scene.notes {
                println!("  {note}");
            }
            println!(
                "Loaded {} in {:.1} s; starting at {}.",
                scene.cell,
                started.elapsed().as_secs_f32(),
                scene.start.via
            );
            format!("nv-rs viewer - {}", scene.cell)
        }
        (None, Some(start)) => format!("nv-rs viewer - {}", start.grid.world.label()),
        (None, None) => unreachable!("open_place gives one or the other"),
    };
    // The game's console gives pitch positive looking down; the camera's
    // is positive looking up.
    let pitch = args.at.map_or(0.0, |at| -at.pitch.to_radians());

    let grading = Grading {
        grade: ImageSpaceGrade::NEUTRAL,
        on: true,
    };
    let mut window = Window { title, ..default() };
    if args.screenshot.is_some() {
        // The game's own resolution here, so pictures line up pixel for pixel.
        window.resolution = WindowResolution::new(1920.0, 1080.0).with_scale_factor_override(1.0);
    }
    // Walking, except for screenshots, which keep the exact eye given.
    let player = walk::Player::new(args.screenshot.is_none() || args.walk);
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        // Lit surfaces do their own lighting (see `lighting`); nothing else
        // should add Bevy's ambient light.
        .insert_resource(AmbientLight::NONE)
        .insert_resource(Settings {
            brightness: args.brightness,
            cloud_time: args.cloud_time,
            anisotropy: anisotropy_setting(&game.settings),
        })
        .insert_resource(StartPitch(pitch))
        .insert_resource(FrameCounter {
            on: args.fps,
            ..default()
        })
        .insert_resource(dialogue::DialogueState(world::dialogue::GameState::new(
            &game.order,
        )))
        .insert_resource(sitting::Seats::new(&game.order))
        .insert_resource(faces::Faces::new(&game))
        .insert_resource(actors::AnimSettings::read(&game.settings))
        .insert_resource(look::LookSettings::read(&game.settings))
        .init_resource::<look::PlayerAnchor>()
        .init_resource::<look::HeadAnchors>()
        .insert_resource(ai::Moves::new(&game))
        .insert_resource(ai::CellBuffer::new(&game))
        .insert_resource(GameFiles(game))
        .init_resource::<scripts::Scripts>()
        .init_resource::<player_idle::PlayerIdle>()
        .init_resource::<scripts::Here>()
        .init_resource::<scripts::ScriptedTalk>()
        .init_resource::<scripts::Notices>()
        .insert_resource(scripts::StartStage(args.stage.clone()))
        .insert_resource(scripts::StartCommands(args.run.clone()))
        .insert_resource(viewmodel::StartWeapon(args.weapon.clone()))
        .insert_resource(lockpick::StartLock(args.lockpick.clone()))
        .insert_resource(emittance::StartRegion(args.weather_region.clone()))
        .insert_resource(viewmodel::ShowInPictures(args.weapon.is_some()))
        .init_resource::<scripts::CellScripts>()
        .init_resource::<ai::CellNav>()
        .init_resource::<ai::CombatSettings>()
        .init_resource::<ai::Moved>()
        .init_resource::<ai::Chats>()
        .init_resource::<ai::Starts>()
        .init_resource::<chatter::Lines>()
        .init_resource::<menus::Menus>()
        .init_resource::<map::MapMarkers>()
        .init_resource::<daylight::Daylight>()
        .init_resource::<weather::Weathers>()
        .init_resource::<emittance::Glows>()
        .init_resource::<lod_objects::DistantObjects>()
        .init_resource::<effects::Effects>()
        .init_resource::<PlayingGroups>()
        .init_resource::<doors::SwingDoors>()
        .init_resource::<doors::DoorPoses>()
        .init_resource::<combat::PlayerAttack>()
        .insert_resource(vats::Vats::new(args.vats))
        .init_resource::<viewmodel::PlaceLighting>()
        .init_resource::<viewmodel::ViewModel>()
        .init_resource::<sounds::SoundRequests>()
        .init_resource::<sounds::Ambient>()
        .init_resource::<scripts::Activatable>()
        .init_resource::<scripts::ActivateRequest>()
        .init_resource::<dialogue::Talkers>()
        .init_resource::<dialogue::TalkTarget>()
        .init_resource::<dialogue::Conversation>()
        .insert_resource(dialogue::AutoTalk(args.talk))
        .insert_resource(player)
        .insert_resource(walk::CellCollision(physics::Collider::new()))
        .insert_resource(walk::Doors(Vec::new()))
        .insert_resource(PendingScene(scene))
        .insert_resource(PendingExterior(outdoors))
        .insert_resource(grading)
        .insert_resource(ScreenshotRequest {
            path: args.screenshot,
            frames: 0,
            taken: false,
            wait: args.wait,
            waited: 0.0,
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }))
        .insert_resource(hud::ShowHud(args.hud))
        .insert_resource(ai::FrozenAi(args.freeze_ai))
        .insert_resource(game_menus::StartMenu(args.open_menu.clone()))
        .insert_resource(game_menus::FixedPointer(args.menu_pointer))
        .add_plugins((GradePlugin, GameLightingPlugin, TerrainPlugin, LodPlugin))
        // After the default plugins: they load shaders.
        .add_plugins((hud::HudPlugin, pipboy::PipboyPlugin))
        .add_plugins(game_menus::GameMenusPlugin)
        .insert_resource(pipboy::StartPipboy(args.pipboy.clone()))
        .add_plugins(grass::GrassPlugin)
        .add_plugins(trees::TreePlugin)
        .add_plugins(water::WaterPlugin)
        .add_plugins(particles::ParticlesPlugin)
        .add_plugins(music::MusicPlugin)
        .add_plugins(hiteffects::HitEffectsPlugin)
        .add_plugins(lockpick::LockpickPlugin)
        .add_audio_source::<sounds::PcmSound>()
        // The first-person camera runs the image space passes with the
        // main camera's grade, once everything has set it.
        .add_systems(PostUpdate, viewmodel::copy_grade.after(grade::adapt_eyes))
        .add_systems(
            Startup,
            (
                setup,
                dialogue::setup_dialogue_text,
                scripts::setup_notices,
                menus::setup_menu_text,
                combat::setup_hud,
            ),
        )
        .add_systems(
            Update,
            (
                spawn_scene,
                exterior::enter_exterior,
                exterior::stream_squares,
                (
                    exterior::stream_distant_land,
                    lod_objects::stream_distant_objects,
                )
                    .chain(),
                // Menus take the keyboard before anything else sees it.
                // V.A.T.S. takes them next while it's on, and the
                // lockpicking menu (keyboard and mouse) while it's open.
                (
                    menus::run_menus,
                    vats::run_vats,
                    vats::scale_target_time,
                    lockpick::pick_locks,
                    look_around,
                )
                    .chain(),
                fly_camera,
                (
                    viewmodel::give_start_weapon,
                    walk::walk,
                    // Apply queued camera tracks before aiming/interactions.
                    player_idle::animate,
                    combat::player_attack,
                    combat::show_dropped_weapons,
                    viewmodel::update_view_model,
                    // A V.A.T.S. camera shot takes the view last.
                    vats::apply_shot_camera,
                )
                    .chain(),
                (dialogue::talk, chatter::say_lines).chain(),
                // The scripts, then E on doors, then the doors' swings.
                (scripts::run_scripts, walk::doors, doors::update_doors).chain(),
                walk::toggle_walking,
                adjust_exposure,
                // The cell's grade, then the screen effects scripts applied.
                (toggle_grade, effects::play_effects).chain(),
                // After the menus, which take Escape while they're open.
                quit_on_escape.after(menus::run_menus),
                take_screenshot,
                update_help,
                (
                    weather::run_weather,
                    follow_sky,
                    daylight::follow_the_clock,
                    emittance::arrive_indoors,
                    emittance::follow_emittance,
                )
                    .chain(),
                (
                    map::find_markers,
                    ai::move_offstage,
                    bring_in_people,
                    bring_in_made,
                    ai::move_actors,
                    scripts::save_and_load,
                    report::report_key,
                    sounds::play_sounds,
                ),
                (
                    look::attach_head_tracking,
                    look::follow_player,
                    actors::script_idles,
                    actors::animate_actors,
                )
                    .chain(),
                report_fps,
            )
                .chain(),
        )
        // Faces: lines' lip sync and blinking, once lines have started and
        // the bones have moved.
        .add_systems(
            Update,
            (
                faces::start_lines,
                faces::release_voices,
                faces::animate_faces,
            )
                .chain()
                .after(actors::animate_actors),
        )
        // Models' own animations (a ceiling fan's blades), and billboards
        // turned to the camera once it has moved.
        .add_systems(
            Update,
            (
                move_pieces.after(spawn_scene),
                face_camera.after(fly_camera).after(walk::walk),
            ),
        )
        .run();
}

/// A piece its model's own animation moves or changes
/// (`cellview::MeshData::motion`): its draw's placement (Bevy's space),
/// the point it's built around (`sort_center`; the piece's own space, game
/// units), and, when the animation changes its material, its own material
/// and the values last put in it.
#[derive(Component)]
struct Moving {
    motion: Arc<cellview::PieceMotion>,
    base: Mat4,
    center: Vec3,
    material: Option<Handle<GameLitMaterial>>,
    shown: Option<(Option<f32>, Option<[f32; 3]>)>,
    /// A piece of a door that swings where it stands: its `Open` / `Close`
    /// sequence follows the door's state (`doors::DoorPoses`).
    door: Option<u32>,
}

/// Sequences scripts started on placed objects (`PlayGroup`): by placed
/// reference, the sequence's name and when it started (the viewer's
/// clock).
#[derive(Resource, Default)]
pub struct PlayingGroups(pub std::collections::HashMap<u32, (String, f32)>);

/// Puts moving pieces where their animation has them now, and their
/// material as it has it: the sequences their objects show from the start
/// (started when the viewer did; the game starts them when the object
/// appears), or the one a script started.
fn move_pieces(
    time: Res<Time>,
    groups: Res<PlayingGroups>,
    door_poses: Res<doors::DoorPoses>,
    mut pieces: Query<(&mut Moving, &scripts::PlacedRef, &mut Transform)>,
    mut materials: ResMut<Assets<GameLitMaterial>>,
) {
    let seconds = time.elapsed_secs();
    for (mut piece, placed, mut transform) in &mut pieces {
        let group = groups
            .0
            .get(&placed.0)
            .map(|(name, at)| (name.as_str(), seconds - at));
        // A door's piece shows its state's sequence at the door's moment
        // (`doors::update_doors`); a script's group still takes over.
        let door_pose = piece
            .door
            .filter(|_| group.is_none())
            .and_then(|d| door_poses.0.get(&d).copied());
        let now = match door_pose {
            Some((opening, at)) => cellview::piece_in_sequence(
                &piece.motion,
                if opening { "Open" } else { "Close" },
                at,
            )
            .unwrap_or_else(|| cellview::piece_now(&piece.motion, seconds, group)),
            None => cellview::piece_now(&piece.motion, seconds, group),
        };
        let moved = Mat4::from_cols_array(&now.matrix);
        let wanted =
            Transform::from_matrix(piece.base * moved * Mat4::from_translation(piece.center));
        if *transform != wanted {
            *transform = wanted;
        }
        let shown = Some((now.opacity, now.emissive));
        if piece.shown == shown {
            continue;
        }
        let Some(material) = piece.material.as_ref().and_then(|h| materials.get_mut(h)) else {
            continue;
        };
        let opacity = now.opacity.unwrap_or(piece.motion.opacity);
        material.base.base_color.set_alpha(opacity);
        material.extension.lighting.environment.w = opacity;
        if let Some([r, g, b]) = now.emissive {
            let w = material.extension.lighting.emissive.w;
            material.extension.lighting.emissive = Vec4::new(r, g, b, w);
        }
        piece.shown = shown;
    }
}

/// A piece that turns to face the camera (`cellview::MeshData::billboard`):
/// its draw's placement (game space, and Bevy's) and the point it's built
/// around.
#[derive(Component)]
struct Facing {
    billboard: cellview::Billboard,
    draw: [f32; 16],
    base: Mat4,
    center: Vec3,
}

/// Turns billboards toward the camera, after it has moved this frame.
fn face_camera(
    cameras: Query<&Transform, (With<FlyCamera>, Without<Facing>)>,
    mut pieces: Query<(&Facing, &mut Transform)>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    // Bevy's axes (y up, −z north, meters) back to the game's.
    let game = |v: Vec3| [v.x, -v.z, v.y];
    let eye = game(camera.translation / space::METERS_PER_UNIT);
    let axes = [
        game(camera.rotation * Vec3::X),
        game(camera.rotation * Vec3::Y),
        game(camera.rotation * Vec3::Z),
    ];
    for (piece, mut transform) in &mut pieces {
        let turned = Mat4::from_cols_array(&cellview::billboard_matrix(
            &piece.billboard,
            &piece.draw,
            eye,
            axes,
        ));
        *transform =
            Transform::from_matrix(piece.base * turned * Mat4::from_translation(piece.center));
    }
}

#[derive(Resource)]
struct Settings {
    brightness: f32,
    /// `--cloud-time`: the clouds held at this many seconds of drift.
    cloud_time: Option<f32>,
    /// The textures' anisotropic filtering, from the INI
    /// ([`anisotropy_setting`]).
    anisotropy: u16,
}

/// The game's texture filtering, from its INI: `[Display] iMaxAnisotropy`
/// (15 in this install's `FalloutPrefs.ini`, 8 in `Fallout_default.ini`).
/// The Goodsprings recording sets `D3DSAMP_MAXANISOTROPY` to it on every
/// scene texture stage (1,239 of the frame's 2,033 sampler-state calls),
/// `MINFILTER` anisotropic, `MAGFILTER` linear, `MIPFILTER` linear (the
/// HUD's none), `MIPMAPLODBIAS` 0: anisotropic trilinear, no bias, as the
/// textures here are sampled. Below 1 the card refuses it; 1 is plain
/// trilinear.
fn anisotropy_setting(ini: &assets::IniSettings) -> u16 {
    ini.get("Display", "iMaxAnisotropy")
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(8)
        .max(1)
}

/// Starting pitch, radians, positive looking up; used once.
#[derive(Resource)]
pub(crate) struct StartPitch(pub(crate) f32);

/// The game's files, open for the whole run (and shared with the threads
/// loading outdoor squares).
#[derive(Resource)]
pub struct GameFiles(pub Arc<Game>);

/// Where to start: an interior (or one outdoor cell's objects) loaded
/// now, or an outdoor place whose squares load once the window is open.
fn open_place(
    game: &Game,
    name: &str,
    at: Option<args::Stance>,
) -> Result<(Option<ViewerScene>, Option<ExteriorStart>), String> {
    let heading = at.map_or(0.0, |a| a.heading.to_radians());
    // A worldspace by name: start where --at says, else in the middle of
    // square 0,0.
    if let Ok(grid) = game.world(name) {
        let (feet, height) = match at {
            Some(a) => ([a.feet[0], a.feet[1]], Some(a.feet[2])),
            None => ([2048.0, 2048.0], None),
        };
        return Ok((
            None,
            Some(ExteriorStart {
                grid,
                feet,
                height,
                heading,
            }),
        ));
    }
    let cell = game.find_cell(name).map_err(|e| e.0)?;
    let info = world::cell_info(&game.order, cell).map_err(|e| e.to_string())?;
    if let (false, Some(world), Some((x, y))) = (info.interior, info.world, info.grid) {
        // An outdoor cell: its worldspace, starting in the middle of it.
        let grid = world::WorldGrid::load(&game.order, world).map_err(|e| e.to_string())?;
        let size = world::land::CELL_SIZE;
        let (feet, height) = match at {
            Some(a) => ([a.feet[0], a.feet[1]], Some(a.feet[2])),
            None => ([(x as f32 + 0.5) * size, (y as f32 + 0.5) * size], None),
        };
        return Ok((
            None,
            Some(ExteriorStart {
                grid,
                feet,
                height,
                heading,
            }),
        ));
    }
    let mut scene = game.load_cell(cell).map_err(|e| e.0)?;
    if let Some(at) = at {
        let [x, y, z] = at.feet;
        scene.start = cellview::Start {
            eye: [x, y, z + cellview::EYE_HEIGHT],
            heading,
            via: "the position given with --at",
        };
    }
    Ok((Some(scene), None))
}

/// How far outdoors people coming into the worldspace are drawn: the
/// loaded squares (two and a half squares each way).
const BRING_IN_REACH: f32 = 2.5 * world::land::CELL_SIZE;

/// People the game has taken into this place after it loaded (through a
/// door, or by a script's `MoveTo`) come on screen: once a second, anyone
/// the state has here (`world::ai::moved_into`) who isn't drawn yet is
/// spawned, lit as the place is; outdoors, those within the loaded
/// squares.
fn bring_in_people(
    time: Res<Time>,
    game: Res<GameFiles>,
    state: Res<dialogue::DialogueState>,
    player: Res<walk::Player>,
    walkers: Query<&ai::Walker>,
    mut spawner: Spawner,
    mut last: Local<f32>,
) {
    let now = time.elapsed_secs();
    if !player.ready || now - *last < 1.0 {
        return;
    }
    *last = now;
    let state = &state.0;
    let order = &game.0.order;
    let Some(space) = state.player_world.or(state.player_cell) else {
        return;
    };
    let shown: std::collections::HashSet<esm::FormId> =
        walkers.iter().map(|w| w.reference).collect();
    let near = |r: esm::FormId| {
        state.player_world.is_none()
            || state
                .place(order, r)
                .zip(state.player_position)
                .is_some_and(|((_, _, p, _), me)| {
                    (p[0] - me[0]).hypot(p[1] - me[1]) <= BRING_IN_REACH
                })
    };
    let new: Vec<esm::FormId> = world::ai::moved_into(order, state, space)
        .into_iter()
        .filter(|r| !shown.contains(r) && near(*r))
        .collect();
    if new.is_empty() {
        return;
    }
    let scene = game.0.actors_scene(&new);
    let lighting = spawner.place_lighting.get();
    for r in &new {
        println!("{r} comes into view");
    }
    spawner.spawn_with(&scene, lighting);
}

/// References scripts made (`PlaceAtMe`, `world::more_functions::placed`)
/// come on screen: once a second, those here not drawn yet and enabled,
/// people and creatures and anything with a model, lit as the place is;
/// outdoors, those within the loaded squares. A place loaded afresh draws
/// them again.
fn bring_in_made(
    time: Res<Time>,
    game: Res<GameFiles>,
    state: Res<dialogue::DialogueState>,
    player: Res<walk::Player>,
    mut spawner: Spawner,
    mut shown: Local<(Option<esm::FormId>, std::collections::HashSet<esm::FormId>)>,
    mut last: Local<f32>,
) {
    let now = time.elapsed_secs();
    if !player.ready || now - *last < 1.0 {
        return;
    }
    *last = now;
    let state = &state.0;
    let order = &game.0.order;
    let Some(space) = state.player_world.or(state.player_cell) else {
        return;
    };
    if shown.0 != Some(space) {
        *shown = (Some(space), Default::default());
    }
    let mut new = Vec::new();
    for (&r, made) in &state.more.placed.refs {
        if shown.1.contains(&r) || !world::enabled_now(order, r, &state.disabled) {
            continue;
        }
        let Some((s, _, p, heading)) = state.place(order, r) else {
            continue;
        };
        let far = state.player_world.is_some()
            && state
                .player_position
                .is_some_and(|me| (p[0] - me[0]).hypot(p[1] - me[1]) > BRING_IN_REACH);
        if s != space || far {
            continue;
        }
        shown.1.insert(r);
        let rotation = [made.rotation[0], made.rotation[1], heading];
        if let Some(placement) = world::made_placement(order, r, made.base, p, rotation) {
            new.push(placement);
        }
    }
    if new.is_empty() {
        return;
    }
    for p in &new {
        println!(
            "{} ({}) comes into view",
            p.form_id,
            p.base_editor_id.as_deref().unwrap_or("?")
        );
    }
    let scene = game.0.made_scene(new);
    let lighting = spawner.place_lighting.get();
    spawner.spawn_with(&scene, lighting);
}

/// `--screenshot`: where to save the picture, and how long it's waited.
#[derive(Resource)]
struct ScreenshotRequest {
    path: Option<std::path::PathBuf>,
    frames: u32,
    taken: bool,
    /// `--wait`: seconds to let the game run first, and how long it has.
    wait: f32,
    waited: f32,
}

/// Frames to wait before the screenshot, so every shader has compiled and
/// every texture has reached the graphics card.
const SCREENSHOT_AFTER_FRAMES: u32 = 300;

/// What screenshots hide: the help and the health line.
type KeptOutOfPictures = Or<(With<HelpText>, With<combat::HudText>)>;

fn take_screenshot(
    // Real time: V.A.T.S. stops the game's clock.
    time: Res<Time<bevy::time::Real>>,
    mut commands: Commands,
    mut request: ResMut<ScreenshotRequest>,
    exterior: Option<Res<exterior::Exterior>>,
    // The help and the health line stay out of pictures compared with
    // the game's.
    mut help: Query<&mut Visibility, KeptOutOfPictures>,
) {
    let Some(path) = request.path.clone() else {
        return;
    };
    for mut visibility in &mut help {
        *visibility = Visibility::Hidden;
    }
    // Outdoors, the count starts once the squares around have loaded.
    if exterior.is_some_and(|e| e.busy()) {
        request.frames = 0;
        return;
    }
    request.frames += 1;
    request.waited += time.delta_secs();
    if request.taken || request.frames < SCREENSHOT_AFTER_FRAMES || request.waited < request.wait {
        return;
    }
    request.taken = true;
    println!("Saving the view to {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        .observe(
            |_: Trigger<ScreenshotCaptured>, mut exit: EventWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}

/// The cell's image space values, and whether its color adjustment is
/// shown (G toggles; bloom stays on).
#[derive(Resource)]
struct Grading {
    grade: ImageSpaceGrade,
    on: bool,
}

/// The loaded cell, until `setup` hands it to Bevy.
#[derive(Resource)]
struct PendingScene(Option<ViewerScene>);

#[derive(Component)]
struct FlyCamera {
    yaw: f32,
    pitch: f32,
    /// Meters per second.
    speed: f32,
    start: (Vec3, f32),
}

#[derive(Component)]
struct HelpText;

/// Every texture is sampled as stored, colors too: the game's samplers
/// never decode sRGB (no `D3DSAMP_SRGBTEXTURE` anywhere in the Goodsprings
/// recording), so filtering averages the stored values and its shaders
/// work on them; the viewer's shaders do the same.
fn gpu_format(format: GpuFormat) -> TextureFormat {
    match format {
        GpuFormat::Bc1 => TextureFormat::Bc1RgbaUnorm,
        GpuFormat::Bc2 => TextureFormat::Bc2RgbaUnorm,
        GpuFormat::Bc3 => TextureFormat::Bc3RgbaUnorm,
        GpuFormat::Bc4 => TextureFormat::Bc4RUnorm,
        GpuFormat::Bc5 => TextureFormat::Bc5RgUnorm,
        GpuFormat::Rgba8 => TextureFormat::Rgba8Unorm,
    }
}

/// A place's ambient light, directional light and fog as the lighting
/// shader takes them (Bevy's space, `--brightness` applied).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LightFields {
    pub ambient: Vec4,
    pub directional_color: Vec4,
    pub directional_direction: Vec4,
    pub fog_color: Vec4,
    pub fog_range: Vec4,
}

pub(crate) fn light_fields(
    ambient: [f32; 3],
    directional: Option<&cellview::Directional>,
    fog: Option<&cellview::Fog>,
    brightness: f32,
) -> LightFields {
    let rgb = |c: [f32; 3], w: f32| Vec4::new(c[0], c[1], c[2], w);
    let (directional_color, directional_direction) = match directional {
        Some(d) => (
            rgb(d.color.map(|c| c * brightness), 0.0),
            rgb(space::direction(d.direction), 0.0),
        ),
        None => (Vec4::ZERO, Vec4::Y),
    };
    let (fog_color, fog_range) = match fog {
        Some(f) => (
            rgb(f.color, f.power),
            Vec4::new(
                f.near * space::METERS_PER_UNIT,
                f.far * space::METERS_PER_UNIT,
                cellview::GAME_NEAR_CLIP * space::METERS_PER_UNIT,
                1.0,
            ),
        ),
        None => (Vec4::ZERO, Vec4::ZERO),
    };
    LightFields {
        ambient: rgb(ambient.map(|c| c * brightness), 0.0),
        directional_color,
        directional_direction,
        fog_color,
        fog_range,
    }
}

impl LightFields {
    /// Puts these into a material's lighting, keeping its own lights.
    pub fn apply(&self, l: &mut GameLighting) {
        l.ambient = self.ambient;
        l.directional_color = self.directional_color;
        l.directional_direction = self.directional_direction;
        l.fog_color = self.fog_color;
        l.fog_range = self.fog_range;
    }
}

/// The cell's lights as the lighting shader takes them, in Bevy's space,
/// scaled by `--brightness`. The first `MAX_LIGHTS` are used.
fn game_lighting(scene: &ViewerScene, brightness: f32) -> GameLighting {
    let lights = game_lights(&scene.lights, brightness);
    let f = light_fields(
        scene.ambient,
        scene.directional.as_ref(),
        scene.fog.as_ref(),
        brightness,
    );
    GameLighting {
        fog_color: f.fog_color,
        fog_range: f.fog_range,
        specular: Vec4::ZERO,
        surface: Vec4::ZERO,
        falloff: NO_FALLOFF,
        environment: Vec4::ZERO,
        // The pull decals get here (kept only on decals' materials): the
        // game's projection differs indoors and out.
        draw: Vec4::new(
            lighting::decal_depth_offset(
                CAMERA_NEAR_METERS,
                if scene.sky.is_none() {
                    cellview::INTERIOR_DEPTH_SCALE
                } else {
                    cellview::EXTERIOR_DEPTH_SCALE
                },
            ),
            0.0,
            0.0,
            0.0,
        ),
        ambient: f.ambient,
        directional_color: f.directional_color,
        directional_direction: f.directional_direction,
        emissive: Vec4::ZERO,
        scale: Vec4::new(
            full_brightness_nits(),
            scene.lights.len().min(MAX_LIGHTS) as f32,
            0.0,
            0.0,
        ),
        lights,
        // Skin's directional light multiplier, from the image space.
        actor: Vec4::new(
            0.0,
            scene.hdr.as_ref().map_or(1.0, |h| h.skin_directional),
            0.0,
            0.0,
        ),
        hair_tint: Vec4::ZERO,
    }
}

/// Placed lights as the lighting shader takes them (Bevy's space,
/// `--brightness` applied); the first `MAX_LIGHTS`.
fn game_lights(list: &[cellview::LightData], brightness: f32) -> [GameLight; MAX_LIGHTS] {
    let mut lights = [GameLight::default(); MAX_LIGHTS];
    for (slot, light) in lights.iter_mut().zip(list) {
        let [x, y, z] = space::point(light.position);
        let [r, g, b] = light
            .color
            .map(|c| c * light.fade.clamp(0.0, 4.0) * brightness);
        *slot = GameLight {
            position_radius: Vec4::new(x, y, z, light.radius * space::METERS_PER_UNIT),
            color: Vec4::new(r, g, b, 0.0),
        };
    }
    lights
}

/// A fade by viewing angle that never fades: opacity 1 at every angle.
const NO_FALLOFF: Vec4 = Vec4::new(0.0, 0.0, 1.0, 1.0);

/// The point a blended piece is sorted by ([`cellview::MaterialData::
/// sort_center`]), or `None` for pieces that aren't sorted (opaque ones,
/// and actors' pieces, which are skinned). Bevy sorts blended entities by
/// their origin, so such a piece is built around this point
/// ([`game_mesh_around`]) and placed with it added back.
fn sort_center(data: &cellview::MeshData) -> Option<[f32; 3]> {
    let sorted = matches!(
        alpha_mode(data.material.blend),
        AlphaMode::Blend | AlphaMode::Add | AlphaMode::Multiply | AlphaMode::Premultiplied
    );
    (sorted && data.rig.is_none()).then_some(data.material.sort_center)
}

/// A mesh for Bevy, drawn unindexed: each triangle gets its own three
/// vertices, and every vertex carries its triangle's three corners, so the
/// shader can work things out per vertex and blend them across the
/// triangle as the game does (see `lighting::ATTRIBUTE_CORNER_A`). The
/// mesh's origin is `origin` (in the piece's own space).
fn game_mesh_around(data: &cellview::MeshData, origin: [f32; 3]) -> Mesh {
    let moved: Vec<[f32; 3]>;
    let positions = if origin == [0.0; 3] {
        &data.positions
    } else {
        moved = data
            .positions
            .iter()
            .map(|p| [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]])
            .collect();
        &moved
    };
    let triangles: Vec<[usize; 3]> = data
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(usize::from))
        .filter(|t| t.iter().all(|&i| i < data.positions.len()))
        .collect();
    fn spread<T: Copy>(values: &[T], triangles: &[[usize; 3]]) -> Vec<T> {
        triangles.iter().flatten().map(|&i| values[i]).collect()
    }
    let corner = |k: usize| -> Vec<[f32; 3]> {
        triangles
            .iter()
            .flat_map(|t| [positions[t[k]]; 3])
            .collect()
    };
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, spread(positions, &triangles));
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, spread(&data.normals, &triangles));
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, spread(&data.uvs, &triangles));
    if let Some(tangents) = &data.tangents {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, spread(tangents, &triangles));
    }
    if let Some(colors) = &data.colors {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, spread(colors, &triangles));
    }
    mesh.insert_attribute(lighting::ATTRIBUTE_CORNER_A, corner(0));
    mesh.insert_attribute(lighting::ATTRIBUTE_CORNER_B, corner(1));
    mesh.insert_attribute(lighting::ATTRIBUTE_CORNER_C, corner(2));
    mesh
}

/// Bevy's blending for a surface. The alpha test itself is the game's
/// exact comparison, done in `game_lit.wgsl` (`GameLighting::draw`), so
/// cut-outs are opaque here and surfaces that test and blend just blend.
fn alpha_mode(blend: Blend) -> AlphaMode {
    match blend {
        Blend::Opaque | Blend::Mask(_) => AlphaMode::Opaque,
        Blend::Blend | Blend::MaskedBlend(_) => AlphaMode::Blend,
        Blend::Add => AlphaMode::Add,
        Blend::Multiply => AlphaMode::Multiply,
        Blend::Premultiplied => AlphaMode::Premultiplied,
    }
}

/// Everything drawn for the current cell, removed when another loads.
#[derive(Component)]
struct SceneEntity;

/// The camera and the on-screen text, once.
fn setup(mut commands: Commands) {
    let camera = commands.spawn((
        Camera3d::default(),
        // HDR, so bright lamps can go past white into the game's bloom
        // (see `grade`). The picture is finished and written out by the
        // first-person camera, which draws over it (`viewmodel`).
        Camera {
            hdr: true,
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
        // The game clips what's too bright rather than rolling it off, and
        // the lighting shader already gives the game's brightness.
        Tonemapping::None,
        Projection::from(PerspectiveProjection {
            // The game's: its 75° setting is the width of a 4:3 picture,
            // and wider windows keep that height.
            fov: cellview::vertical_fov(cellview::GAME_FOV_DEGREES),
            near: CAMERA_NEAR_METERS,
            // Outdoors, distant land reaches the game's far clip plane
            // (about 5 km); the sky dome is beyond it. (Only for culling:
            // Bevy's projection has no far plane.)
            far: 6000.0,
            ..default()
        }),
        Exposure { ev100: START_EV100 },
        Transform::default(),
        FlyCamera {
            yaw: 0.0,
            pitch: 0.0,
            speed: 2.5,
            start: (Vec3::ZERO, 0.0),
        },
        // Always: the pass also does the bloom and turns the stored values
        // the lighting writes into linear light for the screen. Set here,
        // run on the first-person camera.
        ImageSpaceGrade::NEUTRAL,
        grade::GradeDeferred,
    ));
    let camera = camera.id();
    viewmodel::spawn_camera(&mut commands, camera, Exposure { ev100: START_EV100 });
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        HelpText,
    ));
}

/// What putting a loaded place on screen needs.
#[derive(SystemParam)]
pub struct Spawner<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    lit_materials: ResMut<'w, Assets<GameLitMaterial>>,
    terrain_materials: ResMut<'w, Assets<TerrainMaterial>>,
    lod_materials: ResMut<'w, Assets<LodLandMaterial>>,
    inverse_bindposes:
        ResMut<'w, Assets<bevy::render::mesh::skinning::SkinnedMeshInverseBindposes>>,
    images: ResMut<'w, Assets<Image>>,
    device: Option<Res<'w, RenderDevice>>,
    settings: Res<'w, Settings>,
    place_lighting: ResMut<'w, viewmodel::PlaceLighting>,
    water: water::WaterSpawn<'w>,
    particle_materials: ResMut<'w, Assets<particles::ParticleMaterial>>,
}

impl Spawner<'_, '_> {
    /// Puts a loaded place's textures, models and terrain on screen, and
    /// returns what it spawned (so an outdoor square can be dropped later).
    fn spawn(&mut self, scene: &ViewerScene) -> Vec<Entity> {
        self.spawn_parts(scene, None, None).entities
    }

    /// [`Self::spawn`], lit with `lighting` when given (people coming into
    /// a place take its light) instead of the scene's own.
    fn spawn_with(&mut self, scene: &ViewerScene, lighting: Option<GameLighting>) -> Vec<Entity> {
        self.spawn_parts(scene, lighting, None).entities
    }

    /// [`Self::spawn`], also giving the materials it made (an outdoor
    /// square's, to be lit again by its neighbours' lights); its terrain
    /// blends toward the distant land as seen from square `here`, with
    /// the distant land's `noise`.
    fn spawn_square(
        &mut self,
        scene: &ViewerScene,
        here: (i32, i32),
        noise: Option<Handle<Image>>,
    ) -> Spawned {
        self.spawn_parts(scene, None, Some((here, noise)))
    }

    /// Gives the materials of a spawned place these lights.
    fn relight(&mut self, spawned: &Spawned, lights: &[cellview::LightData]) {
        let list = game_lights(lights, self.settings.brightness);
        let count = lights.len().min(MAX_LIGHTS) as f32;
        for m in &spawned.lit {
            if let Some(m) = self.lit_materials.get_mut(m) {
                m.extension.lighting.lights = list;
                m.extension.lighting.scale.y = count;
            }
        }
        for m in &spawned.terrain {
            if let Some(m) = self.terrain_materials.get_mut(m) {
                m.extension.lighting.lights = list;
                m.extension.lighting.scale.y = count;
            }
        }
    }

    /// `land_blend`: outdoors, the player's square and the distant land's
    /// noise, for the terrain's blend toward the distant land.
    fn spawn_parts(
        &mut self,
        scene: &ViewerScene,
        lighting: Option<GameLighting>,
        land_blend: Option<LandBlendFrom>,
    ) -> Spawned {
        // Every desktop graphics card takes block-compressed textures;
        // check when the device is visible from here.
        let compressed = self
            .device
            .as_ref()
            .is_none_or(|d| d.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC));
        let anisotropy = self.settings.anisotropy;
        let textures: Vec<Option<Handle<Image>>> = scene
            .textures
            .iter()
            .map(|t| upload_texture(&mut self.images, t, compressed, anisotropy))
            .collect();
        if scene.lights.len() > MAX_LIGHTS {
            println!(
                "  {} has {} lights; only the first {MAX_LIGHTS} are used",
                scene.cell,
                scene.lights.len()
            );
        }
        let lighting = match lighting {
            Some(l) => l,
            None => {
                let lighting = game_lighting(scene, self.settings.brightness);
                // Kept for what's drawn apart from the place (the
                // first-person view, people coming in); outdoors, the last
                // square loaded.
                self.place_lighting.set(lighting);
                lighting
            }
        };
        let mut spawned = Vec::new();
        let mut terrain_materials = Vec::new();

        // One mesh and material per piece of each model, shared by its
        // placements.
        // Actors' pieces are skinned meshes in their bind pose, moved by
        // their bones (see `actors`).
        let mut pieces = Vec::with_capacity(scene.meshes.len());
        let mut binds = Vec::with_capacity(scene.meshes.len());
        // Blended pieces are built around the point the game sorts them by
        // (see `sort_center`).
        let centers: Vec<Option<[f32; 3]>> = scene.meshes.iter().map(sort_center).collect();
        for (data, center) in scene.meshes.iter().zip(&centers) {
            let (mesh, bind) = match actors::skinned_mesh(data) {
                Some((mesh, bind)) => (
                    self.meshes.add(mesh),
                    Some(self.inverse_bindposes.add(bind)),
                ),
                None => (
                    self.meshes
                        .add(game_mesh_around(data, center.unwrap_or([0.0; 3]))),
                    None,
                ),
            };
            let material = self
                .lit_materials
                .add(lit_material(data, &textures, lighting));
            pieces.push((mesh, material));
            binds.push(bind);
        }
        let actor_joints: Vec<(Entity, Vec<Entity>)> = scene
            .actors
            .iter()
            .enumerate()
            .map(|(i, actor)| {
                actors::spawn_actor(
                    &mut self.commands,
                    actor,
                    i,
                    (SceneEntity, scripts::PlacedRef(actor.reference)),
                )
            })
            .collect();
        spawned.extend(actor_joints.iter().map(|(root, _)| *root));
        let mut own_materials = Vec::new();
        // The doors that swing: their pieces follow the door's state.
        let swing_doors: std::collections::HashSet<u32> =
            scene.swing_doors.iter().map(|d| d.reference.0).collect();
        for draw in &scene.draws {
            let (mesh, material) = &pieces[draw.mesh];
            if let (Some(a), Some(bind)) = (draw.actor, &binds[draw.mesh]) {
                let (root, joints) = &actor_joints[a];
                let joints = actors::skin_joints(&scene.meshes[draw.mesh], joints);
                let mut piece = self.commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::IDENTITY,
                    actors::skinned(bind.clone(), joints),
                    // The skin moves it away from where its own box is.
                    bevy::render::view::NoFrustumCulling,
                    ChildOf(*root),
                ));
                // Head parts talk and blink (`faces`).
                if let Some(face) = faces::FacePiece::of(&scene.meshes[draw.mesh]) {
                    piece.insert(face);
                }
                if let Some(link) = scene.meshes[draw.mesh].material.emittance {
                    piece.insert(emittance::Glow(link));
                }
                continue;
            }
            let base = Mat4::from_cols_array(&space::matrix(&draw.transform));
            let center = Vec3::from(centers[draw.mesh].unwrap_or([0.0; 3]));
            let transform = Transform::from_matrix(base * Mat4::from_translation(center));
            // A piece whose material its animation changes gets one of its
            // own.
            let own_material = scene.meshes[draw.mesh]
                .motion
                .as_ref()
                .filter(|m| m.changes_material())
                .map(|_| {
                    let own = self.lit_materials.add(lit_material(
                        &scene.meshes[draw.mesh],
                        &textures,
                        lighting,
                    ));
                    own_materials.push(own.clone());
                    own
                });
            let mut piece = self.commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(own_material.clone().unwrap_or_else(|| material.clone())),
                transform,
                SceneEntity,
                scripts::PlacedRef(draw.reference),
            ));
            // A glow that follows a region's weather (`emittance`).
            if let Some(link) = scene.meshes[draw.mesh].material.emittance {
                piece.insert(emittance::Glow(link));
            }
            // A piece the model's own animation moves (`move_pieces`), or
            // that turns to face the camera (`face_camera`).
            if let Some(motion) = &scene.meshes[draw.mesh].motion {
                piece.insert(Moving {
                    motion: motion.clone(),
                    base,
                    center,
                    material: own_material,
                    shown: None,
                    door: swing_doors
                        .contains(&draw.reference)
                        .then_some(draw.reference),
                });
            }
            if let Some(billboard) = scene.meshes[draw.mesh].billboard {
                piece.insert(Facing {
                    billboard,
                    draw: draw.transform,
                    base,
                    center,
                });
            }
            spawned.push(piece.id());
        }
        for quarter in &scene.terrain {
            let layers: Vec<terrain::TexturePair> = quarter
                .textures
                .iter()
                .map(|&(diffuse, normal)| {
                    (
                        diffuse.and_then(|i| textures[i].clone()),
                        normal.and_then(|i| textures[i].clone()),
                    )
                })
                .collect();
            let lod = quarter.lod_blend.as_ref();
            let texture = |i: Option<usize>| i.and_then(|i| textures[i].clone());
            let (here, noise) = land_blend.clone().unwrap_or(((0, 0), None));
            let material = self.terrain_materials.add(TerrainMaterial {
                base: StandardMaterial::default(),
                extension: Terrain::new(
                    lighting,
                    &layers,
                    terrain::LandBlend::new(lod.map(|l| l.offset), here),
                    (
                        texture(lod.and_then(|l| l.diffuse)),
                        texture(lod.and_then(|l| l.normals)),
                    ),
                    noise,
                ),
            });
            terrain_materials.push(material.clone());
            let mesh = self.meshes.add(terrain::terrain_mesh(quarter));
            spawned.push(
                self.commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        SceneEntity,
                    ))
                    .id(),
            );
        }
        spawned.extend(water::spawn_water(
            &mut self.commands,
            &mut self.meshes,
            &mut self.water,
            scene,
            &textures,
            full_brightness_nits(),
        ));
        spawned.extend(particles::spawn_particles(
            &mut self.commands,
            &mut self.meshes,
            &mut self.particle_materials,
            scene,
            &textures,
        ));
        Spawned {
            entities: spawned,
            lit: pieces
                .into_iter()
                .map(|(_, m)| m)
                .chain(own_materials)
                .collect(),
            terrain: terrain_materials,
        }
    }
}

/// Where an outdoor square's terrain blends toward the distant land from:
/// the player's square, and the distant land's noise texture.
type LandBlendFrom = ((i32, i32), Option<Handle<Image>>);

/// What spawning a place made: its entities and its materials.
pub struct Spawned {
    pub entities: Vec<Entity>,
    pub lit: Vec<Handle<GameLitMaterial>>,
    pub terrain: Vec<Handle<TerrainMaterial>>,
}

impl Spawner<'_, '_> {
    /// One actor on its own (the first-person view) under `parent`, lit by
    /// `lighting`: its root, its joints and its pieces' entities.
    fn spawn_lone_actor(
        &mut self,
        scene: &ViewerScene,
        lighting: GameLighting,
        parent: Entity,
    ) -> Option<(Entity, Vec<Entity>, Vec<Entity>)> {
        let actor = scene.actors.first()?;
        let compressed = self
            .device
            .as_ref()
            .is_none_or(|d| d.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC));
        let anisotropy = self.settings.anisotropy;
        let textures: Vec<Option<Handle<Image>>> = scene
            .textures
            .iter()
            .map(|t| upload_texture(&mut self.images, t, compressed, anisotropy))
            .collect();
        let root = self
            .commands
            .spawn((Transform::IDENTITY, Visibility::default(), ChildOf(parent)))
            .id();
        let joints: Vec<Entity> = actor
            .skeleton
            .bones
            .iter()
            .map(|_| {
                self.commands
                    .spawn((Transform::IDENTITY, ChildOf(root)))
                    .id()
            })
            .collect();
        let mut pieces = Vec::new();
        for draw in &scene.draws {
            let data = &scene.meshes[draw.mesh];
            let Some((mesh, bind)) = actors::skinned_mesh(data) else {
                continue;
            };
            let mesh = self.meshes.add(mesh);
            let bind = self.inverse_bindposes.add(bind);
            let material = self
                .lit_materials
                .add(lit_material(data, &textures, lighting));
            let skin_joints = actors::skin_joints(data, &joints);
            pieces.push(
                self.commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        actors::skinned(bind, skin_joints),
                        bevy::render::view::NoFrustumCulling,
                        // Drawn by the first-person camera alone.
                        bevy::render::view::RenderLayers::layer(viewmodel::FIRST_PERSON_LAYER),
                        ChildOf(root),
                    ))
                    .id(),
            );
        }
        Some((root, joints, pieces))
    }
}

/// How far the sky dome is drawn, in meters: just past the game's far clip
/// plane (`lod::GAME_FAR_CLIP`, about 5,030 m), so all the distant land the
/// game draws stands in front of it, inside the camera's far plane. The
/// game draws it at the far plane itself (`SKY.vso`).
const SKY_RADIUS_METERS: f32 = 5100.0;

/// The sky dome, kept centred on the camera.
#[derive(Component)]
pub struct SkyEntity;

impl Spawner<'_, '_> {
    /// The sky dome: its stored colours drawn as they are, with no light
    /// and no fog, behind everything.
    fn spawn_sky(&mut self, dome: &cellview::SkyDome) -> Entity {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        let positions: Vec<[f32; 3]> = dome.positions.iter().map(|&p| space::point(p)).collect();
        // The lighting shader takes vertex colors as linear light and
        // encodes them back; the dome's are stored values, so decode them
        // (past 1 too, where its weights add up past 1).
        let colors: Vec<[f32; 4]> = dome
            .colors
            .iter()
            .map(|c| {
                [
                    srgb_decode(c[0]),
                    srgb_decode(c[1]),
                    srgb_decode(c[2]),
                    c[3],
                ]
            })
            .collect();
        let count = positions.len();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(bevy::render::mesh::Indices::U16(dome.indices.clone()));
        let base = StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            ..default()
        };
        let extension = GameLit {
            lighting: GameLighting {
                ambient: Vec4::ZERO,
                directional_color: Vec4::ZERO,
                directional_direction: Vec4::Y,
                emissive: Vec4::new(1.0, 1.0, 1.0, 0.0),
                scale: Vec4::new(full_brightness_nits(), 0.0, 0.0, 0.0),
                fog_color: Vec4::ZERO,
                // No fog on the sky.
                fog_range: Vec4::ZERO,
                specular: Vec4::ZERO,
                surface: Vec4::new(0.0, 0.0, 1.0, 0.0),
                falloff: NO_FALLOFF,
                environment: Vec4::ZERO,
                draw: Vec4::ZERO,
                actor: Vec4::ZERO,
                hair_tint: Vec4::ZERO,
                lights: [GameLight::default(); MAX_LIGHTS],
            },
            glow: None,
            normal_map: None,
            environment: None,
            environment_mask: None,
            key: DrawKey::default(),
        };
        let scale = SKY_RADIUS_METERS / (500.0 * space::METERS_PER_UNIT);
        self.commands
            .spawn((
                Mesh3d(self.meshes.add(mesh)),
                MeshMaterial3d(self.lit_materials.add(GameLitMaterial { base, extension })),
                Transform::from_scale(Vec3::splat(scale)),
                bevy::render::view::NoFrustumCulling,
                daylight::SkyWeights(dome.weights.clone()),
                SkyEntity,
                SceneEntity,
            ))
            .id()
    }
}

/// The sun's square (Bevy's space, game units around the eye): `size`
/// across, facing the eye, toward the sun (`toward`, game space).
pub(crate) fn sun_square(toward: [f32; 3], size: f32) -> Vec<[f32; 3]> {
    let d = Vec3::from(toward).normalize_or(Vec3::Z);
    let side = d.cross(Vec3::Z).try_normalize().unwrap_or(Vec3::X);
    let up = side.cross(d);
    let centre = d * cellview::game::SUN_DISTANCE;
    let h = size * 0.5;
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    corners
        .iter()
        .map(|&(x, y)| space::point((centre + side * (x * h) + up * (y * h)).to_array()))
        .collect()
}

/// A cloud layer's texture scrolling (`cellview::game::CloudLayer`): its
/// own texture coordinates, and how fast they move along V.
#[derive(Component)]
pub struct CloudScroll {
    uvs: Vec<[f32; 2]>,
    speed: f32,
    /// The weather's layer (its colours by time of day) and the dome's
    /// opacity at each vertex.
    pub layer: usize,
    pub alphas: Vec<f32>,
}

impl Spawner<'_, '_> {
    /// The sun (`cellview::game::SunSprite`): its disc and its glare, each a
    /// square of its texture facing the eye, over the dome and under the
    /// clouds; `daylight` places and colours them for the hour.
    fn spawn_sun(&mut self, sun: &cellview::game::SunSprite) {
        self.spawn_sun_square(&sun.texture, sun.half_size, None);
        if let Some(glare) = &sun.glare {
            self.spawn_sun_square(glare, sun.glare_half_size, Some(sun.glare_strength));
        }
    }

    /// One of the sun's squares (dark until `daylight` colours it).
    fn spawn_sun_square(
        &mut self,
        texture: &TextureData,
        half_size: f32,
        glare: Option<f32>,
    ) -> Entity {
        let positions = sun_square([0.0, 0.0, 1.0], half_size * 2.0);
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let uvs: Vec<[f32; 2]> = corners
            .iter()
            .map(|&(x, y)| [(x + 1.0) * 0.5, (1.0 - y) * 0.5])
            .collect();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0, 0.0, 0.0, 0.0]; 4]);
        mesh.insert_indices(bevy::render::mesh::Indices::U16(vec![0, 1, 2, 0, 2, 3]));
        let base = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: self.upload(texture),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            // Added to the sky (a guess: blended over it, the wide halo
            // darkened the sky around the sun).
            alpha_mode: AlphaMode::Add,
            depth_bias: if glare.is_some() { 0.6 } else { 0.5 },
            ..default()
        };
        let extension = GameLit {
            lighting: GameLighting {
                ambient: Vec4::ZERO,
                directional_color: Vec4::ZERO,
                directional_direction: Vec4::Y,
                emissive: Vec4::new(1.0, 1.0, 1.0, 0.0),
                scale: Vec4::new(full_brightness_nits(), 0.0, 0.0, 0.0),
                fog_color: Vec4::ZERO,
                fog_range: Vec4::ZERO,
                specular: Vec4::ZERO,
                surface: Vec4::new(0.0, 0.0, 1.0, 0.0),
                falloff: NO_FALLOFF,
                environment: Vec4::ZERO,
                draw: Vec4::ZERO,
                actor: Vec4::ZERO,
                hair_tint: Vec4::ZERO,
                lights: [GameLight::default(); MAX_LIGHTS],
            },
            glow: None,
            normal_map: None,
            environment: None,
            environment_mask: None,
            key: DrawKey::default(),
        };
        let scale = SKY_RADIUS_METERS / (500.0 * space::METERS_PER_UNIT);
        self.commands
            .spawn((
                Mesh3d(self.meshes.add(mesh)),
                MeshMaterial3d(self.lit_materials.add(GameLitMaterial { base, extension })),
                Transform::from_scale(Vec3::splat(scale * 0.99)),
                bevy::render::view::NoFrustumCulling,
                daylight::SunDisk { half_size, glare },
                SkyEntity,
                SceneEntity,
            ))
            .id()
    }
    /// The night sky (`cellview::game::StarDome`) inside the dome, added
    /// onto it; dark until `daylight` colours it for the hour.
    fn spawn_stars(&mut self, stars: &cellview::game::StarDome) -> Entity {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        let radius = stars
            .positions
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(1.0f32, f32::max);
        // Inside the dome (half the sky radius across), by the same margin
        // as the clouds.
        let scale = SKY_RADIUS_METERS * 0.5 * 0.97 / (radius * space::METERS_PER_UNIT);
        // `SKYSHORIZFADE`: gone below the horizon, full 17 units above the
        // eye (in the world: almost at once).
        let units = radius * scale;
        let fade: Vec<f32> = stars
            .positions
            .iter()
            .map(|p| (p[2] / radius * units / 17.0).clamp(0.0, 1.0))
            .collect();
        let positions: Vec<[f32; 3]> = stars.positions.iter().map(|&p| space::point(p)).collect();
        let count = positions.len();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, stars.uvs.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0, 0.0, 0.0, 0.0]; count]);
        mesh.insert_indices(bevy::render::mesh::Indices::U16(stars.indices.clone()));
        let base = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: self.upload(&stars.texture),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            // Added (One + One in the model; the fade is the alpha, so it
            // weights what's added: a guess where the two disagree).
            alpha_mode: AlphaMode::Add,
            depth_bias: 0.5,
            ..default()
        };
        let extension = GameLit {
            lighting: GameLighting {
                ambient: Vec4::ZERO,
                directional_color: Vec4::ZERO,
                directional_direction: Vec4::Y,
                emissive: Vec4::new(1.0, 1.0, 1.0, 0.0),
                scale: Vec4::new(full_brightness_nits(), 0.0, 0.0, 0.0),
                fog_color: Vec4::ZERO,
                fog_range: Vec4::ZERO,
                specular: Vec4::ZERO,
                surface: Vec4::new(0.0, 0.0, 1.0, 0.0),
                falloff: NO_FALLOFF,
                environment: Vec4::ZERO,
                draw: Vec4::ZERO,
                actor: Vec4::ZERO,
                hair_tint: Vec4::ZERO,
                lights: [GameLight::default(); MAX_LIGHTS],
            },
            glow: None,
            normal_map: None,
            environment: None,
            environment_mask: None,
            key: DrawKey::default(),
        };
        self.commands
            .spawn((
                Mesh3d(self.meshes.add(mesh)),
                MeshMaterial3d(self.lit_materials.add(GameLitMaterial { base, extension })),
                Transform::from_scale(Vec3::splat(scale)),
                bevy::render::view::NoFrustumCulling,
                daylight::Stars { fade },
                SkyEntity,
                SceneEntity,
            ))
            .id()
    }

    /// A weather's cloud layer over the sky dome: its texture times the
    /// layer's colour, faded toward the horizon by the dome's vertex alpha,
    /// no light or fog, kept centred on the camera with the dome.
    fn spawn_clouds(&mut self, layer: &cellview::game::CloudLayer) -> Entity {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        let positions: Vec<[f32; 3]> = layer.positions.iter().map(|&p| space::point(p)).collect();
        let count = positions.len();
        let [r, g, b] = layer.color.map(srgb_decode);
        let colors: Vec<[f32; 4]> = layer.alphas.iter().map(|&a| [r, g, b, a]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, layer.uvs.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(bevy::render::mesh::Indices::U16(layer.indices.clone()));
        let base = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: self.upload(&layer.texture),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            // Over the dome, both centred on the camera.
            depth_bias: 1.0,
            ..default()
        };
        let extension = GameLit {
            lighting: GameLighting {
                ambient: Vec4::ZERO,
                directional_color: Vec4::ZERO,
                directional_direction: Vec4::Y,
                emissive: Vec4::new(1.0, 1.0, 1.0, 0.0),
                scale: Vec4::new(full_brightness_nits(), 0.0, 0.0, 0.0),
                fog_color: Vec4::ZERO,
                fog_range: Vec4::ZERO,
                specular: Vec4::ZERO,
                surface: Vec4::new(0.0, 0.0, 1.0, 0.0),
                falloff: NO_FALLOFF,
                environment: Vec4::ZERO,
                draw: Vec4::ZERO,
                actor: Vec4::ZERO,
                hair_tint: Vec4::ZERO,
                lights: [GameLight::default(); MAX_LIGHTS],
            },
            glow: None,
            normal_map: None,
            environment: None,
            environment_mask: None,
            key: DrawKey::default(),
        };
        let scale = SKY_RADIUS_METERS / (500.0 * space::METERS_PER_UNIT);
        self.commands
            .spawn((
                Mesh3d(self.meshes.add(mesh)),
                MeshMaterial3d(self.lit_materials.add(GameLitMaterial { base, extension })),
                Transform::from_scale(Vec3::splat(scale * 0.98)),
                bevy::render::view::NoFrustumCulling,
                CloudScroll {
                    uvs: layer.uvs.clone(),
                    speed: layer.scroll,
                    layer: layer.layer,
                    alphas: layer.alphas.clone(),
                },
                SkyEntity,
                SceneEntity,
            ))
            .id()
    }

    /// The `--brightness` setting.
    fn brightness(&self) -> f32 {
        self.settings.brightness
    }

    /// One texture for the graphics card.
    fn upload(&mut self, texture: &TextureData) -> Option<Handle<Image>> {
        let compressed = self
            .device
            .as_ref()
            .is_none_or(|d| d.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC));
        upload_texture(
            &mut self.images,
            texture,
            compressed,
            self.settings.anisotropy,
        )
    }
}

/// The light distant land is drawn with: a square's ambient, sun and fog.
fn lod_params(scene: &ViewerScene, brightness: f32) -> LodLandParams {
    let l = game_lighting(scene, brightness);
    LodLandParams {
        ambient: l.ambient,
        sun_color: l.directional_color,
        sun_direction: l.directional_direction,
        fog_color: l.fog_color,
        fog_range: l.fog_range,
        scale: l.scale,
        high_detail: Vec4::ZERO,
    }
}

/// The sRGB curve's decoding, not clamped at 1.
fn srgb_decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// `--fps`: frames counted, and the time since the last report.
#[derive(Resource, Default)]
struct FrameCounter {
    on: bool,
    frames: u32,
    seconds: f32,
}

fn report_fps(time: Res<Time>, mut counter: ResMut<FrameCounter>) {
    if !counter.on {
        return;
    }
    counter.frames += 1;
    counter.seconds += time.delta_secs();
    if counter.seconds >= 2.0 {
        println!(
            "{:.0} frames per second ({:.1} ms a frame)",
            counter.frames as f32 / counter.seconds,
            1000.0 * counter.seconds / counter.frames as f32
        );
        counter.frames = 0;
        counter.seconds = 0.0;
    }
}

/// The sky stays centred on the eye, as the game's does.
fn follow_sky(
    time: Res<Time>,
    settings: Res<Settings>,
    cameras: Query<&Transform, (With<FlyCamera>, Without<SkyEntity>)>,
    mut skies: Query<&mut Transform, With<SkyEntity>>,
    clouds: Query<(&Mesh3d, &CloudScroll)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    for mut sky in &mut skies {
        sky.translation = camera.translation;
    }
    // Clouds drift: their texture moves along V (`TexCoordYOff`), unless
    // `--cloud-time` holds them.
    let t = settings.cloud_time.unwrap_or_else(|| time.elapsed_secs());
    for (mesh, scroll) in &clouds {
        if scroll.speed == 0.0 {
            continue;
        }
        let off = (t * scroll.speed).fract();
        if let Some(m) = meshes.get_mut(&mesh.0) {
            let uvs: Vec<[f32; 2]> = scroll.uvs.iter().map(|&[u, v]| [u, v + off]).collect();
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        }
    }
}

/// A texture for the graphics card, or `None` (with a message) when its
/// data can't be used.
fn upload_texture(
    images: &mut Assets<Image>,
    texture: &TextureData,
    compressed: bool,
    anisotropy: u16,
) -> Option<Handle<Image>> {
    // The game's sampling (see `anisotropy_setting`): anisotropic
    // trilinear at the INI's anisotropy, no LOD bias, repeating.
    let sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy,
        ..default()
    });
    // Reflection cube maps are clamped at their edges, as the game samples
    // them.
    let cube_sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy,
        ..default()
    });
    let cube = texture.layers == 6;
    let format = texture.gpu_format(compressed);
    let data = match texture.level_data(format) {
        Ok(data) => data,
        Err(e) => {
            println!("  skipping texture {}: {e}", texture.path);
            return None;
        }
    };
    let image = Image {
        data: Some(data),
        texture_descriptor: TextureDescriptor {
            label: None,
            size: Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: texture.layers,
            },
            mip_level_count: texture.mip_levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: gpu_format(format),
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: if cube { cube_sampler } else { sampler },
        texture_view_descriptor: cube.then(|| TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        }),
        asset_usage: RenderAssetUsages::RENDER_WORLD,
    };
    Some(images.add(image))
}

/// A model piece's material: the standard material for its texture,
/// blending and culling, with the game's lighting on top.
fn lit_material(
    data: &cellview::MeshData,
    textures: &[Option<Handle<Image>>],
    lighting: GameLighting,
) -> GameLitMaterial {
    let m = &data.material;
    let [r, g, b, a] = m.color;
    let base = StandardMaterial {
        base_color: Color::linear_rgba(r, g, b, a),
        base_color_texture: m.texture.and_then(|i| textures[i].clone()),
        unlit: m.unlit,
        double_sided: m.double_sided,
        cull_mode: if m.double_sided {
            None
        } else {
            Some(Face::Back)
        },
        alpha_mode: alpha_mode(m.blend),
        // Only orders blended surfaces here (the pipeline's own bias is
        // turned off for decals, which the shader pulls nearer instead):
        // the game draws its decals before every other blended surface.
        depth_bias: if m.decal { DECALS_FIRST } else { 0.0 },
        ..default()
    };
    let (test, threshold) = lighting::alpha_test_code(m.alpha_test);
    let draw = Vec4::new(
        if m.decal { lighting.draw.x } else { 0.0 },
        test,
        threshold,
        0.0,
    );
    let key = DrawKey {
        game: true,
        depth_test: m.depth_test,
        depth_write: m.depth_write,
        decal: m.decal,
    };
    let mut extension = if m.unlit {
        let [ur, ug, ub] = m.unlit_color;
        // How the game's no-lighting shaders fog (`Toggles`): added
        // effects fade to black, multiplied ones to white (no effect),
        // the rest toward the fog color.
        let fog_mode = match m.blend {
            Blend::Add => 1.0,
            Blend::Multiply => 2.0,
            _ => 0.0,
        };
        GameLit {
            lighting: GameLighting {
                emissive: Vec4::new(ur, ug, ub, 0.0),
                surface: Vec4::new(0.0, 0.0, 1.0, fog_mode),
                falloff: m.falloff.map_or(NO_FALLOFF, |f| {
                    Vec4::new(f.start_cos, f.stop_cos, f.start_opacity, f.stop_opacity)
                }),
                ..lighting
            },
            key,
            glow: None,
            normal_map: None,
            environment: None,
            environment_mask: None,
        }
    } else {
        let glow = m.glow.and_then(|i| textures[i].clone());
        let normal_map = m
            .normal_map
            .filter(|_| data.tangents.is_some())
            .and_then(|i| textures[i].clone());
        let [er, eg, eb] = m.emissive;
        let specular = m.specular.map_or(Vec4::ZERO, |s| {
            Vec4::new(s.color[0], s.color[1], s.color[2], s.glossiness)
        });
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        let reflection = m
            .environment
            .and_then(|e| Some((e, textures[e.cube].clone()?)));
        let environment_mask = reflection
            .as_ref()
            .and_then(|(e, _)| e.mask)
            .and_then(|i| textures[i].clone());
        GameLit {
            lighting: GameLighting {
                emissive: Vec4::new(er, eg, eb, flag(glow.is_some())),
                specular,
                surface: Vec4::new(
                    flag(normal_map.is_some()),
                    flag(m.specular.is_some()),
                    0.0,
                    flag(matches!(alpha_mode(m.blend), AlphaMode::Blend)),
                ),
                environment: reflection.as_ref().map_or(Vec4::ZERO, |(e, _)| {
                    Vec4::new(
                        e.strength,
                        flag(environment_mask.is_some()),
                        flag(e.window),
                        m.color[3],
                    )
                }),
                actor: Vec4::new(
                    GameLighting::shading_code(m.shading),
                    lighting.actor.y,
                    0.0,
                    0.0,
                ),
                hair_tint: m
                    .hair_tint
                    .map_or(Vec4::ZERO, |[r, g, b]| Vec4::new(r, g, b, 1.0)),
                ..lighting
            },
            key,
            glow,
            normal_map,
            environment: reflection.map(|(_, cube)| cube),
            environment_mask,
        }
    };
    extension.lighting.draw = draw;
    GameLitMaterial { base, extension }
}

/// Where decals sort among blended surfaces (Bevy's depth bias, added to
/// the distance it sorts by): before all of them.
const DECALS_FIRST: f32 = -1.0e6;

/// Puts a newly loaded interior on screen: its textures, meshes and
/// lights, the camera at its start, its image space, collision and doors.
#[allow(clippy::too_many_arguments)]
fn spawn_scene(
    mut pending: ResMut<PendingScene>,
    mut spawner: Spawner,
    mut start_pitch: ResMut<StartPitch>,
    mut grading: ResMut<Grading>,
    mut player: ResMut<walk::Player>,
    mut clear: ResMut<ClearColor>,
    old: Query<Entity, With<SceneEntity>>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera, &mut ImageSpaceGrade)>,
    mut windows: Query<&mut Window>,
    (mut swing_doors, mut state, game): (
        ResMut<doors::SwingDoors>,
        ResMut<dialogue::DialogueState>,
        Res<GameFiles>,
    ),
) {
    let Some(scene) = pending.0.take() else {
        return;
    };
    // The place's doors that swing, and how long their models' sequences
    // play (the state's `GetOpenState` needs it).
    swing_doors.read_settings(&game.0.settings);
    swing_doors.replace(scene.swing_doors.iter(), &mut state.0);
    for entity in &old {
        spawner.commands.entity(entity).despawn();
    }
    exterior::leave(&mut spawner.commands);
    clear.0 = Color::BLACK;
    for mut window in &mut windows {
        window.title = format!("nv-rs viewer - {}", scene.cell);
    }
    spawner.spawn(&scene);

    let eye = Vec3::from(space::point(scene.start.eye));
    let yaw = space::heading_to_yaw(scene.start.heading);
    // The starting pitch (`--at`) applies to the first cell only.
    let pitch = std::mem::take(&mut start_pitch.0);
    grading.grade = ImageSpaceGrade::from_cell(scene.grade.as_ref(), scene.hdr.as_ref());
    let shown = if grading.on {
        grading.grade
    } else {
        grading.grade.without_cinematic()
    };
    for (mut transform, mut fly, mut grade) in &mut cameras {
        *transform = Transform::from_translation(eye).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw,
            pitch,
            0.0,
        ));
        fly.yaw = yaw;
        fly.pitch = pitch;
        fly.start = (eye, yaw);
        *grade = shown;
    }
    let [x, y, z] = scene.start.eye;
    player.arrive([x, y, z - cellview::EYE_HEIGHT]);
    spawner.commands.insert_resource(dialogue::Talkers(
        scene
            .actors
            .iter()
            .map(dialogue::Talker::from_actor)
            .collect(),
    ));
    spawner
        .commands
        .insert_resource(walk::CellCollision(scene.collision));
    spawner
        .commands
        .insert_resource(scripts::ObjectBounds::from_scene(&scene.object_bounds));
    spawner.commands.insert_resource(walk::Doors(scene.doors));
    spawner
        .commands
        .insert_resource(scripts::Here(Some(scene.cell_id)));
}

fn help_text(ev100: f32, speed: f32, walking: bool) -> String {
    let moving = if walking {
        "Walking: hold the right mouse button to look, left to attack (R reloads); WASD, \
         Shift to walk slowly, Ctrl to sneak, Space to jump, E to use things"
            .to_string()
    } else {
        format!(
            "Flying: hold a mouse button to look; WASD, Space/Ctrl up/down, Shift faster, \
             wheel: speed ({speed:.1} m/s), E to use things"
        )
    };
    format!(
        "{moving}\n\
         F: walk/fly   V: V.A.T.S.   Tab: Pip-Boy   T: wait   F5/F9: save/load   \
         [ ]: exposure (EV {ev100:.1})   G: image space   Home: start   Esc: quit"
    )
}

/// The help line, kept up to date.
fn update_help(
    cameras: Query<(&Exposure, &FlyCamera)>,
    player: Res<walk::Player>,
    mut help: Query<&mut Text, With<HelpText>>,
) {
    let Ok((exposure, camera)) = cameras.single() else {
        return;
    };
    let wanted = help_text(exposure.ev100, camera.speed, player.walking);
    for mut text in &mut help {
        if text.0 != wanted {
            text.0 = wanted.clone();
        }
    }
}

/// Holding a mouse button turns the view, walking or flying.
fn look_around(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    state: Res<dialogue::DialogueState>,
    player: Res<walk::Player>,
    start_stage: Res<scripts::StartStage>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let Ok((mut transform, mut camera)) = cameras.single_mut() else {
        return;
    };
    // Scripts can block looking through controls or a player AI package;
    // the free-flying camera (F) isn't the player and always looks.
    // Walking, the left button attacks (`combat`), so the right one looks.
    // The initial stage is dispatched later in this Update chain. Do not
    // accept an input frame before it installs the player's native package
    // lock (005cc4f0 -> 005cc7a0), or while the place is still loading.
    let locked = player.walking
        && (!player.ready || start_stage.0.is_some() || state.0.player_looking_blocked());
    let held =
        mouse.pressed(MouseButton::Right) || (!player.walking && mouse.pressed(MouseButton::Left));
    if !locked && held {
        camera.yaw -= motion.delta.x * LOOK_SPEED;
        camera.pitch = (camera.pitch - motion.delta.y * LOOK_SPEED).clamp(-1.54, 1.54);
    }
    transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
}

/// G switches the cell's color adjustment (saturation, tint, contrast,
/// brightness) off and on, for comparing. Bloom stays on.
fn toggle_grade(
    keys: Res<ButtonInput<KeyCode>>,
    mut grading: ResMut<Grading>,
    mut cameras: Query<&mut ImageSpaceGrade>,
) {
    if !keys.just_pressed(KeyCode::KeyG) {
        return;
    }
    grading.on = !grading.on;
    let grade = if grading.on {
        grading.grade
    } else {
        grading.grade.without_cinematic()
    };
    for mut current in &mut cameras {
        *current = grade;
    }
}

/// Flying: free movement through everything.
fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    player: Res<walk::Player>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    if player.walking {
        return;
    }
    let Ok((mut transform, mut camera)) = cameras.single_mut() else {
        return;
    };
    if scroll.delta.y != 0.0 {
        let notches = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        camera.speed = (camera.speed * 1.15f32.powf(notches.clamp(-5.0, 5.0))).clamp(0.2, 50.0);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let (eye, yaw) = camera.start;
        transform.translation = eye;
        camera.yaw = yaw;
        camera.pitch = 0.0;
    }
    transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);

    let forward = transform.forward().as_vec3();
    let right = transform.right().as_vec3();
    let mut direction = Vec3::ZERO;
    for (key, step) in [
        (KeyCode::KeyW, forward),
        (KeyCode::KeyS, -forward),
        (KeyCode::KeyD, right),
        (KeyCode::KeyA, -right),
        (KeyCode::Space, Vec3::Y),
        (KeyCode::ControlLeft, -Vec3::Y),
        (KeyCode::KeyQ, -Vec3::Y),
    ] {
        if keys.pressed(key) {
            direction += step;
        }
    }
    if direction != Vec3::ZERO {
        let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            4.0
        } else {
            1.0
        };
        transform.translation += direction.normalize() * camera.speed * boost * time.delta_secs();
    }
}

fn adjust_exposure(keys: Res<ButtonInput<KeyCode>>, mut cameras: Query<&mut Exposure>) {
    for mut exposure in &mut cameras {
        // Lower EV100 means a longer exposure: a brighter picture.
        if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::Equal) {
            exposure.ev100 -= 0.5;
        }
        if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::Minus) {
            exposure.ev100 += 0.5;
        }
    }
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: EventWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_motion_cannot_turn_player_during_script_package_and_releases_afterward() {
        let mut app = App::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Right);
        let state = world::scripting::GameState::default();
        app.insert_resource(mouse)
            .insert_resource(AccumulatedMouseMotion {
                delta: Vec2::new(12.0, -8.0),
            })
            .insert_resource(dialogue::DialogueState(state))
            .insert_resource(walk::Player::new(true))
            .insert_resource(scripts::StartStage(Some(("TestOpening".into(), 0))))
            .add_systems(Update, look_around);
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                FlyCamera {
                    yaw: 0.3,
                    pitch: 0.2,
                    speed: 1.0,
                    start: (Vec3::ZERO, 0.3),
                },
            ))
            .id();
        app.update();
        let input = app.world().get::<FlyCamera>(camera).unwrap();
        assert_eq!((input.yaw, input.pitch), (0.3, 0.2));
        // The initial script runs after input in the application schedule.
        // It installs the first package; replacements must not leak input.
        app.world_mut().resource_mut::<scripts::StartStage>().0 = None;
        for frame in 0..600 {
            if frame % 180 == 0 {
                app.world_mut()
                    .resource_mut::<dialogue::DialogueState>()
                    .0
                    .script_packages
                    .insert(world::dialogue::PLAYER_REF, esm::FormId(1 + frame / 180));
            }
            app.update();
            let input = app.world().get::<FlyCamera>(camera).unwrap();
            assert_eq!((input.yaw, input.pitch), (0.3, 0.2), "frame {frame}");
        }
        app.world_mut()
            .resource_mut::<dialogue::DialogueState>()
            .0
            .script_packages
            .clear();
        app.update();
        let input = app.world().get::<FlyCamera>(camera).unwrap();
        assert!(input.yaw < 0.3 && input.pitch > 0.2);
        let previous = (input.yaw, input.pitch);
        app.world_mut()
            .resource_mut::<dialogue::DialogueState>()
            .0
            .script_packages
            .insert(world::dialogue::PLAYER_REF, esm::FormId(1));
        app.world_mut().resource_mut::<walk::Player>().walking = false;
        app.update();
        let input = app.world().get::<FlyCamera>(camera).unwrap();
        assert!(
            input.yaw < previous.0 && input.pitch > previous.1,
            "research free camera remains usable"
        );
    }

    #[test]
    fn textures_are_filtered_at_the_inis_anisotropy() {
        let mut ini = assets::IniSettings::default();
        assert_eq!(anisotropy_setting(&ini), 8, "Fallout_default.ini's");
        ini.add("[Display]\r\niMaxAnisotropy=15\r\n");
        assert_eq!(
            anisotropy_setting(&ini),
            15,
            "this install's FalloutPrefs.ini"
        );
        ini.add("[Display]\r\niMaxAnisotropy=0\r\n");
        assert_eq!(anisotropy_setting(&ini), 1, "off: plain trilinear");
    }

    #[test]
    fn every_texture_is_sampled_as_stored() {
        // The game never sets `D3DSAMP_SRGBTEXTURE`: its filtering averages
        // stored values. (sRGB formats would average decoded light, which
        // brightened the Goodsprings terrain and statics by 1-3%.)
        for format in [
            GpuFormat::Bc1,
            GpuFormat::Bc2,
            GpuFormat::Bc3,
            GpuFormat::Bc4,
            GpuFormat::Bc5,
            GpuFormat::Rgba8,
        ] {
            assert!(!gpu_format(format).is_srgb(), "{format:?}");
        }
    }

    #[test]
    fn fog_is_measured_from_the_games_near_plane() {
        // Goodsprings by day: fog from 10 to 120000 units, power 0.5. The
        // game's fog distance takes the projected depth, which starts at its
        // near plane (5 units): the shaders get that in `fog_range.z`.
        let fog = cellview::Fog {
            color: [0.588, 0.659, 0.745],
            near: 10.0,
            far: 120_000.0,
            power: 0.5,
        };
        let f = light_fields([0.0; 3], None, Some(&fog), 1.0);
        let m = space::METERS_PER_UNIT;
        assert!((f.fog_range.x - 10.0 * m).abs() < 1e-6);
        assert!((f.fog_range.z - 5.0 * m).abs() < 1e-6);
        assert_eq!(f.fog_range.w, 1.0);
        assert_eq!(f.fog_color.w, 0.5);
    }

    fn piece(blend: Blend, center: [f32; 3]) -> cellview::MeshData {
        cellview::MeshData {
            name: "piece".into(),
            shape_name: "piece".into(),
            positions: vec![[1.0, 2.0, 3.0], [4.0, 2.0, 3.0], [1.0, 5.0, 3.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: None,
            uvs: vec![[0.0; 2]; 3],
            colors: None,
            indices: vec![0, 1, 2],
            material: cellview::MaterialData {
                texture: None,
                color: [1.0; 4],
                blend,
                alpha_test: None,
                depth_test: true,
                depth_write: true,
                sort_center: center,
                unlit: false,
                double_sided: false,
                emissive: [0.0; 3],
                glow: None,
                decal: false,
                normal_map: None,
                specular: None,
                unlit_color: [1.0; 3],
                falloff: None,
                environment: None,
                emittance: None,
                shading: preview::cell::Shading::Plain,
                hair_tint: None,
            },
            effect: false,
            rig: None,
            motion: None,
            billboard: None,
        }
    }

    #[test]
    fn cut_outs_are_tested_in_the_shader_and_tested_glass_blends() {
        // The alpha test is the shader's (the game's exact comparison), so
        // Bevy's own cut-out and coverage modes aren't used.
        assert_eq!(alpha_mode(Blend::Mask(0.5)), AlphaMode::Opaque);
        assert_eq!(alpha_mode(Blend::MaskedBlend(0.8)), AlphaMode::Blend);
        assert_eq!(alpha_mode(Blend::MaskedBlend(0.1)), AlphaMode::Blend);
        assert_eq!(alpha_mode(Blend::Add), AlphaMode::Add);
    }

    #[test]
    fn blended_pieces_are_built_around_their_sort_point() {
        let c = [2.0, 3.0, 3.0];
        // Opaque pieces keep their own origin; blended ones are sorted by
        // the centre of their stored bound.
        assert_eq!(sort_center(&piece(Blend::Opaque, c)), None);
        assert_eq!(sort_center(&piece(Blend::Mask(0.5), c)), None);
        assert_eq!(sort_center(&piece(Blend::Blend, c)), Some(c));
        assert_eq!(sort_center(&piece(Blend::Add, c)), Some(c));
        let data = piece(Blend::Blend, c);
        let mesh = game_mesh_around(&data, c);
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(p)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        assert_eq!(p[0], [-1.0, -1.0, 0.0]);
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(a)) =
            mesh.attribute(lighting::ATTRIBUTE_CORNER_B)
        else {
            panic!("no corners");
        };
        assert_eq!(a[0], [2.0, -1.0, 0.0]);
    }
}
