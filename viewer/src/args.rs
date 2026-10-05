//! Command-line arguments. Kept free of Bevy so it can be tested on its own.

use std::path::PathBuf;

pub const USAGE: &str = "\
nv-viewer - walk around a Fallout: New Vegas cell

USAGE:
    nv-viewer <DATA FOLDER> <CELL> [OPTIONS]

    DATA FOLDER  the game's Data folder, or the install folder containing it
    CELL         an interior cell's editor ID, form ID or name, e.g.
                 GSDocMitchellHouse (nvinspect's `cells` command lists them);
                 an outdoor cell (Goodsprings); or a worldspace
                 (WastelandNV, with --at to say where; `nvinspect worlds`
                 lists them). Outdoors, the cells around the player load
                 as they walk.

OPTIONS:
    --official              load only FalloutNV.esm and the official DLC
    --plugins FILE          read the active plugin list from FILE
    --ini FILE              read the archive list (SArchiveList) from FILE
    --brightness F          scale every light (default 1)
    --keep-root-transforms  place models as model viewers do (not the game)
    --at X,Y,Z,HEADING[,PITCH]
                            start standing here instead: the numbers the
                            game's console gives for player.getpos x, y, z
                            and player.getangle z (and x, for looking up or
                            down), to line up with an in-game screenshot
    --screenshot FILE       open at 1920x1080, save a PNG of the view to
                            FILE once everything has loaded, then quit
    --wait SECONDS          with --screenshot, let the game run this long
                            first (people walking, scripts)
    --walk                  walk even with --screenshot (which otherwise
                            flies, keeping the exact eye position given)
    --fps                   print the frame rate every two seconds
    --talk                  start talking to the nearest person once loaded
    --stage QUEST STAGE     set a quest's stage once loaded, as a script
                            would (VCG01 0 starts Doc Mitchell's intro)
    --new-game              start the game: the opening quest (VCG00) from
                            its first stage (movie playback is not yet
                            implemented); scripts take you to Doc's house
                            (the CELL can then be left out)
    --character FILE        start as a ready-made test character: a file
                            of the game's script lines (editor IDs) run
                            on the new game before its first frame, plus
                            `level N` (see characters/README.md)
    --weapon ID             start with this weapon (editor ID or form ID)
                            equipped and 50 rounds for it; screenshots
                            then show it in your hands
    --weather-region ID     the player's weather region (editor ID or form
                            ID), as a saved game would have it; it colours
                            glows without an Emittance of their own.
                            Starting indoors otherwise acts as if you came
                            in through the place's door from outside
    --run \"COMMAND\"         once loaded, run a line of the game's script
                            language, as its console does (for example
                            \"SunnyREF.StartCombat player\"); can be given
                            more than once
    --no-hud                leave out the game's HUD (health, compass,
                            crosshair, messages); screenshots then show
                            the scene alone
    --freeze-ai             people and creatures stand where they are and
                            do nothing but their idle, as after the game's
                            console command tai (for lining up with a
                            recording made that way)
    --pipboy MENU[:PAGE]    once loaded, raise the Pip-Boy on this menu
                            (stats, items or data) and page or tab (from
                            0: stats:1 is S.P.E.C.I.A.L., data:2 the
                            quests); screenshots then show it
    --vats [N]              for testing: open V.A.T.S. three seconds after
                            loading (as V does); with N, queue N attacks on
                            the part it opens on and play them
    --lockpick REF          for testing: once loaded, try the lock on this
                            placed door or container (editor ID or form
                            ID) as E on it would: the lockpicking menu
    --cloud-time SECONDS    hold the clouds where this many seconds of
                            drift put them, to line up with a recording of
                            the game (Goodsprings' reference: 58.022)
    --open-menu MENU[:ID]   for testing: once loaded, open one of the game's
                            menus as the game would: container:REF (a
                            container or a body), barter:REF (a merchant),
                            quantity:N (how many, up to N), levelup (the
                            player goes up a level; levelup:perks also
                            gives the points and goes on to the perks),
                            wait or sleep (the sleep/wait menu; wait:N or
                            sleep:N also chooses N hours and presses Wait)
    --menu-pointer X,Y      for testing: put the menus' pointer at this
                            pixel (screenshots have no mouse)

CONTROLS:
    right mouse button + move      look around (flying: either button)
    left mouse button              attack (walking)
    V                              V.A.T.S.: Left/Right target, Up/Down
                                   part, Enter or E queue an attack, X a
                                   special one, Backspace undo, Space or
                                   R play the queue, V/Esc/Tab leave
    W A S D                        move
    F                              walk (with collision) / fly
    walking:  Shift walk slowly, Ctrl or C sneak, Space jump, R reload
    flying:   Space / Ctrl (or Q) up, down; Shift faster; mouse
              wheel changes speed
    E                              go through the load door in view,
                                   talk to the person in view, take
                                   an item, open a container, search
                                   a body, or use a machine
    picking a lock: move the mouse to place the pick, W A S D (the
              game's movement controls) turn the lock, F force it,
              E leave
    1-9, Space, Tab                choose, skip a line, end talking
    menus: arrows, Space, Enter    move, pick, accept
    Tab                            the Pip-Boy (held: its light)
    Pip-Boy: arrows, Enter         move, equip / use; Shift + left or
                                   right change menu; the letters
                                   press their buttons
    F5, F9                         save, load (nv-rs-quicksave.txt)
    F12                            report something that differs from
                                   the game: a picture, where you are and
                                   the state, in reports\\NNN (write what's
                                   wrong in its note.txt)
    [ and ] (or - and =)           darker, brighter
    G                              the cell's image space (color
                                   adjustment) off and on
    Home                           back to the start
    Esc                            quit
";

/// The game's opening quest ("Welcome To Fabulous New Vegas"): its first
/// stage moves the player into Doc Mitchell's house and starts his intro
/// (`VCG01` stage 0), through its own scripts.
const NEW_GAME_QUEST: &str = "VCG00";
/// Where to load first for a new game (the scripts move the player there
/// anyway).
const NEW_GAME_CELL: &str = "GSDocMitchellHouse";

/// What the viewer was asked to do.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub data: PathBuf,
    pub cell: String,
    pub options: cellview::Options,
    pub brightness: f32,
    pub at: Option<Stance>,
    pub screenshot: Option<PathBuf>,
    /// Seconds to let the game run before the screenshot.
    pub wait: f32,
    /// Walk even when taking a screenshot.
    pub walk: bool,
    /// Print the frame rate.
    pub fps: bool,
    /// Talk to the nearest person once loaded.
    pub talk: bool,
    /// A quest stage to set once loaded: the quest's editor ID and stage.
    pub stage: Option<(String, u16)>,
    /// A ready-made test character to start as (`world::character`).
    pub character: Option<PathBuf>,
    /// A weapon to start with, equipped.
    pub weapon: Option<String>,
    /// Script lines to run once loaded, as console commands.
    pub run: Vec<String>,
    /// The player's weather region to start with.
    pub weather_region: Option<String>,
    /// Draw the game's HUD.
    pub hud: bool,
    /// `--vats [N]`: open V.A.T.S. once loaded, and queue and play N
    /// attacks.
    pub vats: Option<u32>,
    /// Seconds of cloud drift to hold the clouds at.
    pub cloud_time: Option<f32>,
    /// Hold everyone's AI still (the console's `tai`).
    pub freeze_ai: bool,
    /// Raise the Pip-Boy once loaded: its menu and page (stats:1).
    pub pipboy: Option<String>,
    /// `--lockpick REF`: try this lock once loaded.
    pub lockpick: Option<String>,
    /// `--open-menu`: a game menu to open once loaded (`name[:id]`).
    pub open_menu: Option<String>,
    /// `--menu-pointer`: the menus' pointer at this pixel.
    pub menu_pointer: Option<(f32, f32)>,
}

/// Where to stand, in the game's terms: feet position in game units, and
/// the console's angles in degrees (heading clockwise from north; pitch
/// positive looking down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stance {
    pub feet: [f32; 3],
    pub heading: f32,
    pub pitch: f32,
}

fn parse_stance(v: &str) -> Result<Stance, String> {
    let numbers: Vec<f32> = v
        .split(',')
        .map(|n| n.trim().parse::<f32>().ok().filter(|f| f.is_finite()))
        .collect::<Option<_>>()
        .ok_or_else(|| format!("--at expects numbers separated by commas, got '{v}'"))?;
    match numbers[..] {
        [x, y, z, heading] => Ok(Stance {
            feet: [x, y, z],
            heading,
            pitch: 0.0,
        }),
        [x, y, z, heading, pitch] => Ok(Stance {
            feet: [x, y, z],
            heading,
            pitch,
        }),
        _ => Err(format!(
            "--at expects X,Y,Z,HEADING or X,Y,Z,HEADING,PITCH, got '{v}'"
        )),
    }
}

/// Parses the arguments after the program name. `Err` carries a message
/// for the user; `Ok(None)` means help was asked for.
pub fn parse(args: &[String]) -> Result<Option<Args>, String> {
    let mut positional = Vec::new();
    let mut options = cellview::Options::default();
    let mut brightness = 1.0;
    let mut at = None;
    let mut screenshot = None;
    let mut wait = 0.0;
    let mut walk = false;
    let mut fps = false;
    let mut talk = false;
    let mut stage = None;
    let mut new_game = false;
    let mut weapon = None;
    let mut character = None;
    let mut run = Vec::new();
    let mut weather_region = None;
    let mut hud = true;
    let mut vats = None;
    let mut cloud_time = None;
    let mut freeze_ai = false;
    let mut pipboy = None;
    let mut lockpick = None;
    let mut open_menu = None;
    let mut menu_pointer = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--official" => options.official = true,
            "--keep-root-transforms" => options.keep_root_transforms = true,
            "--plugins" => options.plugins_txt = Some(value("--plugins")?.into()),
            "--ini" => options.ini = Some(value("--ini")?.into()),
            "--brightness" => {
                let v = value("--brightness")?;
                brightness = v
                    .parse::<f32>()
                    .ok()
                    .filter(|b| b.is_finite() && *b > 0.0)
                    .ok_or_else(|| format!("--brightness expects a number above 0, got '{v}'"))?;
            }
            "--at" => at = Some(parse_stance(&value("--at")?)?),
            "--screenshot" => screenshot = Some(value("--screenshot")?.into()),
            "--wait" => {
                let v = value("--wait")?;
                wait = v
                    .parse::<f32>()
                    .ok()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .ok_or_else(|| format!("--wait expects seconds, got '{v}'"))?;
            }
            "--walk" => walk = true,
            "--fps" => fps = true,
            "--talk" => talk = true,
            "--stage" => {
                let quest = value("--stage")?;
                let n = value("--stage")?;
                let n = n.parse::<u16>().map_err(|_| {
                    format!("--stage expects a quest and a stage number, got '{n}'")
                })?;
                stage = Some((quest, n));
            }
            "--new-game" => new_game = true,
            "--weapon" => weapon = Some(value("--weapon")?),
            "--character" => character = Some(value("--character")?.into()),
            "--run" => run.push(value("--run")?),
            "--weather-region" => weather_region = Some(value("--weather-region")?),
            "--no-hud" => hud = false,
            "--freeze-ai" => freeze_ai = true,
            "--lockpick" => lockpick = Some(value("--lockpick")?),
            "--pipboy" => {
                let v = value("--pipboy")?;
                let menu = v.split(':').next().unwrap_or_default().to_ascii_lowercase();
                if !["stats", "items", "data"].contains(&menu.as_str()) {
                    return Err(format!("--pipboy expects stats, items or data, got '{v}'"));
                }
                pipboy = Some(v);
            }
            "--open-menu" => open_menu = Some(value("--open-menu")?),
            "--menu-pointer" => {
                let v = value("--menu-pointer")?;
                let p: Vec<f32> = v.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                match p[..] {
                    [x, y] => menu_pointer = Some((x, y)),
                    _ => return Err(format!("--menu-pointer expects X,Y, got '{v}'")),
                }
            }
            "--cloud-time" => {
                let v = value("--cloud-time")?;
                cloud_time = Some(
                    v.parse::<f32>()
                        .ok()
                        .filter(|s| s.is_finite() && *s >= 0.0)
                        .ok_or_else(|| format!("--cloud-time expects seconds, got '{v}'"))?,
                );
            }
            "--vats" => {
                // An optional count of attacks after it.
                let n = iter.clone().next().and_then(|v| v.parse::<u32>().ok());
                if n.is_some() {
                    iter.next();
                }
                vats = Some(n.unwrap_or(0));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option '{flag}'")),
            _ => positional.push(arg.clone()),
        }
    }
    // A new game: the opening quest's first stage (its scripts move the
    // player to where the game starts), from Doc Mitchell's house unless
    // a place is given.
    if new_game {
        if stage.is_some() {
            return Err("--new-game sets the opening quest's stage; leave out --stage".into());
        }
        stage = Some((NEW_GAME_QUEST.to_string(), 0));
        if positional.len() == 1 {
            positional.push(NEW_GAME_CELL.to_string());
        }
    }
    match positional.as_slice() {
        [data, cell] => Ok(Some(Args {
            data: data.into(),
            cell: cell.clone(),
            options,
            brightness,
            at,
            screenshot,
            wait,
            walk,
            fps,
            talk,
            stage,
            character,
            weapon,
            run,
            weather_region,
            hud,
            vats,
            cloud_time,
            freeze_ai,
            pipboy,
            lockpick,
            open_menu,
            menu_pointer,
        })),
        [] | [_] => Err("expected the Data folder and a cell".into()),
        [_, _, extra, ..] => Err(format!("unexpected argument '{extra}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_folder_cell_and_options() {
        let args = parse(&strings(&[
            "C:\\Games\\FNV",
            "GSDocMitchellHouse",
            "--official",
            "--brightness",
            "1.5",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(args.data, PathBuf::from("C:\\Games\\FNV"));
        assert_eq!(args.cell, "GSDocMitchellHouse");
        assert!(args.options.official);
        assert_eq!(args.brightness, 1.5);
        assert_eq!(args.at, None);
        assert!(args.hud);
        let bare = parse(&strings(&["Data", "Cell", "--no-hud"]))
            .unwrap()
            .unwrap();
        assert!(!bare.hud);
        assert!(!bare.freeze_ai);
        let frozen = parse(&strings(&["Data", "Cell", "--freeze-ai"]))
            .unwrap()
            .unwrap();
        assert!(frozen.freeze_ai);
        let character = parse(&strings(&["Data", "Cell", "--character", "c.txt"]))
            .unwrap()
            .unwrap();
        assert_eq!(character.character, Some(PathBuf::from("c.txt")));
        assert_eq!(frozen.character, None);
    }

    #[test]
    fn parses_a_stance_and_a_screenshot() {
        let args = parse(&strings(&[
            "Data",
            "Cell",
            "--at",
            "2352,1604.5,7360,270",
            "--screenshot",
            "out.png",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            args.at,
            Some(Stance {
                feet: [2352.0, 1604.5, 7360.0],
                heading: 270.0,
                pitch: 0.0
            })
        );
        assert_eq!(args.screenshot, Some(PathBuf::from("out.png")));
        let tilted = parse(&strings(&["Data", "Cell", "--at", "1,2,3,90,-10"]))
            .unwrap()
            .unwrap();
        assert_eq!(tilted.at.unwrap().pitch, -10.0);
        assert!(parse(&strings(&["Data", "Cell", "--at", "1,2,3"]))
            .unwrap_err()
            .contains("X,Y,Z,HEADING"));
    }

    #[test]
    fn a_new_game_starts_the_opening_quest() {
        let args = parse(&strings(&["Data", "--new-game"])).unwrap().unwrap();
        assert_eq!(args.stage, Some(("VCG00".to_string(), 0)));
        assert_eq!(args.cell, "GSDocMitchellHouse");
        assert!(parse(&strings(&["Data", "--new-game", "--stage", "Q", "1"])).is_err());
    }

    #[test]
    fn parses_a_weather_region() {
        let args = parse(&strings(&[
            "Data",
            "Cell",
            "--weather-region",
            "VMapGoodspringsRegion",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            args.weather_region.as_deref(),
            Some("VMapGoodspringsRegion")
        );
        assert!(parse(&strings(&["Data", "Cell", "--weather-region"])).is_err());
    }

    #[test]
    fn parses_vats_with_or_without_a_count() {
        let args = parse(&strings(&["Data", "Cell", "--vats"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.vats, Some(0));
        let args = parse(&strings(&["Data", "Cell", "--vats", "3", "--fps"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.vats, Some(3));
        assert!(args.fps);
        // The cell can come after it.
        let args = parse(&strings(&["Data", "--vats", "Cell"]))
            .unwrap()
            .unwrap();
        assert_eq!((args.vats, args.cell.as_str()), (Some(0), "Cell"));
    }

    #[test]
    fn parses_a_lock_to_try() {
        let args = parse(&strings(&["Data", "Cell", "--lockpick", "SafeREF"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.lockpick.as_deref(), Some("SafeREF"));
        assert!(parse(&strings(&["Data", "Cell", "--lockpick"])).is_err());
    }

    #[test]
    fn parses_a_cloud_time() {
        let args = parse(&strings(&["Data", "Cell", "--cloud-time", "58.022"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.cloud_time, Some(58.022));
        assert!(parse(&strings(&["Data", "Cell", "--cloud-time", "-1"])).is_err());
    }

    #[test]
    fn parses_the_pipboy_to_raise() {
        let args = parse(&strings(&["Data", "Cell", "--pipboy", "items:1"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.pipboy.as_deref(), Some("items:1"));
        assert!(parse(&strings(&["Data", "Cell", "--pipboy", "map"]))
            .unwrap_err()
            .contains("stats, items or data"));
    }

    #[test]
    fn parses_a_menu_to_open() {
        let args = parse(&strings(&["Data", "Cell", "--open-menu", "container:Box"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.open_menu.as_deref(), Some("container:Box"));
        let args = parse(&strings(&["Data", "Cell", "--menu-pointer", "10,20.5"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.menu_pointer, Some((10.0, 20.5)));
        assert!(parse(&strings(&["Data", "Cell", "--menu-pointer", "10"])).is_err());
        assert!(parse(&strings(&["Data", "Cell", "--open-menu"])).is_err());
    }

    #[test]
    fn parses_a_quest_stage() {
        let args = parse(&strings(&["Data", "Cell", "--stage", "VCG01", "0"]))
            .unwrap()
            .unwrap();
        assert_eq!(args.stage, Some(("VCG01".to_string(), 0)));
        assert!(parse(&strings(&["Data", "Cell", "--stage", "VCG01", "x"]))
            .unwrap_err()
            .contains("stage number"));
    }

    #[test]
    fn explains_mistakes() {
        assert_eq!(parse(&strings(&["--help"])), Ok(None));
        assert!(parse(&strings(&["Data"])).unwrap_err().contains("a cell"));
        assert!(parse(&strings(&["Data", "Cell", "--brightness", "-1"]))
            .unwrap_err()
            .contains("above 0"));
        assert_eq!(
            parse(&strings(&["Data", "Cell", "--fly"])).unwrap_err(),
            "unknown option '--fly'"
        );
        assert_eq!(
            parse(&strings(&["Data", "Cell", "More"])).unwrap_err(),
            "unexpected argument 'More'"
        );
    }
}
