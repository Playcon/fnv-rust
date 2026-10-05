//! Dialogue: topics (`DIAL`) and the lines said under them (`INFO`, in each
//! topic's child group), with the conditions that pick who says which
//! line and when.
//!
//! A line holds one or more responses (`TRDT` + `NAM1`: the emotion, the
//! response number and the text), the player's prompt for it (`RNAM`), the
//! topics offered next (`TCLT`), and conditions (`CTDA`), each a game
//! function (see [`crate::functions`]) compared with a value.

use esm::{FormId, FourCC, LoadOrder, Record, RecordRef};

use crate::cell::{le_f32, le_u32};
use crate::functions::function_name;

const DIAL: FourCC = FourCC::new(b"DIAL");
const INFO: FourCC = FourCC::new(b"INFO");
const TRDT: FourCC = FourCC::new(b"TRDT");
const NAM1: FourCC = FourCC::new(b"NAM1");
const CTDA: FourCC = FourCC::new(b"CTDA");
const TCLT: FourCC = FourCC::new(b"TCLT");
const NAME: FourCC = FourCC::new(b"NAME");
const RNAM: FourCC = FourCC::new(b"RNAM");
const QSTI: FourCC = FourCC::new(b"QSTI");
const PNAM: FourCC = FourCC::new(b"PNAM");
const KNAM: FourCC = FourCC::new(b"KNAM");
const TDUM: FourCC = FourCC::new(b"TDUM");
const FULL: FourCC = FourCC::new(b"FULL");
const SCTX: FourCC = FourCC::new(b"SCTX");
const NEXT: FourCC = FourCC::new(b"NEXT");

/// Function numbers the conditions use most.
pub mod functions {
    pub const GET_IS_ID: u16 = 72;
    pub const GET_IS_VOICE_TYPE: u16 = 427;
    pub const GET_IS_SEX: u16 = 70;
    pub const GET_IS_RACE: u16 = 69;
    pub const GET_IN_FACTION: u16 = 71;
    pub const GET_STAGE: u16 = 58;
    pub const GET_STAGE_DONE: u16 = 59;
    pub const GET_QUEST_RUNNING: u16 = 56;
    pub const GET_GLOBAL_VALUE: u16 = 74;
    pub const GET_IS_REFERENCE: u16 = 136;
    pub const GET_ACTOR_VALUE: u16 = 14;
    pub const HAS_PERK: u16 = 449;
}

/// A dialogue topic.
#[derive(Debug, Clone, PartialEq)]
pub struct Topic {
    pub form_id: FormId,
    pub editor_id: Option<String>,
    /// What the player picks (`FULL`).
    pub name: Option<String>,
    /// `DATA`: the kind (0 an ordinary topic, 1 conversation, 2 combat, 3
    /// persuasion, 4 detection, 5 service, 6 miscellaneous, 7 radio) and
    /// flags (0x01 rumors, 0x02 top-level). Checked on Sunny Smiles'
    /// topics: her questions ("What do you do around here?") are 0 / 0x02,
    /// the follow-ups only offered after a line ("How many are there?")
    /// 0 / 0, her barks 1 / 0.
    pub kind: u8,
    pub flags: u8,
    /// `PNAM`: the topic's priority (50 unless set; Sunny's questions 75
    /// to 97).
    pub priority: f32,
    /// `TDUM`: what a player of low Intelligence says instead.
    pub dumb_prompt: Option<String>,
    /// The quests with lines under it (`QSTI`).
    pub quests: Vec<FormId>,
}

/// Topic `DATA` kind: an ordinary topic, the kind the player picks.
pub const ORDINARY_TOPIC: u8 = 0;
/// Topic `DATA` flag: offered in the list of things to ask about.
pub const TOP_LEVEL: u8 = 0x02;

/// One response of a line.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// `TRDT`: the emotion (0 neutral, 1 anger, 2 disgust, 3 fear, 4 sad,
    /// 5 happy, 6 surprise, 7 pained), its strength, and the response
    /// number (the voice file's last part).
    pub emotion: u32,
    pub emotion_value: i32,
    pub number: u8,
    pub text: String,
}

/// How a condition compares (the top three bits of its first byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

/// One condition: `function(params) <comparison> value`. Conditions are
/// joined with AND unless a condition's OR flag joins it to the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub comparison: Comparison,
    /// Joined to the next condition with OR rather than AND.
    pub or: bool,
    /// The value is a global variable's (`GLOB`) instead.
    pub value: f32,
    pub global: Option<FormId>,
    pub function: u16,
    /// The parameters as stored, and read as form IDs (in load-order
    /// numbering) for functions that take records.
    pub params: [u32; 2],
    pub param_forms: [FormId; 2],
    /// Who the function is asked about: 0 the speaker, 1 the target (the
    /// player in dialogue), 2 a reference, ...
    pub run_on: u32,
    pub reference: Option<FormId>,
}

impl Condition {
    pub fn function_name(&self) -> String {
        function_name(self.function)
    }

    /// Whether `result` passes the comparison.
    pub fn compare(&self, result: f32, value: f32) -> bool {
        match self.comparison {
            Comparison::Equal => result == value,
            Comparison::NotEqual => result != value,
            Comparison::Greater => result > value,
            Comparison::GreaterOrEqual => result >= value,
            Comparison::Less => result < value,
            Comparison::LessOrEqual => result <= value,
        }
    }
}

/// A dialogue line.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub form_id: FormId,
    pub topic: Option<FormId>,
    /// The quest the line belongs to (`QSTI`).
    pub quest: Option<FormId>,
    /// The line before it in its topic's order (`PNAM`).
    pub previous: Option<FormId>,
    /// `DATA`: type, next speaker, flags (0x01 goodbye, 0x02 random, 0x04
    /// say once, ...).
    pub flags: u8,
    /// `DATA` byte 3, the second flags (the game keeps the four bytes at
    /// `INFO+0x23`, `0061dbd0`): 0x02 "always darken" (`00763fd0` reads
    /// `+0x26 & 2`).
    pub flags2: u8,
    pub responses: Vec<Response>,
    pub conditions: Vec<Condition>,
    /// The player's line that leads to this one (`RNAM`), when it differs
    /// from the topic's name.
    pub prompt: Option<String>,
    /// The skill, S.P.E.C.I.A.L. (`AVIF`) or perk the choice is a check of
    /// (`KNAM`; Trudy's "< Speech 25 >" line names `AVSpeech`), for its tag.
    pub check: Option<FormId>,
    /// Topics offered after this line (`TCLT`), and added to what the
    /// player can ask about (`NAME`).
    pub choices: Vec<FormId>,
    pub add_topics: Vec<FormId>,
    /// Result scripts (`SCTX`): run as the line starts, and after it's
    /// said (the second, after the `NEXT` marker).
    pub begin_script: Option<String>,
    pub end_script: Option<String>,
}

/// `DATA` flag: the conversation ends after this line.
pub const GOODBYE: u8 = 0x01;
/// `DATA` flag: said only once a game (Doc Mitchell's "You're awake. How
/// about that." has it, and the intro asks for the topic again later).
pub const SAY_ONCE: u8 = 0x04;

fn global(rr: &RecordRef<'_>, data: &[u8]) -> FormId {
    rr.plugin.to_global(FormId(le_u32(data, 0)))
}

impl Topic {
    pub fn load(order: &LoadOrder, id: FormId) -> Option<Topic> {
        let rr = order.get(id).filter(|r| r.entry.header.kind == DIAL)?;
        let record = rr.record().ok()?;
        Some(Topic::parse(&rr, &record))
    }

    fn parse(rr: &RecordRef<'_>, record: &Record) -> Topic {
        let data = record.get(esm::sig::DATA).map(|s| s.data.as_slice());
        let byte = |i: usize| data.and_then(|d| d.get(i).copied()).unwrap_or(0);
        Topic {
            form_id: rr.form_id,
            editor_id: record.editor_id(),
            name: record.get(FULL).map(|s| s.zstring()),
            kind: byte(0),
            flags: byte(1),
            priority: record
                .get(PNAM)
                .filter(|s| s.data.len() >= 4)
                .map_or(50.0, |s| le_f32(&s.data, 0)),
            dumb_prompt: record
                .get(TDUM)
                .map(|s| s.zstring())
                .filter(|s| !s.is_empty()),
            quests: record
                .get_all(QSTI)
                .filter(|s| s.data.len() >= 4)
                .map(|s| global(rr, &s.data))
                .collect(),
        }
    }

    /// Offered in the list of things to ask about: an ordinary topic
    /// flagged top-level.
    pub fn is_top_level(&self) -> bool {
        self.kind == ORDINARY_TOPIC && self.flags & TOP_LEVEL != 0
    }

    /// What the menu shows for it when its line has no prompt of its own.
    pub fn label(&self) -> Option<String> {
        self.name
            .clone()
            .filter(|n| !n.is_empty())
            .or_else(|| self.editor_id.clone())
    }
}

/// Every top-level topic in the game, highest priority first (the order
/// the menu lists them in, an assumption: equal priorities keep the files'
/// order).
pub fn top_level_topics(order: &LoadOrder) -> Vec<Topic> {
    let mut topics: Vec<Topic> = order
        .records_of_type(DIAL)
        .filter(|rr| !rr.entry.header.is_deleted())
        .filter_map(|rr| {
            let record = rr.record().ok()?;
            Some(Topic::parse(&rr, &record))
        })
        .filter(Topic::is_top_level)
        .collect();
    topics.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    topics
}

/// One thing the player can say: the topic, the line the speaker would
/// answer with, and the text shown (the line's prompt, else the topic's
/// name).
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub topic: Topic,
    pub info: Info,
    pub label: String,
    /// Shown dimmed (the dialogue menu's `_line_alpha` 128, `007638b0`):
    /// its line has been said before, or is flagged "always darken"
    /// (`DATA` byte 3, 0x02).
    pub dim: bool,
    /// A skill check the player doesn't pass: its tag reads "[Speech
    /// 12/25]", and the menu's highlight turns dark red on it (`007638b0`
    /// sets the tile's flag, `00764ef0` copies it).
    pub failed: bool,
}

/// `DATA` byte 3 flag: the choice is always shown dimmed.
pub const ALWAYS_DARKEN: u8 = 0x02;

/// Each topic the speaker has a line for now, as a choice, in the order
/// given.
fn answered(
    order: &LoadOrder,
    topics: &[&Topic],
    speaker: &Speaker,
    state: &GameState,
) -> Vec<Choice> {
    topics
        .iter()
        // Only a running quest's lines can be said; the topic's record
        // lists the quests it has lines for.
        .filter(|t| t.quests.is_empty() || t.quests.iter().any(|q| state.running.contains(q)))
        .filter_map(|t| {
            let info = pick(order, t.form_id, speaker, state)?;
            let mut label = prompt(order, t, &info, speaker, state)?;
            let mut failed = false;
            if let Some((tag, passed)) = check_tag_passed(order, &info, speaker, state) {
                label = format!("{tag}  {label}");
                failed = !passed;
            }
            let dim = state.said.contains(&info.form_id) || info.flags2 & ALWAYS_DARKEN != 0;
            Some(Choice {
                topic: (*t).clone(),
                info,
                label,
                dim,
                failed,
            })
        })
        .collect()
}

/// What a choice says (`0083db00`): the topic's "dumb" prompt (`TDUM`)
/// when the player's Intelligence is at most
/// `iDialogueDummySpeakThisIntOrBelow` (3) and the topic has one; else the
/// line's own prompt (`RNAM`); else the topic's name.
pub fn prompt(
    order: &LoadOrder,
    topic: &Topic,
    info: &Info,
    speaker: &Speaker,
    state: &GameState,
) -> Option<String> {
    if let Some(dumb) = &topic.dumb_prompt {
        let most = crate::scripting::game_setting(order, "iDialogueDummySpeakThisIntOrBelow")
            .unwrap_or(4.0);
        let facts = crate::scripting::Facts {
            order,
            state,
            speaker: Some(speaker),
        };
        let intelligence = facts.current_actor_value(PLAYER_REF, 9).unwrap_or(5.0);
        if intelligence <= f64::from(most) {
            return Some(dumb.clone());
        }
    }
    info.prompt.clone().or_else(|| topic.label())
}

/// The tag a skill check's choice is shown with (`007638b0`, read from the
/// game's code): only for a line naming a skill, S.P.E.C.I.A.L. or perk in
/// `KNAM`, that record's name as stored; then from the line's first
/// `GetActorValue` condition (whatever its comparison, run-on or value
/// asked about) the value needed and the player's own (rounded down) of the
/// value it names: "[Speech 25]" when the player has enough, "[Speech
/// 12/25]" when not; a line with no such condition (most perks) just
/// "[Black Widow]". The menu puts two spaces before the prompt. Trudy's
/// "< Speech 25 >" line: `KNAM` `AVSpeech`, `GetActorValue Speech >= 25`.
/// (The words "[SUCCEEDED]" and "[FAILED]" are in the lines' own text.)
pub fn check_tag(
    order: &LoadOrder,
    info: &Info,
    speaker: &Speaker,
    state: &GameState,
) -> Option<String> {
    check_tag_passed(order, info, speaker, state).map(|(tag, _)| tag)
}

/// [`check_tag`], and whether the player passes the check (a line with no
/// `GetActorValue` condition counts as passed: the menu marks only a value
/// short of the one needed, `007638b0`).
pub fn check_tag_passed(
    order: &LoadOrder,
    info: &Info,
    speaker: &Speaker,
    state: &GameState,
) -> Option<(String, bool)> {
    let checked = info.check?;
    let name = order
        .get(checked)
        .and_then(|rr| rr.record().ok())
        .and_then(|r| r.full_name())?;
    let Some(c) = info
        .conditions
        .iter()
        .find(|c| c.function == functions::GET_ACTOR_VALUE)
    else {
        return Some((format!("[{name}]"), true));
    };
    let needed = match c.global {
        Some(g) => state.globals.get(&g).copied().unwrap_or(0.0),
        None => c.value,
    }
    .trunc();
    let facts = crate::scripting::Facts {
        order,
        state,
        speaker: Some(speaker),
    };
    let have = facts
        .current_actor_value(PLAYER_REF, c.params[0] as u16)
        .unwrap_or(0.0)
        .floor();
    if have >= f64::from(needed) {
        Some((format!("[{name} {needed}]"), true))
    } else {
        Some((format!("[{name} {have}/{needed}]"), false))
    }
}

/// The main list of things to ask about, shown after a line with no
/// follow-ups of its own (`TCLT`): the follow-ups of the line that opened
/// the conversation (`opening`), every top-level topic, and every topic
/// the player has learned (`AddTopic`, a line's `NAME`), each once, for
/// which the speaker has a line now, highest priority first.
///
/// Read from Sunny Smiles' records: her greeting's follow-ups are her
/// top-level questions plus a "Goodbye." that isn't top-level, stored in
/// exactly descending priority (100 … 75, then 50), and "That's all I
/// wanted to know. Let's talk about something else." has no follow-ups, so
/// it leads back to the main list. That the main list keeps the opening
/// line's follow-ups (so "Goodbye." stays) is a guess, not traced in the
/// game's code.
pub fn menu_topics(
    order: &LoadOrder,
    top_level: &[Topic],
    opening: &[FormId],
    speaker: &Speaker,
    state: &GameState,
) -> Vec<Choice> {
    let mut extra: Vec<FormId> = opening.to_vec();
    let mut learned: Vec<FormId> = state.topics.iter().copied().collect();
    learned.sort_by_key(|id| id.0);
    extra.extend(learned);
    let mut seen: std::collections::HashSet<FormId> = top_level.iter().map(|t| t.form_id).collect();
    let extra: Vec<Topic> = extra
        .into_iter()
        .filter(|id| seen.insert(*id))
        .filter_map(|id| Topic::load(order, id))
        .collect();
    let mut topics: Vec<&Topic> = extra.iter().chain(top_level).collect();
    topics.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    answered(order, &topics, speaker, state)
}

/// What the player can say after a line: its follow-ups (`TCLT`) as
/// stored, when it has any; otherwise the main list ([`menu_topics`]).
pub fn next_choices(
    order: &LoadOrder,
    info: &Info,
    top_level: &[Topic],
    opening: &[FormId],
    speaker: &Speaker,
    state: &GameState,
) -> Vec<Choice> {
    if info.choices.is_empty() {
        return menu_topics(order, top_level, opening, speaker, state);
    }
    let topics: Vec<Topic> = info
        .choices
        .iter()
        .filter_map(|&id| Topic::load(order, id))
        .collect();
    let topics: Vec<&Topic> = topics.iter().collect();
    answered(order, &topics, speaker, state)
}

/// A line starts: it's been said (for "say once"), the speaker has talked
/// to the player, and the topics it names (`NAME`) are learned.
pub fn line_begins(state: &mut GameState, info: &Info, speaker: FormId) {
    state.said.insert(info.form_id);
    state.talked_to.insert(speaker);
    state.topics.extend(info.add_topics.iter().copied());
}

pub(crate) fn read_condition(rr: &RecordRef<'_>, data: &[u8]) -> Option<Condition> {
    if data.len() < 24 {
        return None;
    }
    let kind = data[0];
    let comparison = match kind >> 5 {
        0 => Comparison::Equal,
        1 => Comparison::NotEqual,
        2 => Comparison::Greater,
        3 => Comparison::GreaterOrEqual,
        4 => Comparison::Less,
        _ => Comparison::LessOrEqual,
    };
    let uses_global = kind & 0x04 != 0;
    Some(Condition {
        comparison,
        or: kind & 0x01 != 0,
        value: if uses_global { 0.0 } else { le_f32(data, 4) },
        global: uses_global.then(|| global(rr, &data[4..])),
        function: u16::from_le_bytes([data[8], data[9]]),
        params: [le_u32(data, 12), le_u32(data, 16)],
        param_forms: [global(rr, &data[12..]), global(rr, &data[16..])],
        run_on: le_u32(data, 20),
        reference: (data.len() >= 28)
            .then(|| global(rr, &data[24..]))
            .filter(|id| id.0 != 0),
    })
}

impl Info {
    pub fn parse(order: &LoadOrder, rr: &RecordRef<'_>, record: &Record) -> Info {
        let form = |kind: FourCC| {
            record
                .get(kind)
                .filter(|s| s.data.len() >= 4)
                .map(|s| global(rr, &s.data))
                .filter(|id| id.0 != 0)
        };
        let mut responses = Vec::new();
        let mut pending: Option<Response> = None;
        let mut conditions = Vec::new();
        let mut choices = Vec::new();
        let mut add_topics = Vec::new();
        let mut scripts: [Option<String>; 2] = [None, None];
        let mut after_next = false;
        for sub in &record.subrecords {
            match sub.kind {
                k if k == NEXT => after_next = true,
                k if k == SCTX => {
                    let text = esm::text::decode_cp1252(&sub.data);
                    scripts[usize::from(after_next)] = Some(text);
                }
                k if k == TRDT && sub.data.len() >= 13 => {
                    if let Some(r) = pending.take() {
                        responses.push(r);
                    }
                    pending = Some(Response {
                        emotion: le_u32(&sub.data, 0),
                        emotion_value: le_u32(&sub.data, 4) as i32,
                        number: sub.data[12],
                        text: String::new(),
                    });
                }
                k if k == NAM1 => {
                    if let Some(r) = pending.as_mut() {
                        r.text = sub.zstring();
                    }
                }
                k if k == CTDA => conditions.extend(read_condition(rr, &sub.data)),
                k if k == TCLT && sub.data.len() >= 4 => choices.push(global(rr, &sub.data)),
                k if k == NAME && sub.data.len() >= 4 => add_topics.push(global(rr, &sub.data)),
                _ => {}
            }
        }
        if let Some(r) = pending.take() {
            responses.push(r);
        }
        Info {
            form_id: rr.form_id,
            topic: order.topic_of(rr),
            quest: form(QSTI),
            previous: form(PNAM),
            flags: record
                .get(esm::sig::DATA)
                .and_then(|s| s.data.get(2).copied())
                .unwrap_or(0),
            flags2: record
                .get(esm::sig::DATA)
                .and_then(|s| s.data.get(3).copied())
                .unwrap_or(0),
            responses,
            conditions,
            prompt: record
                .get(RNAM)
                .map(|s| s.zstring())
                .filter(|s| !s.is_empty()),
            check: form(KNAM),
            choices,
            add_topics,
            begin_script: scripts[0].take(),
            end_script: scripts[1].take(),
        }
    }

    pub fn load(order: &LoadOrder, id: FormId) -> Option<Info> {
        let rr = order.get(id).filter(|r| r.entry.header.kind == INFO)?;
        let record = rr.record().ok()?;
        Some(Info::parse(order, &rr, &record))
    }

    /// Whether a condition names this subject: `GetIsID` of the base, or
    /// `GetIsVoiceType` of its voice, asked about the speaker.
    pub fn names_speaker(&self, base: FormId, voice: Option<FormId>) -> bool {
        self.conditions.iter().any(|c| {
            c.run_on == 0
                && c.comparison == Comparison::Equal
                && c.value == 1.0
                && ((c.function == functions::GET_IS_ID && c.param_forms[0] == base)
                    || (c.function == functions::GET_IS_VOICE_TYPE
                        && voice == Some(c.param_forms[0])))
        })
    }
}

/// Every line whose conditions name the speaker (its base record, or its
/// voice type), in record order. Lines anyone can say aren't included.
pub fn lines_for(order: &LoadOrder, base: FormId, voice: Option<FormId>) -> Vec<Info> {
    let mut out = Vec::new();
    for rr in order.records_of_type(INFO) {
        let Ok(record) = rr.record() else {
            continue;
        };
        // Quick check before parsing: the base's form ID somewhere in a
        // condition.
        let mentions = record.get_all(CTDA).any(|s| {
            s.data.len() >= 16 && {
                let p = global(&rr, &s.data[12..]);
                p == base || voice == Some(p)
            }
        });
        if !mentions {
            continue;
        }
        let info = Info::parse(order, &rr, &record);
        if info.names_speaker(base, voice) {
            out.push(info);
        }
    }
    out
}

const VTCK: FourCC = FourCC::new(b"VTCK");
const VNAM: FourCC = FourCC::new(b"VNAM");
const RNAM_RACE: FourCC = FourCC::new(b"RNAM");
const ACBS: FourCC = FourCC::new(b"ACBS");
const SNAM: FourCC = FourCC::new(b"SNAM");

/// The player's reference and base record (fixed forms in every game).
pub const PLAYER_REF: FormId = FormId(0x14);
pub const PLAYER_BASE: FormId = FormId(0x7);

pub use crate::scripting::GameState;

/// Who's speaking: what conditions ask about them.
#[derive(Debug, Clone, PartialEq)]
pub struct Speaker {
    pub reference: FormId,
    pub base: FormId,
    pub name: Option<String>,
    pub voice: Option<FormId>,
    pub race: Option<FormId>,
    pub female: bool,
    pub factions: Vec<FormId>,
}

impl Speaker {
    /// A placed NPC (or creature) by its reference and base.
    pub fn load(order: &LoadOrder, reference: FormId, base: FormId) -> Option<Speaker> {
        let rr = order.get(base)?;
        let record = rr.record().ok()?;
        let form = |kind: FourCC| {
            record
                .get(kind)
                .filter(|s| s.data.len() >= 4)
                .map(|s| global(&rr, &s.data))
                .filter(|id| id.0 != 0)
        };
        Some(Speaker {
            reference,
            base,
            name: record.full_name(),
            // A talking activator's voice type is its VNAM.
            voice: form(VTCK).or_else(|| {
                (rr.entry.header.kind.as_bytes() == b"TACT")
                    .then(|| form(VNAM))
                    .flatten()
            }),
            race: form(RNAM_RACE),
            female: record
                .get(ACBS)
                .filter(|s| s.data.len() >= 4)
                .is_some_and(|s| le_u32(&s.data, 0) & 1 != 0),
            factions: record
                .get_all(SNAM)
                .filter(|s| s.data.len() >= 4)
                .map(|s| global(&rr, &s.data))
                .collect(),
        })
    }
}

/// Whether a line's conditions pass, said by `speaker` to the player (see
/// [`crate::scripting::Facts::conditions_pass`]).
pub fn passes(order: &LoadOrder, info: &Info, speaker: &Speaker, state: &GameState) -> bool {
    let facts = crate::scripting::Facts {
        order,
        state,
        speaker: Some(speaker),
    };
    facts.conditions_pass(&info.conditions, speaker.reference, PLAYER_REF)
}

/// A topic's lines in the order the game tries them: by their quests'
/// priority (highest first), then as stored.
pub fn topic_lines(order: &LoadOrder, topic: FormId) -> Vec<Info> {
    let priority = |quest: Option<FormId>| -> u8 {
        quest
            .and_then(|q| order.get(q))
            .and_then(|r| r.record().ok())
            .and_then(|r| r.get(esm::sig::DATA).and_then(|s| s.data.get(1).copied()))
            .unwrap_or(0)
    };
    let mut lines: Vec<(u8, Info)> = order
        .in_topic(topic)
        .into_iter()
        .filter(|rr| rr.entry.header.kind == INFO && !rr.entry.header.is_deleted())
        .filter_map(|rr| {
            let record = rr.record().ok()?;
            let info = Info::parse(order, &rr, &record);
            Some((priority(info.quest), info))
        })
        .collect();
    lines.sort_by_key(|(p, _)| std::cmp::Reverse(*p));
    lines.into_iter().map(|(_, i)| i).collect()
}

/// The first line of a topic the speaker can say now: its quest (`QSTI`)
/// is running and the quest's own conditions pass for the speaker (they
/// apply to all its dialogue: without them, Sunny Smiles greets the
/// player with one of ED-E's beeps, a line with no conditions of its own
/// in `vDialogueEDE`, whose conditions name ED-E), its own conditions
/// pass, and it isn't a "say once" line already said.
pub fn pick(
    order: &LoadOrder,
    topic: FormId,
    speaker: &Speaker,
    state: &GameState,
) -> Option<Info> {
    let facts = crate::scripting::Facts {
        order,
        state,
        speaker: Some(speaker),
    };
    let mut quest_ok: std::collections::HashMap<FormId, bool> = Default::default();
    topic_lines(order, topic).into_iter().find(|info| {
        if info.responses.is_empty()
            || (info.flags & SAY_ONCE != 0 && state.said.contains(&info.form_id))
        {
            return false;
        }
        if let Some(q) = info.quest {
            let ok = *quest_ok.entry(q).or_insert_with(|| {
                state.running.contains(&q)
                    && facts.conditions_pass(
                        &crate::quest::quest_conditions(order, q),
                        speaker.reference,
                        PLAYER_REF,
                    )
            });
            if !ok {
                return false;
            }
        }
        passes(order, info, speaker, state)
    })
}

/// The quest and topic parts of a voice file's name: whole when together
/// they're at most 25 letters, else the quest's first 10 and the topic's
/// first 15 (`vfreeformgoodsprings_hit_00126f2b_1.ogg`, Sunny Smiles' hurt
/// line, but `vfreeformg_greeting_00107220_1.ogg`; every one of the 16,293
/// voice files in `Fallout - Voices1.bsa` with a part longer than 10 or 15
/// letters has the two together at most 25).
pub fn voice_name_parts(quest: &str, topic: &str) -> (String, String) {
    let (q, t) = (quest.to_ascii_lowercase(), topic.to_ascii_lowercase());
    if q.chars().count() + t.chars().count() <= 25 {
        (q, t)
    } else {
        (q.chars().take(10).collect(), t.chars().take(15).collect())
    }
}

/// The voice file for a response: `sound\voice\<plugin>\<voice type>\
/// <quest>_<topic>_<line's form ID>_<response>.ogg` (the quest and topic
/// cut as [`voice_name_parts`] says: `vfreeformg_greeting_00107220_1.ogg`
/// for Doc Mitchell's line 00107220 of `VFreeformGoodsprings`, topic
/// `GREETING`). The form ID's load-order byte is the line's own plugin's
/// index among its masters: 00 for `FalloutNV.esm`.
pub fn voice_path(
    order: &LoadOrder,
    info: &Info,
    response: &Response,
    voice: FormId,
) -> Option<String> {
    let edid = |id: Option<FormId>| -> Option<String> {
        order
            .get(id?)?
            .editor_id()
            .ok()?
            .map(|e| e.to_ascii_lowercase())
    };
    let voice_name = edid(Some(voice))?;
    let (quest, topic) = voice_name_parts(&edid(info.quest)?, &edid(info.topic)?);
    let rr = order.get(info.form_id)?;
    let local = rr.entry.header.form_id.0 & 0x00FF_FFFF;
    Some(format!(
        "sound\\voice\\{}\\{voice_name}\\{quest}_{topic}_{local:08x}_{}.ogg",
        rr.plugin.name.to_ascii_lowercase(),
        response.number
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker() -> Speaker {
        Speaker {
            reference: FormId(0x100),
            base: FormId(0x200),
            name: None,
            voice: Some(FormId(0x300)),
            race: None,
            female: false,
            factions: vec![FormId(0x400)],
        }
    }

    fn cond(function: u16, form: u32, comparison: Comparison, value: f32, or: bool) -> Condition {
        Condition {
            comparison,
            or,
            value,
            global: None,
            function,
            params: [form, 0],
            param_forms: [FormId(form), FormId(0)],
            run_on: 0,
            reference: None,
        }
    }

    fn info(conditions: Vec<Condition>) -> Info {
        Info {
            form_id: FormId(1),
            topic: None,
            quest: None,
            previous: None,
            flags: 0,
            flags2: 0,
            responses: Vec::new(),
            conditions,
            prompt: None,
            check: None,
            choices: Vec::new(),
            add_topics: Vec::new(),
            begin_script: None,
            end_script: None,
        }
    }

    /// A load order to ask in (the conditions here don't read it).
    fn order(tag: &str) -> (testdata::TempData, LoadOrder) {
        let data = testdata::quests(tag);
        let order =
            LoadOrder::from_data_dir(data.path(), &esm::ActivePlugins::OfficialOnly).unwrap();
        (data, order)
    }

    #[test]
    fn lines_are_picked_by_who_speaks_and_quest_stages() {
        let (_data, order) = order("dialogue-unit-pick");
        let state = GameState {
            stages: [(FormId(0x500), 20)].into(),
            ..GameState::default()
        };
        let s = speaker();
        let passes = |i: &Info| passes(&order, i, &s, &state);
        let mine = info(vec![cond(
            functions::GET_IS_ID,
            0x200,
            Comparison::Equal,
            1.0,
            false,
        )]);
        assert!(passes(&mine));
        let other = info(vec![cond(
            functions::GET_IS_ID,
            0x999,
            Comparison::Equal,
            1.0,
            false,
        )]);
        assert!(!passes(&other));
        let early = info(vec![cond(
            functions::GET_STAGE,
            0x500,
            Comparison::Less,
            10.0,
            false,
        )]);
        assert!(!passes(&early));
        // Functions not carried out (GetDistance) give 0.
        let unknown = info(vec![cond(1, 0x500, Comparison::Equal, 0.0, false)]);
        assert!(passes(&unknown));
    }

    #[test]
    fn or_binds_before_and() {
        let (_data, order) = order("dialogue-unit-or");
        let s = speaker();
        let state = GameState::default();
        let passes = |i: &Info| passes(&order, i, &s, &state);
        // (wrong id OR my faction) AND my voice: passes.
        let ok = info(vec![
            cond(functions::GET_IS_ID, 0x999, Comparison::Equal, 1.0, true),
            cond(
                functions::GET_IN_FACTION,
                0x400,
                Comparison::Equal,
                1.0,
                false,
            ),
            cond(
                functions::GET_IS_VOICE_TYPE,
                0x300,
                Comparison::Equal,
                1.0,
                false,
            ),
        ]);
        assert!(passes(&ok));
        // (wrong id OR wrong faction) AND my voice: fails.
        let no = info(vec![
            cond(functions::GET_IS_ID, 0x999, Comparison::Equal, 1.0, true),
            cond(
                functions::GET_IN_FACTION,
                0x998,
                Comparison::Equal,
                1.0,
                false,
            ),
            cond(
                functions::GET_IS_VOICE_TYPE,
                0x300,
                Comparison::Equal,
                1.0,
                false,
            ),
        ]);
        assert!(!passes(&no));
    }

    #[test]
    fn comparisons_follow_the_top_bits() {
        let c = |kind: u8| Condition {
            comparison: match kind >> 5 {
                0 => Comparison::Equal,
                2 => Comparison::Greater,
                _ => Comparison::LessOrEqual,
            },
            or: kind & 1 != 0,
            value: 3.0,
            global: None,
            function: 58,
            params: [0, 0],
            param_forms: [FormId(0); 2],
            run_on: 0,
            reference: None,
        };
        assert!(c(0x00).compare(3.0, 3.0));
        assert!(c(0x40).compare(4.0, 3.0) && !c(0x40).compare(3.0, 3.0));
        assert_eq!(c(0x01).function_name(), "GetStage");
    }
}
