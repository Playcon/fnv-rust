//! Talking to people: look at someone within reach and press E. They say
//! the first greeting their conditions allow (`world::dialogue`), in their
//! own recorded voice with the text shown; then their line's follow-up
//! topics come up as numbered choices, or, after a line with none, the
//! main list (`dialogue::menu_topics`: the greeting's follow-ups and the
//! top-level and learned topics they answer). Space skips a line; Tab or
//! Esc ends the conversation.

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use cellview::{ActorData, ACTIVATE_REACH};
use esm::FormId;
use world::dialogue::{self, Choice, GameState, Info, Speaker, Topic};
use world::scripting::{Runner, ScriptCache};

use crate::scripts::{ScriptedTalk, Scripts};
use crate::walk::{game_point, CellCollision, Player, Prompt};
use crate::{FlyCamera, GameFiles};

/// The `GREETING` topic (a fixed form in every game).
const GREETING: FormId = FormId(0xC8);

/// People who can be talked to in what's loaded: reference, base, feet.
#[derive(Resource, Default)]
pub struct Talkers(pub Vec<Talker>);

#[derive(Debug, Clone, Copy)]
pub struct Talker {
    pub reference: FormId,
    pub base: FormId,
    pub position: [f32; 3],
}

impl Talker {
    pub fn from_actor(a: &ActorData) -> Talker {
        Talker {
            reference: FormId(a.reference),
            base: FormId(a.base),
            position: a.position,
        }
    }
}

/// What dialogue conditions ask about (quest stages, globals).
#[derive(Resource)]
pub struct DialogueState(pub GameState);

/// `--talk`: start talking to the nearest person once loaded.
#[derive(Resource, Default)]
pub struct AutoTalk(pub bool, pub std::collections::VecDeque<usize>);

/// The person looked at, and their name.
#[derive(Resource, Default)]
pub struct TalkTarget(pub Option<(Talker, String)>);

/// The conversation going on, if any, and every top-level topic in the
/// game (read the first time someone is talked to).
#[derive(Resource, Default)]
pub struct Conversation(pub Option<Talk>, Option<Vec<Topic>>);

pub struct Talk {
    speaker: Speaker,
    name: String,
    info: Info,
    /// The follow-ups of the line that opened the conversation: they stay
    /// in the main list (`dialogue::menu_topics`).
    opening: Vec<FormId>,
    /// The response being said, and since when (seconds).
    response: usize,
    since: f32,
    voice: Option<Entity>,
    /// Once the line is said: what the player can answer.
    choices: Option<Vec<Choice>>,
    /// Only a line said (`SayTo`): no dialogue menu, no choices after it,
    /// and the player and the game carry on meanwhile.
    line_only: bool,
    /// Said with `SayTo`, of this topic: the speaker's `SayToDone` blocks
    /// run once it's said.
    say_to: Option<FormId>,
    /// What the game's dialogue menu shows now (`game_menus::dialog`): the
    /// line and response, or the topics.
    shown_line: Option<(FormId, usize)>,
    shown_topics: bool,
}

impl Talk {
    pub fn is_line_only(&self) -> bool {
        self.line_only
    }

    /// Who's talking (their reference).
    pub fn speaker(&self) -> FormId {
        self.speaker.reference
    }
}

/// The text panel at the bottom of the screen.
#[derive(Component)]
pub struct DialogueText;

pub fn setup_dialogue_text(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.85, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(40.0),
            left: Val::Percent(15.0),
            width: Val::Percent(70.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Visibility::Hidden,
        DialogueText,
    ));
}

/// Where a ray from the eye meets an upright cylinder around a person
/// (radius 25, height 130 above their feet), if it does.
fn ray_person(eye: [f32; 3], dir: [f32; 3], feet: [f32; 3]) -> Option<f32> {
    const RADIUS: f32 = 25.0;
    const HEIGHT: f32 = 130.0;
    let (ox, oy) = (eye[0] - feet[0], eye[1] - feet[1]);
    let a = dir[0] * dir[0] + dir[1] * dir[1];
    let b = 2.0 * (ox * dir[0] + oy * dir[1]);
    let c = ox * ox + oy * oy - RADIUS * RADIUS;
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
    (0.0..=HEIGHT).contains(&z).then_some(t)
}

/// The voice file for a line when the speaker's own voice type has none:
/// the same file name under another voice type of the same plugin. Dead
/// Money's narrator is a `MaleAdult01Default` actor whose lines are all
/// recorded in Elijah's voice type folder (how the game finds them there
/// isn't traced).
fn other_voice(game: &cellview::Game, path: &str) -> Option<String> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<String, String>> =
        std::sync::OnceLock::new();
    // Keyed by the plugin and the file name, whatever the voice type.
    let key = |lower: &str| -> Option<String> {
        let (folder, name) = lower.rsplit_once('\\')?;
        let plugin = folder.rsplit_once('\\').map_or(folder, |(p, _)| p);
        Some(format!("{plugin}\\{name}"))
    };
    let index = INDEX.get_or_init(|| {
        let mut index = std::collections::HashMap::new();
        for p in game.assets.paths() {
            let lower = p.to_ascii_lowercase();
            if lower.starts_with("sound\\voice\\") && lower.ends_with(".ogg") {
                if let Some(k) = key(&lower) {
                    index.entry(k).or_insert(lower);
                }
            }
        }
        index
    });
    index.get(&key(&path.to_ascii_lowercase())?).cloned()
}

/// Plays a response's voice, if its file is found.
fn play_voice(
    commands: &mut Commands,
    audio: &mut Assets<crate::sounds::PcmSound>,
    game: &cellview::Game,
    talk: &Talk,
) -> Option<Entity> {
    let response = talk.info.responses.get(talk.response)?;
    let voice = talk.speaker.voice?;
    let path = dialogue::voice_path(&game.order, &talk.info, response, voice)?;
    let (path, bytes) = match game.assets.read(&path).ok()? {
        Some(bytes) => (path, bytes),
        None => {
            let path = other_voice(game, &path)?;
            println!("{} says this line in another voice type: {path}", talk.name);
            let bytes = game.assets.read(&path).ok()??;
            (path, bytes)
        }
    };
    let source = crate::sounds::voice_handle(&path, &bytes, audio)?;
    // With a lip sync file beside it, the voice waits out its lead-in and
    // the speaker's face says it (`faces`).
    let (settings, voice) = crate::faces::voice_playback(game, &path, talk.speaker.reference);
    Some(commands.spawn((AudioPlayer(source), settings, voice)).id())
}

/// How long a response stays up without a voice: a reading pace.
fn reading_time(text: &str) -> f32 {
    (text.split_whitespace().count() as f32 * 0.35).max(2.0)
}

/// Runs one of a line's result scripts, on the speaker (whose script
/// variables are its own).
fn run_line_script(
    order: &esm::LoadOrder,
    scripts: &ScriptCache,
    state: &mut GameState,
    source: Option<&str>,
    speaker: FormId,
) {
    if let Some(source) = source {
        let lines: Vec<&str> = source
            .lines()
            .map(|l| l.split(';').next().unwrap_or("").trim())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }
        println!("  result script: {}", lines.join(" / "));
        let flow =
            Runner::new(order, scripts, state).run_source(source, Some(speaker), Some(speaker));
        if flow == world::scripting::Flow::Stopped {
            println!("  (stopped: it needs a function that isn't carried out yet)");
        }
    }
}

/// A line starts: it's been said, the speaker has talked to the player,
/// the topics it names are learned, and its first result script runs.
fn begin_line(
    order: &esm::LoadOrder,
    scripts: &ScriptCache,
    state: &mut GameState,
    info: &Info,
    speaker: FormId,
) {
    dialogue::line_begins(state, info, speaker);
    let said: Vec<&str> = info.responses.iter().map(|r| r.text.as_str()).collect();
    println!("{} ({}): {}", speaker, info.form_id, said.join(" "));
    run_line_script(order, scripts, state, info.begin_script.as_deref(), speaker);
}

/// What `talk` uses besides: `--talk`, scripts, scripted talking, the
/// menus, and the people (who flees).
type TalkExtras<'w, 's> = (
    ResMut<'w, AutoTalk>,
    Res<'w, Scripts>,
    ResMut<'w, ScriptedTalk>,
    ResMut<'w, crate::menus::Menus>,
    Query<'w, 's, &'static crate::ai::Walker>,
    ResMut<'w, crate::game_menus::GameMenus>,
);

/// Looking for someone to talk to, and the conversation itself.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn talk(
    mut commands: Commands,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time>,
    game: Res<GameFiles>,
    mut state: ResMut<DialogueState>,
    talkers: Res<Talkers>,
    collision: Res<CellCollision>,
    mut target: ResMut<TalkTarget>,
    (mut auto_talk, scripts, mut scripted, mut menus, walkers, mut game_menus): TalkExtras<'_, '_>,
    mut conversation: ResMut<Conversation>,
    mut player: ResMut<Player>,
    mut audio: ResMut<Assets<crate::sounds::PcmSound>>,
    voices: Query<(), With<AudioPlayer<crate::sounds::PcmSound>>>,
    cameras: Query<&Transform, With<FlyCamera>>,
    mut prompt: Query<&mut Text, (With<Prompt>, Without<DialogueText>)>,
    mut panel: Query<(&mut Text, &mut Visibility), With<DialogueText>>,
) {
    let now = time.elapsed_secs();
    let order = &game.0.order;
    // Who is saying a line now, for scripts' `IsTalking`.
    let speaking = conversation
        .0
        .as_ref()
        .filter(|t| t.choices.is_none())
        .map(|t| t.speaker.reference);
    if state.0.speaking.iter().copied().ne(speaking) {
        state.0.speaking = speaking.into_iter().collect();
    }
    if conversation.0.is_some() {
        let line_only = conversation.0.as_ref().is_some_and(|t| t.line_only);
        // The game's dialogue menu shows the conversation when its files
        // can be read (`game_menus::dialog`); it has the mouse and keys.
        let mut screen = if line_only { None } else { game_menus.screen() };
        if let (Some(s), Some(talk)) = (screen.as_deref_mut(), conversation.0.as_ref()) {
            if crate::game_menus::dialog::menu(s).is_none()
                && !crate::game_menus::dialog::open(s, &game.0, &talk.name)
            {
                screen = None;
            }
        }
        let answer = screen
            .as_deref_mut()
            .and_then(crate::game_menus::dialog::take_answer);
        let ended = screen.is_none()
            && !line_only
            && (keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::Escape));
        keys.clear_just_pressed(KeyCode::Escape);
        if ended {
            end(&mut commands, &mut conversation.0, &mut player, &mut panel);
            return;
        }
        let Conversation(Some(talk), top_level) = &mut *conversation else {
            return;
        };
        let skip =
            keys.just_pressed(KeyCode::Space) || answer == Some(ui::menus::dialog::Answer::Skip);
        if talk.choices.is_none() {
            // Saying the line: next response when the voice ends (or after
            // a reading pause), or on Space.
            let voice_done = talk.voice.is_none_or(|v| voices.get(v).is_err());
            let text = talk
                .info
                .responses
                .get(talk.response)
                .map(|r| r.text.clone())
                .unwrap_or_default();
            let done = if talk.voice.is_some() {
                voice_done
            } else {
                now - talk.since > reading_time(&text)
            };
            if done || skip {
                if let Some(v) = talk.voice.take() {
                    if let Ok(mut e) = commands.get_entity(v) {
                        e.despawn();
                        // Skipped: the face stops saying it too.
                        crate::faces::cut_short(&mut commands, talk.speaker.reference);
                    }
                }
                talk.response += 1;
                talk.since = now;
                if talk.response < talk.info.responses.len() {
                    talk.voice = play_voice(&mut commands, &mut audio, &game.0, talk);
                } else {
                    // The line is said: its second result script.
                    run_line_script(
                        order,
                        &scripts.0,
                        &mut state.0,
                        talk.info.end_script.as_deref(),
                        talk.speaker.reference,
                    );
                    if talk.line_only || talk.info.flags & dialogue::GOODBYE != 0 {
                        let (speaker, said) = (talk.speaker.reference, talk.say_to);
                        if let Some(s) = screen.as_deref_mut() {
                            crate::game_menus::dialog::end(s);
                        }
                        end(&mut commands, &mut conversation.0, &mut player, &mut panel);
                        // `SayTo`'s line is said: the speaker's
                        // `SayToDone` blocks (which may start the next).
                        if let Some(topic) = said {
                            Runner::new(order, &scripts.0, &mut state.0)
                                .say_to_done(speaker, topic);
                        }
                        return;
                    }
                    let top = top_level.get_or_insert_with(|| dialogue::top_level_topics(order));
                    let list = dialogue::next_choices(
                        order,
                        &talk.info,
                        top,
                        &talk.opening,
                        &talk.speaker,
                        &state.0,
                    );
                    if list.is_empty() {
                        // Nothing more to say.
                        if let Some(s) = screen.as_deref_mut() {
                            crate::game_menus::dialog::end(s);
                        }
                        end(&mut commands, &mut conversation.0, &mut player, &mut panel);
                        return;
                    }
                    talk.choices = Some(list);
                }
            }
        } else if let Some(list) = &talk.choices {
            // Choosing: the game's menu, else number keys.
            let digits = [
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
                KeyCode::Digit9,
            ];
            let picked = match answer {
                Some(ui::menus::dialog::Answer::Topic(i)) => Some(i),
                // `--choose`: the next reply on the list.
                _ if !auto_talk.1.is_empty() => auto_talk.1.pop_front().map(|n| n - 1),
                _ if screen.is_some() => None,
                _ => digits.iter().position(|k| keys.just_pressed(*k)),
            };
            match picked {
                Some(i) if i < list.len() => {
                    let info = list[i].info.clone();
                    begin_line(
                        order,
                        &scripts.0,
                        &mut state.0,
                        &info,
                        talk.speaker.reference,
                    );
                    talk.info = info;
                    talk.response = 0;
                    talk.since = now;
                    talk.choices = None;
                    talk.voice = play_voice(&mut commands, &mut audio, &game.0, talk);
                }
                Some(i) if i == list.len() && screen.is_none() && needs_leave(list) => {
                    end(&mut commands, &mut conversation.0, &mut player, &mut panel);
                    return;
                }
                _ => {}
            }
        }
        // The game's menu shows the line or the topics.
        if let Some(s) = screen {
            let talk = conversation.0.as_mut().unwrap();
            match &talk.choices {
                None => {
                    let key = (talk.info.form_id, talk.response);
                    if talk.shown_line != Some(key) {
                        let text = talk
                            .info
                            .responses
                            .get(talk.response)
                            .map(|r| r.text.clone())
                            .unwrap_or_default();
                        crate::game_menus::dialog::show_line(s, &text);
                        talk.shown_line = Some(key);
                        talk.shown_topics = false;
                    }
                }
                Some(list) => {
                    if !talk.shown_topics {
                        crate::game_menus::dialog::show_topics(s, list, f64::from(now));
                        talk.shown_topics = true;
                        talk.shown_line = None;
                    }
                }
            }
            for (_, mut visible) in &mut panel {
                *visible = Visibility::Hidden;
            }
            return;
        }
        let talk = conversation.0.as_ref().unwrap();
        let mut lines = String::new();
        match &talk.choices {
            None => {
                if let Some(r) = talk.info.responses.get(talk.response) {
                    lines = format!("{}: {}", talk.name, r.text);
                }
            }
            Some(list) => {
                for (i, c) in list.iter().enumerate() {
                    lines.push_str(&format!("{}) {}\n", i + 1, c.label));
                }
                if needs_leave(list) {
                    lines.push_str(&format!("{}) (Leave)", list.len() + 1));
                }
            }
        }
        for (mut text, mut visible) in &mut panel {
            if text.0 != lines {
                text.0 = lines.clone();
            }
            *visible = Visibility::Visible;
        }
        return;
    }

    // Not talking: who's in view, within reach?
    let Ok(camera) = cameras.single() else {
        return;
    };
    let eye = game_point(camera.translation);
    let f = camera.forward().as_vec3();
    let dir = [f.x, -f.z, f.y];
    let mut best: Option<(f32, Talker)> = None;
    for t in &talkers.0 {
        if let Some(d) = ray_person(eye, dir, t.position) {
            if d <= ACTIVATE_REACH && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, *t));
            }
        }
    }
    // Walls in the way hide them.
    let best = best.filter(|(d, _)| {
        collision
            .0
            .raycast(eye, dir, *d)
            .is_none_or(|(wall, _)| wall >= d - 10.0)
    });
    target.0 = best.and_then(|(_, t)| {
        if target
            .0
            .as_ref()
            .is_some_and(|(old, _)| old.reference == t.reference)
        {
            return target.0.take();
        }
        let name = order.get(t.base)?.record().ok()?.full_name()?;
        Some((t, name))
    });
    // A script asked someone to talk to the player (`SayTo`,
    // `StartConversation`): they start, about the topic given.
    if let Some((speaker, topic, menu, say_to)) = scripted.0.take() {
        let found = talkers
            .0
            .iter()
            .find(|t| t.reference == speaker)
            .copied()
            // A talking activator isn't an actor: its own base and place.
            .or_else(|| {
                let base = world::scripting::base_of(order, speaker)?;
                (order.get(base)?.entry.header.kind.as_bytes() == b"TACT").then_some(Talker {
                    reference: speaker,
                    base,
                    position: [0.0; 3],
                })
            })
            .and_then(|t| {
                let name = order.get(t.base)?.record().ok()?.full_name()?;
                Some((t, name))
            });
        match found {
            Some((talker, name)) => {
                let topic = topic.unwrap_or(GREETING);
                if let Some(talk) =
                    start_talk(order, &scripts.0, &mut state.0, talker, name, topic, now)
                {
                    let mut talk = talk;
                    talk.line_only = !menu;
                    talk.say_to = say_to.then_some(topic);
                    talk.voice = play_voice(&mut commands, &mut audio, &game.0, &talk);
                    if menu {
                        player.ready = false;
                        for mut text in &mut prompt {
                            text.0.clear();
                        }
                    }
                    conversation.0 = Some(talk);
                }
            }
            None => {
                println!("A script has {speaker} talk to the player, but they aren't loaded here.")
            }
        }
        return;
    }
    // Scripts can turn the roll-over text, and with it talking to people,
    // off (`DisablePlayerControls`).
    if state.0.controls_off[world::scripting::controls::ROLLOVER] && !auto_talk.0 {
        return;
    }
    // `--talk`: the nearest person, once the place has loaded.
    let auto = auto_talk.0 && player.ready && !talkers.0.is_empty();
    let chosen = if auto {
        auto_talk.0 = false;
        talkers
            .0
            .iter()
            .min_by(|a, b| {
                let d = |t: &Talker| {
                    (t.position[0] - eye[0]).powi(2) + (t.position[1] - eye[1]).powi(2)
                };
                d(a).total_cmp(&d(b))
            })
            .and_then(|t| {
                let name = order.get(t.base)?.record().ok()?.full_name()?;
                Some((*t, name))
            })
    } else {
        target.0.clone()
    };
    let Some((talker, name)) = chosen else {
        return;
    };
    // Using a person (`world::living::pickpocket::use_person`): the dead
    // are searched (their inventory opens as a container); sneaking, the
    // living's pockets picked; the unconscious and fleeing refuse; else
    // talking.
    use world::living::pickpocket::{use_person, Use};
    let fleeing = walkers
        .iter()
        .any(|w| w.reference == talker.reference && w.fleeing.is_some());
    let using = if auto {
        Use::Talk
    } else {
        use_person(order, &state.0, talker.reference, fleeing)
    };
    if !auto {
        let verb = |setting: &str, exe: &str| {
            world::scripting::game_setting_text(order, setting).unwrap_or_else(|| exe.into())
        };
        for mut text in &mut prompt {
            let line = match &using {
                Use::Search => format!("E) {} {name}", verb("sSearch", "Search")),
                Use::Pickpocket => format!("E) {} {name}", verb("sPickpocket", "Pickpocket")),
                _ => format!("E) Talk to {name}"),
            };
            if text.0 != line {
                text.0 = line;
            }
        }
        if !keys.just_pressed(KeyCode::KeyE) {
            return;
        }
        keys.clear_just_pressed(KeyCode::KeyE);
    }
    match using {
        Use::Search => {
            menus.push(crate::menus::Menu::Container(talker.reference, name));
            return;
        }
        Use::Pickpocket => {
            // The game's container menu in its pickpocket mode
            // (`menus\container_menu.xml`) isn't drawn here yet; its rules
            // are `world::living::pickpocket`.
            println!("The pickpocket menu for {name} would open here (not drawn yet).");
            return;
        }
        Use::Refused(why) => {
            // Shown as the game's notice (`world::scripting::Event`).
            println!("{why}");
            state.0.events.push(world::scripting::Event::Message {
                title: None,
                text: why,
                buttons: Vec::new(),
            });
            return;
        }
        Use::Talk => {}
    }
    let Some(mut talk) = start_talk(order, &scripts.0, &mut state.0, talker, name, GREETING, now)
    else {
        return;
    };
    talk.voice = play_voice(&mut commands, &mut audio, &game.0, &talk);
    player.ready = false;
    for mut text in &mut prompt {
        text.0.clear();
    }
    conversation.0 = Some(talk);
}

/// Someone starts talking about a topic: the first line their conditions
/// allow, its first result script run.
fn start_talk(
    order: &esm::LoadOrder,
    scripts: &ScriptCache,
    state: &mut GameState,
    talker: Talker,
    name: String,
    topic: FormId,
    now: f32,
) -> Option<Talk> {
    let speaker = Speaker::load(order, talker.reference, talker.base)?;
    let Some(info) = dialogue::pick(order, topic, &speaker, state) else {
        println!("{name} has nothing to say about {topic}.");
        return None;
    };
    begin_line(order, scripts, state, &info, talker.reference);
    Some(Talk {
        speaker,
        name,
        opening: info.choices.clone(),
        info,
        response: 0,
        since: now,
        voice: None,
        choices: None,
        line_only: false,
        say_to: None,
        shown_line: None,
        shown_topics: false,
    })
}

/// Whether the menu needs a way out of its own: none of its choices ends
/// the conversation (the game's people mostly have a "Goodbye." topic;
/// Tab leaves too).
fn needs_leave(list: &[Choice]) -> bool {
    !list.iter().any(|c| c.info.flags & dialogue::GOODBYE != 0)
}

fn end(
    commands: &mut Commands,
    conversation: &mut Option<Talk>,
    player: &mut Player,
    panel: &mut Query<(&mut Text, &mut Visibility), With<DialogueText>>,
) {
    if let Some(talk) = conversation.take() {
        if let Some(v) = talk.voice {
            if let Ok(mut e) = commands.get_entity(v) {
                e.despawn();
                crate::faces::cut_short(commands, talk.speaker.reference);
            }
        }
    }
    player.ready = true;
    for (mut text, mut visible) in panel.iter_mut() {
        text.0.clear();
        *visible = Visibility::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_find_people_in_front_at_body_height() {
        let feet = [100.0, 0.0, 0.0];
        // Looking along +x at eye height: hits the near side of the body.
        let d = ray_person([0.0, 0.0, 120.0], [1.0, 0.0, 0.0], feet).unwrap();
        assert!((d - 75.0).abs() < 1e-3, "{d}");
        // Looking away, or over their head.
        assert!(ray_person([0.0, 0.0, 120.0], [-1.0, 0.0, 0.0], feet).is_none());
        let up = [0.6, 0.0, 0.8];
        assert!(ray_person([0.0, 0.0, 120.0], up, feet).is_none());
    }
}
