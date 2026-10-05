//! The Pip-Boy's values read from the engine's game state (`world`): what
//! the menus' code reads from the player in the game (actor values,
//! inventory, perks, quests, notes, map markers).

use esm::{FormId, FourCC, LoadOrder};
use world::dialogue::PLAYER_REF;
use world::scripting::{Facts, GameState};

use super::{
    ItemLine, ItemTab, MarkerLine, NoteLine, PipboyInput, QuestLine, ReputationLine, StatLine,
    WorldMapLine,
};

const WEAP: FourCC = FourCC::new(b"WEAP");
const ARMO: FourCC = FourCC::new(b"ARMO");
const ICON: FourCC = FourCC::new(b"ICON");
const DESC: FourCC = FourCC::new(b"DESC");
const DNAM: FourCC = FourCC::new(b"DNAM");
const MNAM: FourCC = FourCC::new(b"MNAM");
const TNAM: FourCC = FourCC::new(b"TNAM");
const REPU: FourCC = FourCC::new(b"REPU");

/// Where the player is, for the DATA menu.
#[derive(Debug, Clone, Default)]
pub struct Whereabouts {
    /// The place's name (the cell's, or the worldspace's outdoors).
    pub location: String,
    /// The worldspace the world map shows, and its map markers.
    pub world: Option<FormId>,
    pub markers: Vec<world::map::MapMarker>,
    /// The player's feet and heading (degrees clockwise from north).
    pub player: Option<([f32; 3], f32)>,
}

fn record_text(order: &LoadOrder, id: FormId, sub: FourCC) -> Option<String> {
    order
        .get(id)?
        .record()
        .ok()?
        .get(sub)
        .map(|s| s.zstring())
        .filter(|s| !s.is_empty())
}

fn record_name(order: &LoadOrder, id: FormId) -> Option<String> {
    order.get(id)?.record().ok()?.full_name()
}

/// An actor value's line: its `AVIF` name, picture and description.
fn actor_value_line(order: &LoadOrder, facts: &Facts, av: u16) -> StatLine {
    let script_name = script::ACTOR_VALUES
        .get(usize::from(av))
        .copied()
        .unwrap_or("?");
    let avif = order.form_by_editor_id(&format!("AV{script_name}"));
    let value = facts.current_actor_value(PLAYER_REF, av).unwrap_or(0.0);
    StatLine {
        name: world::chargen::actor_value_name(order, av),
        value: Some(value.floor() as i32),
        description: avif
            .and_then(|a| record_text(order, a, DESC))
            .unwrap_or_default(),
        icon: avif.and_then(|a| record_text(order, a, ICON)),
    }
}

/// Which ITEMS tab an item goes under, or none (`00782620`, the list's
/// filter): keys (`KEYM`) never (they're on the keyring), nor apparel
/// whose `BMDT` general flags have 0x40 (not playable: the Pip-Boy 3000
/// and its glove, `00480d90`); weapons `WEAP`; apparel `ARMO` (and
/// Fallout 3's `CLOT`); aid `ALCH`, `INGR` and books `BOOK`; ammunition
/// `AMMO` except what the `RegeneratingAmmo` form list holds (energy
/// weapons' own charge); misc everything else. Caps and casino chips
/// aren't listed (the headline shows the caps; where the code leaves
/// them out isn't traced). Also in the filter, not here: items whose
/// form flag 0x400 is set unless a flag on the player (+0x240, not
/// traced) is.
pub fn item_tab(order: &LoadOrder, state: &GameState, item: FormId) -> Option<ItemTab> {
    let rr = order.get(item)?;
    let kind = rr.entry.header.kind;
    match kind.as_bytes() {
        b"KEYM" | b"CHIP" => None,
        b"WEAP" => Some(ItemTab::Weapons),
        b"ARMO" | b"CLOT" => {
            let record = rr.record().ok()?;
            let playable = record
                .get(FourCC::new(b"BMDT"))
                .and_then(|s| s.data.get(4).copied())
                .map_or(true, |flags| flags & 0x40 == 0);
            playable.then_some(ItemTab::Apparel)
        }
        b"ALCH" | b"INGR" | b"BOOK" => Some(ItemTab::Aid),
        b"AMMO" => {
            let regenerating = order
                .form_by_editor_id("RegeneratingAmmo")
                .is_some_and(|list| {
                    world::script_functions::form_list(order, state, list).contains(&item)
                });
            (!regenerating).then_some(ItemTab::Ammo)
        }
        _ => Some(ItemTab::Misc),
    }
}

/// The calendar line (`0079aba0`: "%s, %d:%02d" with the date, the whole
/// hours of `GameHour` (24-hour) and its fraction × 60, both truncated;
/// the date `00867970`: "%02d.%02d.%02d" with `GameMonth` + 1, `GameDay`
/// and `GameYear` % 100: "10.18.81, 12:00").
pub fn date_time(state: &GameState, order: &LoadOrder) -> String {
    let g = |n: &str, d: f32| state.global(order, n).unwrap_or(d);
    let hour = g("GameHour", 9.0);
    let whole = hour.trunc();
    let (h, m) = (whole as i32, ((hour - whole) * 60.0) as i32);
    format!(
        "{:02}.{:02}.{:02}, {}:{:02}",
        g("GameMonth", 9.0) as i32 + 1,
        g("GameDay", 19.0) as i32,
        (g("GameYear", 2281.0) as i32).rem_euclid(100),
        h,
        m
    )
}

/// A default object (the `DOBJ` record `DefaultObjectManager`: its `DATA`
/// an array of forms, `0058db10` reading slot n): 0 the Stimpak, 2 Rad-X,
/// 3 RadAway, 21 the Doctor's Bag (the STATS menu's aid buttons,
/// `007da2c0`).
pub fn default_object(order: &LoadOrder, index: usize) -> Option<FormId> {
    let rr = order.records_of_type(FourCC::new(b"DOBJ")).next()?;
    let rr = order.get(rr.form_id)?;
    let record = rr.record().ok()?;
    let data = record.get(esm::sig::DATA)?.data.clone();
    let at = index * 4;
    let raw = u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?);
    (raw != 0).then(|| rr.plugin.to_global(FormId(raw)))
}

/// The world map for a worldspace: its `ICON` picture, `MNAM` (usable
/// width and height u32, then the north-west and south-east cells' x, y
/// as i16), and where its markers and the player land on it (`0079cdb0`,
/// `0079c380`: north-west corner (x × 4096, y × 4096 + 4096), south-east
/// ((x + 1) × 4096, y × 4096); a place's share across and down between
/// them).
pub fn world_map(order: &LoadOrder, state: &GameState, at: &Whereabouts) -> Option<WorldMapLine> {
    let world = at.world?;
    let record = order.get(world)?.record().ok()?;
    let picture = record.get(ICON).map(|s| s.zstring())?;
    let mnam = record.get(MNAM).filter(|s| s.data.len() >= 16)?;
    let d = &mnam.data;
    let u32_at = |i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
    let i16_at = |i: usize| i16::from_le_bytes([d[i], d[i + 1]]) as f32;
    let size = [u32_at(0) as f32, u32_at(4) as f32];
    let nw = [i16_at(8) * 4096.0, i16_at(10) * 4096.0 + 4096.0];
    let se = [i16_at(12) * 4096.0 + 4096.0, i16_at(14) * 4096.0];
    let place = |p: [f32; 3]| {
        [
            (p[0] - nw[0]) / (se[0] - nw[0]),
            (p[1] - nw[1]) / (se[1] - nw[1]),
        ]
    };
    let markers = at
        .markers
        .iter()
        .filter(|m| world::map::shown(state, m))
        .map(|m| MarkerLine {
            form: m.reference.0,
            name: m.name.clone(),
            at: place(m.position),
            kind: m.kind,
            travel: world::map::can_travel(state, m),
        })
        .collect();
    Some(WorldMapLine {
        picture,
        size,
        markers,
        player: at.player.map(|(p, h)| (place(p), h)),
    })
}

/// Everything the Pip-Boy shows, from the game's state.
pub fn gather(order: &LoadOrder, state: &GameState, at: &Whereabouts) -> PipboyInput {
    let facts = Facts {
        order,
        state,
        speaker: None,
    };
    let av = |n: u16| facts.current_actor_value(PLAYER_REF, n).unwrap_or(0.0) as f32;
    let perm = |n: u16| facts.permanent_actor_value(PLAYER_REF, n).unwrap_or(0.0) as f32;
    let level = i32::from(state.player_level);
    let max_level = i32::from(world::experience::max_level(order));
    let xp = world::experience::xp(state) as i32;
    let next = world::experience::xp_for_level(order, state.player_level + 1) as i32;
    let health = (
        world::combat::health(order, state, PLAYER_REF).unwrap_or(0.0) as f32,
        world::combat::max_health(order, state, PLAYER_REF).unwrap_or(0.0) as f32,
    );
    let limbs = [25u16, 26, 27, 28, 29, 30].map(&av);

    // SPECIAL (actor values 5 .. 11) and the skills the menus show.
    let special = world::chargen::SPECIAL
        .iter()
        .map(|&a| actor_value_line(order, &facts, a))
        .collect();
    let skills = world::chargen::SHOWN_SKILLS
        .iter()
        .map(|&a| actor_value_line(order, &facts, a))
        .collect();

    // Perks held, not hidden (`007dd710`: "%s", "%s (%d)" from rank 2),
    // sorted by name (`007e04e0` compares their names).
    let mut perks: Vec<StatLine> = state
        .perks
        .iter()
        .filter(|&&p| world::perks::perk_data(order, p).is_some_and(|d| !d.hidden))
        .filter_map(|&p| {
            let name = record_name(order, p)?;
            let rank = world::perks::rank(state, p);
            Some(StatLine {
                name: if rank >= 2 {
                    format!("{name} ({rank})")
                } else {
                    name
                },
                value: None,
                description: record_text(order, p, DESC).unwrap_or_default(),
                icon: record_text(order, p, ICON),
            })
        })
        .collect();
    perks.sort_by(|a, b| a.name.cmp(&b.name));

    // The misc statistics (`007da2c0`'s loop over the 43 of them).
    let general = world::stats::NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| StatLine {
            name: name.to_string(),
            value: Some(world::stats::get(state, i as u8) as i32),
            ..StatLine::default()
        })
        .collect();

    // Karma and reputations (those with fame or infamy, `007dd8c0`).
    let karma = world::reputation::karma(order, state);
    let karma_band = world::reputation::alignment(order, karma);
    let alignment_setting = match karma_band {
        0 => "sAlignGood",
        2 => "sAlignEvil",
        3 => "sAlignVeryGood",
        4 => "sAlignVeryEvil",
        _ => "sAlignNeutral",
    };
    let alignment = world::scripting::game_setting_text(order, alignment_setting)
        .or_else(|| crate::game::exe_text_setting(alignment_setting).map(str::to_string))
        .unwrap_or_default();
    let mut reputations: Vec<ReputationLine> = order
        .records_of_type(REPU)
        .filter(|rr| {
            world::reputation::get(state, rr.form_id, world::reputation::FAME) > 0.0
                || world::reputation::get(state, rr.form_id, world::reputation::INFAMY) > 0.0
        })
        .map(|rr| {
            let (fame, infamy) = world::reputation::levels(order, state, rr.form_id);
            ReputationLine {
                name: record_name(order, rr.form_id).unwrap_or_default(),
                title: world::reputation::title(order, fame, infamy),
                icon: record_text(order, rr.form_id, ICON),
            }
        })
        .collect();
    reputations.sort_by(|a, b| a.name.cmp(&b.name));

    // What's carried.
    let caps_form = world::barter::caps(order);
    let mut items = Vec::new();
    let mut caps = 0;
    // The aid buttons' items: default objects 0, 21, 3, 2 (in the order of
    // `PipboyInput::aid`: Stimpak, Doctor's Bag, RadAway, Rad-X).
    let aid_forms = [0, 21, 3, 2].map(|i| default_object(order, i));
    let mut aid_counts = [0i32; 4];
    for (item, count) in state.inventory(order, PLAYER_REF) {
        if item == caps_form {
            caps += count;
            continue;
        }
        if let Some(slot) = aid_forms.iter().position(|f| *f == Some(item)) {
            aid_counts[slot] += count;
        }
        let Some(rr) = order.get(item) else { continue };
        let kind = rr.entry.header.kind;
        let Some(tab) = item_tab(order, state, item) else {
            continue;
        };
        let info = world::items::item_info(order, item);
        let name = info
            .as_ref()
            .map(|i| i.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| item.to_string());
        let mut line = ItemLine {
            form: item.0,
            name,
            count,
            tab,
            equipped: state.is_equipped(PLAYER_REF, item),
            usable: matches!(
                kind.as_bytes(),
                b"WEAP" | b"ARMO" | b"ALCH" | b"INGR" | b"BOOK"
            ),
            value: info.as_ref().map_or(0, |i| i.value),
            weight: info.as_ref().map_or(0.0, |i| i.weight),
            icon: record_text(order, item, ICON),
            damage: None,
            dps: None,
            projectiles: 1,
            damage_resistance: None,
            damage_threshold: None,
            condition: None,
            strength: None,
            ammo: None,
            weight_class: None,
            effects: None,
        };
        if kind == WEAP {
            if let Some(w) = world::combat::Weapon::load(order, item) {
                // The damage card (`006450f0`, drawn at the file's alpha 0):
                // the hit's damage (`00644ce0`, `world::combat::
                // hit_damage`); `006450f0` then applies the ammunition's
                // damage effects and perk entry 0, not done here.
                let damage = world::combat::hit_damage(order, state, PLAYER_REF, &w);
                line.damage = Some(damage);
                let ammo = w.ammo_in_use(order, state, PLAYER_REF);
                line.projectiles = w.shot(order, ammo).0.max(1);
                // The DPS card's value (`00645380`: the damage with the
                // skill, condition and critical terms, over the time a clip
                // takes with its reload) isn't worked out yet: left empty.
                line.dps = None;
                line.condition = Some(world::combat::weapon_condition(state, PLAYER_REF, item));
                if let Some(a) = ammo.or_else(|| w.ammo.first().copied()) {
                    let held = state.item_count(order, PLAYER_REF, a);
                    // In the clip: the equipped weapon's own clip in the
                    // game; here as much as a full clip of what's carried.
                    let in_clip = (w.clip as i32).min(held);
                    // The ammunition's short name (`ONAM`; "9mm"), else
                    // its name (`00663b40`, then `00408da0` when that's
                    // empty).
                    let name = record_text(order, a, FourCC::new(b"ONAM"))
                        .or_else(|| record_name(order, a));
                    if let Some(n) = name {
                        // "%s (%i/%i)": in the clip and the rest.
                        line.ammo = Some(format!("{n} ({in_clip}/{})", held - in_clip));
                    }
                }
            }
            // The strength needed: `DNAM` u32 at 168 (the weapon's +0x19c,
            // `00663b60`; the same field V.A.T.S.'s wobble reads), less
            // perk entry 53's reduction (not applied here).
            line.strength = order
                .get(item)
                .and_then(|r| r.record().ok())
                .and_then(|r| {
                    let d = r.get(DNAM)?.data.clone();
                    (d.len() >= 172)
                        .then(|| u32::from_le_bytes([d[168], d[169], d[170], d[171]]) as i32)
                })
                .filter(|&s| s > 0);
        } else if kind == ARMO {
            // `DNAM`: DR i16 at 0, DT f32 at 4.
            if let Some(d) = order
                .get(item)
                .and_then(|r| r.record().ok())
                .and_then(|r| r.get(DNAM).map(|s| s.data.clone()))
            {
                if d.len() >= 8 {
                    line.damage_resistance = Some(f32::from(i16::from_le_bytes([d[0], d[1]])));
                    line.damage_threshold = Some(f32::from_le_bytes([d[4], d[5], d[6], d[7]]));
                }
            }
            // The weight class (`00707e30`: the biped data's general flags,
            // 0x80 heavy (`004c0bd0`), 0x08 medium (`00514450`), else
            // light).
            let flags = order
                .get(item)
                .and_then(|r| r.record().ok())
                .and_then(|r| {
                    r.get(FourCC::new(b"BMDT"))
                        .and_then(|s| s.data.get(4).copied())
                })
                .unwrap_or(0);
            line.weight_class = Some(if flags & 0x80 != 0 {
                2
            } else if flags & 0x08 != 0 {
                1
            } else {
                0
            });
            line.condition = Some(1.0);
        }
        // Aid's and ammunition's effects text (`00406620`, `00503a70`: the
        // effects with their magnitudes and durations, joined) isn't
        // written yet, so their effects card stays hidden, as the game's
        // does for an item without one.
        items.push(line);
    }

    // Quests with objectives shown, each with its objectives in their
    // order (the list's own order isn't traced: by form ID here).
    let mut quests: Vec<QuestLine> = Vec::new();
    for (&(q, index), &done) in &state.objectives {
        let text = world::quest::Quest::load(order, q)
            .and_then(|qu| qu.objective(index).map(|o| o.text.clone()))
            .unwrap_or_else(|| format!("objective {index}"));
        match quests.iter_mut().find(|l| l.form == q.0) {
            Some(l) => l.objectives.push((text, done)),
            None => quests.push(QuestLine {
                form: q.0,
                name: record_name(order, q).unwrap_or_else(|| q.to_string()),
                completed: state.completed.contains(&q),
                active: state.active_quest == Some(q),
                objectives: vec![(text, done)],
            }),
        }
    }

    // Notes (`NOTE`: `FULL`, the text in `TNAM`; listed by name, the list's
    // order not traced).
    let mut notes: Vec<NoteLine> = state
        .notes
        .iter()
        .map(|&n| NoteLine {
            form: n.0,
            name: record_name(order, n).unwrap_or_else(|| n.to_string()),
            text: record_text(order, n, TNAM).unwrap_or_default(),
        })
        .collect();
    notes.sort_by(|a, b| a.name.cmp(&b.name));

    // Effects on the player (`007ddf00`): a row per source, its
    // name, then its actor value changes as "%s %+d" (the `AVIF`'s `ANAM`
    // abbreviation: "STR +2"), joined with ", ". The effects the code
    // writes as text of their own aren't listed (which text isn't traced).
    let mut effects: Vec<(String, String)> = Vec::new();
    for e in state
        .active_effects
        .iter()
        .filter(|e| e.target == PLAYER_REF)
    {
        let Some(source) = record_name(order, e.source) else {
            continue;
        };
        if e.actor_value < 0 {
            continue;
        }
        let script_name = script::ACTOR_VALUES
            .get(e.actor_value as usize)
            .copied()
            .unwrap_or("?");
        let Some(abbreviation) = order
            .form_by_editor_id(&format!("AV{script_name}"))
            .and_then(|a| record_text(order, a, FourCC::new(b"ANAM")))
        else {
            continue;
        };
        let amount = if e.detrimental {
            -(e.magnitude.abs() as i32)
        } else {
            e.magnitude.abs() as i32
        };
        let part = format!("{abbreviation} {amount:+}");
        match effects.iter_mut().find(|(s, _)| *s == source) {
            Some((_, what)) => {
                what.push_str(", ");
                what.push_str(&part);
            }
            None => effects.push((source, part)),
        }
    }

    PipboyInput {
        name: world::chargen::player_name(order, state),
        level,
        xp: (level < max_level).then_some((xp, next)),
        health,
        action_points: (av(12), perm(12)),
        limbs,
        rads: av(54),
        rad_resistance: av(20),
        hardcore: false,
        effects,
        stimpaks: aid_counts[0],
        doctors_bags: aid_counts[1],
        radaway: aid_counts[2],
        radx: aid_counts[3],
        aid: aid_forms.map(|f| f.map(|f| f.0)),
        aid_names: aid_forms.map(|f| f.and_then(|f| record_name(order, f)).unwrap_or_default()),
        special,
        skills,
        perks,
        general,
        karma_band,
        alignment,
        karma_title: world::reputation::karmic_title(order, state).unwrap_or_default(),
        reputations,
        items,
        caps,
        // Carried against Carry Weight (actor value 13).
        weight: (state.inventory_weight(order, PLAYER_REF), perm(13)),
        damage_resistance: av(18),
        damage_threshold: world::combat::damage_threshold(order, state, PLAYER_REF),
        location: at.location.clone(),
        date_time: date_time(state, order),
        quests,
        notes,
        world_map: world_map(order, state, at),
        stations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testdata::{group, record, sub, zstr};

    /// A Data folder with the item kinds the list's filter tells apart and
    /// a worldspace with a map, removed afterwards.
    struct Plugin(std::path::PathBuf);

    impl Drop for Plugin {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn plugin(tag: &str) -> (Plugin, LoadOrder) {
        let dir = std::env::temp_dir().join(format!("nv-rs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let edid = |s: &str| sub(b"EDID", &zstr(s));
        let with = |name: &str, more: &[u8]| {
            let mut d = edid(name);
            d.extend(more);
            d
        };
        // `BMDT`: biped flags u32, general flags u8 (0x40 not playable).
        let bmdt = |general: u8| sub(b"BMDT", &[0x04, 0, 0, 0, general, 0, 0, 0]);
        let mut armo = record(b"ARMO", 0x800, &with("TestArmor", &bmdt(0x00)));
        armo.extend(record(b"ARMO", 0x801, &with("TestPipBoy", &bmdt(0x40))));
        let keym = record(b"KEYM", 0x802, &edid("TestKey"));
        let mut ammo = record(b"AMMO", 0x803, &edid("TestCharge"));
        ammo.extend(record(b"AMMO", 0x804, &edid("TestRound")));
        let flst = record(
            b"FLST",
            0x805,
            &with("RegeneratingAmmo", &sub(b"LNAM", &0x803u32.to_le_bytes())),
        );
        let book = record(b"BOOK", 0x806, &edid("TestBook"));
        let ingr = record(b"INGR", 0x807, &edid("TestHerb"));
        let misc = record(b"MISC", 0x808, &edid("TestCan"));
        let weap = record(b"WEAP", 0x809, &edid("TestGun"));
        // A map 1000 × 800 across cells -2, 2 (north-west) to 1, -1.
        let mut mnam = 1000u32.to_le_bytes().to_vec();
        mnam.extend(800u32.to_le_bytes());
        for v in [-2i16, 2, 1, -1] {
            mnam.extend(v.to_le_bytes());
        }
        let mut w = with(
            "TestWorld",
            &sub(b"ICON", &zstr("interface\\worldmap\\test.dds")),
        );
        w.extend(sub(b"MNAM", &mnam));
        let wrld = record(b"WRLD", 0x810, &w);
        let mut hedr = 1.34f32.to_le_bytes().to_vec();
        hedr.extend([0; 8]);
        let mut p = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        for (label, contents) in [
            (*b"ARMO", armo),
            (*b"KEYM", keym),
            (*b"AMMO", ammo),
            (*b"FLST", flst),
            (*b"BOOK", book),
            (*b"INGR", ingr),
            (*b"MISC", misc),
            (*b"WEAP", weap),
            (*b"WRLD", wrld),
        ] {
            p.extend(group(label, 0, &contents));
        }
        std::fs::write(dir.join("FalloutNV.esm"), p).unwrap();
        let order = LoadOrder::from_data_dir(&dir, &esm::ActivePlugins::OfficialOnly).unwrap();
        (Plugin(dir), order)
    }

    #[test]
    fn items_go_under_the_tabs_the_lists_filter_puts_them() {
        let (_dir, order) = plugin("pipboy-item-tabs");
        let state = GameState::new(&order);
        let tab = |id: u32| item_tab(&order, &state, FormId(id));
        assert_eq!(tab(0x800), Some(ItemTab::Apparel));
        // Not playable (the Pip-Boy and its glove), keys: not listed.
        assert_eq!(tab(0x801), None);
        assert_eq!(tab(0x802), None);
        // An energy weapon's own charge (`RegeneratingAmmo`) isn't either.
        assert_eq!(tab(0x803), None);
        assert_eq!(tab(0x804), Some(ItemTab::Ammo));
        // Books and ingredients are aid.
        assert_eq!(tab(0x806), Some(ItemTab::Aid));
        assert_eq!(tab(0x807), Some(ItemTab::Aid));
        assert_eq!(tab(0x808), Some(ItemTab::Misc));
        assert_eq!(tab(0x809), Some(ItemTab::Weapons));
    }

    #[test]
    fn places_land_on_the_world_map_between_its_corner_cells() {
        let (_dir, order) = plugin("pipboy-world-map");
        let state = GameState::new(&order);
        let marker = |reference: u32, position: [f32; 3], flags: u8| world::map::MapMarker {
            reference: FormId(reference),
            name: format!("{reference:X}"),
            position,
            radius: 1000.0,
            flags,
            kind: 2,
            arrival: None,
        };
        let at = Whereabouts {
            location: "Here".into(),
            world: Some(FormId(0x810)),
            // One shown from the start (flag 0x01), one not found yet.
            markers: vec![
                marker(0x900, [0.0, 0.0, 0.0], 0x01),
                marker(0x901, [0.0, 0.0, 0.0], 0),
            ],
            player: Some(([-8192.0, 12288.0, 0.0], 45.0)),
        };
        let map = world_map(&order, &state, &at).unwrap();
        assert_eq!(map.picture, "interface\\worldmap\\test.dds");
        assert_eq!(map.size, [1000.0, 800.0]);
        // North-west corner (-8192, 12288), south-east (8192, -4096).
        assert_eq!(map.markers.len(), 1);
        assert_eq!(map.markers[0].at, [0.5, 0.75]);
        assert!(!map.markers[0].travel);
        assert_eq!(map.player, Some(([0.0, 0.0], 45.0)));
    }
}
