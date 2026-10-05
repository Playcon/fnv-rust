//! DATA (`map_menu.xml`, the `MapMenu` class): five tabs (Local Map,
//! World Map, Quests, Misc, Radio), the place and time along the top.
//! Filled as `00796b90` (setup), `0079cdb0` (the world map and its
//! markers) and the list code fill it.
//!
//! Not here yet: the local map (the game renders the place from above into
//! a picture), challenges, the radio's stations playing, and
//! the player's arrow turning with the heading (the menus' `rotateangle`
//! isn't drawn).

use super::{by_id, text, trait_id, Action, Key, PipboyInput, QuestLine};
use crate::listbox::ListBox;
use crate::names::t;
use crate::tabline;
use crate::tile::{TileId, Ui};

/// The map markers' pictures by `TNAM` (the table at `011a0404`: 1 city
/// .. 14 vault), and an undiscovered one's.
pub const MARKER_PICTURES: [&str; 15] = [
    "",
    "Interface\\Icons\\World Map\\icon_map_city.dds",
    "Interface\\Icons\\World Map\\icon_map_settlement.dds",
    "Interface\\Icons\\World Map\\icon_map_encampment.dds",
    "Interface\\Icons\\World Map\\icon_map_natural_landmark.dds",
    "Interface\\Icons\\World Map\\icon_map_cave.dds",
    "Interface\\Icons\\World Map\\icon_map_factory.dds",
    "Interface\\Icons\\World Map\\icon_map_monument.dds",
    "Interface\\Icons\\World Map\\icon_map_military.dds",
    "Interface\\Icons\\World Map\\icon_map_office.dds",
    "Interface\\Icons\\World Map\\icon_map_ruins_town.dds",
    "Interface\\Icons\\World Map\\icon_map_ruins_urban.dds",
    "Interface\\Icons\\World Map\\icon_map_ruins_sewer.dds",
    "Interface\\Icons\\World Map\\icon_map_metro.dds",
    "Interface\\Icons\\World Map\\icon_map_vault.dds",
];
pub const UNDISCOVERED_PICTURE: &str = "Interface\\Icons\\World Map\\icon_map_undiscovered.dds";

/// The world map's markers' size at a magnification (`0079c5a0`: the
/// magnification's share between `fWorldMapMinZoom` 0.75 and `MaxZoom` 5,
/// clamped, in a straight line from `fWorldMapMarkerMinSize` 20 to
/// `MaxSize` 50).
pub fn marker_size(magnification: f32) -> f32 {
    let (lo, hi) = (0.75, 5.0);
    let k = ((magnification - lo) / (hi - lo)).clamp(0.0, 1.0);
    20.0 + (50.0 - 20.0) * k
}

/// The DATA menu.
pub struct DataMenu {
    pub menu: TileId,
    pub tab: usize,
    pub tabline: Option<TileId>,
    pub tabs: Vec<TileId>,
    pub quests: ListBox,
    pub notes: ListBox,
    pub radio: ListBox,
    pub objectives: ListBox,
    world: Option<TileId>,
    cursor: Option<TileId>,
    data_rect: Option<TileId>,
    /// The world map's markers' tiles, and the one chosen.
    markers: Vec<TileId>,
    pub marker: Option<usize>,
    filled: Option<PipboyInput>,
}

impl DataMenu {
    /// Reads the menu, builds its tab line (`00796b90`: ids from 0x20,
    /// `sLocalMapTabText` .. `sCommsTabText`) and starts on the world map.
    pub fn load(
        ui: &mut Ui,
        read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<DataMenu, String> {
        let menu = super::load_menu(ui, super::DATA_FILE, read)?;
        let list = |ui: &mut Ui, id: i32| {
            let tile = by_id(ui, menu, id).unwrap_or(menu);
            ListBox::new(menu, tile, "MM_ListMarkerTemplate")
        };
        let quests = list(ui, 7);
        let notes = list(ui, 8);
        let radio = list(ui, 10);
        let objectives = list(ui, 15);
        let tabline_tile = by_id(ui, menu, 17);
        let labels: Vec<String> = [
            "sLocalMapTabText",
            "sWorldMapTabText",
            "sQuestsTabText",
            "sMiscTabText",
            "sCommsTabText",
        ]
        .iter()
        .map(|s| text(ui, s))
        .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let tabs = match tabline_tile {
            Some(tl) => tabline::build(ui, menu, tl, 0x20, &refs),
            None => Vec::new(),
        };
        let mut d = DataMenu {
            menu,
            tab: 1,
            tabline: tabline_tile,
            tabs,
            quests,
            notes,
            radio,
            objectives,
            world: by_id(ui, menu, 4),
            cursor: by_id(ui, menu, 5),
            data_rect: by_id(ui, menu, 13),
            markers: Vec::new(),
            marker: None,
            filled: None,
        };
        d.set_tab(ui, 1);
        Ok(d)
    }

    fn set_tab(&mut self, ui: &mut Ui, tab: usize) {
        self.tab = tab.min(4);
        if let Some(tl) = self.tabline {
            tabline::set_current(ui, tl, self.tab);
        }
        if let Some(rect) = self.data_rect {
            ui.set_number(
                rect,
                t::VISIBLE,
                if (2..=3).contains(&self.tab) {
                    1.0
                } else {
                    0.0
                },
            );
        }
    }

    /// The place and time, the map and its markers, the lists.
    pub fn fill(&mut self, ui: &mut Ui, input: &PipboyInput) {
        if let Some(tile) = by_id(ui, self.menu, 0) {
            ui.set_string(tile, t::STRING, &input.location);
        }
        if let Some(tile) = by_id(ui, self.menu, 1) {
            ui.set_string(tile, t::STRING, &input.date_time);
        }
        if self.filled.as_ref() == Some(input) {
            return;
        }
        self.fill_world_map(ui, input);
        self.fill_quests(ui, &input.quests);
        let keep = self.notes.selected;
        self.notes.clear(ui);
        for n in &input.notes {
            self.notes.add(ui, Some(&n.name));
        }
        if !input.notes.is_empty() {
            self.notes
                .select(ui, Some(keep.unwrap_or(0).min(input.notes.len() - 1)));
        }
        self.radio.clear(ui);
        for s in &input.stations {
            self.radio.add(ui, Some(s));
        }
        self.filled = Some(input.clone());
        self.show_selected(ui, input);
    }

    /// The world map (`0079cdb0`): the worldspace's picture with its
    /// usable size as the file size, a marker per map marker shown
    /// (`MapMarkerTemplate`: `_LocationName`, `id` 26, `_x`/`_y` its place,
    /// its picture by kind or the undiscovered one with `user0` 0, and
    /// `_MarkerIndex`), the player's arrow at the player's place.
    fn fill_world_map(&mut self, ui: &mut Ui, input: &PipboyInput) {
        let Some(world) = self.world else {
            return;
        };
        for m in self.markers.drain(..) {
            ui.remove(m);
        }
        let Some(map) = &input.world_map else {
            ui.set_number(world, t::VISIBLE, 0.0);
            return;
        };
        ui.set_number(world, t::VISIBLE, 1.0);
        ui.set_string(world, t::FILENAME, &map.picture);
        ui.set_number(world, t::FILEWIDTH, map.size[0]);
        ui.set_number(world, t::FILEHEIGHT, map.size[1]);
        let mag = trait_id(ui, "_Magnification");
        let magnification = ui.number(world, mag).max(0.01);
        ui.set_number(world, t::USER0, marker_size(magnification));
        // The player's arrow (`fWorldfPlayerCursorMinSize` 40 ..
        // `MaxSize` 70) and quest markers (30 .. 60), by the same rule.
        let k = ((magnification - 0.75) / (5.0 - 0.75)).clamp(0.0, 1.0);
        ui.set_number(world, t::USER0 + 1, 30.0 + 30.0 * k);
        ui.set_number(world, t::USER0 + 2, 40.0 + 30.0 * k);
        let (x_id, y_id) = (trait_id(ui, "_x"), trait_id(ui, "_y"));
        let name_id = trait_id(ui, "_LocationName");
        let index_id = trait_id(ui, "_MarkerIndex");
        for (i, m) in map.markers.iter().enumerate() {
            let Some(tile) = ui.instantiate(self.menu, world, "MapMarkerTemplate") else {
                continue;
            };
            ui.set_string(tile, name_id, &m.name);
            ui.set_number(tile, t::ID, 26.0);
            ui.set_number(tile, x_id, m.at[0]);
            ui.set_number(tile, y_id, m.at[1]);
            if m.travel {
                let picture = MARKER_PICTURES
                    .get(usize::from(m.kind))
                    .copied()
                    .unwrap_or("");
                ui.set_string(tile, t::FILENAME, picture);
            } else {
                ui.set_string(tile, t::FILENAME, UNDISCOVERED_PICTURE);
                ui.set_number(tile, t::USER0, 0.0);
            }
            ui.set_number(tile, index_id, i as f32);
            self.markers.push(tile);
        }
        if let Some(cursor) = self.cursor {
            match map.player {
                Some((at, _heading)) => {
                    ui.set_number(cursor, x_id, at[0]);
                    ui.set_number(cursor, y_id, at[1]);
                    ui.set_number(cursor, t::VISIBLE, 1.0);
                }
                None => ui.set_number(cursor, t::VISIBLE, 0.0),
            }
        }
        // Centred on the player to start with (the map's `x` keeps adding
        // the mouse's drag and stays inside the window: its own operators).
        let centre = map.player.map(|(at, _)| at);
        self.marker = None;
        if let Some(at) = centre {
            self.centre_on(ui, at);
        }
    }

    /// Moves the world map so a place on it sits in the window's middle
    /// (427.5, 250: where the highlight box is).
    fn centre_on(&mut self, ui: &mut Ui, at: [f32; 2]) {
        let Some(world) = self.world else {
            return;
        };
        let w = ui.number(world, t::WIDTH);
        let h = ui.number(world, t::HEIGHT);
        ui.set_base(world, t::X, 427.5 - at[0] * w);
        ui.set_base(world, t::Y, 250.0 - at[1] * h);
    }

    /// The quests (`MM_ListMarkerTemplate`: the active one's square filled,
    /// `_selected`). (How the game marks finished quests in the list isn't
    /// traced: they're listed as the others.)
    fn fill_quests(&mut self, ui: &mut Ui, quests: &[QuestLine]) {
        let keep = self.quests.selected;
        self.quests.clear(ui);
        let selected = trait_id(ui, "_selected");
        for q in quests {
            if let Some(row) = self.quests.add(ui, Some(&q.name)) {
                ui.set_number(row, selected, if q.active { 1.0 } else { 0.0 });
            }
        }
        if !quests.is_empty() {
            self.quests
                .select(ui, Some(keep.unwrap_or(0).min(quests.len() - 1)));
        }
    }

    /// What's chosen shows in the data rectangle: a quest's objectives
    /// (`_ItemType` 4, each a row, done ones' squares filled), a note's
    /// text (1); the world map's chosen marker's name in the highlight box.
    fn show_selected(&mut self, ui: &mut Ui, input: &PipboyInput) {
        let item_type = trait_id(ui, "_ItemType");
        let Some(rect) = self.data_rect else {
            return;
        };
        match self.tab {
            2 => {
                ui.set_number(rect, item_type, 4.0);
                self.objectives.clear(ui);
                let selected = trait_id(ui, "_selected");
                let show_empty = trait_id(ui, "_ShowEmptyMarker");
                if let Some(q) = self.quests.selected.and_then(|i| input.quests.get(i)) {
                    for (text, done) in &q.objectives {
                        if let Some(row) = self.objectives.add(ui, Some(text)) {
                            ui.set_number(row, show_empty, 1.0);
                            ui.set_number(row, selected, if *done { 1.0 } else { 0.0 });
                        }
                    }
                }
            }
            3 => {
                ui.set_number(rect, item_type, 1.0);
                if let (Some(n), Some(text)) = (
                    self.notes.selected.and_then(|i| input.notes.get(i)),
                    ui.find_below(rect, "MM_DataText"),
                ) {
                    ui.set_string(text, t::STRING, &n.text);
                }
            }
            _ => ui.set_number(rect, item_type, 0.0),
        }
        if let Some(highlight) = by_id(ui, self.menu, 6) {
            let title = trait_id(ui, "_Title");
            let name = self
                .marker
                .and_then(|i| input.world_map.as_ref()?.markers.get(i))
                .map(|m| m.name.clone())
                .unwrap_or_default();
            ui.set_string(highlight, title, &name);
        }
    }

    /// Shows a tab (0 Local Map .. 4 Radio).
    pub fn show_tab(&mut self, ui: &mut Ui, tab: usize, input: &PipboyInput) {
        self.set_tab(ui, tab);
        self.show_selected(ui, input);
    }

    /// A key (`00799790`): left and right change tab, wrapping round the
    /// five; up and down choose in the Quests, Misc and Radio lists; the A
    /// button on the world map presses the marker under the cursor (the
    /// map's markers are reached with the mouse or the stick: neither is
    /// here yet, so none is chosen), on a quest makes it the active one.
    pub fn key(&mut self, ui: &mut Ui, key: Key, input: &PipboyInput) -> Vec<Action> {
        let mut out = Vec::new();
        match key {
            Key::Left | Key::Right => {
                let tab = if key == Key::Right {
                    (self.tab + 1) % 5
                } else {
                    (self.tab + 4) % 5
                };
                self.set_tab(ui, tab);
                self.show_selected(ui, input);
                out.push(Action::Sound("UIPipBoyTab".into()));
            }
            Key::Up | Key::Down => {
                let by = if key == Key::Down { 1 } else { -1 };
                let list = match self.tab {
                    2 => Some(&mut self.quests),
                    3 => Some(&mut self.notes),
                    4 => Some(&mut self.radio),
                    _ => None,
                };
                if let Some(list) = list {
                    let before = list.selected;
                    list.step(ui, by);
                    if list.selected != before {
                        out.push(Action::Sound("UIPipBoyScroll".into()));
                    }
                }
                self.show_selected(ui, input);
            }
            Key::Activate => match self.tab {
                1 => {
                    if let Some(m) = self
                        .marker
                        .and_then(|i| input.world_map.as_ref()?.markers.get(i))
                    {
                        out.push(Action::Travel(m.form));
                    }
                }
                2 => {
                    if let Some(q) = self.quests.selected.and_then(|i| input.quests.get(i)) {
                        out.push(Action::ActiveQuest(q.form));
                    }
                }
                3 => {
                    if let Some(n) = self.notes.selected.and_then(|i| input.notes.get(i)) {
                        out.push(Action::PlayNote(n.form));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipboy::{MarkerLine, WorldMapLine};

    /// `map_menu.xml` cut down to the tiles the code finds by `id` and
    /// name.
    const MENU: &str = r#"<menu name="MapMenu"><locus>&true;</locus>
      <text name="location"><id>0</id></text><text name="time"><id>1</id></text>
      <image name="world"><id>4</id><width>855</width><height>500</height><_Magnification>1</_Magnification></image>
      <image name="cursor"><id>5</id></image>
      <rect name="highlight"><id>6</id></rect>
      <hotrect name="quests"><id>7</id><x>0</x><y>100</y><width>300</width><height>400</height>
        <_scroll_delta>0</_scroll_delta><_highlight_y>-1</_highlight_y><_selected_height>0</_selected_height>
        <image name="lb_scrollbar"><_number_of_items>1</_number_of_items><_current_value>0</_current_value></image>
      </hotrect>
      <rect name="data"><id>13</id>
        <hotrect name="objectives"><id>15</id><x>0</x><y>0</y><width>300</width><height>400</height>
          <_scroll_delta>0</_scroll_delta><_highlight_y>-1</_highlight_y><_selected_height>0</_selected_height>
          <image name="lb_scrollbar"><_number_of_items>1</_number_of_items><_current_value>0</_current_value></image>
        </hotrect>
      </rect>
      <template name="MM_ListMarkerTemplate"><hotrect name="row"><height>30</height></hotrect></template>
      <template name="MapMarkerTemplate"><image name="marker"><user0>1</user0></image></template>
    </menu>"#;

    fn input() -> PipboyInput {
        let marker = |form: u32, name: &str, travel: bool| MarkerLine {
            form,
            name: name.into(),
            at: [0.25, 0.75],
            kind: 2,
            travel,
        };
        PipboyInput {
            location: "Goodsprings".into(),
            world_map: Some(WorldMapLine {
                picture: "interface\\worldmap\\test.dds".into(),
                size: [1000.0, 800.0],
                markers: vec![
                    marker(0x900, "Unknown Place", false),
                    marker(0x901, "Goodsprings", true),
                ],
                player: Some(([0.5, 0.5], 90.0)),
            }),
            quests: vec![QuestLine {
                form: 0x700,
                name: "Back in the Saddle".into(),
                completed: false,
                active: false,
                objectives: vec![("Meet Sunny".into(), true), ("Shoot bottles".into(), false)],
            }],
            notes: vec![crate::pipboy::NoteLine {
                form: 0x800,
                name: "Dog Command Tape".into(),
                text: String::new(),
            }],
            ..PipboyInput::default()
        }
    }

    fn load(ui: &mut Ui) -> DataMenu {
        let mut read = |p: &str| (p == crate::pipboy::DATA_FILE).then(|| MENU.as_bytes().to_vec());
        DataMenu::load(ui, &mut read).unwrap()
    }

    #[test]
    fn the_world_map_gets_its_picture_markers_and_the_player() {
        let mut ui = crate::pipboy::tests::ui();
        let mut d = load(&mut ui);
        let input = input();
        d.fill(&mut ui, &input);
        assert_eq!(d.tab, 1);
        let world = by_id(&ui, d.menu, 4).unwrap();
        assert_eq!(ui.number(world, t::FILEWIDTH), 1000.0);
        assert_eq!(ui.number(world, t::FILEHEIGHT), 800.0);
        // Markers' size at magnification 1: 20 + 30 × 0.25 / 4.25.
        assert!((ui.number(world, t::USER0) - (20.0 + 30.0 * 0.25 / 4.25)).abs() < 1e-4);
        assert_eq!(d.markers.len(), 2);
        let (x, y) = (
            ui.names.lookup("_x").unwrap(),
            ui.names.lookup("_y").unwrap(),
        );
        let found = d.markers[1];
        assert_eq!(ui.string(found, t::FILENAME).unwrap(), MARKER_PICTURES[2]);
        assert_eq!((ui.number(found, x), ui.number(found, y)), (0.25, 0.75));
        let unknown = d.markers[0];
        assert_eq!(
            ui.string(unknown, t::FILENAME).unwrap(),
            UNDISCOVERED_PICTURE
        );
        assert_eq!(ui.number(unknown, t::USER0), 0.0);
        let cursor = by_id(&ui, d.menu, 5).unwrap();
        assert_eq!(ui.number(cursor, x), 0.5);
        // Nothing is under a cursor: the A button presses no marker.
        assert!(d.key(&mut ui, Key::Activate, &input).is_empty());
        // Left from the world map: the local map; again: round to Radio.
        d.key(&mut ui, Key::Left, &input);
        assert_eq!(d.tab, 0);
        d.key(&mut ui, Key::Left, &input);
        assert_eq!(d.tab, 4);
        let location = by_id(&ui, d.menu, 0).unwrap();
        assert_eq!(ui.string(location, t::STRING).unwrap(), "Goodsprings");
    }

    #[test]
    fn a_note_chosen_in_misc_is_played() {
        let mut ui = crate::pipboy::tests::ui();
        let mut d = load(&mut ui);
        let input = input();
        d.fill(&mut ui, &input);
        d.show_tab(&mut ui, 3, &input);
        assert_eq!(
            d.key(&mut ui, Key::Activate, &input),
            [Action::PlayNote(0x800)]
        );
    }

    #[test]
    fn quests_list_their_objectives_and_can_be_made_active() {
        let mut ui = crate::pipboy::tests::ui();
        let mut d = load(&mut ui);
        let input = input();
        d.fill(&mut ui, &input);
        d.show_tab(&mut ui, 2, &input);
        let rect = by_id(&ui, d.menu, 13).unwrap();
        assert_eq!(ui.number(rect, t::VISIBLE), 1.0);
        let item_type = ui.names.lookup("_ItemType").unwrap();
        assert_eq!(ui.number(rect, item_type), 4.0);
        assert_eq!(d.quests.rows.len(), 1);
        assert_eq!(d.objectives.rows.len(), 2);
        // Done objectives' squares filled.
        let selected = ui.names.lookup("_selected").unwrap();
        let done: Vec<f32> = d
            .objectives
            .rows
            .clone()
            .into_iter()
            .map(|r| ui.number(r, selected))
            .collect();
        assert_eq!(done, [1.0, 0.0]);
        assert_eq!(
            d.key(&mut ui, Key::Activate, &input),
            [Action::ActiveQuest(0x700)]
        );
        // The world map's tab hides the data rectangle.
        d.key(&mut ui, Key::Left, &input);
        assert_eq!(ui.number(rect, t::VISIBLE), 0.0);
    }
}
