//! Menus scripts open with their data: `ShowRecipeMenu` (crafting) and the
//! casino games' `Show…MenuParams`. nv-rs has neither crafting nor the
//! casino games yet: these send what the game's menus are opened with
//! ([`Shown::RecipeMenu`], [`Shown::CasinoMenu`]) and the menu's number,
//! so its `MenuMode` blocks run. Notes: `docs/DEAD_MONEY.md` "Crafting and
//! casino menus".

use esm::{FormId, FourCC};

use super::{is_actor, kind_of, placed, Shown, TACT};
use crate::scripting::{Event, Runner, Value};

/// The functions here, by the game's own names.
pub const FUNCTIONS: &[&str] = &[
    "ShowRecipeMenu",
    "ShowSlotMachineMenuParams",
    "ShowBlackJackMenuParams",
    "ShowRouletteMenuParams",
];

/// The menus' numbers (`00a09030`'s ids, as `MenuMode` takes them).
pub const RECIPE_MENU: u16 = 1077;
pub const SLOT_MACHINE_MENU: u16 = 1080;
pub const BLACKJACK_MENU: u16 = 1081;
pub const ROULETTE_MENU: u16 = 1082;

const CSNO: FourCC = FourCC::new(b"CSNO");

/// A casino game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasinoGame {
    SlotMachine,
    Blackjack,
    Roulette,
}

impl CasinoGame {
    /// Its menu's number.
    pub fn menu(self) -> u16 {
        match self {
            CasinoGame::SlotMachine => SLOT_MACHINE_MENU,
            CasinoGame::Blackjack => BLACKJACK_MENU,
            CasinoGame::Roulette => ROULETTE_MENU,
        }
    }

    /// The game's name for it in its messages.
    fn script_name(self) -> &'static str {
        match self {
            CasinoGame::SlotMachine => "ShowSlotMachineMenu",
            CasinoGame::Blackjack => "ShowBlackJackMenu",
            // The game's roulette handler names blackjack in its second
            // message (`005cf1a0`); kept.
            CasinoGame::Roulette => "ShowRouletteMenu",
        }
    }
}

/// Carries out one of [`FUNCTIONS`]; `on` is the reference the script runs
/// on.
pub(super) fn carry_out(
    runner: &mut Runner,
    name: &str,
    on: Option<FormId>,
    args: &[Value],
) -> Option<f64> {
    let order = runner.order;
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Number(0.0));
    let game = match name {
        "ShowSlotMachineMenuParams" => CasinoGame::SlotMachine,
        "ShowBlackJackMenuParams" => CasinoGame::Blackjack,
        "ShowRouletteMenuParams" => CasinoGame::Roulette,
        // `005deb10` → `00704fc0` → `00726ff0`: the recipe menu, sold by
        // the person the script runs on, or by the speaker of the talking
        // activator it runs on (its base's +0x90); the category is
        // optional. No vendor: nothing (the game prints "Recipe menu
        // called with NULL vendor!").
        "ShowRecipeMenu" => {
            let r = on?;
            let vendor = if is_actor(order, runner.state, r) {
                Some(r)
            } else {
                placed::base_now(order, runner.state, r)
                    .filter(|&b| kind_of(order, b) == Some(TACT))
                    .and_then(|b| runner.state.more.speakers.get(&b).copied())
            };
            if let Some(vendor) = vendor {
                let category = args.first().map(Value::form).filter(|f| f.0 != 0);
                let events = &mut runner.state.events;
                events.push(Event::More(Shown::RecipeMenu { vendor, category }));
                events.push(Event::Menu(RECIPE_MENU));
            } else {
                println!("Recipe menu called with NULL vendor!  Oh, noes!");
            }
            return Some(1.0);
        }
        _ => return None,
    };
    // `005cf040`, `005cf0f0`, `005cf1a0`: the casino (`CSNO`) and three
    // numbers, handed to the game's menu (`007c0a40`, `00733630`,
    // `007bbe20`), which keeps them. No casino: only reported.
    let casino = arg(0).form();
    if casino.0 == 0 || kind_of(order, casino) != Some(CSNO) {
        println!(
            "Invalid EditorFormID used in script {} -- is not a valid EditorFormID",
            game.script_name()
        );
        return Some(1.0);
    }
    let numbers = [1, 2, 3].map(|i| arg(i).number() as i32);
    let events = &mut runner.state.events;
    events.push(Event::More(Shown::CasinoMenu {
        game,
        casino,
        numbers,
    }));
    events.push(Event::Menu(game.menu()));
    Some(1.0)
}
