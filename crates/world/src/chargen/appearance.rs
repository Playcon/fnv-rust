//! Choices exposed by the face and body menu (`RaceSexMenu`).
//!
//! The filters below follow the traced setup and choice predicates in
//! `docs/FACE_CREATION.md`: playable races use the runtime race flag, while
//! hair and eye records use their playable and sex flags plus the race's
//! corresponding form list. Results follow `LoadOrder::records_of_type`
//! iteration order; native menu ordering has not been established.

use std::collections::HashSet;

use esm::{FormId, FourCC, LoadOrder, Record, RecordRef};

const RACE: FourCC = FourCC::new(b"RACE");
const HAIR: FourCC = FourCC::new(b"HAIR");
const EYES: FourCC = FourCC::new(b"EYES");
const DATA: FourCC = FourCC::new(b"DATA");
const HNAM: FourCC = FourCC::new(b"HNAM");
const ENAM: FourCC = FourCC::new(b"ENAM");
const FULL: FourCC = FourCC::new(b"FULL");
const DNAM: FourCC = FourCC::new(b"DNAM");

/// A selectable race, hairstyle, or eye set. `name` comes only from `FULL`;
/// it remains `None` when the record has no such subrecord and `Some("")`
/// when the game record explicitly has an empty name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub form: FormId,
    pub name: Option<String>,
}

/// Races whose `DATA` runtime flags at byte 32 have bit 0 set:
/// setup007acb60 calls0059f610; loader00610cd0 copies DATA to race+0x50.
pub fn races(order: &LoadOrder) -> Vec<Choice> {
    order
        .records_of_type(RACE)
        .filter(|rr| !rr.entry.header.is_deleted())
        .filter_map(|rr| {
            let record = rr.record().ok()?;
            let data = record.get(DATA)?.data.get(32..36)?;
            (u32::from_le_bytes(data.try_into().ok()?) & 1 != 0).then(|| choice(rr, &record))
        })
        .collect()
}

/// Playable hair permitted by the selected race and sex (`false` male,
/// `true` female). The race's `HNAM` list is read in the race record's
/// owning plugin, so references from overrides and reordered masters map
/// to load-order form IDs correctly.
/// Menu007af300 uses005fdf40/005fdfa0; loader005fdcb0 reads DATA into+0x48.
pub fn hair(order: &LoadOrder, race: FormId, female: bool) -> Vec<Choice> {
    choices_for_parts(order, race, female, HAIR, HNAM)
}

/// Playable eye sets permitted by the selected race and sex (`false` male,
/// `true` female). The race's `ENAM` list is read in the race record's
/// owning plugin, so references from overrides and reordered masters map
/// to load-order form IDs correctly.
/// Menu007af450 uses005fc4d0/005fc5f0; loader005fc220 reads DATA into+0x30.
pub fn eyes(order: &LoadOrder, race: FormId, female: bool) -> Vec<Choice> {
    choices_for_parts(order, race, female, EYES, ENAM)
}

/// The race's default hairstyle for a sex (`RACE` `DNAM`: male, then
/// female; loaded into race `+0x94`/`+0x98` by `00610cd0`, read by
/// `00613870`). `None` without one.
pub fn default_hair(order: &LoadOrder, race: FormId, female: bool) -> Option<FormId> {
    let rr = race_record(order, race)?;
    let record = rr.record().ok()?;
    let data = &record.get(DNAM)?.data;
    let at = if female { 4 } else { 0 };
    let raw = u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?);
    (raw != 0).then(|| rr.plugin.to_global(FormId(raw)))
}

/// The race's hair (`HNAM`) or eyes (`ENAM`) list in the order the game
/// keeps it: each `HNAM`/`ENAM` in turn (one whose length isn't a whole
/// number of IDs skipped), each ID appended (`00905820` adds at the end)
/// unless already listed or not a record of that type (logged as "Could
/// not find hair"/"eyes", `00610cd0`).
fn race_list(order: &LoadOrder, race: FormId, kind: FourCC, part: FourCC) -> Vec<FormId> {
    let Some(rr) = race_record(order, race) else {
        return Vec::new();
    };
    let Ok(record) = rr.record() else {
        return Vec::new();
    };
    let mut list = Vec::new();
    for sub in record.get_all(kind) {
        if sub.data.is_empty() || sub.data.len() % 4 != 0 {
            continue;
        }
        for bytes in sub.data.chunks_exact(4) {
            let raw = u32::from_le_bytes(bytes.try_into().expect("four bytes"));
            if raw == 0 {
                continue;
            }
            let form = rr.plugin.to_global(FormId(raw));
            let found = order
                .get(form)
                .is_some_and(|p| p.entry.header.kind == part && !p.entry.header.is_deleted());
            if found && !list.contains(&form) {
                list.push(form);
            }
        }
    }
    list
}

/// A hair or eyes record's flags (`DATA`'s byte; `005fdcb0`, `005fc220`):
/// playable, not for men, not for women.
fn part_flags(order: &LoadOrder, form: FormId, part: FourCC) -> Option<u8> {
    let rr = order
        .get(form)
        .filter(|rr| rr.entry.header.kind == part && !rr.entry.header.is_deleted())?;
    let record = rr.record().ok()?;
    let flags = *record.get(DATA)?.data.first()?;
    Some(flags)
}

fn sex_allows(flags: u8, female: bool) -> bool {
    if female {
        flags & 4 == 0
    } else {
        flags & 2 == 0
    }
}

/// Whether a hairstyle suits a race and sex (`005fdfa0`): in the race's
/// hair list and allowed for the sex (`005fdf60`/`005fdf80`). Playable
/// isn't asked.
pub fn hair_fits(order: &LoadOrder, race: FormId, female: bool, hair: FormId) -> bool {
    part_flags(order, hair, HAIR).is_some_and(|f| sex_allows(f, female))
        && race_list(order, race, HNAM, HAIR).contains(&hair)
}

/// Whether eyes suit a race and sex (`005fc5f0`): in the race's eyes list
/// and allowed for the sex (`005fc530`/`005fc590`). Playable isn't asked.
pub fn eyes_fit(order: &LoadOrder, race: FormId, female: bool, eyes: FormId) -> bool {
    part_flags(order, eyes, EYES).is_some_and(|f| sex_allows(f, female))
        && race_list(order, race, ENAM, EYES).contains(&eyes)
}

/// After the race or sex changes in the face menu (`007b1ca0`), a
/// hairstyle and eyes that no longer suit are replaced:
///
/// * hair: the race's default for the sex ([`default_hair`], taken as it
///   is); without one, the first playable hairstyle in the race's list that
///   the sex allows; without that, none;
/// * eyes: the first in the race's eyes list, as it is (nothing checked);
///   none when the list is empty.
///
/// Returns the (hair, eyes) to keep.
pub fn fit_to_race(
    order: &LoadOrder,
    race: FormId,
    female: bool,
    hair: Option<FormId>,
    eyes: Option<FormId>,
) -> (Option<FormId>, Option<FormId>) {
    let hair = match hair.filter(|&h| hair_fits(order, race, female, h)) {
        Some(h) => Some(h),
        None => default_hair(order, race, female).or_else(|| {
            race_list(order, race, HNAM, HAIR).into_iter().find(|&h| {
                part_flags(order, h, HAIR).is_some_and(|f| f & 1 != 0 && sex_allows(f, female))
            })
        }),
    };
    let eyes = match eyes.filter(|&e| eyes_fit(order, race, female, e)) {
        Some(e) => Some(e),
        None => race_list(order, race, ENAM, EYES).first().copied(),
    };
    (hair, eyes)
}

fn race_record(order: &LoadOrder, race: FormId) -> Option<RecordRef<'_>> {
    order
        .get(race)
        .filter(|rr| rr.entry.header.kind == RACE && !rr.entry.header.is_deleted())
}

fn choices_for_parts(
    order: &LoadOrder,
    race: FormId,
    female: bool,
    part_type: FourCC,
    list_type: FourCC,
) -> Vec<Choice> {
    let Some(race_ref) = order
        .get(race)
        .filter(|rr| rr.entry.header.kind == RACE && !rr.entry.header.is_deleted())
    else {
        return Vec::new();
    };
    let Ok(race_record) = race_ref.record() else {
        return Vec::new();
    };
    let members = listed_forms(race_ref, &race_record, list_type);
    if members.is_empty() {
        return Vec::new();
    }

    order
        .records_of_type(part_type)
        .filter(|rr| !rr.entry.header.is_deleted())
        .filter_map(|rr| {
            let record = rr.record().ok()?;
            let flags = *record.get(DATA)?.data.first()?;
            let playable = flags & 1 != 0;
            let male_allowed = flags & 2 == 0;
            let female_allowed = flags & 4 == 0;
            let sex_allowed = if female { female_allowed } else { male_allowed };
            (playable && sex_allowed && members.contains(&rr.form_id)).then(|| choice(rr, &record))
        })
        .collect()
}

fn listed_forms(rr: RecordRef<'_>, record: &Record, kind: FourCC) -> HashSet<FormId> {
    record
        .get_all(kind)
        .filter(|sub| !sub.data.is_empty() && sub.data.len() % 4 == 0)
        .flat_map(|sub| {
            sub.data.chunks_exact(4).filter_map(|bytes| {
                let raw = u32::from_le_bytes(bytes.try_into().ok()?);
                (raw != 0).then(|| rr.plugin.to_global(FormId(raw)))
            })
        })
        .collect()
}

fn choice(rr: RecordRef<'_>, record: &Record) -> Choice {
    Choice {
        form: rr.form_id,
        name: record.get(FULL).map(|sub| sub.zstring()),
    }
}

#[cfg(test)]
mod tests {
    use esm::{FormId, LoadOrder, Plugin};
    use testdata::{group, record, sub, zstr};

    use super::{default_hair, eyes, eyes_fit, fit_to_race, hair, hair_fits, races};

    fn plugin(masters: &[&str], groups: &[(&[u8; 4], Vec<u8>)]) -> Plugin {
        let mut header = 1.34f32.to_le_bytes().to_vec();
        header.extend([0; 8]);
        let mut header_data = sub(b"HEDR", &header);
        for master in masters {
            header_data.extend(sub(b"MAST", &zstr(master)));
            header_data.extend(sub(b"DATA", &[0; 8]));
        }
        let mut bytes = record(b"TES4", 0, &header_data);
        for (kind, contents) in groups {
            bytes.extend(group(**kind, 0, contents));
        }
        Plugin::from_bytes(bytes).unwrap()
    }

    fn deleted(mut bytes: Vec<u8>) -> Vec<u8> {
        let flags = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) | 0x20;
        bytes[8..12].copy_from_slice(&flags.to_le_bytes());
        bytes
    }

    fn race(id: u32, runtime_flags: u32, hair_ids: &[u32], eye_ids: &[u32]) -> Vec<u8> {
        let mut data = vec![0; 36];
        data[32..36].copy_from_slice(&runtime_flags.to_le_bytes());
        let mut fields = sub(b"FULL", &zstr("Race"));
        fields.extend(sub(b"DATA", &data));
        if !hair_ids.is_empty() {
            let ids: Vec<u8> = hair_ids.iter().flat_map(|id| id.to_le_bytes()).collect();
            fields.extend(sub(b"HNAM", &ids));
        }
        if !eye_ids.is_empty() {
            let ids: Vec<u8> = eye_ids.iter().flat_map(|id| id.to_le_bytes()).collect();
            fields.extend(sub(b"ENAM", &ids));
        }
        record(b"RACE", id, &fields)
    }

    fn part(kind: &[u8; 4], id: u32, flags: Option<u8>, name: Option<&str>) -> Vec<u8> {
        let mut fields = Vec::new();
        if let Some(name) = name {
            fields.extend(sub(b"FULL", &zstr(name)));
        }
        if let Some(flags) = flags {
            fields.extend(sub(b"DATA", &[flags]));
        }
        record(kind, id, &fields)
    }

    fn one_plugin(records: &[(&[u8; 4], Vec<u8>)]) -> LoadOrder {
        let groups: Vec<_> = [b"RACE", b"HAIR", b"EYES", b"ARMO"]
            .iter()
            .map(|kind| {
                let contents = records
                    .iter()
                    .filter(|(record_type, _)| *record_type == *kind)
                    .flat_map(|(_, bytes)| bytes.iter().copied())
                    .collect();
                (*kind, contents)
            })
            .collect();
        LoadOrder::single("Test.esm", None, plugin(&[], &groups)).unwrap()
    }

    fn with_default_hair(mut race: Vec<u8>, male: u32, female: u32) -> Vec<u8> {
        // Appended inside the record: grow its size field by the new field.
        let mut ids = male.to_le_bytes().to_vec();
        ids.extend(female.to_le_bytes());
        let extra = sub(b"DNAM", &ids);
        let size = u32::from_le_bytes(race[4..8].try_into().unwrap()) + extra.len() as u32;
        race[4..8].copy_from_slice(&size.to_le_bytes());
        race.extend(extra);
        race
    }

    #[test]
    fn a_race_change_keeps_what_suits_and_falls_back_as_the_game_does() {
        let order = one_plugin(&[
            // Race 0x800: no default hair; its list starts with a hair not
            // playable, then a female-only one, then one for both.
            (
                b"RACE",
                race(0x800, 1, &[0x813, 0x811, 0x810, 0x810], &[0x821, 0x820]),
            ),
            // Race 0x801: a default for each sex (the female one not even
            // in its list), and no eyes.
            (
                b"RACE",
                with_default_hair(race(0x801, 1, &[0x810], &[]), 0x812, 0x815),
            ),
            (b"HAIR", part(b"HAIR", 0x810, Some(1), Some("Both"))),
            (b"HAIR", part(b"HAIR", 0x811, Some(3), Some("Female only"))),
            (b"HAIR", part(b"HAIR", 0x812, Some(5), Some("Male only"))),
            (b"HAIR", part(b"HAIR", 0x813, Some(0), Some("Not playable"))),
            (b"HAIR", part(b"HAIR", 0x815, Some(1), Some("Not listed"))),
            (b"EYES", part(b"EYES", 0x820, Some(1), Some("Both"))),
            (b"EYES", part(b"EYES", 0x821, Some(5), Some("Male only"))),
        ]);
        let (r0, r1) = (FormId(0x800), FormId(0x801));
        assert_eq!(default_hair(&order, r0, false), None);
        assert_eq!(default_hair(&order, r1, false), Some(FormId(0x812)));
        assert_eq!(default_hair(&order, r1, true), Some(FormId(0x815)));
        // Suiting: listed and allowed for the sex; playable isn't asked.
        assert!(hair_fits(&order, r0, false, FormId(0x813)));
        assert!(!hair_fits(&order, r0, false, FormId(0x811)));
        assert!(hair_fits(&order, r0, true, FormId(0x811)));
        assert!(!eyes_fit(&order, r0, true, FormId(0x821)));
        // What suits stays.
        assert_eq!(
            fit_to_race(&order, r0, true, Some(FormId(0x811)), Some(FormId(0x820))),
            (Some(FormId(0x811)), Some(FormId(0x820)))
        );
        // A man in race 0x800 with female-only hair and no eyes: the first
        // playable hair he may wear, and the first eyes listed (male-only
        // ones, taken unchecked).
        assert_eq!(
            fit_to_race(&order, r0, false, Some(FormId(0x811)), None),
            (Some(FormId(0x810)), Some(FormId(0x821)))
        );
        // Race 0x801: its default for the sex, as it is; no eyes to give.
        assert_eq!(
            fit_to_race(&order, r1, true, Some(FormId(0x812)), Some(FormId(0x820))),
            (Some(FormId(0x815)), None)
        );
    }

    #[test]
    fn playable_races_need_the_runtime_flag_and_complete_data() {
        let mut short = sub(b"FULL", &zstr("Short"));
        short.extend(sub(b"DATA", &[0; 35]));
        let records = [
            (b"RACE", race(0x800, 1, &[], &[])),
            (b"RACE", race(0x801, 0, &[], &[])),
            (b"RACE", record(b"RACE", 0x802, &short)),
            (b"RACE", record(b"RACE", 0x803, &sub(b"DATA", &[0; 36]))),
        ];
        let order = one_plugin(&records);
        assert_eq!(
            races(&order).iter().map(|c| c.form).collect::<Vec<_>>(),
            [FormId(0x800)]
        );
    }

    #[test]
    fn hair_and_eyes_obey_playable_sex_and_membership_flags() {
        let race_record = race(
            0x800,
            1,
            &[0x810, 0x811, 0x812, 0x813, 0x814],
            &[0x820, 0x821, 0x823, 0x824],
        );
        let order = one_plugin(&[
            (b"RACE", race_record),
            (b"HAIR", part(b"HAIR", 0x810, Some(1), Some("Both"))),
            (b"HAIR", part(b"HAIR", 0x811, Some(3), Some("Female only"))),
            (b"HAIR", part(b"HAIR", 0x812, Some(5), Some("Male only"))),
            (b"HAIR", part(b"HAIR", 0x813, Some(0), Some("Not playable"))),
            (b"HAIR", part(b"HAIR", 0x814, Some(7), Some("Neither"))),
            (b"HAIR", part(b"HAIR", 0x815, Some(1), Some("Not listed"))),
            (b"EYES", part(b"EYES", 0x820, Some(1), Some("Both"))),
            (b"EYES", part(b"EYES", 0x821, Some(5), Some("Male only"))),
            (b"EYES", part(b"EYES", 0x823, Some(3), Some("Female only"))),
            (b"EYES", part(b"EYES", 0x824, Some(7), Some("Neither"))),
            (b"EYES", part(b"EYES", 0x822, Some(1), Some("Not listed"))),
            (b"ARMO", part(b"ARMO", 0x816, Some(1), Some("Wrong type"))),
        ]);
        let hair_male = hair(&order, FormId(0x800), false);
        let hair_female = hair(&order, FormId(0x800), true);
        assert_eq!(
            hair_male
                .iter()
                .map(|c| c.name.as_deref())
                .collect::<Vec<_>>(),
            [Some("Both"), Some("Male only")]
        );
        assert_eq!(
            hair_female
                .iter()
                .map(|c| c.name.as_deref())
                .collect::<Vec<_>>(),
            [Some("Both"), Some("Female only")]
        );
        assert_eq!(
            eyes(&order, FormId(0x800), false)
                .iter()
                .map(|c| c.name.as_deref())
                .collect::<Vec<_>>(),
            [Some("Both"), Some("Male only")]
        );
        assert_eq!(
            eyes(&order, FormId(0x800), true)
                .iter()
                .map(|c| c.name.as_deref())
                .collect::<Vec<_>>(),
            [Some("Both"), Some("Female only")]
        );
        assert!(hair(&order, FormId(0x999), false).is_empty());
        assert!(hair(&order, FormId(0x816), false).is_empty());
    }

    #[test]
    fn malformed_race_lists_and_missing_or_wrong_type_forms_are_ignored() {
        let mut fields = sub(b"DATA", &{
            let mut d = vec![0; 36];
            d[32..36].copy_from_slice(&1u32.to_le_bytes());
            d
        });
        fields.extend(sub(b"HNAM", &[0x10, 0x08, 0x00])); // malformed list
        fields.extend(sub(b"HNAM", &[0x11, 0x08, 0x00, 0x00, 0xff])); // valid ID plus bad tail
        fields.extend(sub(b"HNAM", &[0x10, 0x08, 0x00, 0x00])); // valid-size list
        fields.extend(sub(b"HNAM", &[0x12, 0x08, 0x00, 0x00])); // missing form
        let malformed_race = record(b"RACE", 0x800, &fields);
        let armo = part(b"ARMO", 0x810, Some(1), Some("Wrong type"));
        let order = one_plugin(&[
            (b"RACE", malformed_race),
            (
                b"HAIR",
                part(b"HAIR", 0x811, Some(1), Some("Missing from list")),
            ),
            (b"ARMO", armo),
        ]);
        assert!(hair(&order, FormId(0x800), false).is_empty());
        assert!(hair(&order, FormId(0x810), false).is_empty());
    }

    #[test]
    fn overridden_race_lists_map_references_through_the_owner_plugin() {
        let master_a = plugin(
            &[],
            &[
                (b"RACE", race(0x800, 1, &[0x801], &[])),
                (b"HAIR", part(b"HAIR", 0x801, Some(1), Some("Old"))),
            ],
        );
        let master_b = plugin(
            &[],
            &[(b"HAIR", part(b"HAIR", 0x820, Some(1), Some("Remapped")))],
        );
        // Patch masters are deliberately reversed: local index 0 maps to B,
        // while local index 1 maps to A. The race override lives in A.
        let patch = plugin(
            &["B.esm", "A.esm"],
            &[(
                b"RACE",
                race(
                    0x0100_0800,
                    1,
                    &[0x0000_0820, 0x0000_0999, 0x0000_0830],
                    &[],
                ),
            )],
        );
        let order = LoadOrder::from_plugins(vec![
            ("A.esm".into(), None, master_a),
            ("B.esm".into(), None, master_b),
            ("Patch.esp".into(), None, patch),
        ])
        .unwrap();
        let choices = hair(&order, FormId(0x800), false);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].form, FormId(0x0100_0820));
        assert_eq!(choices[0].name.as_deref(), Some("Remapped"));
    }

    #[test]
    fn deleted_winning_race_hair_and_eye_overrides_are_not_choices() {
        let master = plugin(
            &[],
            &[
                (
                    b"RACE",
                    [
                        race(0x800, 1, &[0x810], &[0x820]),
                        race(0x801, 1, &[0x810], &[0x820]),
                    ]
                    .concat(),
                ),
                (b"HAIR", part(b"HAIR", 0x810, Some(1), Some("Master hair"))),
                (b"EYES", part(b"EYES", 0x820, Some(1), Some("Master eyes"))),
            ],
        );
        let patch = plugin(
            &["Master.esm"],
            &[
                (b"RACE", deleted(race(0x0000_0800, 1, &[0x810], &[0x820]))),
                (
                    b"HAIR",
                    deleted(part(b"HAIR", 0x0000_0810, Some(1), Some("Deleted hair"))),
                ),
                (
                    b"EYES",
                    deleted(part(b"EYES", 0x0000_0820, Some(1), Some("Deleted eyes"))),
                ),
            ],
        );
        let order = LoadOrder::from_plugins(vec![
            ("Master.esm".into(), None, master),
            ("Patch.esp".into(), None, patch),
        ])
        .unwrap();
        assert_eq!(
            races(&order).iter().map(|c| c.form).collect::<Vec<_>>(),
            [FormId(0x801)]
        );
        assert!(hair(&order, FormId(0x800), false).is_empty());
        assert!(eyes(&order, FormId(0x800), false).is_empty());
        // The surviving race must not resurrect deleted parts from its lists.
        assert!(hair(&order, FormId(0x801), false).is_empty());
        assert!(eyes(&order, FormId(0x801), false).is_empty());
    }

    #[test]
    fn missing_and_empty_full_names_are_not_invented() {
        // The empty FULL is represented by the subrecord's terminating NUL.
        let mut race_fields = sub(b"DATA", &{
            let mut d = vec![0; 36];
            d[32..36].copy_from_slice(&1u32.to_le_bytes());
            d
        });
        race_fields.extend(sub(b"HNAM", &0x810u32.to_le_bytes()));
        race_fields.extend(sub(b"HNAM", &0x811u32.to_le_bytes()));
        let order = one_plugin(&[
            (b"RACE", record(b"RACE", 0x800, &race_fields)),
            (b"HAIR", part(b"HAIR", 0x810, Some(1), None)),
            (b"HAIR", part(b"HAIR", 0x811, Some(1), Some(""))),
        ]);
        let choices = hair(&order, FormId(0x800), false);
        assert_eq!(choices[0].name, None);
        assert_eq!(choices[1].name.as_deref(), Some(""));
    }
}
