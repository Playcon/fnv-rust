//! Computer terminals (`TERM`) and the notes they show (`NOTE`).
//!
//! A terminal: `DESC` the header, `DNAM` (hacking difficulty u8: 0 very
//! easy … 4 very hard; flags u8: 0x02 unlocked; server type u8), then its
//! menu items, each `ITXT` the item's text, `RNAM` what it prints when
//! picked, `ANAM` flags, `INAM` a note to show, `TNAM` a sub-menu (another
//! terminal), a result script (`SCHR` … `SCTX`) and `CTDA` conditions
//! deciding whether it's listed. Read from the game's terminals:
//! `CampSearchlightFireChief` shows the note "Chief Fire Officer Report";
//! `P04CompanionFireTerminal` (flags 0x02) leads to a sub-menu. A note:
//! `FULL` its title, `DATA` its kind (0 sound, 1 text, 2 image, 3 voice),
//! `TNAM` the text.
//!
//! Hacking: the game's word-guessing game isn't here. A locked terminal
//! opens when the player's Science reaches 25 × its difficulty (the
//! thresholds as the game's players know them; not found in its settings
//! or traced in its code: a guess).

use esm::{FormId, FourCC, LoadOrder};

use crate::cell::le_u32;
use crate::dialogue::Condition;

const TERM: FourCC = FourCC::new(b"TERM");
const NOTE: FourCC = FourCC::new(b"NOTE");

/// `DNAM` flag: no hacking needed.
pub const UNLOCKED: u8 = 0x02;

/// The terminal menu's number (`ComputersMenu`, `&ComputersMenu;`).
pub const TERMINAL_MENU: u16 = 1057;

/// One menu item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerminalItem {
    pub text: String,
    /// Printed when picked.
    pub result: Option<String>,
    pub flags: u8,
    pub note: Option<FormId>,
    pub submenu: Option<FormId>,
    pub script: Option<String>,
    pub conditions: Vec<Condition>,
}

/// A terminal's screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub form_id: FormId,
    pub name: String,
    pub header: String,
    pub difficulty: u8,
    pub flags: u8,
    pub items: Vec<TerminalItem>,
}

impl Terminal {
    pub fn load(order: &LoadOrder, id: FormId) -> Option<Terminal> {
        let rr = order.get(id).filter(|r| r.entry.header.kind == TERM)?;
        let record = rr.record().ok()?;
        let text = |s: &esm::Subrecord| {
            esm::text::decode_cp1252(s.data.strip_suffix(&[0]).unwrap_or(&s.data))
        };
        let form = |s: &esm::Subrecord| {
            (s.data.len() >= 4)
                .then(|| rr.plugin.to_global(FormId(le_u32(&s.data, 0))))
                .filter(|f| f.0 != 0)
        };
        let dnam = record
            .get(FourCC::new(b"DNAM"))
            .map(|s| s.data.clone())
            .unwrap_or_default();
        let mut items: Vec<TerminalItem> = Vec::new();
        for sub in &record.subrecords {
            match sub.kind.as_bytes() {
                b"ITXT" => items.push(TerminalItem {
                    text: text(sub),
                    ..TerminalItem::default()
                }),
                b"RNAM" => {
                    if let Some(i) = items.last_mut() {
                        i.result = Some(text(sub)).filter(|t| !t.trim().is_empty());
                    }
                }
                b"ANAM" => {
                    if let Some(i) = items.last_mut() {
                        i.flags = sub.data.first().copied().unwrap_or(0);
                    }
                }
                b"INAM" => {
                    if let Some(i) = items.last_mut() {
                        i.note = form(sub);
                    }
                }
                b"TNAM" => {
                    if let Some(i) = items.last_mut() {
                        i.submenu = form(sub);
                    }
                }
                b"SCTX" => {
                    if let Some(i) = items.last_mut() {
                        i.script = Some(text(sub)).filter(|t| !t.trim().is_empty());
                    }
                }
                b"CTDA" => {
                    if let Some(i) = items.last_mut() {
                        i.conditions
                            .extend(crate::dialogue::read_condition(&rr, &sub.data));
                    }
                }
                _ => {}
            }
        }
        Some(Terminal {
            form_id: id,
            name: record.full_name().unwrap_or_default(),
            header: record
                .get(FourCC::new(b"DESC"))
                .map(text)
                .unwrap_or_default(),
            difficulty: dnam.first().copied().unwrap_or(0),
            flags: dnam.get(1).copied().unwrap_or(0),
            items,
        })
    }

    /// Whether it opens without hacking.
    pub fn unlocked(&self) -> bool {
        self.flags & UNLOCKED != 0
    }

    /// The Science skill that opens it when locked (a guess; see the
    /// module notes).
    pub fn science_needed(&self) -> u16 {
        u16::from(self.difficulty.min(4)) * 25
    }
}

/// How the player gets into a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Unlocked (by its flags, or hacked before, or by a script's
    /// `UnLock`).
    Open,
    /// Hacked now: it stays open, and gives the setting's experience
    /// (`iXPRewardHackComputer<difficulty>`).
    Hacked,
    /// Locked: hacking it needs this much Science.
    NeedsScience(u16),
}

/// The player uses a placed terminal: open, hacked now (see the module
/// notes) or still locked. Kept per reference in `GameState::locks`, as
/// scripts lock and unlock terminals there too.
pub fn try_hack(
    order: &LoadOrder,
    state: &mut crate::scripting::GameState,
    terminal: &Terminal,
    reference: FormId,
) -> Access {
    let locked = match state.locks.get(&reference) {
        Some(lock) => lock.is_some(),
        None => !terminal.unlocked(),
    };
    if !locked {
        return Access::Open;
    }
    let needed = terminal.science_needed();
    let science = crate::scripting::Facts {
        order,
        state,
        speaker: None,
    }
    .current_actor_value(crate::dialogue::PLAYER_REF, SCIENCE)
    .unwrap_or(0.0);
    if science < f64::from(needed) {
        return Access::NeedsScience(needed);
    }
    state.locks.insert(reference, None);
    crate::stats::bump(state, crate::stats::COMPUTERS_HACKED, 1);
    let setting = crate::experience::by_difficulty("iXPRewardHackComputer", terminal.difficulty);
    crate::experience::reward_setting(order, state, &setting);
    Access::Hacked
}

/// The Science skill's actor value.
pub const SCIENCE: u16 = 40;

/// A note's title and text (`NOTE` `FULL`, `TNAM`); text notes only.
pub fn note_text(order: &LoadOrder, id: FormId) -> Option<(String, String)> {
    let rr = order.get(id).filter(|r| r.entry.header.kind == NOTE)?;
    let record = rr.record().ok()?;
    let text = record
        .get(FourCC::new(b"TNAM"))
        .map(|s| esm::text::decode_cp1252(s.data.strip_suffix(&[0]).unwrap_or(&s.data)))?;
    Some((record.full_name().unwrap_or_default(), text))
}
