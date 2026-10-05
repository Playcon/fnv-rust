//! The Pip-Boy 3000: the game's three Pip-Boy menus (`menus\main\
//! stats_menu.xml` STATS, `inventory_menu.xml` ITEMS, `map_menu.xml` DATA)
//! read and worked out by the menu system, and filled the way
//! FalloutNV.exe's menu classes fill them (`StatsMenu` vtable `0106ffd4`,
//! made by `007da2c0`; `InventoryMenu` `010739b4`, `0077fc10`; `MapMenu`
//! `01074d44`, `00796b90`): the code finds its tiles by their `id`, writes
//! the values the files read through `io()` (`user5` the health text, ...),
//! makes list rows and tab buttons from the files' templates, and moves
//! between pages as keys come in.
//!
//! With `[Pipboy] bUsePipboyMode` 1 (this install) the menus aren't drawn
//! on the screen: they're drawn into a picture (1280 × 960 menu units,
//! `007fba00`) shown on the Pip-Boy model's `pipboyscreen` (see
//! [`screen`] for how that picture is made to look like a screen).
//!
//! The game state comes in as plain values ([`PipboyInput`]; `gather`
//! reads them from the engine's state).

pub mod data;
pub mod gather;
pub mod items;
pub mod screen;
pub mod stats;

use crate::names::t;
use crate::tile::{TileId, Ui};

pub use data::DataMenu;
pub use items::ItemsMenu;
pub use stats::StatsMenu;

/// The three menus' files.
pub const STATS_FILE: &str = "menus\\main\\stats_menu.xml";
pub const ITEMS_FILE: &str = "menus\\main\\inventory_menu.xml";
pub const DATA_FILE: &str = "menus\\main\\map_menu.xml";

/// The menus' picture: the orthographic camera the rendered-menu object
/// draws them with (`007fba00`: frustum 0 .. 1280 across, 0 .. 960 down,
/// near 0, far 10000), in menu units. The Pip-Boy's screen mesh shows the
/// top left of it (its texture coordinates run 0 .. 0.75 across and 0 ..
/// 0.76 down), and the mouse's place on the screen maps back through it
/// (`007f8720`: u × 960 × 4/3, v × 960).
pub const PICTURE_SIZE: [f32; 2] = [1280.0, 960.0];

/// A stat line: a SPECIAL, a skill, a perk, a misc statistic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatLine {
    pub name: String,
    /// Shown right of the name (`user1`; none: hidden, -1 in the file).
    pub value: Option<i32>,
    pub description: String,
    /// Its picture (`ICON`), as written.
    pub icon: Option<String>,
}

/// A reputation (`REPU`): its name, title now, and picture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReputationLine {
    pub name: String,
    pub title: String,
    pub icon: Option<String>,
}

/// Which tab of the ITEMS menu an item is under (the tab line's order:
/// Weapons, Apparel, Aid, Misc, Ammo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemTab {
    Weapons = 0,
    Apparel = 1,
    Aid = 2,
    Misc = 3,
    Ammo = 4,
}

/// An item carried.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemLine {
    /// Its form ID (for the caller to act on).
    pub form: u32,
    pub name: String,
    pub count: i32,
    pub tab: ItemTab,
    pub equipped: bool,
    /// Can be equipped or used (`_EquippableItem`).
    pub usable: bool,
    pub value: i32,
    pub weight: f32,
    /// `ICON`, as written.
    pub icon: Option<String>,
    /// The item card's numbers, those it has. (`dps`: `00645380`'s value,
    /// not worked out yet; `gather` leaves it out.)
    pub damage: Option<f32>,
    pub dps: Option<f32>,
    /// A weapon's projectiles a shot (the DPS card writes "%.1fx%d" when
    /// more than one).
    pub projectiles: u32,
    pub damage_resistance: Option<f32>,
    pub damage_threshold: Option<f32>,
    /// 0 to 1.
    pub condition: Option<f32>,
    pub strength: Option<i32>,
    /// "Ammo name (in clip/rest)".
    pub ammo: Option<String>,
    /// Apparel's weight class: 0 light, 1 medium (`BMDT` general flag
    /// 0x08), 2 heavy (0x80).
    pub weight_class: Option<u8>,
    pub effects: Option<String>,
}

/// A quest in the DATA menu.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestLine {
    pub form: u32,
    pub name: String,
    pub completed: bool,
    /// The active quest (`ForceActiveQuest`, or chosen here).
    pub active: bool,
    /// Its objectives shown, and whether each is done.
    pub objectives: Vec<(String, bool)>,
}

/// A note (`NOTE` added to the Pip-Boy).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteLine {
    pub form: u32,
    pub name: String,
    pub text: String,
}

/// A map marker on the world map.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerLine {
    pub form: u32,
    pub name: String,
    /// Where on the map picture, 0 to 1 across and down (`0079c380`).
    pub at: [f32; 2],
    /// `TNAM` (1 city .. 14 vault).
    pub kind: u8,
    /// Can be travelled to (found).
    pub travel: bool,
}

/// The world map: the worldspace's picture (`ICON`), its usable size
/// (`MNAM`), markers, and the player's place and heading.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldMapLine {
    pub picture: String,
    pub size: [f32; 2],
    pub markers: Vec<MarkerLine>,
    pub player: Option<([f32; 2], f32)>,
}

/// What the Pip-Boy shows, from the game's state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PipboyInput {
    pub name: String,
    pub level: i32,
    /// The XP text: "XP/next level" (`%d/%d`), or none at the top level
    /// (the code writes `sStatsXPMax`).
    pub xp: Option<(i32, i32)>,
    pub health: (f32, f32),
    pub action_points: (f32, f32),
    /// Head, torso, left arm, right arm, left leg, right leg (actor values
    /// 25 to 30), 0 to 100.
    pub limbs: [f32; 6],
    pub rads: f32,
    pub rad_resistance: f32,
    pub hardcore: bool,
    pub effects: Vec<(String, String)>,
    pub stimpaks: i32,
    pub doctors_bags: i32,
    pub radaway: i32,
    pub radx: i32,
    /// The aid items' forms (Stimpak, Doctor's Bag, RadAway, Rad-X: default
    /// objects 0, 21, 3, 2), for the Status page's buttons, and their names
    /// (the buttons' `user0`, `007da2c0`).
    pub aid: [Option<u32>; 4],
    pub aid_names: [String; 4],
    pub special: Vec<StatLine>,
    pub skills: Vec<StatLine>,
    pub perks: Vec<StatLine>,
    pub general: Vec<StatLine>,
    /// Karma: 0 good, 1 neutral, 2 bad, 3 very good, 4 very evil (the
    /// order of `007dd090`'s pictures), the alignment's name and the
    /// karmic title.
    pub karma_band: u8,
    pub alignment: String,
    pub karma_title: String,
    pub reputations: Vec<ReputationLine>,
    pub items: Vec<ItemLine>,
    pub caps: i32,
    pub weight: (f32, f32),
    pub damage_resistance: f32,
    pub damage_threshold: f32,
    pub location: String,
    pub date_time: String,
    pub quests: Vec<QuestLine>,
    pub notes: Vec<NoteLine>,
    pub world_map: Option<WorldMapLine>,
    pub stations: Vec<String>,
}

/// The three Pip-Boy menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Stats,
    Items,
    Data,
}

/// A key as the menus see it: the codes the interface manager hands the
/// menus' key handlers (`0070f6e0` → `00717f80` → the menu's slot 0x38,
/// `007db680` and kin). On a PC keyboard (`007154b0`, `0070c4a0`): the
/// arrows give 1 up, 2 down, 4 left, 3 right, and with Shift held left and
/// right give 0x0D / 0x0E (the previous / next menu, the pad's triggers);
/// Enter gives the A button (-2: the chosen tile), with Shift the X
/// button (0x0B), with Alt the Y button (0x0C); Page Up / Page Down the
/// bumpers (0x0F / 0x10); other keys their letter, looked up as the menu's
/// `_PCButton_<letter>` trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    /// The next page or tab (code 3).
    Right,
    /// The previous one (code 4).
    Left,
    /// The next or previous menu (STATS, ITEMS, DATA: codes 0x0E / 0x0D).
    NextSection,
    PrevSection,
    /// The A button (Enter): equip, use, travel, make active.
    Activate,
    /// The X and Y buttons (Shift + Enter, Alt + Enter).
    ButtonX,
    ButtonY,
    /// A letter: the button the menu's `_PCButton_<letter>` names
    /// (`stats_stimpak_button` for S), clicked when it shows and can be
    /// clicked (`0070c4a0`).
    Letter(char),
}

/// What the Pip-Boy asks of the game.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Play a sound record (by editor ID).
    Sound(String),
    /// Equip or take off an item, or use it (aid, books).
    Equip(u32),
    Use(u32),
    /// Fast travel to a map marker (its reference).
    Travel(u32),
    /// Make a quest the active one.
    ActiveQuest(u32),
    /// Play a voice note (a `NOTE` of type 3: audio logs, holotapes).
    PlayNote(u32),
}

/// A tile found by its `id` in a menu (the menu objects keep their tiles
/// by id: `StatsMenu` 60 of them from `+0x90`, checked by `007db340`).
pub fn by_id(ui: &Ui, root: TileId, id: i32) -> Option<TileId> {
    let mut stack = vec![root];
    while let Some(tile) = stack.pop() {
        if ui.tiles[tile]
            .traits
            .get(&t::ID)
            .is_some_and(|tr| tr.value.number == id as f32 && tr.actions.is_empty())
        {
            return Some(tile);
        }
        for &c in ui.tiles[tile].children.iter().rev() {
            stack.push(c);
        }
    }
    None
}

/// A custom trait's number.
pub(crate) fn trait_id(ui: &mut Ui, name: &str) -> i32 {
    ui.names.lookup_or_add(name).unwrap_or(0)
}

/// A setting's text.
pub(crate) fn text(ui: &Ui, name: &str) -> String {
    ui.setting_text(name).unwrap_or_default()
}

/// Reads a menu file (with its prefabs) and puts it on the screen hidden.
pub(crate) fn load_menu(
    ui: &mut Ui,
    file: &str,
    read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<TileId, String> {
    let bytes = read(file).ok_or_else(|| format!("{file} not found"))?;
    ui.load_menu(&bytes, read)
        .map_err(|e| format!("{file}: {e}"))
}

/// The Pip-Boy: its three menus and which shows.
pub struct Pipboy {
    pub stats: StatsMenu,
    pub items: ItemsMenu,
    pub data: DataMenu,
    pub section: Section,
}

impl Pipboy {
    /// Reads the three menus and sets them up as their makers do.
    pub fn load(
        ui: &mut Ui,
        read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<Pipboy, String> {
        let stats = StatsMenu::load(ui, read)?;
        let items = ItemsMenu::load(ui, read)?;
        let data = DataMenu::load(ui, read)?;
        let mut p = Pipboy {
            stats,
            items,
            data,
            section: Section::Stats,
        };
        p.show(ui, Section::Stats);
        Ok(p)
    }

    /// The shown menu's tile.
    pub fn menu(&self) -> TileId {
        match self.section {
            Section::Stats => self.stats.menu,
            Section::Items => self.items.menu,
            Section::Data => self.data.menu,
        }
    }

    /// Shows one of the three (the others hidden).
    pub fn show(&mut self, ui: &mut Ui, section: Section) {
        self.section = section;
        for (menu, on) in [
            (self.stats.menu, section == Section::Stats),
            (self.items.menu, section == Section::Items),
            (self.data.menu, section == Section::Data),
        ] {
            ui.set_number(menu, t::VISIBLE, if on { 1.0 } else { 0.0 });
        }
    }

    /// Fills all three from the game's state.
    pub fn fill(&mut self, ui: &mut Ui, input: &PipboyInput) {
        self.stats.fill(ui, input);
        self.items.fill(ui, input);
        self.data.fill(ui, input);
        ui.refresh();
    }

    /// A key: moves within the shown menu or to the next one. Returns what
    /// the game should do (sounds, equipping, travel).
    pub fn key(&mut self, ui: &mut Ui, key: Key, input: &PipboyInput) -> Vec<Action> {
        let mut out = Vec::new();
        match key {
            // Round the three (`007db680`, `00782190`, `00799790`: STATS
            // goes back to DATA and on to ITEMS, DATA on to STATS).
            Key::NextSection | Key::PrevSection => {
                let order = [Section::Stats, Section::Items, Section::Data];
                let at = order.iter().position(|&s| s == self.section).unwrap_or(0);
                let next = if key == Key::NextSection {
                    (at + 1) % 3
                } else {
                    (at + 2) % 3
                };
                self.show(ui, order[next]);
                // The tab knob turning (`007fa0f0`).
                out.push(Action::Sound("UIPipBoyTab".into()));
            }
            Key::Letter(c) => {
                if let Some(id) = self.pc_button(ui, c) {
                    out.extend(self.click(ui, id, input));
                }
            }
            _ => match self.section {
                Section::Stats => out.extend(self.stats.key(ui, key, input)),
                Section::Items => out.extend(self.items.key(ui, key, input)),
                Section::Data => out.extend(self.data.key(ui, key, input)),
            },
        }
        ui.refresh();
        out
    }

    /// The `id` of the button a letter presses (`0070c4a0`): the shown
    /// menu's `_PCButton_<letter>` trait names a tile; it's pressed when it
    /// shows (`visible`) and can be (`target`).
    fn pc_button(&self, ui: &mut Ui, c: char) -> Option<i32> {
        let menu = self.menu();
        let name = format!("_PCButton_{}", c.to_ascii_uppercase());
        let trait_id = ui.names.lookup(&name)?;
        let tile_name = ui.string(menu, trait_id)?;
        let tile = ui.find(menu, tile_name.trim())?;
        (ui.number(tile, t::VISIBLE) != 0.0 && ui.number(tile, t::TARGET) != 0.0)
            .then(|| ui.number(tile, t::ID) as i32)
    }

    /// A button of the shown menu pressed (the menus' click handlers, slot
    /// 0x0C: `007db380` STATS, `00780140` ITEMS, `00796fd0` DATA).
    pub fn click(&mut self, ui: &mut Ui, id: i32, input: &PipboyInput) -> Vec<Action> {
        match self.section {
            Section::Stats => self.stats.click(ui, id, input),
            // ITEMS' lettered buttons (Repair, Mod, the keyring's Cancel)
            // and DATA's (R: `MM_ButtonY`) do what isn't here yet.
            Section::Items | Section::Data => Vec::new(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tile::{Screen, SystemColors};

    pub fn ui() -> Ui {
        crate::game::new_ui(
            &mut |_| None,
            &|_: &str, _: &str| None,
            std::collections::HashMap::new(),
            1920,
            1080,
        )
    }

    #[test]
    fn tiles_by_id_ignore_ones_worked_out() {
        let mut ui = Ui::new(
            Screen {
                width_px: 1920,
                height_px: 1080,
                safe_x: 15.0,
                safe_y: 15.0,
            },
            SystemColors::new(None, None),
            Box::new(|_| None),
        );
        let m = ui
            .load_menu(
                b"<menu name=\"m\"><rect name=\"a\"><id>5</id></rect><rect name=\"b\"><id><copy>5</copy></id></rect>
                  <rect name=\"c\"><id>0</id></rect></menu>",
                &mut |_| None,
            )
            .unwrap();
        assert_eq!(by_id(&ui, m, 5), ui.find(m, "a"));
        assert_eq!(by_id(&ui, m, 0), ui.find(m, "c"));
        assert_eq!(by_id(&ui, m, 9), None);
    }
}
