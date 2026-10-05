//! The radio's script functions, as the game's radio (`FalloutRadio`,
//! `0083xxxx`) keeps them: whether the Pip-Boy radio is on and which
//! station it's tuned to, the conversation each station was told to start,
//! and the people playing a station through their own speaker
//! (`SetNPCRadio`). Notes: `docs/DEAD_MONEY.md` "Radio".
//!
//! Kept here is only what the functions change. What a station then plays
//! (its conversation's lines, one after another, the
//! `RadioConvTask` at `008373a0`), its range and static, and the sound
//! itself aren't carried out yet. The radio's "disabled" flag (`011dd436`,
//! which makes every one of these do nothing) is taken as clear: what
//! sets it isn't traced.

use std::collections::BTreeMap;

use esm::{FormId, FourCC};

use super::{is_actor, kind_of, placed, st};
use crate::scripting::{GameState, Runner, Value};

const TACT: FourCC = FourCC::new(b"TACT");

/// What the radio functions keep (`GameState::more.radio`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Radio {
    /// The Pip-Boy radio is on (`011dd434`).
    pub on: bool,
    /// The station reference it's tuned to (the station object at
    /// `011dd42c`, whose +0 is the reference); none when off.
    pub tuned: Option<FormId>,
    /// The conversation each station was last told to start
    /// (`StartRadioConversation`), by station reference: a topic, or
    /// `None` for the game's default (`0061a2d0(7, 0)`: the first topic
    /// of its radio dialogue list, not worked out here).
    pub conversations: BTreeMap<FormId, Option<FormId>>,
    /// People playing a station (`SetNPCRadio 1`), by person.
    pub npc_radio: BTreeMap<FormId, FormId>,
    /// `ResetPipboyManager` asked the player's Pip-Boy manager to reset
    /// (its +0x16c set to 1; what reads it isn't traced).
    pub pipboy_reset: bool,
}

/// The radio's functions, by the game's own names.
pub const CHANGES: &[&str] = &[
    "PipboyRadio",
    "PipBoyRadioOff",
    "StartRadioConversation",
    "SetNPCRadio",
    "ForceRadioStationUpdate",
    "ResetPipboyManager",
];

/// Whether a reference can be a station by itself: its base is a talking
/// activator (form type 0x16; `00832cb0`). Activators and actors get
/// their station another way (their base's radio template, `004fd3c0` →
/// `008356e0`), not carried out here.
fn is_station(runner: &Runner, r: FormId) -> bool {
    placed::base_now(runner.order, runner.state, r).and_then(|b| kind_of(runner.order, b))
        == Some(TACT)
}

/// Switches the Pip-Boy radio on or off (`008324e0`). Off also forgets
/// the station it was tuned to.
fn set_on(runner: &mut Runner, on: bool) {
    let r = &mut st(runner).radio;
    r.on = on;
    if !on {
        r.tuned = None;
    }
}

/// Tunes the Pip-Boy radio (`00832240(station, 1)`): only while it's on.
/// A reference that can't be a station switches the radio off (the
/// station object isn't made, `00832cb0` gives none). No station given:
/// the game picks one the player is in range of, not carried out here.
fn tune(runner: &mut Runner, station: FormId) {
    if !st(runner).radio.on || station.0 == 0 {
        return;
    }
    if is_station(runner, station) {
        st(runner).radio.tuned = Some(station);
    } else {
        set_on(runner, false);
    }
}

/// Carries out one of [`CHANGES`]; `None` when it isn't one.
pub(super) fn carry_out(
    runner: &mut Runner,
    name: &str,
    target: Option<FormId>,
    args: &[Value],
) -> Option<f64> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Number(0.0));
    match name {
        // `005d7fb0`: a word, then an optional station. A word starting
        // with `1`, or `enable` or `on`: on, and tuned to the station;
        // starting with `0`, or `disable` or `off`: off; `tune`: tuned
        // (while on). Dead Money writes `Tune`, so the words are compared
        // without case (`00404dc0`).
        "PipboyRadio" => {
            let word = match arg(0) {
                Value::Text(t) => t.to_ascii_lowercase(),
                Value::Number(n) => format!("{n}"),
                Value::Form(f) => format!("{}", f.0),
            };
            let station = arg(1).form();
            if word.starts_with('1') || word == "enable" || word == "on" {
                set_on(runner, true);
                tune(runner, station);
            } else if word.starts_with('0') || word == "disable" || word == "off" {
                set_on(runner, false);
            } else if word == "tune" {
                tune(runner, station);
            }
        }
        // `005dc580` → `008324e0(0)`.
        "PipBoyRadioOff" => set_on(runner, false),
        // `005d82a0` → `00835be0`: on a station, its conversation starts
        // now, replacing whatever it was playing; no topic given, the
        // default one.
        "StartRadioConversation" => {
            let station = target?;
            if is_station(runner, station) {
                let topic = Some(arg(0).form()).filter(|f| f.0 != 0);
                st(runner).radio.conversations.insert(station, topic);
            }
        }
        // `005d8100`: on a person (vtable +0x100) with a station: 1 plays
        // the station through them (`00835810`), 0 stops it (`00835980`);
        // other numbers do nothing.
        "SetNPCRadio" => {
            let who = target.filter(|&t| is_actor(runner.order, runner.state, t))?;
            let station = arg(1).form();
            if station.0 == 0 {
                return Some(0.0);
            }
            let r = &mut st(runner).radio;
            match arg(0).number() as i32 {
                1 => {
                    r.npc_radio.insert(who, station);
                }
                0 => {
                    r.npc_radio.remove(&who);
                }
                _ => {}
            }
        }
        // `005d8280` → `00832ad0(1)`: the stations update now rather than
        // at their next interval. Nothing here updates stations yet.
        "ForceRadioStationUpdate" => {}
        // `005db490`: the player's Pip-Boy manager (`00705990`), when
        // there is one, gets its reset flag (`005db4c0(1)`, +0x16c).
        "ResetPipboyManager" => st(runner).radio.pipboy_reset = true,
        _ => return None,
    }
    Some(1.0)
}

/// Saved lines.
pub(crate) fn save_lines(state: &GameState, line: &mut dyn FnMut(String)) {
    let r = &state.more.radio;
    let id = |f: FormId| format!("{:08X}", f.0);
    if r.on {
        line(format!(
            "radio on {}",
            r.tuned.map_or("00000000".into(), id)
        ));
    }
    for (station, topic) in &r.conversations {
        line(format!(
            "radioconversation {} {}",
            id(*station),
            topic.map_or("00000000".into(), id)
        ));
    }
    for (who, station) in &r.npc_radio {
        line(format!("npcradio {} {}", id(*who), id(*station)));
    }
    if r.pipboy_reset {
        line("pipboyreset".into());
    }
}

/// A saved line back.
pub(crate) fn load_line(state: &mut GameState, parts: &[&str]) -> Option<Result<(), String>> {
    let form = |i: usize| {
        parts
            .get(i)
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .map(FormId)
    };
    let nonzero = |f: FormId| Some(f).filter(|f| f.0 != 0);
    let bad = || Err(format!("can't read '{}'", parts.join(" ")));
    let r = &mut state.more.radio;
    Some(match *parts.first()? {
        "radio" if parts.get(1) == Some(&"on") => match form(2) {
            Some(t) => {
                r.on = true;
                r.tuned = nonzero(t);
                Ok(())
            }
            None => bad(),
        },
        "radioconversation" => match (form(1), form(2)) {
            (Some(s), Some(t)) => {
                r.conversations.insert(s, nonzero(t));
                Ok(())
            }
            _ => bad(),
        },
        "npcradio" => match (form(1), form(2)) {
            (Some(w), Some(s)) => {
                r.npc_radio.insert(w, s);
                Ok(())
            }
            _ => bad(),
        },
        "pipboyreset" => {
            r.pipboy_reset = true;
            Ok(())
        }
        _ => return None,
    })
}
