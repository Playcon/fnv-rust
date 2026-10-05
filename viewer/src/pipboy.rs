//! The Pip-Boy 3000 (`ui::pipboy`): the game's three Pip-Boy menus (STATS,
//! ITEMS, DATA) filled from the game's state, drawn as the game draws them
//! with `[Pipboy] bUsePipboyMode` on (this install): into a picture of
//! their own (1280 × 960 menu units, `007fba00`), which the screen effect
//! (`ISIFSCANBLEND`, `pipboy.wgsl`: the glow, scanlines, the rolling
//! picture and the bright band, timed by `ui::pipboy::screen`) turns into
//! the texture of the arm model's `pipboyscreen:0` (`PipBoy3000\
//! PipBoyArm.NIF`, the `PipBoy` apparel's model, held on the first-person
//! skeleton's `Bip01 L ForeTwist` as its `Prn` says), raised in front of
//! the eye by the first-person `pipboy.kf` and drawn with
//! `fPipboy1stPersonFOV` (47 in this install's `Fallout.ini`).
//!
//! Keys (read from the code): Tab (control 14 "Pip-Boy", its default key
//! in `00a24b70`) puts it up when let go before `fPlayerPipBoyLightTimer`
//! (0.8 s); held that long it switches the Pip-Boy light instead
//! (`009673d0`; the `PipBoyLight` ability, whose `PipLight` effect's light
//! is `PipboyLight640`: radius 768, colour 194, 245, 209). Up, Tab again
//! puts it away. Inside, the PC keyboard as the game reads it in menus
//! (`007154b0`, `0070c4a0`): the arrows (up and down the lists, left and
//! right the pages and tabs; with Shift the previous or next menu, round
//! the three), Enter the A button (equip, use, make a quest active),
//! Shift + Enter X and Alt + Enter Y (the Status page's aid), and letters
//! the buttons the menus' `_PCButton_` traits name (S Stimpak, A RadAway,
//! X Rad-X, E Doctor's Bag, R the General page's reputations).
//!
//! Sounds: `UIPipBoyAccessUp` / `Down` putting it up and away, the hum
//! `UIPipBoyHumLP` while it's up, `UIPipBoyTab` (the knob) between menus and
//! pages, `UIPipBoyScroll`, `UIPipBoySelect`, `UIPipBoyLightOn` / `Off`.
//! The lamps over STATS, ITEMS and DATA: only the shown menu's lit
//! (`007fa010`); the light's cone shown with the light on (`007fa310`).
//!
//! Guesses: the picture's size in pixels (one a menu unit); the arm held at
//! the raising animation's `Hit` key while up (where it's highest) and
//! lowered by playing on from there.
//! Not done: the mouse (the game maps the cursor through the screen's
//! texture coordinates, `007f8720`: pressing buttons, the world map's
//! markers and dragging the map, so travelling from the map isn't
//! possible yet), the keys held repeating, the light lighting the place
//! (only its cone on the arm shows), the world paused while it's up, the
//! knobs, needle and buttons moving, the `xbox` button labels swapped for
//! the PC's, Page Up / Page Down (the pad's bumpers: zooming the maps,
//! Mod and hot keys on ITEMS).
// The shader-layout derive generates checking functions the compiler
// reports as unused.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{load_internal_asset, weak_handle, RenderAssetUsages};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderRef, ShaderType, TextureDimension, TextureFormat, TextureUsages,
    WgpuFeatures,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::RenderLayers;
use bevy::sprite::{AlphaMode2d, Material2d, Material2dPlugin};
use cellview::space;
use esm::FormId;
use preview::cell::ActorSkeleton;
use ui::draw::{DrawItem, DrawKind};
use ui::pipboy::screen::{ScreenEffects, ScreenSettings};
use ui::pipboy::{Action, Key, PipboyInput, Section};
use world::dialogue::PLAYER_REF;

use crate::dialogue::DialogueState;
use crate::hud::{Files, Quad, TileMaterial};
use crate::lighting::GameLitMaterial;
use crate::menus::Menus;
use crate::sounds::{PcmSound, SoundRequests};
use crate::walk::{game_point, Player};
use crate::{FlyCamera, GameFiles, Spawner};

const SCREEN_SHADER: Handle<Shader> = weak_handle!("6f1d0b9e-3c52-4a8e-b7f4-2e9a5c0d81b7");

/// The render layers only the Pip-Boy's two cameras see.
const MENU_LAYER: usize = 24;
const SCREEN_LAYER: usize = 25;

/// The menus' picture in pixels: the game's 1280 × 960 menu units, one
/// pixel each (the size of the game's render target isn't traced).
const PICTURE: UVec2 = UVec2::new(1280, 960);

/// `fPipboy1stPersonFOV` in this install's `Fallout.ini` (the exe's
/// reader isn't traced; a 4:3 width like the other fields of view).
const PIPBOY_FOV_DEGREES: f32 = 47.0;

/// `fPlayerPipBoyLightTimer` (`011cd098`): how long Tab is held to switch
/// the light.
const LIGHT_HOLD_SECONDS: f32 = 0.8;

/// The first-person raising animation's files (`Characters\_1stPerson\
/// Locomotion\Male\Pipboy.kf`, 0.73 s, and the female one).
const RAISE_MALE: &str = "Characters\\_1stPerson\\Locomotion\\Male\\Pipboy.kf";
const RAISE_FEMALE: &str = "Characters\\_1stPerson\\Locomotion\\Female\\PipboyFemale.kf";

/// The Pip-Boy (`ARMO` `PipBoy`) and its glove (`PipBoyGlove`): the
/// player record's two items, worn.
const PIPBOY_ITEM: &str = "PipBoy";
const GLOVE_ITEM: &str = "PipBoyGlove";

pub struct PipboyPlugin;

impl Plugin for PipboyPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SCREEN_SHADER, "pipboy.wgsl", Shader::from_wgsl);
        app.add_plugins(Material2dPlugin::<ScreenMaterial>::default())
            .init_resource::<Pipboy>()
            .add_systems(Startup, setup_pipboy)
            .add_systems(
                Update,
                (
                    pipboy_keys.before(crate::menus::run_menus),
                    update_pipboy
                        .after(crate::scripts::run_scripts)
                        .after(crate::viewmodel::update_view_model)
                        .before(crate::actors::animate_actors),
                ),
            );
    }
}

/// `--pipboy SECTION[:PAGE]`: open it on this once the place is up.
#[derive(Resource, Default)]
pub struct StartPipboy(pub Option<String>);

/// The screen effect's constants (`ISIFSCANBLEND`'s `Params`,
/// `DistortParams`, `Tint`, `Offsets`).
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct ScreenUniform {
    params: Vec4,
    distort: Vec4,
    tint: Vec4,
    offsets: Vec4,
}

/// The screen effect: the menus' picture, the scanlines and the band.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct ScreenMaterial {
    #[uniform(0)]
    u: ScreenUniform,
    #[texture(1)]
    #[sampler(2)]
    picture: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    scanlines: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    band: Handle<Image>,
}

impl Material2d for ScreenMaterial {
    fn fragment_shader() -> ShaderRef {
        SCREEN_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

/// The menus as built, and the pieces of their picture on screen.
struct Built {
    ui: ui::Ui,
    pipboy: ui::pipboy::Pipboy,
    sizes: HashMap<String, Option<(u32, u32)>>,
    atlases: HashMap<String, Option<ui::Atlas>>,
    images: HashMap<(String, bool, bool), Option<Handle<Image>>>,
    font_images: HashMap<(usize, u32), Option<Handle<Image>>>,
    last: Vec<DrawItem>,
    drawn: Vec<(Entity, Handle<Mesh>, Handle<TileMaterial>)>,
}

/// The arm on screen.
struct Arm {
    worn: Vec<FormId>,
    weapon: Option<FormId>,
    female: bool,
    lighting: u64,
    holder: Entity,
    root: Entity,
    joints: Vec<Entity>,
    skeleton: Arc<ActorSkeleton>,
    turned: Vec<usize>,
    looking: usize,
    camera: usize,
    raise: Option<Raise>,
    /// The screen piece, until its texture has been swapped.
    screen: Option<Entity>,
    light_effect: Vec<Entity>,
    /// The lamps over the STATS, ITEMS and DATA buttons.
    glows: Vec<Entity>,
}

/// The Pip-Boy's state.
#[derive(Resource, Default)]
pub struct Pipboy {
    built: Option<Box<Built>>,
    failed: bool,
    /// Up (or going up).
    pub open: bool,
    /// When it last went up or down (seconds), and whether that was at
    /// once (screenshots).
    since: Option<f32>,
    at_once: bool,
    /// Tab held since, and whether this hold switched the light.
    tab_down: Option<f32>,
    held_for_light: bool,
    pub light: bool,
    input: Option<PipboyInput>,
    effects: Option<ScreenEffects>,
    picture: Option<Handle<Image>>,
    screen: Option<Handle<Image>>,
    white: Option<Handle<Image>>,
    material: Option<Handle<ScreenMaterial>>,
    hum: Option<Entity>,
    arm: Option<Arm>,
    /// A menu and page to show once filled.
    pending: Option<(Section, Option<usize>)>,
}

/// A picture to render into, read as stored values.
fn target_image(images: &mut Assets<Image>, size: UVec2) -> Handle<Image> {
    let mut image = Image::new_uninit(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = crate::hud::sampler(false, false);
    images.add(image)
}

/// The two pictures and their cameras: the menus drawn into one, the
/// screen effect drawing that into the other.
fn setup_pipboy(
    mut commands: Commands,
    game: Res<GameFiles>,
    mut pipboy: ResMut<Pipboy>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ScreenMaterial>>,
    device: Option<Res<RenderDevice>>,
) {
    let game = &game.0;
    let picture = target_image(&mut images, PICTURE);
    let screen = target_image(&mut images, PICTURE);
    let camera = |target: &Handle<Image>, order: isize, layer: usize| {
        (
            Camera2d,
            Camera {
                target: RenderTarget::from(target.clone()),
                order,
                hdr: true,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                is_active: false,
                ..default()
            },
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            RenderLayers::layer(layer),
            PipboyCamera,
        )
    };
    commands.spawn(camera(&picture, -5, MENU_LAYER));
    commands.spawn(camera(&screen, -4, SCREEN_LAYER));
    let compressed = device
        .as_ref()
        .is_none_or(|d| d.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC));
    let mut white = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    white.sampler = crate::hud::sampler(false, false);
    let white = images.add(white);
    // `sScanlineTexture:InterfaceFX` (`Data\Textures\Pipboy3000\
    // PipboyScanlines.dds`), repeated; the band's map, clamped.
    let scanlines = crate::hud::upload_picture(
        &mut images,
        game,
        "textures\\pipboy3000\\pipboyscanlines.dds",
        (true, true),
        compressed,
    )
    .unwrap_or_else(|| white.clone());
    let band = crate::hud::upload_picture(
        &mut images,
        game,
        "textures\\pipboy3000\\pipboydistorteffectmap.dds",
        (false, false),
        compressed,
    )
    .unwrap_or_else(|| white.clone());
    let material = materials.add(ScreenMaterial {
        u: ScreenUniform {
            params: Vec4::new(0.0, 0.0, 1.0, 0.0),
            distort: Vec4::ZERO,
            tint: Vec4::ONE,
            offsets: Vec4::ZERO,
        },
        picture: picture.clone(),
        scanlines,
        band,
    });
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(PICTURE.x as f32, PICTURE.y as f32))),
        MeshMaterial2d(material.clone()),
        Transform::IDENTITY,
        RenderLayers::layer(SCREEN_LAYER),
    ));
    let ini = |s: &str, k: &str| game.settings.get(s, k).map(str::to_string);
    pipboy.effects = Some(ScreenEffects::new(ScreenSettings::from_ini(&ini), 0.0));
    pipboy.picture = Some(picture);
    pipboy.screen = Some(screen);
    pipboy.white = Some(white);
    pipboy.material = Some(material);
}

/// The Pip-Boy's two cameras (on only while it's up).
#[derive(Component)]
pub struct PipboyCamera;

/// Reads the menus from the game's files.
fn build(game: &cellview::Game) -> Result<Built, String> {
    let mut read = |p: &str| game.assets.read(p).ok().flatten();
    let ini = |s: &str, k: &str| game.settings.get(s, k).map(str::to_string);
    let (w, h) = (
        game.settings
            .get("Display", "iSize W")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1920),
        game.settings
            .get("Display", "iSize H")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1080),
    );
    let mut ui = ui::game::new_ui(
        &mut read,
        &ini,
        crate::hud::text_settings(&game.order),
        w,
        h,
    );
    let pipboy = ui::pipboy::Pipboy::load(&mut ui, &mut read)?;
    for w in &ui.warnings {
        println!("  Pip-Boy: {w}");
    }
    Ok(Built {
        ui,
        pipboy,
        sizes: HashMap::new(),
        atlases: HashMap::new(),
        images: HashMap::new(),
        font_images: HashMap::new(),
        last: Vec::new(),
        drawn: Vec::new(),
    })
}

/// A sound record by editor ID, queued.
fn sound(order: &esm::LoadOrder, requests: &mut SoundRequests, name: &str) {
    if let Some(id) = order.form_by_editor_id(name) {
        requests.0.push(id);
    }
}

/// The menu keys pressed this frame, as the game reads a PC keyboard in
/// its menus (`007154b0`, `0070c4a0`): the arrows (Shift with left or
/// right: the previous or next menu), Enter (Shift: the X button, Alt: the
/// Y button), and the letters for the menus' `_PCButton_` traits.
fn menu_keys(keys: &ButtonInput<KeyCode>) -> Vec<Key> {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let mut out = Vec::new();
    let pressed = |code: KeyCode| keys.just_pressed(code);
    if pressed(KeyCode::ArrowUp) {
        out.push(Key::Up);
    }
    if pressed(KeyCode::ArrowDown) {
        out.push(Key::Down);
    }
    if pressed(KeyCode::ArrowLeft) {
        out.push(if shift { Key::PrevSection } else { Key::Left });
    }
    if pressed(KeyCode::ArrowRight) {
        out.push(if shift { Key::NextSection } else { Key::Right });
    }
    if pressed(KeyCode::Enter) || pressed(KeyCode::NumpadEnter) {
        out.push(if shift {
            Key::ButtonX
        } else if alt {
            Key::ButtonY
        } else {
            Key::Activate
        });
    }
    for (i, code) in [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ]
    .into_iter()
    .enumerate()
    {
        if pressed(code) {
            out.push(Key::Letter((b'a' + i as u8) as char));
        }
    }
    out
}
/// What opening, closing and the light need besides the Pip-Boy itself.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Around<'w> {
    time: Res<'w, Time>,
    game: Res<'w, GameFiles>,
    state: ResMut<'w, DialogueState>,
    menus: ResMut<'w, Menus>,
    player: ResMut<'w, Player>,
    conversation: Res<'w, crate::dialogue::Conversation>,
    requests: ResMut<'w, SoundRequests>,
    messages: ResMut<'w, crate::hud::HudMessages>,
    wavs: ResMut<'w, Assets<PcmSound>>,
    markers: Res<'w, crate::map::MapMarkers>,
    talkers: Res<'w, crate::dialogue::Talkers>,
}

/// Tab, the light, and the keys while it's up.
fn pipboy_keys(
    mut commands: Commands,
    mut pipboy: ResMut<Pipboy>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    around: Around,
) {
    let Around {
        time,
        game,
        mut state,
        mut menus,
        mut player,
        conversation,
        mut requests,
        mut messages,
        markers,
        talkers,
        ..
    } = around;
    let order = &game.0.order;
    let now = time.elapsed_secs();
    let pipboy = &mut *pipboy;
    // Scripts can take the Pip-Boy away (`DisablePlayerControls`).
    let allowed = !state.0.controls_off[world::scripting::controls::PIPBOY];
    let free = !menus.others_open() && conversation.0.is_none();

    if pipboy.open {
        // The Pip-Boy control again puts it away.
        if keys.just_pressed(KeyCode::Tab) {
            close(
                &mut commands,
                pipboy,
                &mut menus,
                &mut player,
                &conversation,
                now,
            );
            sound(order, &mut requests, "UIPipBoyAccessDown");
            keys.reset(KeyCode::Tab);
            return;
        }
    } else {
        // Tab (control 14, `00a24b70`'s default): held past
        // `fPlayerPipBoyLightTimer`, the light; let go sooner, the Pip-Boy
        // (`009673d0`).
        if keys.just_pressed(KeyCode::Tab) && free && allowed {
            pipboy.tab_down = Some(now);
            pipboy.held_for_light = false;
        }
        if let Some(down) = pipboy.tab_down {
            if !pipboy.held_for_light && now - down >= LIGHT_HOLD_SECONDS {
                pipboy.held_for_light = true;
                pipboy.light = !pipboy.light;
                sound(
                    order,
                    &mut requests,
                    if pipboy.light {
                        "UIPipBoyLightOn"
                    } else {
                        "UIPipBoyLightOff"
                    },
                );
                println!(
                    "The Pip-Boy light is {}.",
                    if pipboy.light { "on" } else { "off" }
                );
            }
            if keys.just_released(KeyCode::Tab) || !keys.pressed(KeyCode::Tab) {
                pipboy.tab_down = None;
                if !pipboy.held_for_light
                    && free
                    && allowed
                    && open(pipboy, &mut menus, &mut player, now, false)
                {
                    sound(order, &mut requests, "UIPipBoyAccessUp");
                }
            }
        }
        if !pipboy.open {
            return;
        }
    }
    let pressed = menu_keys(&keys);
    // The keyboard is the Pip-Boy's while it's up (Escape left to the
    // viewer).
    let held: Vec<KeyCode> = keys
        .get_pressed()
        .copied()
        .filter(|&k| k != KeyCode::Escape)
        .collect();
    for k in held {
        keys.reset(k);
    }
    let (Some(b), Some(input)) = (pipboy.built.as_mut(), pipboy.input.clone()) else {
        return;
    };
    let mut actions = Vec::new();
    for key in pressed {
        let before = (
            b.pipboy.section,
            b.pipboy.stats.page,
            b.pipboy.items.tab,
            b.pipboy.data.tab,
        );
        actions.extend(b.pipboy.key(&mut b.ui, key, &input));
        let after = (
            b.pipboy.section,
            b.pipboy.stats.page,
            b.pipboy.items.tab,
            b.pipboy.data.tab,
        );
        if before != after {
            if let Some(fx) = pipboy.effects.as_mut() {
                fx.tab_changed(now * 1000.0);
            }
        }
    }
    let state = &mut state.0;
    let mut say = |text: String| {
        if text.is_empty() {
            return;
        }
        println!("{text}");
        if messages.on {
            messages.queue.push(text);
        }
    };
    for action in actions {
        match action {
            Action::Sound(name) => sound(order, &mut requests, &name),
            Action::Equip(form) => {
                let item = FormId(form);
                if state.is_equipped(PLAYER_REF, item) {
                    state.unequip(PLAYER_REF, item);
                } else {
                    state.equip(order, PLAYER_REF, item);
                }
            }
            Action::Use(form) => {
                let item = FormId(form);
                let kind = order.get(item).map(|r| *r.entry.header.kind.as_bytes());
                let said = match kind {
                    Some(k) if &k == b"BOOK" => world::items::read_book(order, state, item),
                    _ => world::items::use_item(order, state, PLAYER_REF, item),
                };
                say(said.unwrap_or_default());
            }
            Action::Travel(reference) => {
                let Some(m) = markers
                    .list
                    .iter()
                    .find(|m| m.reference.0 == reference)
                    .cloned()
                else {
                    continue;
                };
                match world::map::travel(order, state, &m) {
                    Ok(_) => {
                        say(format!("Travelling to {}.", m.name));
                        close(
                            &mut commands,
                            pipboy,
                            &mut menus,
                            &mut player,
                            &conversation,
                            now,
                        );
                    }
                    Err(why) => say(why),
                }
            }
            Action::ActiveQuest(form) => state.active_quest = Some(FormId(form)),
            // A voice note: its speaker (`SNAM`) says its topic (`TNAM`),
            // line and result scripts as when a script has them `SayTo`.
            Action::PlayNote(form) => {
                let Some((speaker, topic)) = voice_note(order, FormId(form)) else {
                    continue;
                };
                // Whoever of that kind is here says it (the game's own
                // rules for a note's speaker aren't traced).
                let Some(who) = talkers.0.iter().find(|t| t.base == speaker) else {
                    say("There's no one here to play it for.".into());
                    continue;
                };
                state.events.push(world::scripting::Event::Talk {
                    speaker: who.reference,
                    to: PLAYER_REF,
                    topic: Some(topic),
                    conversation: false,
                });
            }
        }
    }
}
/// A voice note's speaker (`SNAM`) and topic (`TNAM`); `None` for other
/// notes (`DATA` 3 is a voice note).
fn voice_note(order: &esm::LoadOrder, note: FormId) -> Option<(FormId, FormId)> {
    let rr = order.get(note)?;
    let record = rr.record().ok()?;
    if record.get(esm::FourCC::new(b"DATA"))?.data.first() != Some(&3) {
        return None;
    }
    let form = |tag: &[u8; 4]| {
        let s = record.get(esm::FourCC::new(tag))?;
        let raw = u32::from_le_bytes(s.data.get(..4)?.try_into().ok()?);
        Some(rr.plugin.to_global(FormId(raw)))
    };
    Some((form(b"SNAM")?, form(b"TNAM")?))
}

/// Puts it up; false when the menus can't be read.
fn open(
    pipboy: &mut Pipboy,
    menus: &mut Menus,
    player: &mut Player,
    now: f32,
    at_once: bool,
) -> bool {
    if pipboy.failed {
        return false;
    }
    pipboy.open = true;
    pipboy.since = Some(now);
    pipboy.at_once = at_once;
    menus.pipboy = true;
    player.ready = false;
    if let Some(fx) = pipboy.effects.as_mut() {
        fx.open(if at_once { -1.0e6 } else { now * 1000.0 });
    }
    true
}

/// Puts it away (the arm goes down, then hides).
fn close(
    commands: &mut Commands,
    pipboy: &mut Pipboy,
    menus: &mut Menus,
    player: &mut Player,
    conversation: &crate::dialogue::Conversation,
    now: f32,
) {
    pipboy.open = false;
    pipboy.since = Some(now);
    pipboy.at_once = false;
    menus.pipboy = false;
    if !menus.others_open() && conversation.0.is_none() {
        player.ready = true;
    }
    if let Some(e) = pipboy.hum.take() {
        if let Ok(mut e) = commands.get_entity(e) {
            e.despawn();
        }
    }
}

/// Where the player is, for the DATA menu: the place's name (`00578870`:
/// the cell's own name; outdoors in a cell without one the game names the
/// place from the worldspace by the player's position, `TESWorldSpace`
/// slot 0x138, not traced: left empty), the world map of the worldspace
/// the player last walked in (`MapMarkers`; none before going outdoors:
/// which map the game shows then isn't traced) with the player on it when
/// outdoors there.
fn whereabouts(
    order: &esm::LoadOrder,
    state: &world::scripting::GameState,
    markers: &crate::map::MapMarkers,
    heading: f32,
) -> ui::pipboy::gather::Whereabouts {
    let location = state
        .player_cell
        .and_then(|id| order.get(id))
        .and_then(|r| r.record().ok())
        .and_then(|r| r.full_name())
        .unwrap_or_default();
    let world = markers.world;
    let outdoors = state.player_world.is_some() && state.player_world == world;
    ui::pipboy::gather::Whereabouts {
        location,
        world,
        markers: markers.list.clone(),
        player: state
            .player_position
            .filter(|_| outdoors)
            .map(|p| (p, heading)),
    }
}
/// What drawing the picture needs.
/// (Pictures, meshes and the arm's materials come through the `Spawner`.)
#[derive(bevy::ecs::system::SystemParam)]
pub struct Drawing<'w> {
    tiles: ResMut<'w, Assets<TileMaterial>>,
    screens: ResMut<'w, Assets<ScreenMaterial>>,
}

/// Every frame while it's up: the menus filled from the game's state, the
/// picture's pieces, the screen effect's values, the arm.
#[allow(clippy::too_many_arguments)]
fn update_pipboy(
    mut commands: Commands,
    mut pipboy: ResMut<Pipboy>,
    start: Option<ResMut<StartPipboy>>,
    around: Around,
    drawing: Drawing,
    mut spawner: Spawner,
    mut cameras: Query<&mut Camera, With<PipboyCamera>>,
    views: Query<(Entity, &Transform, &Projection), With<FlyCamera>>,
    mut transforms: Query<&mut Transform, (Without<FlyCamera>, Without<PipboyCamera>)>,
    mut visibility: Query<&mut Visibility>,
    piece_materials: Query<&MeshMaterial3d<GameLitMaterial>>,
) {
    let Around {
        time,
        game,
        state,
        mut menus,
        mut player,
        mut wavs,
        markers,
        ..
    } = around;
    let Drawing {
        mut tiles,
        mut screens,
    } = drawing;
    let order = &game.0.order;
    let now = time.elapsed_secs();
    let pipboy = &mut *pipboy;

    // `--pipboy`: up at once once the place is.
    if let Some(mut start) = start {
        if player.ready && !pipboy.open {
            if let Some(which) = start.0.take() {
                if open(pipboy, &mut menus, &mut player, now, true) {
                    pipboy.pending = Some(parse_start(&which));
                }
            }
        }
    }

    // Where the arm is in its raising animation (none: down and away).
    let (start, hit, stop) = pipboy
        .arm
        .as_ref()
        .and_then(|a| a.raise.as_ref())
        .map_or((0.0, 0.33, 0.73), |r| {
            (r.sequence.start, r.hit, r.sequence.stop)
        });
    let since = pipboy
        .since
        .filter(|_| !pipboy.at_once)
        .map(|s| (now - s).max(0.0));
    let raise_at = raise_time(start, hit, stop, pipboy.open, since);
    let shown = pipboy.open || raise_at.is_some();
    for mut camera in &mut cameras {
        if camera.is_active != shown {
            camera.is_active = shown;
        }
    }
    if let Some(arm) = pipboy.arm.as_ref() {
        if let Ok(mut v) = visibility.get_mut(arm.holder) {
            let want = if shown {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            if *v != want {
                *v = want;
            }
        }
        // The light's cone (`PipboyLightEffect`) shown with the light on
        // (`007fa310`); of the three lamps over STATS, ITEMS and DATA only
        // the shown menu's lit (`007f9070` finds them, `007fa010` hides
        // all three and shows one).
        let section = pipboy.built.as_ref().map(|b| b.pipboy.section);
        let lamps = arm.glows.iter().enumerate().map(|(i, &e)| {
            let on = section.is_some_and(|s| s as usize == i);
            (e, on)
        });
        let cones = arm.light_effect.iter().map(|&e| (e, pipboy.light));
        for (e, on) in cones.chain(lamps) {
            if let Ok(mut v) = visibility.get_mut(e) {
                let want = if on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *v != want {
                    *v = want;
                }
            }
        }
    }
    if !shown {
        return;
    }

    // The menus, read once.
    if pipboy.built.is_none() && !pipboy.failed {
        match build(&game.0) {
            Ok(b) => pipboy.built = Some(Box::new(b)),
            Err(e) => {
                println!("The Pip-Boy's menus can't be shown: {e}");
                pipboy.failed = true;
                pipboy.open = false;
                menus.pipboy = false;
                player.ready = true;
                return;
            }
        }
    }
    let Some(b) = pipboy.built.as_mut() else {
        return;
    };

    // The hum while it's up.
    if pipboy.open && pipboy.hum.is_none() {
        if let Some(s) = order
            .form_by_editor_id("UIPipBoyHumLP")
            .and_then(|id| world::sound::Sound::load(order, id))
        {
            pipboy.hum =
                crate::sounds::play(&mut commands, &game.0, &mut wavs, &s, state.0.dice, true);
        }
    }

    // Filled from the game's state.
    let view = views.single().ok();
    let heading = view.map_or(0.0, |(_, t, _)| {
        let f = t.forward().as_vec3();
        f.x.atan2(-f.z).to_degrees()
    });
    let at = whereabouts(order, &state.0, &markers, heading);
    let input = ui::pipboy::gather::gather(order, &state.0, &at);
    b.pipboy.fill(&mut b.ui, &input);
    if let Some((section, page)) = pipboy.pending.take() {
        b.pipboy.show(&mut b.ui, section);
        if let Some(p) = page {
            match section {
                Section::Stats => b.pipboy.stats.show_page(&mut b.ui, p, &input),
                Section::Items => b.pipboy.items.show_tab(&mut b.ui, p, &input),
                Section::Data => b.pipboy.data.show_tab(&mut b.ui, p, &input),
            }
        }
        b.ui.refresh();
    }
    pipboy.input = Some(input);

    // The picture's pieces, when something changed.
    let compressed = spawner
        .device
        .as_ref()
        .is_none_or(|d| d.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC));
    let menu = b.pipboy.menu();
    let items = {
        let mut files = Files {
            game: &game.0,
            sizes: &mut b.sizes,
            atlases: &mut b.atlases,
        };
        ui::draw::update_file_sizes(&mut b.ui, menu, &mut files);
        ui::draw_list(&mut b.ui, menu, &mut files, &|_| None)
    };
    if items != b.last {
        for (e, mesh, material) in b.drawn.drain(..) {
            commands.entity(e).despawn();
            spawner.meshes.remove(&mesh);
            tiles.remove(&material);
        }
        let Some(white) = pipboy.white.clone() else {
            return;
        };
        for (i, item) in items.iter().enumerate() {
            let tint = Vec4::from_array(item.color);
            let mut pieces: Vec<(Handle<Image>, Vec<Quad>)> = Vec::new();
            match &item.kind {
                DrawKind::Image {
                    texture,
                    rect,
                    uv,
                    repeat_u,
                    ..
                } => {
                    let repeat = (*repeat_u, false);
                    let key = (texture.clone(), repeat.0, repeat.1);
                    let handle = b
                        .images
                        .entry(key)
                        .or_insert_with(|| {
                            crate::hud::upload_picture(
                                &mut spawner.images,
                                &game.0,
                                texture,
                                repeat,
                                compressed,
                            )
                        })
                        .clone();
                    if let Some(handle) = handle {
                        let corners = [
                            [uv[0], uv[1]],
                            [uv[2], uv[1]],
                            [uv[0], uv[3]],
                            [uv[2], uv[3]],
                        ];
                        pieces.push((handle, vec![(*rect, corners)]));
                    }
                }
                DrawKind::Text { font, glyphs } => {
                    let Some(f) = b.ui.fonts.get(font - 1).cloned().flatten() else {
                        continue;
                    };
                    let paths = ui::draw::font_textures(&f);
                    let mut by_picture: HashMap<u32, Vec<Quad>> = HashMap::new();
                    for (rect, uv, picture) in glyphs {
                        by_picture.entry(*picture).or_default().push((*rect, *uv));
                    }
                    let mut pictures: Vec<_> = by_picture.into_iter().collect();
                    pictures.sort_by_key(|(p, _)| *p);
                    for (picture, quads) in pictures {
                        let Some(path) = paths.get(picture as usize) else {
                            continue;
                        };
                        let handle = b
                            .font_images
                            .entry((*font, picture))
                            .or_insert_with(|| {
                                crate::hud::upload_font_picture(&mut spawner.images, &game.0, path)
                            })
                            .clone();
                        if let Some(texture) = handle {
                            pieces.push((texture, quads));
                        }
                    }
                }
            }
            for (texture, quads) in pieces {
                // One menu unit a pixel.
                let mesh = spawner
                    .meshes
                    .add(crate::hud::quads_mesh(&quads, 1.0, PICTURE));
                let material = tiles.add(TileMaterial::plain(tint, texture, white.clone()));
                let entity = commands
                    .spawn((
                        Mesh2d(mesh.clone()),
                        MeshMaterial2d(material.clone()),
                        Transform::from_xyz(0.0, 0.0, i as f32 * 0.01),
                        RenderLayers::layer(MENU_LAYER),
                    ))
                    .id();
                b.drawn.push((entity, mesh, material));
            }
        }
        b.last = items;
    }

    // The screen effect's values this frame.
    if let (Some(fx), Some(handle)) = (pipboy.effects.as_mut(), pipboy.material.as_ref()) {
        let p = fx.params(now * 1000.0);
        let tint = pipboy_colour(&game.0);
        if let Some(m) = screens.get_mut(handle) {
            m.u = ScreenUniform {
                params: Vec4::new(p.blur_intensity, p.scroll, 1.0, p.scanline_frequency),
                distort: p.distort.map_or(Vec4::ZERO, |(v, progress, h)| {
                    Vec4::new(v, progress, h, 0.0)
                }),
                tint,
                offsets: Vec4::new(
                    p.blur_radius / PICTURE.x as f32,
                    p.blur_radius / PICTURE.y as f32,
                    0.0,
                    0.0,
                ),
            };
        }
    }

    // The arm.
    let Some((camera, camera_transform, projection)) = view else {
        return;
    };
    let Some(lighting) = spawner.place_lighting.get() else {
        return;
    };
    let lighting_changes = spawner.place_lighting.changes();
    let female = state.0.player_female.unwrap_or(false);
    let worn: Vec<FormId> = state
        .0
        .equipped
        .get(&PLAYER_REF)
        .into_iter()
        .flatten()
        .copied()
        .filter(|&i| {
            order
                .get(i)
                .is_some_and(|r| r.entry.header.kind.as_bytes() == b"ARMO")
        })
        .collect();
    // The weapon in hand, as the ordinary first-person view holds it.
    let weapon = world::combat::weapon_in_hand(order, &state.0, PLAYER_REF).and_then(|w| {
        let model = order
            .get(w.form_id)?
            .record()
            .ok()?
            .get(esm::FourCC::new(b"MODL"))?
            .zstring();
        Some((w.form_id, model, w.animation))
    });
    let weapon_id = weapon.as_ref().map(|(id, _, _)| *id);
    let rebuild = pipboy.arm.as_ref().is_none_or(|a| {
        a.worn != worn
            || a.female != female
            || a.lighting != lighting_changes
            || a.weapon != weapon_id
    });
    if rebuild {
        if let Some(old) = pipboy.arm.take() {
            if let Ok(mut e) = commands.get_entity(old.holder) {
                e.despawn();
            }
        }
        pipboy.arm = build_arm(
            &mut commands,
            &game.0,
            &mut spawner,
            camera,
            lighting,
            lighting_changes,
            &worn,
            female,
            weapon,
        );
    }
    let Some(arm) = pipboy.arm.as_mut() else {
        return;
    };
    // The screen piece takes the screen's picture (once its material is
    // there).
    if let (Some(piece), Some(screen)) = (arm.screen, pipboy.screen.as_ref()) {
        if let Ok(handle) = piece_materials.get(piece) {
            if let Some(m) = spawner.lit_materials.get_mut(&handle.0) {
                m.base.base_color_texture = Some(screen.clone());
            }
            arm.screen = None;
        }
    }
    pose_arm(arm, raise_at, camera_transform, projection, &mut transforms);
}

/// `--pipboy`'s value: `stats`, `items` or `data`, with `:PAGE` (the stats
/// page or the tab, from 0).
fn parse_start(which: &str) -> (Section, Option<usize>) {
    let (section, page) = match which.split_once(':') {
        Some((s, p)) => (s, p.trim().parse::<usize>().ok()),
        None => (which, None),
    };
    let section = match section.trim().to_ascii_lowercase().as_str() {
        "items" => Section::Items,
        "data" => Section::Data,
        _ => Section::Stats,
    };
    (section, page)
}

/// `uPipboyColor` (`[Interface]`; 4290134783 in this install's
/// `FalloutPrefs.ini`), 0 to 1.
fn pipboy_colour(game: &cellview::Game) -> Vec4 {
    colour_of(
        game.settings
            .get("Interface", "uPipboyColor")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0xFFB6_42FF),
    )
}

/// A colour setting's value as red, green, blue, alpha bytes from the top
/// (0xFFB642FF: 255, 182, 66); alpha taken as 1.
fn colour_of(v: u32) -> Vec4 {
    let byte = |shift: u32| ((v >> shift) & 0xFF) as f32 / 255.0;
    Vec4::new(byte(24), byte(16), byte(8), 1.0)
}

/// The first-person model with the Pip-Boy: the first-person look (the
/// clothes worn, the hands, the weapon in hand, world::actor::
/// first_person_look) with the Pip-Boy and its glove (first-person version)
/// worn, as the player always wears them.
#[allow(clippy::too_many_arguments)]
fn build_arm(
    commands: &mut Commands,
    game: &cellview::Game,
    spawner: &mut Spawner,
    camera: Entity,
    lighting: crate::lighting::GameLighting,
    lighting_changes: u64,
    worn: &[FormId],
    female: bool,
    weapon: Option<(FormId, String, u32)>,
) -> Option<Arm> {
    let order = &game.order;
    let mut wearing: Vec<FormId> = worn.to_vec();
    let glove = order.form_by_editor_id(GLOVE_ITEM);
    for item in [glove, order.form_by_editor_id(PIPBOY_ITEM)]
        .into_iter()
        .flatten()
    {
        if !wearing.contains(&item) {
            wearing.push(item);
        }
    }
    let held = weapon
        .as_ref()
        .map(|(_, model, animation)| (model.clone(), *animation));
    let mut look = world::actor::first_person_look(order, female, &wearing, held)?;
    // The Pip-Boy's model is made in its bone's own axes (the forearm
    // along x from 4 to 17 units), the bone its top node's `Prn` names
    // (`Bip01 L ForeTwist`): held there as a weapon is at `Weapon`.
    if let Some(pipboy) = order
        .form_by_editor_id(PIPBOY_ITEM)
        .and_then(|p| world::actor::Armor::load(order, p))
    {
        let models = [pipboy.male.clone(), pipboy.female.clone()];
        for part in &mut look.parts {
            if models.iter().flatten().any(|m| *m == part.model) {
                part.bone = attach_bone(game, &part.model);
            }
        }
    }
    // The glove's first-person version (`LeftHandPipboyGlove1st.nif`).
    if let Some(glove) = glove.and_then(|g| world::actor::Armor::load(order, g)) {
        let models = [glove.male.clone(), glove.female.clone()];
        for part in &mut look.parts {
            if models.iter().flatten().any(|m| *m == part.model)
                && !part.model.to_ascii_lowercase().contains("1st.")
            {
                part.model = world::actor::first_person_hand(&part.model);
            }
        }
    }
    let scene = game.actor_scene(&look);
    let holder = commands
        .spawn((Transform::IDENTITY, Visibility::Hidden, ChildOf(camera)))
        .id();
    let (root, joints, pieces) = spawner.spawn_lone_actor(&scene, lighting, holder)?;
    // The pieces in the order spawned (the actor's draws with a rig), told
    // apart by their texture: `pipboyscreen:0` is the unlit one showing
    // `Pipboy3000\Screen.dds` (the lit `ScreenLit:8` lies just behind
    // it), `PipboyLightEffect:0` the unlit `effects\FXWHITE.dds` cone.
    let kinds: Vec<(String, bool)> = scene
        .draws
        .iter()
        .map(|d| &scene.meshes[d.mesh])
        .filter(|m| m.rig.is_some())
        .map(|m| {
            let texture = m
                .material
                .texture
                .and_then(|i| scene.textures.get(i))
                .map(|t| t.path.to_ascii_lowercase())
                .unwrap_or_default();
            (texture, m.material.unlit)
        })
        .collect();
    let mut screen = None;
    let mut light_effect = Vec::new();
    let mut glows = Vec::new();
    for ((texture, unlit), &piece) in kinds.iter().zip(&pieces) {
        if texture.ends_with("pipboy3000\\screen.dds") && *unlit {
            screen = Some(piece);
        } else if texture.ends_with("effects\\fxwhite.dds") && *unlit {
            light_effect.push(piece);
        } else if texture.ends_with("pipboybtnglow01.dds") {
            // `StatsGlow`, `ItemsGlow`, `DataGlow`, in the file's order.
            glows.push(piece);
        }
    }
    let skeleton = scene.actors.first()?.skeleton.clone();
    let find = |name: &str| {
        skeleton
            .bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(name))
    };
    let looking = find("Bip01 Looking")?;
    let camera_bone = find("Camera1st")?;
    let turned = (0..skeleton.bones.len())
        .filter(|&i| {
            let mut b = Some(i);
            while let Some(j) = b {
                if j == looking {
                    return true;
                }
                b = skeleton.bones[j].parent;
            }
            false
        })
        .collect();
    let raise = raise_animation(game, if female { RAISE_FEMALE } else { RAISE_MALE })
        .or_else(|| raise_animation(game, RAISE_MALE));
    // Said once (outdoors the arm is built again as squares load).
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        println!(
            "  Pip-Boy: {}; {}",
            look.parts
                .iter()
                .map(|p| p.model.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            match (&raise, screen.is_some()) {
                (Some(r), true) => format!(
                    "raised by {} (held at {:.2} s of {:.2})",
                    r.sequence.name,
                    r.hit - r.sequence.start,
                    r.sequence.stop - r.sequence.start
                ),
                (None, _) => "no raising animation found".into(),
                (_, false) => "no screen found on the model".into(),
            }
        )
    });
    Some(Arm {
        worn: worn.to_vec(),
        weapon: weapon.map(|(id, _, _)| id),
        female,
        lighting: lighting_changes,
        holder,
        root,
        joints,
        skeleton,
        turned,
        looking,
        camera: camera_bone,
        raise,
        screen,
        light_effect,
        glows,
    })
}

/// The bone a model's top node names in its `Prn` (`nif::Nif::attach_bone`).
fn attach_bone(game: &cellview::Game, model: &str) -> Option<String> {
    let bytes = game.assets.read(&assets::mesh_path(model)).ok()??;
    nif::Nif::parse(bytes).ok()?.attach_bone()
}

/// The raising animation and the moment it holds at while the Pip-Boy is
/// up: its `Hit` text key (0.33 s of 0.73; the arm is highest there),
/// else halfway (a guess at how the game uses the key: it plays to it on
/// opening and on from it to the end on closing).
fn raise_animation(game: &cellview::Game, path: &str) -> Option<Raise> {
    let bytes = game.assets.read(&assets::mesh_path(path)).ok()??;
    let nif = nif::Nif::parse(bytes).ok()?;
    let sequence = nif.sequences().ok()?.into_iter().next()?;
    let hit = nif
        .text_keys()
        .ok()
        .and_then(|keys| {
            keys.into_iter()
                .find(|(_, k)| k.eq_ignore_ascii_case("hit"))
                .map(|(t, _)| t)
        })
        .unwrap_or((sequence.start + sequence.stop) / 2.0)
        .clamp(sequence.start, sequence.stop);
    Some(Raise { sequence, hit })
}

/// The raising animation (see [`raise_animation`]).
struct Raise {
    sequence: nif::Sequence,
    hit: f32,
}

/// Where in the raising animation the arm is: going up from the start to
/// `Hit` over that many seconds after opening, held there, and on from
/// `Hit` to the end after closing. `since` is how long ago it was opened
/// or closed (`None`: never).
fn raise_time(start: f32, hit: f32, stop: f32, open: bool, since: Option<f32>) -> Option<f32> {
    match (open, since) {
        (true, Some(t)) => Some((start + t).min(hit)),
        (true, None) => Some(hit),
        (false, Some(t)) if hit + t < stop => Some(hit + t),
        _ => None,
    }
}

/// Poses the arm: the hold pose with the raising animation at `raise_at`
/// over it, turned by the view's pitch about `Bip01 Looking`, with
/// `Camera1st` at the eye; drawn with the Pip-Boy's field of view.
fn pose_arm(
    arm: &Arm,
    raise_at: Option<f32>,
    camera_transform: &Transform,
    projection: &Projection,
    transforms: &mut Query<&mut Transform, (Without<FlyCamera>, Without<PipboyCamera>)>,
) {
    let world_fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => cellview::vertical_fov(cellview::GAME_FOV_DEGREES),
    };
    let own_fov = cellview::vertical_fov(PIPBOY_FOV_DEGREES);
    let k = (world_fov * 0.5).tan() / (own_fov * 0.5).tan();
    if let Ok(mut t) = transforms.get_mut(arm.holder) {
        *t = Transform::from_scale(Vec3::new(k, k, 1.0));
    }
    let f = camera_transform.forward().as_vec3();
    let dir = [f.x, -f.z, f.y];
    let heading = dir[0].atan2(dir[1]);
    let pitch = dir[2].clamp(-1.0, 1.0).asin();
    let mut layers: Vec<(&nif::Sequence, f32)> = Vec::new();
    if let Some(s) = arm.skeleton.idle.as_ref() {
        layers.push((s, s.start));
    }
    if let (Some(r), Some(at)) = (arm.raise.as_ref(), raise_at) {
        layers.push((&r.sequence, at));
    }
    let mut pose = nif::posed_layers(&arm.skeleton.bones, &layers);
    let pivot = pose[arm.looking].translation;
    let (s, c) = pitch.sin_cos();
    let turn = nif::Transform {
        rotation: [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        translation: [
            0.0,
            pivot[1] - (c * pivot[1] - s * pivot[2]),
            pivot[2] - (s * pivot[1] + c * pivot[2]),
        ],
        scale: 1.0,
    };
    for &i in &arm.turned {
        pose[i] = turn.then_child(&pose[i]);
    }
    for (joint, bone) in arm.joints.iter().zip(&pose) {
        if let Ok(mut t) = transforms.get_mut(*joint) {
            *t = crate::actors::bevy_transform(bone);
        }
    }
    let eye = game_point(camera_transform.translation);
    let (sh, ch) = heading.sin_cos();
    let cam = pose[arm.camera].translation;
    let feet = [
        eye[0] - (ch * cam[0] + sh * cam[1]),
        eye[1] - (-sh * cam[0] + ch * cam[1]),
        eye[2] - cam[2],
    ];
    let game = [
        ch, -sh, 0.0, 0.0, sh, ch, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, feet[0], feet[1], feet[2], 1.0,
    ];
    let world_root = Mat4::from_cols_array(&space::matrix(&game));
    let local = camera_transform.compute_matrix().inverse() * world_root;
    if let Ok(mut t) = transforms.get_mut(arm.root) {
        *t = Transform::from_matrix(local);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipboy_colour_is_read_as_red_green_blue_alpha() {
        // This install's 4290134783 = 0xFFB642FF: 255, 182, 66.
        let c = colour_of(4_290_134_783);
        assert_eq!(
            [c.x, c.y, c.z].map(|c| (c * 255.0).round() as u8),
            [255, 182, 66]
        );
        assert_eq!(c.w, 1.0);
    }

    #[test]
    fn the_start_option_names_a_menu_and_a_page() {
        assert_eq!(parse_start("items:1"), (Section::Items, Some(1)));
        assert_eq!(parse_start("DATA"), (Section::Data, None));
        assert_eq!(parse_start("stats:4"), (Section::Stats, Some(4)));
    }
}
