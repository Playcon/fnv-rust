//! Functions for an item's script while it's carried (its containing
//! object set, [`Runner::on_add`]): `GetContainer`, `RemoveMe`, and the
//! Caravan cards' `AddCardToPlayer`. Notes: `docs/DEAD_MONEY.md`
//! "Caravan cards".

use std::collections::BTreeSet;

use esm::{FormId, FourCC};

use super::{kind_of, placed, st};
use crate::scripting::{GameState, Runner, Value};

const CCRD: FourCC = FourCC::new(b"CCRD");

/// The functions here, by the game's own names.
pub const FUNCTIONS: &[&str] = &["GetContainer", "RemoveMe", "AddCardToPlayer"];

/// The player's Caravan cards (`AddCardToPlayer`), by card (`CCRD`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cards(pub BTreeSet<FormId>);

/// Carries out one of [`FUNCTIONS`]; `item` is the reference the script
/// runs on.
pub(super) fn carry_out(
    runner: &mut Runner,
    name: &str,
    item: Option<FormId>,
    args: &[Value],
) -> Option<f64> {
    let order = runner.order;
    let container = runner.container;
    match name {
        // `005ce5c0`: the containing object, when there's an item and a
        // container; else 0.
        "GetContainer" => Some(match (item, container) {
            (Some(_), Some(c)) => f64::from(c.0),
            _ => 0.0,
        }),
        // `005b53d0`: one of the item (its base) leaves the container, into
        // the container given if any (the containing object's RemoveItem,
        // vtable +0x17c, with count 1). Nothing without an item or a
        // container. The game's handler then returns failure, which ends
        // the script; nv-rs's scripts go on (Dead Money's call it last).
        // An actor's equipped instance (`004bfda0`) isn't told apart.
        "RemoveMe" => {
            let (Some(item), Some(from)) = (item, container) else {
                return Some(1.0);
            };
            let base = placed::base_now(order, runner.state, item)?;
            let to = args.first().map(Value::form).filter(|f| f.0 != 0);
            runner.state.stock(order, from);
            let had = runner.state.items.get(&(from, base)).copied().unwrap_or(0);
            if had > 0 {
                runner.state.items.insert((from, base), had - 1);
                if let Some(to) = to {
                    runner.state.stock(order, to);
                    *runner.state.items.entry((to, base)).or_insert(0) += 1;
                }
            }
            Some(0.0)
        }
        // `005cf3d0`: the item's base must be a Caravan card
        // (`TESCaravanCard`); it joins the player's cards unless it's there
        // already (`00969bc0`). Otherwise the game only reports it.
        "AddCardToPlayer" => {
            let base = item.and_then(|r| placed::base_now(order, runner.state, r));
            if let Some(card) = base.filter(|&b| kind_of(order, b) == Some(CCRD)) {
                st(runner).cards.0.insert(card);
            }
            Some(1.0)
        }
        _ => None,
    }
}

/// Saved lines.
pub(crate) fn save_lines(state: &GameState, line: &mut dyn FnMut(String)) {
    for c in &state.more.cards.0 {
        line(format!("caravancard {:08X}", c.0));
    }
}

/// A saved line back.
pub(crate) fn load_line(state: &mut GameState, parts: &[&str]) -> Option<Result<(), String>> {
    if *parts.first()? != "caravancard" {
        return None;
    }
    Some(
        match parts.get(1).and_then(|s| u32::from_str_radix(s, 16).ok()) {
            Some(c) => {
                state.more.cards.0.insert(FormId(c));
                Ok(())
            }
            None => Err(format!("can't read '{}'", parts.join(" "))),
        },
    )
}
