//! What people do: AI packages (`PACK`) and finding their way on the
//! navmesh (`NAVM`).
//!
//! A person's base lists packages (`PKID`) in order; the one they follow is
//! the first whose conditions pass and whose schedule covers the time (a
//! script can put one first, `AddScriptPackage`). A package (`PACK`):
//! `PKDT` (flags u32, type u8, unused u8, behaviour flags u16, type flags
//! u16, 2 unused bytes), `PLDT` where (kind i32, form u32, radius i32),
//! `PSDT` when (month i8, day of week i8, date u8, hour i8, duration i32;
//! -1 / 0 for "any"), `PTDT` the target, `CTDA` conditions asked about the
//! person. Package types (New Vegas): 0 find, 1 follow, 2 escort, 3 eat,
//! 4 sleep, 5 wander, 6 travel, 7 accompany, 8 use item at, 9 ambush, 10
//! flee, 12 sandbox, 13 patrol, 14 guard, 15 dialogue, 16 use weapon
//! (`VCG01DocMitchellTravelToPlayerAtTester` is a 6, with
//! "`GetStage VCG01 >= 55`").
//!
//! A navmesh: `NVVX` vertices (3 floats), `NVTR` triangles of 16 bytes
//! (three vertex numbers, the triangle across each edge, -1 for none,
//! flags, cover flags). The triangle across edge `i` shares the edge from
//! vertex `i` to vertex `i + 1` (checked on Doc Mitchell's house: its
//! triangle 0 lists triangle 1 across its third edge, and they share those
//! two vertices). A path is first tried as a straight line over the
//! navmesh (`bUseStraightLineCheckFirst`, `006cc5e0` → `006cd1f0`): when
//! the line stays on it, that's the path. Otherwise it goes from triangle
//! to triangle (A*, between their middles) and is pulled straight through
//! the shared edges (the "funnel"); the game's smoothers (`0069f010`,
//! `PathSmootherPOVSearch` `006ad770`) aren't traced, so that part is a
//! stand-in for them.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use esm::{FormId, FourCC, LoadOrder};

use crate::cell::{le_f32, le_u32};
use crate::dialogue::{read_condition, Condition};
use crate::movement::Spot;
use crate::scripting::{Facts, GameState};

pub mod actions;

const PACK: FourCC = FourCC::new(b"PACK");
const PKDT: FourCC = FourCC::new(b"PKDT");
const PLDT: FourCC = FourCC::new(b"PLDT");
const PSDT: FourCC = FourCC::new(b"PSDT");
const PTDT: FourCC = FourCC::new(b"PTDT");
const PKDD: FourCC = FourCC::new(b"PKDD");
const CTDA: FourCC = FourCC::new(b"CTDA");
const PKID: FourCC = FourCC::new(b"PKID");
const NAVM: FourCC = FourCC::new(b"NAVM");
const NVVX: FourCC = FourCC::new(b"NVVX");
const NVTR: FourCC = FourCC::new(b"NVTR");
const NVEX: FourCC = FourCC::new(b"NVEX");
const NVDP: FourCC = FourCC::new(b"NVDP");
const XLKR: FourCC = FourCC::new(b"XLKR");

/// Package types.
pub mod kinds {
    pub const FIND: u8 = 0;
    pub const FOLLOW: u8 = 1;
    pub const ESCORT: u8 = 2;
    pub const WANDER: u8 = 5;
    pub const TRAVEL: u8 = 6;
    pub const ACCOMPANY: u8 = 7;
    pub const SANDBOX: u8 = 12;
    pub const PATROL: u8 = 13;
    pub const GUARD: u8 = 14;
    pub const DIALOGUE: u8 = 15;
}

/// Where a package takes place (`PLDT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// 0 near a reference, 1 in a cell, 2 near the current location, 3 near
    /// the editor location, 4 an object, 5 an object type, 6 near the
    /// linked reference, 7 at the package's location.
    pub kind: i32,
    pub form: FormId,
    pub radius: i32,
}

/// When a package applies (`PSDT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Schedule {
    /// 0..11, -1 any.
    pub month: i8,
    /// 0 Sunday … 6 Saturday, 7 weekdays, 8 weekends, 9 Monday Wednesday
    /// Friday, 10 Tuesday Thursday; -1 any.
    pub day_of_week: i8,
    /// Day of the month, 0 any.
    pub date: u8,
    /// Starting hour, -1 any.
    pub hour: i8,
    /// Hours.
    pub duration: i32,
}

impl Schedule {
    /// Whether it covers a moment: month (0..11), day of the week (0
    /// Sunday), date and hour (fractional).
    pub fn covers(&self, month: i32, weekday: i32, date: i32, hour: f32) -> bool {
        if self.month >= 0 && i32::from(self.month) != month {
            return false;
        }
        let day_ok = match self.day_of_week {
            d if d < 0 => true,
            d @ 0..=6 => i32::from(d) == weekday,
            7 => (1..=5).contains(&weekday),
            8 => weekday == 0 || weekday == 6,
            9 => [1, 3, 5].contains(&weekday),
            10 => [2, 4].contains(&weekday),
            _ => true,
        };
        if !day_ok || (self.date > 0 && i32::from(self.date) != date) {
            return false;
        }
        if self.hour < 0 || self.duration <= 0 {
            return true;
        }
        let start = f32::from(self.hour);
        let end = start + self.duration as f32;
        // Past midnight it wraps.
        (hour >= start && hour < end) || (end > 24.0 && hour < end - 24.0)
    }
}

/// An AI package.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub form_id: FormId,
    pub editor_id: Option<String>,
    pub kind: u8,
    pub flags: u32,
    pub location: Option<Location>,
    pub schedule: Schedule,
    pub conditions: Vec<Condition>,
    /// Who it's aimed at (`PTDT`: kind i32, form, a count or distance):
    /// for dialogue packages, who to talk to and how close they must come
    /// (Sunny Smiles' greeting: the player, 256).
    pub target: Option<(i32, FormId, i32)>,
    /// A dialogue package's topic (`PKDD`, after the field of view);
    /// `None` for a greeting.
    pub topic: Option<FormId>,
    /// The package's begin, end and change actions. These are separate
    /// from its ordinary idle list; the opening uses them for the player's
    /// wakeup, situp and standup animations.
    pub actions: actions::PackageActions,
}

impl Package {
    pub fn load(order: &LoadOrder, id: FormId) -> Option<Package> {
        let rr = order.get(id).filter(|r| r.entry.header.kind == PACK)?;
        let record = rr.record().ok()?;
        let pkdt = record.get(PKDT).filter(|s| s.data.len() >= 5)?;
        let location = record.get(PLDT).filter(|s| s.data.len() >= 12).map(|s| {
            let kind = le_u32(&s.data, 0) as i32;
            // Kinds that name a record: a reference, a cell, an object.
            let raw = le_u32(&s.data, 4);
            let form = if matches!(kind, 0 | 1 | 4) {
                rr.plugin.to_global(FormId(raw))
            } else {
                FormId(raw)
            };
            Location {
                kind,
                form,
                radius: le_u32(&s.data, 8) as i32,
            }
        });
        let schedule = record
            .get(PSDT)
            .filter(|s| s.data.len() >= 8)
            .map(|s| Schedule {
                month: s.data[0] as i8,
                day_of_week: s.data[1] as i8,
                date: s.data[2],
                hour: s.data[3] as i8,
                duration: le_u32(&s.data, 4) as i32,
            })
            .unwrap_or(Schedule {
                month: -1,
                day_of_week: -1,
                date: 0,
                hour: -1,
                duration: 0,
            });
        let target = record.get(PTDT).filter(|s| s.data.len() >= 12).map(|s| {
            let kind = le_u32(&s.data, 0) as i32;
            let raw = FormId(le_u32(&s.data, 4));
            // Kind 0 names a reference.
            let form = if kind == 0 {
                rr.plugin.to_global(raw)
            } else {
                raw
            };
            (kind, form, le_u32(&s.data, 8) as i32)
        });
        let topic = record
            .get(PKDD)
            .filter(|s| s.data.len() >= 8)
            .map(|s| rr.plugin.to_global(FormId(le_u32(&s.data, 4))))
            .filter(|t| t.0 != 0);
        Some(Package {
            form_id: id,
            editor_id: record.editor_id(),
            kind: pkdt.data[4],
            flags: le_u32(&pkdt.data, 0),
            location,
            schedule,
            conditions: record
                .get_all(CTDA)
                .filter_map(|s| read_condition(&rr, &s.data))
                .collect(),
            target,
            topic,
            actions: actions::PackageActions::read(&record, |id| rr.plugin.to_global(id)),
        })
    }
}

const PLD2: FourCC = FourCC::new(b"PLD2");

/// A dialogue package's own data (`PKDD`, 24 bytes; laid out as the
/// package-data save and load `0067b630`/`0067b7d0` and the getters
/// `00672710`–`00672850` read it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogueData {
    /// f32 at 0: only the dialogue camera's zoom reads it (× FOV ÷ 100,
    /// `00761a20`, `00953060`): not a field of view the speaker needs.
    pub fov: f32,
    /// Form at 4: the topic (`None`: their greeting).
    pub topic: Option<FormId>,
    /// Byte 8 flag 0x01: no head tracking (`00672800`).
    pub no_head_tracking: bool,
    /// Byte 9 flag 0x01: the target's movement isn't taken over
    /// (`006727b0`).
    pub dont_control_target: bool,
    /// Byte 16 non-zero: "Say To" (`00672710`): a line said, no dialogue.
    pub say_to: bool,
}

/// A dialogue package's data ([`DialogueData`]).
pub fn dialogue_data(order: &LoadOrder, package: FormId) -> Option<DialogueData> {
    let rr = order.get(package).filter(|r| r.entry.header.kind == PACK)?;
    let record = rr.record().ok()?;
    let s = record.get(PKDD).filter(|s| s.data.len() >= 8)?;
    let byte = |i: usize| s.data.get(i).copied().unwrap_or(0);
    Some(DialogueData {
        fov: le_f32(&s.data, 0),
        topic: Some(rr.plugin.to_global(FormId(le_u32(&s.data, 4)))).filter(|t| t.0 != 0),
        no_head_tracking: byte(8) & 0x01 != 0,
        dont_control_target: byte(9) & 0x01 != 0,
        say_to: byte(16) != 0,
    })
}

/// A package's second location (`PLD2`, laid out as `PLDT`; `00672dd0`):
/// for dialogue packages, where the target must be before the talk starts.
pub fn second_location(order: &LoadOrder, package: FormId) -> Option<Location> {
    let rr = order.get(package).filter(|r| r.entry.header.kind == PACK)?;
    let record = rr.record().ok()?;
    let s = record.get(PLD2).filter(|s| s.data.len() >= 12)?;
    let kind = le_u32(&s.data, 0) as i32;
    let raw = le_u32(&s.data, 4);
    let form = if matches!(kind, 0 | 1 | 4) {
        rr.plugin.to_global(FormId(raw))
    } else {
        FormId(raw)
    };
    Some(Location {
        kind,
        form,
        radius: le_u32(&s.data, 8) as i32,
    })
}

/// What a reference is, for its radius ([`crate::movement::Spot`]):
/// furniture, an `XMarker`/`XMarkerHeading`, someone (asleep when their
/// sit state is 9: settled in a bed), or anything else; with half its
/// bounds' diagonal (`OBND` × its scale; `00571600`, `0050ebf0`).
pub fn spot_of(order: &LoadOrder, state: &GameState, reference: FormId) -> Spot {
    let base = crate::scripting::base_of(order, reference);
    if base == Some(crate::movement::X_MARKER) || base == Some(crate::movement::X_MARKER_HEADING) {
        return Spot::Marker;
    }
    if GameState::is_furniture(order, reference) {
        return Spot::Furniture;
    }
    let half_diagonal = half_bounds_diagonal(order, reference);
    let person = order
        .get(reference)
        .is_some_and(|rr| matches!(rr.entry.header.kind.as_bytes(), b"ACHR" | b"ACRE"))
        || reference == crate::dialogue::PLAYER_REF;
    if person {
        let asleep = state
            .sitters
            .get(&reference)
            .is_some_and(|s| s.question().1 == 3);
        Spot::Person {
            asleep,
            half_diagonal,
        }
    } else {
        Spot::Object { half_diagonal }
    }
}

const OBND: FourCC = FourCC::new(b"OBND");
const XSCL: FourCC = FourCC::new(b"XSCL");

/// Half the diagonal of a reference's bounds: its base's `OBND` box ×
/// its scale (`XSCL`).
pub fn half_bounds_diagonal(order: &LoadOrder, reference: FormId) -> f32 {
    let scale = order
        .get(reference)
        .and_then(|rr| rr.record().ok())
        .and_then(|r| {
            r.get(XSCL)
                .filter(|s| s.data.len() >= 4)
                .map(|s| le_f32(&s.data, 0))
        })
        .unwrap_or(1.0);
    crate::scripting::base_of(order, reference)
        .and_then(|b| order.get(b))
        .and_then(|rr| rr.record().ok())
        .and_then(|r| {
            r.get(OBND).filter(|s| s.data.len() >= 12).map(|s| {
                let v =
                    |i: usize| f32::from(i16::from_le_bytes([s.data[i * 2], s.data[i * 2 + 1]]));
                let d = [v(3) - v(0), v(4) - v(1), v(5) - v(2)];
                0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * scale
            })
        })
        .unwrap_or(0.0)
}

/// Whether someone is at a dialogue package's second location (`PLD2`;
/// the package's vfunc +0x140, `00676390`): for a trigger (an activator
/// with a primitive, `0067f220` → `0062ddb0`) inside its volume; else
/// within its radius (`PLD2`'s own, else by what the reference is,
/// [`crate::movement::location_radius`]) of it, measured as
/// `IsWithinDistance` (adding their radius except for markers and
/// furniture). Not in the same place: no.
pub fn at_second_location(
    order: &LoadOrder,
    state: &GameState,
    location: &Location,
    who: FormId,
    min_radius: f32,
) -> bool {
    let Some((here, _, at, _)) = state.place(order, who) else {
        return false;
    };
    let reference = match location.kind {
        0 => location.form,
        _ => return false,
    };
    let Some((there, _, spot_at, _)) = state.place(order, reference) else {
        return false;
    };
    if here != there {
        return false;
    }
    if let Some(p) = crate::placement_of(order, reference).filter(|p| p.primitive.is_some()) {
        if p.base_type.as_bytes() == b"ACTI" {
            let prim = p.primitive.expect("checked");
            let s = if p.scale > 0.0 { p.scale } else { 1.0 };
            let m = crate::RotationConvention::DEFAULT.matrix(p.rotation);
            // The body against the volume: three points of it, 10, 64 and
            // 110 above the feet, as the viewer's triggers test (the game
            // tests the actor's collision shape, `0062df20`).
            return [10.0, 64.0, 110.0].iter().any(|up| {
                let d = [
                    at[0] - spot_at[0],
                    at[1] - spot_at[1],
                    at[2] + up - spot_at[2],
                ];
                let local = [0, 1, 2].map(|i| m[0][i] * d[0] + m[1][i] * d[1] + m[2][i] * d[2]);
                if prim.shape == 2 {
                    local.iter().map(|v| v * v).sum::<f32>() <= (prim.half[0] * s).powi(2)
                } else {
                    (0..3).all(|i| local[i].abs() <= prim.half[i] * s)
                }
            });
        }
    }
    let spot = spot_of(order, state, reference);
    let radius = crate::movement::location_radius(location.radius, spot, min_radius);
    let add = !matches!(spot, Spot::Marker | Spot::Furniture);
    crate::movement::within_distance(
        at,
        128.0,
        Some(crate::combat_ai::PERSON_RADIUS),
        spot_at,
        radius,
        add,
    )
}

/// What a dialogue package (type 15) has its person do now: its list is
/// TRAVEL → DIALOGUE_ACTIVATE → WAIT → DIALOGUE (`011a3ff0` list 10).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DialogueStep {
    /// To the package's place first (TRAVEL, `008e5e90`).
    Travel,
    /// The target isn't at the second location (`PLD2`) yet.
    Wait,
    /// Walk up to the target until within `reach` (a path with that
    /// radius, `008b36f0`; for "Say To" measured in 3D, else as
    /// `IsWithinDistance` adding the walker's radius).
    Approach { reach: f32 },
    /// Say the topic (else `HELLO`) as a line, no dialogue menu (the GREET
    /// procedure).
    Say,
    /// Start the conversation: the dialogue menu for the player (the
    /// type-0x1c package activates the player, `008e9640`), else a
    /// conversation between the two.
    Talk,
}

/// The step of a dialogue package for `who` now (`008e8600`, with the
/// type-0x1c package it makes, `008b19c0`, and the activate procedure
/// `008e9640`): first the travel to its place (`PLDT`; skipped when there's
/// none, or it's the person themselves without "near the editor location").
/// Then, for "Say To" with the player as target: within the target distance
/// (`PTDT`, ≤ 0 → 120, 3D) the line, else walk up. Otherwise, with the
/// player as target and a second location (`PLD2`): wait until the player
/// is at it ([`at_second_location`]); then talk at once if within reach
/// (the package vfunc +0x144 `00676e40`), else walk up and talk there.
/// Without one: walk up until within reach — the target distance, else
/// `iAIDistanceRadiusMinLocation` (100), plus the walker's radius (at least
/// 32), in 2D at body height ([`crate::movement::within_distance`]) — and
/// talk. (`at_place`: whether the travel is done.)
pub fn dialogue_step(
    order: &LoadOrder,
    state: &GameState,
    who: FormId,
    package: &Package,
    at_place: bool,
    my_radius: f32,
) -> Option<DialogueStep> {
    let (kind, target, distance) = package.target?;
    if kind != 0 || target.0 == 0 {
        return None;
    }
    let travel = package
        .location
        .is_some_and(|l| !(l.kind == 2 || (l.kind == 0 && l.form == who)));
    if travel && !at_place {
        return Some(DialogueStep::Travel);
    }
    let data = dialogue_data(order, package.form_id);
    let (here, _, me, _) = state.place(order, who)?;
    let (there, _, at, _) = state.place(order, target)?;
    if here != there {
        return Some(DialogueStep::Wait);
    }
    let player = target == crate::dialogue::PLAYER_REF;
    if player && data.is_some_and(|d| d.say_to) {
        let reach = crate::movement::say_to_reach(distance);
        let d = (0..3).map(|i| (at[i] - me[i]).powi(2)).sum::<f32>().sqrt();
        return Some(if d > reach {
            DialogueStep::Approach { reach }
        } else {
            DialogueStep::Say
        });
    }
    let min = min_location_radius(order);
    let reach = crate::movement::target_reach(distance, spot_of(order, state, target), min);
    let within = crate::movement::within_distance(me, 128.0, Some(my_radius), at, reach, true);
    if player {
        if let Some(l2) = second_location(order, package.form_id) {
            if !at_second_location(order, state, &l2, target, min) {
                return Some(DialogueStep::Wait);
            }
        }
    }
    Some(if within {
        DialogueStep::Talk
    } else {
        DialogueStep::Approach { reach }
    })
}

/// The ring a wander spot is chosen in (`008ed420`): from 32 to 0.75 × the
/// package's radius around the centre (`0040ebd0(32.0, r × 0.75)`). How the
/// game's navmesh search picks within it isn't traced.
pub fn wander_ring(radius: f32) -> (f32, f32) {
    (32.0, 0.75 * radius)
}

/// Below this radius a wander package's people just stand (`008ed420`:
/// 60). Only wander packages (the package's list type 1): the procedure
/// run for a sandbox (or a guard) package skips this test, and the
/// [`WANDER_LEASH`] one.
pub const LEAST_WANDER_RADIUS: f32 = 60.0;

/// How far beyond its radius a wander package's person may stray from the
/// middle before going back to it (`008ed420`: radius + 250, then the
/// procedure goes back to the travel). Not for sandboxes (their own rule is
/// radius + 150, [`crate::sandbox::Sandbox::strayed`]).
pub const WANDER_LEASH: f32 = 250.0;

/// What the wander procedure (`008ed420`) has someone with a wander package
/// (type 5) do, once the travel to the package's place is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WanderStep {
    /// Back to the place (the travel again): farther than the radius +
    /// [`WANDER_LEASH`] from the middle, or (with a radius under
    /// [`LEAST_WANDER_RADIUS`]) not at the place.
    Back,
    /// Stand where they are, facing an `XMarkerHeading`'s heading if the
    /// place is one ([`arrival_heading`]): a radius under
    /// [`LEAST_WANDER_RADIUS`].
    Stand,
    /// Wander: a spot in [`wander_ring`] around the middle now and then
    /// ([`crate::sandbox::WanderTimer`]).
    Wander,
}

/// The wander procedure's choice for a wander package (`008ed420`, list
/// type 1): `radius` the package's (`PLDT`), `own_place` when the middle is
/// the person's own position ("in a cell" without a reference: then the
/// radius test is skipped), `at_place` whether they're at the package's
/// place (the package's own test, vfunc +0x13c), `to_middle` their distance
/// from the middle (3D, `00457910`).
pub fn wander_step(radius: f32, own_place: bool, at_place: bool, to_middle: f32) -> WanderStep {
    if radius < LEAST_WANDER_RADIUS && !own_place {
        return if at_place {
            WanderStep::Stand
        } else {
            WanderStep::Back
        };
    }
    if to_middle > radius + WANDER_LEASH {
        WanderStep::Back
    } else {
        WanderStep::Wander
    }
}

/// The radius a wander package's person wanders in, and whether its middle
/// is their own position (`008ed420`): the package's `PLDT` radius
/// (`00676280`: for an activator with radius 0, half its bounds' diagonal,
/// rounded); "in a cell" (kind 1) puts the middle on the person, and indoors
/// the spots then come from a radius of 800.
pub fn wander_radius(order: &LoadOrder, package: &Package, interior: bool) -> (f32, bool) {
    let Some(loc) = package.location else {
        return (0.0, false);
    };
    if loc.kind == 1 {
        let r = if interior { 800.0 } else { loc.radius as f32 };
        return (r, true);
    }
    let mut r = loc.radius.max(0) as f32;
    if r == 0.0 && loc.kind == 0 {
        let activator = crate::scripting::base_of(order, loc.form)
            .and_then(|b| order.get(b))
            .is_some_and(|rr| rr.entry.header.kind.as_bytes() == b"ACTI");
        if activator {
            r = half_bounds_diagonal(order, loc.form).round();
        }
    }
    (r, false)
}

/// The heading of a package's place when it's an `XMarkerHeading` (form
/// 0x34): the marker's own.
pub fn marker_heading(order: &LoadOrder, state: &GameState, package: &Package) -> Option<f32> {
    let loc = package.location.filter(|l| l.kind == 0)?;
    (crate::scripting::base_of(order, loc.form) == Some(crate::movement::X_MARKER_HEADING))
        .then(|| state.place(order, loc.form).map(|p| p.3))
        .flatten()
}

/// The heading someone turns to at the end of the travel procedure to
/// their package's place (`008e5e90` in sight: a turn in place, `008bb5c0`;
/// `0090ad40` out of sight: set at once): the place's own when it's an
/// `XMarkerHeading` ([`marker_heading`]), else for "near the editor
/// location" (kind 3) where they were placed facing (the actor's start
/// rotation, +0x16c; that the location gives no reference then is
/// inferred). Only for the packages known to start with that procedure:
/// travel (list 0) and dialogue (list 10); the other lists' steps aren't
/// traced. Not while using furniture (the caller's).
pub fn arrival_heading(
    order: &LoadOrder,
    state: &GameState,
    actor: FormId,
    package: &Package,
) -> Option<f32> {
    if package.kind != kinds::TRAVEL && package.kind != kinds::DIALOGUE {
        return None;
    }
    let loc = package.location?;
    match loc.kind {
        0 => marker_heading(order, state, package),
        3 => crate::scripting::whereabouts(order, actor).map(|w| w.heading),
        _ => None,
    }
}

/// A person's packages in order (`PKID` on their base).
pub fn packages_of(order: &LoadOrder, base: FormId) -> Vec<FormId> {
    let Some(rr) = order.get(base) else {
        return Vec::new();
    };
    let Ok(record) = rr.record() else {
        return Vec::new();
    };
    record
        .get_all(PKID)
        .filter(|s| s.data.len() >= 4)
        .map(|s| rr.plugin.to_global(FormId(le_u32(&s.data, 0))))
        .collect()
}

/// The package a person follows now: one a script gave them, else the
/// first of theirs whose conditions pass (asked about them, with the
/// player as the target) and whose schedule covers the game's clock.
pub fn current_package(order: &LoadOrder, state: &GameState, actor: FormId) -> Option<Package> {
    // Coming to warn the trespassing player (`world::living::trespass`).
    if let Some(p) = crate::living::trespass::package(state, actor) {
        return Some(p);
    }
    if let Some(&p) = state.script_packages.get(&actor) {
        return Package::load(order, p);
    }
    let base = crate::scripting::base_of(order, actor)?;
    let g = |name: &str| state.global(order, name).unwrap_or(0.0);
    let (year, month, date, hour) = (
        g("GameYear") as i32,
        g("GameMonth") as i32,
        g("GameDay") as i32,
        g("GameHour"),
    );
    let weekday = crate::scripting::day_of_week(year, month, date);
    let facts = Facts {
        order,
        state,
        speaker: None,
    };
    packages_of(order, base)
        .into_iter()
        .filter_map(|p| Package::load(order, p))
        .find(|p| {
            p.schedule.covers(month, weekday, date, hour)
                && facts.conditions_pass(&p.conditions, actor, crate::dialogue::PLAYER_REF)
        })
}

/// A script's travel package has finished when its person is where it sends
/// them (or it sends them nowhere, as "near the current location"): the
/// End action is asked for, once. That a package ends when the travel does
/// is a guess [G]; the game runs the End action when it finishes a package
/// (slot `0x5a0`), and what finishes a travel isn't traced. False for
/// other packages, or one the script didn't give.
pub fn finish_travel(state: &mut GameState, actor: FormId, package: &Package) -> bool {
    package.kind == kinds::TRAVEL
        && state.script_packages.get(&actor) == Some(&package.form_id)
        && state.end_script_package(actor)
}

/// Whom a follow or accompany package keeps near, and how near: its
/// target (`PTDT`, a specific reference) and the target's value, which
/// for these is the distance (`CheyenneAccompany`: Sunny Smiles, 128;
/// `VFactionSquadPackageNCRFollowLeader`: 420). A guess at the field's
/// meaning, from those values; at least 64.
pub fn followed(package: &Package) -> Option<(FormId, f32)> {
    if package.kind != kinds::FOLLOW && package.kind != kinds::ACCOMPANY {
        return None;
    }
    let (kind, who, distance) = package.target?;
    (kind == 0 && who.0 != 0).then_some((who, (distance as f32).max(64.0)))
}

/// `iAIDistanceRadiusMinLocation` (100): the radius when nothing else
/// gives one.
pub fn min_location_radius(order: &LoadOrder) -> f32 {
    crate::scripting::game_setting(order, "iAIDistanceRadiusMinLocation").unwrap_or(100.0)
}

/// Where a package sends a person, as a point and how close counts as
/// there: near a reference (where it stands), near the person's editor
/// location (where they're placed), near their linked reference, near
/// whom they follow. The radius is the travel's (`00678670`,
/// [`crate::movement::location_radius`]): the package's own, else by what's
/// there; "near the editor location" measures by the person themselves
/// (`0067f2a0` gives the actor as the reference). `None` for packages that
/// don't lead anywhere this can work out.
pub fn destination(
    order: &LoadOrder,
    state: &GameState,
    actor: FormId,
    package: &Package,
) -> Option<([f32; 3], f32)> {
    if let Some((who, distance)) = followed(package) {
        let (here, ..) = state.place(order, actor)?;
        let (there, _, position, _) = state.place(order, who)?;
        return (here == there).then_some((position, distance));
    }
    let loc = package.location?;
    let min = min_location_radius(order);
    let radius_of = |reference: FormId| {
        crate::movement::location_radius(loc.radius, spot_of(order, state, reference), min)
    };
    let radius = if matches!(loc.kind, 0 | 3 | 6) {
        0.0
    } else {
        loc.radius.max(0) as f32
    };
    let target = match loc.kind {
        0 => loc.form,
        3 => actor,
        6 => {
            let rr = order.get(actor)?;
            let record = rr.record().ok()?;
            let s = record.get(XLKR).filter(|s| s.data.len() >= 4)?;
            // New Vegas's XLKR: a keyword then the reference, or just the
            // reference.
            let at = if s.data.len() >= 8 { 4 } else { 0 };
            rr.plugin.to_global(FormId(le_u32(&s.data, at)))
        }
        _ => return None,
    };
    if loc.kind == 3 {
        // Their editor location: where they were placed.
        let placed = crate::scripting::whereabouts(order, actor)?;
        return Some((placed.position, radius_of(actor).max(radius)));
    }
    // Only somewhere in the same interior or worldspace as where they are
    // now: the way to another place is through a door (`door_toward`).
    let (here, ..) = state.place(order, actor)?;
    let (there, _, position, _) = state.place(order, target)?;
    if here != there {
        return None;
    }
    Some((position, radius_of(target).max(radius)))
}

/// Where a package's target is: its interior cell or worldspace, and
/// position (where scripts have moved it, else as placed).
pub fn target_place(
    order: &LoadOrder,
    state: &GameState,
    package: &Package,
) -> Option<(FormId, [f32; 3])> {
    let target = match (followed(package), package.location) {
        (Some((who, _)), _) => who,
        (None, Some(loc)) if loc.kind == 0 => loc.form,
        _ => return None,
    };
    let (space, _, position, _) = state.place(order, target)?;
    Some((space, position))
}

/// A load door someone can take toward another place: the door to walk
/// to, and where it puts them (the place, the cell, position and heading,
/// radians clockwise from north).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoorWay {
    pub door: FormId,
    pub at: [f32; 3],
    pub to_space: FormId,
    pub to_cell: FormId,
    pub to: [f32; 3],
    pub heading: f32,
}

/// The load door nearest `actor` that leads straight into `space` (an
/// interior cell or a worldspace): one in the cell they're in whose far
/// side (`XTEL`) is there, else, when `space` is an interior, one of its
/// doors' far sides that stands where they are (the door outside a
/// building). One door only: places two doors away aren't reached. How the
/// game's people find their way between places isn't traced.
pub fn door_toward(
    order: &LoadOrder,
    state: &GameState,
    actor: FormId,
    space: FormId,
) -> Option<DoorWay> {
    let (here_space, here_cell, here, _) = state.place(order, actor)?;
    let space_of = |r: FormId| {
        let w = crate::scripting::whereabouts(order, r)?;
        Some((w.world.unwrap_or(w.cell), w.cell))
    };
    let mut ways: Vec<DoorWay> = Vec::new();
    // A door here whose far side is there.
    for rr in order.references_in_cell(here_cell) {
        let Some(t) = crate::placement::teleport_of(&rr) else {
            continue;
        };
        let Some((to_space, to_cell)) = space_of(t.door) else {
            continue;
        };
        if to_space != space {
            continue;
        }
        let Some(w) = crate::scripting::whereabouts(order, rr.form_id) else {
            continue;
        };
        ways.push(DoorWay {
            door: rr.form_id,
            at: w.position,
            to_space,
            to_cell,
            to: t.position,
            heading: t.rotation[2],
        });
    }
    // Into an interior: its doors' far sides that stand here.
    if ways.is_empty()
        && order
            .get(space)
            .is_some_and(|r| r.entry.header.kind == esm::sig::CELL)
    {
        for rr in order.references_in_cell(space) {
            let Some(t) = crate::placement::teleport_of(&rr) else {
                continue;
            };
            let outside = t.door;
            if space_of(outside).map(|s| s.0) != Some(here_space) {
                continue;
            }
            let Some(w) = crate::scripting::whereabouts(order, outside) else {
                continue;
            };
            let Some(back) = order
                .get(outside)
                .and_then(|o| crate::placement::teleport_of(&o))
            else {
                continue;
            };
            ways.push(DoorWay {
                door: outside,
                at: w.position,
                to_space: space,
                to_cell: space,
                to: back.position,
                heading: back.rotation[2],
            });
        }
    }
    let d = |p: [f32; 3]| (0..3).map(|i| (p[i] - here[i]).powi(2)).sum::<f32>();
    ways.into_iter().min_by(|a, b| d(a.at).total_cmp(&d(b.at)))
}

/// The navmeshes people out of sight walk on, loaded once each: an
/// interior's whole navmesh, outdoors the 3 × 3 squares around a square.
#[derive(Default)]
pub struct NavCache {
    meshes: HashMap<(FormId, Option<(i32, i32)>), NavMesh>,
    grids: HashMap<FormId, Option<crate::WorldGrid>>,
}

impl NavCache {
    /// The navmesh around a point in a place (an interior cell or a
    /// worldspace).
    pub fn around(&mut self, order: &LoadOrder, space: FormId, at: [f32; 3]) -> &NavMesh {
        let interior = order
            .get(space)
            .is_some_and(|r| r.entry.header.kind == esm::sig::CELL);
        let key = if interior {
            (space, None)
        } else {
            (space, Some(crate::square_of(at)))
        };
        if !self.meshes.contains_key(&key) {
            let mesh = match key.1 {
                None => NavMesh::load(order, space),
                Some((x, y)) => {
                    let grid = self
                        .grids
                        .entry(space)
                        .or_insert_with(|| crate::WorldGrid::load(order, space).ok());
                    let cells: Vec<FormId> = grid
                        .as_ref()
                        .map(|g| {
                            (-1..=1)
                                .flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
                                .filter_map(|s| g.cells.get(&s).copied())
                                .collect()
                        })
                        .unwrap_or_default();
                    NavMesh::load_cells(order, &cells)
                }
            };
            self.meshes.insert(key, mesh);
        }
        &self.meshes[&key]
    }
}

/// What an update out of sight did ([`move_offstage`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offstage {
    /// Nowhere to go, or no way there.
    Stayed,
    /// Walked on (and through doors, if any).
    Moved,
    /// At their package's place.
    Arrived,
}

/// The most doors someone out of sight goes through in one update.
const MOST_DOORS: usize = 4;

/// One update of someone out of sight (`009ea8a0`, the virtual path
/// handler, with the low process's travel `0090ad40`): they walk `distance`
/// ([`crate::movement::offstage_speed`] × [`crate::movement::
/// offstage_seconds`]) along their navmesh path toward their package's
/// place ([`destination`]), point to point, stopping partway; toward a
/// place elsewhere to the load door that leads there ([`door_toward`]) and
/// through it (put at its far side, as `009ead90` teleports them), going on
/// with what's left of the distance. No path: they stay. (The game's paths
/// run through doors themselves; here a door is the end of one path and the
/// start of the next.)
pub fn move_offstage(
    order: &LoadOrder,
    state: &mut GameState,
    who: FormId,
    distance: f32,
    navs: &mut NavCache,
) -> Offstage {
    let Some(package) = current_package(order, state, who) else {
        return Offstage::Stayed;
    };
    let mut left = distance;
    let mut moved = false;
    let result = |moved: bool| {
        if moved {
            Offstage::Moved
        } else {
            Offstage::Stayed
        }
    };
    for _ in 0..MOST_DOORS {
        let Some((space, _, here, heading)) = state.place(order, who) else {
            break;
        };
        if let Some((to, radius)) = destination(order, state, who, &package) {
            // At the place: facing an `XMarkerHeading`'s heading (or their
            // editor heading), set at once (`0090ad40`).
            let facing = arrival_heading(order, state, who, &package);
            if crate::movement::arrived(here, to, radius) {
                if let Some(h) = facing.filter(|h| *h != heading) {
                    state.positions.insert(who, (here, h));
                }
                return if moved {
                    Offstage::Moved
                } else {
                    Offstage::Arrived
                };
            }
            let Some(path) = navs.around(order, space, here).path(here, to) else {
                return result(moved);
            };
            let w = crate::movement::walk_polyline(&path[1..], here, heading, left, radius);
            if w.done {
                state
                    .positions
                    .insert(who, (w.at, facing.unwrap_or(w.heading)));
                return Offstage::Arrived;
            }
            state.positions.insert(who, (w.at, w.heading));
            return Offstage::Moved;
        }
        let Some((target_space, _)) = target_place(order, state, &package) else {
            break;
        };
        if target_space == space {
            break;
        }
        let Some(way) = door_toward(order, state, who, target_space) else {
            break;
        };
        let Some(path) = navs.around(order, space, here).path(here, way.at) else {
            break;
        };
        let w = crate::movement::walk_polyline(&path[1..], here, heading, left, 0.0);
        if !w.done {
            state.positions.insert(who, (w.at, w.heading));
            return Offstage::Moved;
        }
        left = w.left;
        state.stand(who);
        state.spaces.insert(who, (way.to_space, way.to_cell));
        state.positions.insert(who, (way.to, way.heading));
        moved = true;
    }
    result(moved)
}

/// People scripts or doors have taken into a place from elsewhere: those
/// the state has in `space` (an interior cell or a worldspace) whose
/// placement is in another interior or worldspace, so loading the place
/// doesn't already bring them.
pub fn moved_into(order: &LoadOrder, state: &GameState, space: FormId) -> Vec<FormId> {
    let mut out: Vec<FormId> = state
        .spaces
        .iter()
        .filter(|(_, (s, _))| *s == space)
        .map(|(r, _)| *r)
        .filter(|&r| {
            crate::scripting::whereabouts(order, r)
                .is_some_and(|w| w.world.unwrap_or(w.cell) != space)
        })
        .filter(|r| {
            order.get(*r).is_some_and(|rr| {
                rr.entry.header.kind == esm::sig::ACHR || rr.entry.header.kind == esm::sig::ACRE
            })
        })
        .collect();
    out.sort();
    out
}

/// How far from the navmesh a path may start or end (a person or marker
/// standing just off its edge).
pub const OFF_MESH: f32 = 128.0;

/// A cell's navmesh: every `NAVM` in it, joined.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavMesh {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<NavTriangle>,
    /// Door portals (`NVDP`, 8 bytes each: the door reference, its
    /// triangle, 2 unused): the triangles a path crosses a door on, by
    /// triangle (indices into `triangles`). Someone walking onto one
    /// opens the door if it's shut (`world::doors`, `009e20c0`).
    pub door_portals: HashMap<usize, FormId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavTriangle {
    pub vertices: [usize; 3],
    /// The triangle across each edge (edge `i` runs from vertex `i` to
    /// vertex `i + 1`).
    pub neighbors: [Option<usize>; 3],
}

impl NavMesh {
    /// A cell's navmesh.
    pub fn load(order: &LoadOrder, cell: FormId) -> NavMesh {
        NavMesh::load_cells(order, &[cell])
    }

    /// The navmeshes of several cells (the outdoor squares around the
    /// player), joined where they connect. An edge whose bit is set in the
    /// triangle's flags (0x1 edge 0, 0x2 edge 1, 0x4 edge 2) leads to
    /// another navmesh: its link counts into the external connections
    /// (`NVEX`, 10 bytes each: 4 unknown, the navmesh's form ID, the
    /// triangle in it). Checked on Goodsprings' square: 21 + 24 + 8 flagged
    /// edges for its 53 external connections, and their links all below
    /// 53.
    pub fn load_cells(order: &LoadOrder, cells: &[FormId]) -> NavMesh {
        struct Part {
            form: FormId,
            first_vertex: usize,
            first_triangle: usize,
            raw: Vec<([u16; 3], [u16; 3], u16)>,
            external: Vec<(FormId, u16)>,
            doors: Vec<(FormId, u16)>,
        }
        let mut mesh = NavMesh::default();
        let mut parts = Vec::new();
        for &cell in cells {
            for rr in order.in_cell(cell) {
                if rr.entry.header.kind != NAVM || rr.entry.header.is_deleted() {
                    continue;
                }
                let Ok(record) = rr.record() else { continue };
                let (Some(vx), Some(tr)) = (record.get(NVVX), record.get(NVTR)) else {
                    continue;
                };
                let first_vertex = mesh.vertices.len();
                mesh.vertices.extend(
                    vx.data
                        .chunks_exact(12)
                        .map(|c| [le_f32(c, 0), le_f32(c, 4), le_f32(c, 8)]),
                );
                let raw: Vec<([u16; 3], [u16; 3], u16)> = tr
                    .data
                    .chunks_exact(16)
                    .map(|c| {
                        let u = |i: usize| u16::from_le_bytes([c[i], c[i + 1]]);
                        ([u(0), u(2), u(4)], [u(6), u(8), u(10)], u(12))
                    })
                    .collect();
                let external = record
                    .get(NVEX)
                    .map(|s| {
                        s.data
                            .chunks_exact(10)
                            .map(|c| {
                                (
                                    rr.plugin.to_global(FormId(le_u32(c, 4))),
                                    u16::from_le_bytes([c[8], c[9]]),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let doors = record
                    .get(NVDP)
                    .map(|s| {
                        s.data
                            .chunks_exact(8)
                            .map(|c| {
                                (
                                    rr.plugin.to_global(FormId(le_u32(c, 0))),
                                    u16::from_le_bytes([c[4], c[5]]),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                parts.push(Part {
                    form: rr.form_id,
                    first_vertex,
                    first_triangle: 0,
                    raw,
                    external,
                    doors,
                });
            }
        }
        let mut next = 0;
        for p in &mut parts {
            p.first_triangle = next;
            next += p.raw.len();
        }
        let start_of: HashMap<FormId, (usize, usize)> = parts
            .iter()
            .map(|p| (p.form, (p.first_triangle, p.raw.len())))
            .collect();
        let vertex_count = mesh.vertices.len();
        for p in &parts {
            let triangles = p.raw.len();
            for &(v, n, flags) in &p.raw {
                let vertices = v.map(|v| (p.first_vertex + usize::from(v)).min(vertex_count - 1));
                let mut neighbors = [None; 3];
                for e in 0..3 {
                    let link = n[e];
                    if link == 0xFFFF {
                        continue;
                    }
                    neighbors[e] = if flags & (1 << e) != 0 {
                        // To another navmesh, if it's loaded.
                        p.external.get(usize::from(link)).and_then(|(form, t)| {
                            let (first, count) = *start_of.get(form)?;
                            (usize::from(*t) < count).then(|| first + usize::from(*t))
                        })
                    } else {
                        (usize::from(link) < triangles)
                            .then(|| p.first_triangle + usize::from(link))
                    };
                }
                mesh.triangles.push(NavTriangle {
                    vertices,
                    neighbors,
                });
            }
            for &(door, t) in &p.doors {
                if usize::from(t) < triangles && door.0 != 0 {
                    mesh.door_portals
                        .insert(p.first_triangle + usize::from(t), door);
                }
            }
        }
        mesh
    }

    fn corner(&self, t: usize, i: usize) -> [f32; 3] {
        self.vertices[self.triangles[t].vertices[i % 3]]
    }

    fn centroid(&self, t: usize) -> [f32; 3] {
        let [a, b, c] = [0, 1, 2].map(|i| self.corner(t, i));
        [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0)
    }

    /// The triangle under a point, if the point is on (or within
    /// [`OFF_MESH`] of) the navmesh.
    fn triangle_near(&self, p: [f32; 3]) -> Option<usize> {
        let t = self.triangle_at(p)?;
        let [a, b, c] = [0, 1, 2].map(|i| self.corner(t, i));
        if height_in(a, b, c, p).is_some_and(|z| (z - p[2]).abs() < 200.0) {
            return Some(t);
        }
        // Nearest corner within reach.
        let near = [a, b, c, self.centroid(t)]
            .iter()
            .map(|q| distance2(*q, p))
            .fold(f32::INFINITY, f32::min);
        (near <= OFF_MESH * OFF_MESH).then_some(t)
    }

    /// The triangle a point stands on: one whose outline (seen from above)
    /// holds it, nearest in height; else the one whose middle is nearest.
    pub fn triangle_at(&self, p: [f32; 3]) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for t in 0..self.triangles.len() {
            let [a, b, c] = [0, 1, 2].map(|i| self.corner(t, i));
            if let Some(z) = height_in(a, b, c, p) {
                let dz = (z - p[2]).abs();
                if best.map_or(true, |(d, _)| dz < d) {
                    best = Some((dz, t));
                }
            }
        }
        if let Some((dz, t)) = best {
            if dz < 200.0 {
                return Some(t);
            }
        }
        (0..self.triangles.len()).min_by(|&x, &y| {
            distance2(self.centroid(x), p).total_cmp(&distance2(self.centroid(y), p))
        })
    }

    /// A path from one point to another over the navmesh: the points to
    /// walk through, ending at `to`. `None` when no path joins them, or
    /// either end is off the navmesh (more than [`OFF_MESH`] from it).
    pub fn path(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        self.path_with_doors(from, to).map(|(points, _)| points)
    }

    /// [`Self::path`], and the doors the path goes through: for each door
    /// portal triangle crossed (`NVDP`, in order), the door and the middle
    /// of its triangle. Someone walking the path opens each closed door
    /// when they reach it (`009e20c0`).
    #[allow(clippy::type_complexity)]
    pub fn path_with_doors(
        &self,
        from: [f32; 3],
        to: [f32; 3],
    ) -> Option<(Vec<[f32; 3]>, Vec<(FormId, [f32; 3])>)> {
        let start = self.triangle_near(from)?;
        let goal = self.triangle_near(to)?;
        let doors_on = |triangles: &[usize]| -> Vec<(FormId, [f32; 3])> {
            triangles
                .iter()
                .filter_map(|&t| self.door_portals.get(&t).map(|&d| (d, self.centroid(t))))
                .collect()
        };
        // `bUseStraightLineCheckFirst` (1): a straight line that stays on
        // the navmesh is the path (`006cc5e0` → `006cd1f0`). (The check
        // there also uses `fPathingLargeActorRadius`, 80, in a way not
        // traced; the line itself is tested here.) The doors are those of
        // the triangles it crosses.
        if let Some(crossed) = self.line_crossing(from, to, start, goal) {
            return Some((vec![from, to], doors_on(&crossed)));
        }
        let corridor = self.corridor(start, goal, &[])?;
        Some((self.funnel(from, to, &corridor), doors_on(&corridor)))
    }

    /// A path keeping away from others in the way (`009e5ae0` makes a new
    /// path request with them as avoid nodes, [`crate::movement::
    /// AvoidNode`]): triangles whose middle is inside a node cost its cost ×
    /// as much to cross (how the game's search weighs its avoid nodes isn't
    /// traced; the straight line isn't tried, since it would go through
    /// them).
    pub fn path_avoiding(
        &self,
        from: [f32; 3],
        to: [f32; 3],
        avoid: &[crate::movement::AvoidNode],
    ) -> Option<Vec<[f32; 3]>> {
        let start = self.triangle_near(from)?;
        let goal = self.triangle_near(to)?;
        let corridor = self.corridor(start, goal, avoid)?;
        Some(self.funnel(from, to, &corridor))
    }

    /// Whether the straight line from `from` (on triangle `start`) to `to`
    /// (on `goal`) stays on the navmesh, crossing from triangle to triangle
    /// through shared edges: the triangles it crosses, in order, when it
    /// does.
    fn line_crossing(
        &self,
        from: [f32; 3],
        to: [f32; 3],
        start: usize,
        goal: usize,
    ) -> Option<Vec<usize>> {
        let mut t = start;
        let mut entered = -1.0f32;
        let mut crossed = Vec::new();
        for _ in 0..=self.triangles.len() {
            crossed.push(t);
            if t == goal {
                return Some(crossed);
            }
            // The edge the line leaves through: the crossing farthest on.
            let mut exit: Option<(f32, Option<usize>)> = None;
            for i in 0..3 {
                let (a, b) = (self.corner(t, i), self.corner(t, i + 1));
                if let Some(s) = crossing(from, to, a, b) {
                    if s > entered + 1e-5 && exit.map_or(true, |(e, _)| s > e) {
                        exit = Some((s, self.triangles[t].neighbors[i]));
                    }
                }
            }
            match exit {
                Some((s, Some(n))) => {
                    entered = s;
                    t = n;
                }
                _ => return None,
            }
        }
        None
    }

    /// Triangles from `start` to `goal` (A* between their middles), those
    /// inside avoid nodes costing more.
    fn corridor(
        &self,
        start: usize,
        goal: usize,
        avoid: &[crate::movement::AvoidNode],
    ) -> Option<Vec<usize>> {
        let weight = |t: usize| {
            let c = self.centroid(t);
            avoid
                .iter()
                .filter(|n| {
                    (c[0] - n.position[0]).powi(2) + (c[1] - n.position[1]).powi(2)
                        <= n.radius * n.radius
                })
                .fold(1.0f32, |w, n| w * n.cost.max(1.0))
        };
        #[derive(PartialEq)]
        struct Node(f32, usize);
        impl Eq for Node {}
        impl Ord for Node {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        impl PartialOrd for Node {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        let goal_at = self.centroid(goal);
        let mut open = BinaryHeap::new();
        let mut cost: HashMap<usize, f32> = HashMap::new();
        let mut came: HashMap<usize, usize> = HashMap::new();
        cost.insert(start, 0.0);
        open.push(Node(0.0, start));
        while let Some(Node(_, t)) = open.pop() {
            if t == goal {
                let mut path = vec![goal];
                let mut at = goal;
                while let Some(&prev) = came.get(&at) {
                    path.push(prev);
                    at = prev;
                }
                path.reverse();
                return Some(path);
            }
            let here = self.centroid(t);
            for n in self.triangles[t].neighbors.into_iter().flatten() {
                let c = cost[&t] + distance2(here, self.centroid(n)).sqrt() * weight(n);
                if cost.get(&n).map_or(true, |&old| c < old) {
                    cost.insert(n, c);
                    came.insert(n, t);
                    open.push(Node(c + distance2(self.centroid(n), goal_at).sqrt(), n));
                }
            }
        }
        None
    }

    /// The edge two neighbouring triangles share, as (left, right) seen
    /// going from `a` into `b` (from above, left is counterclockwise).
    /// Leaving a counterclockwise triangle across the edge from its corner
    /// `i` to `i + 1`, corner `i + 1` is on the left; a clockwise one the
    /// other way round.
    fn portal(&self, a: usize, b: usize) -> Option<([f32; 3], [f32; 3])> {
        let i = self.triangles[a]
            .neighbors
            .iter()
            .position(|&n| n == Some(b))?;
        let (p, q) = (self.corner(a, i), self.corner(a, i + 1));
        let counterclockwise =
            cross2(self.corner(a, 0), self.corner(a, 1), self.corner(a, 2)) > 0.0;
        Some(if counterclockwise { (q, p) } else { (p, q) })
    }

    /// The shortest line through the corridor's shared edges (the
    /// "simple stupid funnel").
    fn funnel(&self, from: [f32; 3], to: [f32; 3], corridor: &[usize]) -> Vec<[f32; 3]> {
        let mut portals: Vec<([f32; 3], [f32; 3])> = vec![(from, from)];
        for w in corridor.windows(2) {
            if let Some(p) = self.portal(w[0], w[1]) {
                portals.push(p);
            }
        }
        portals.push((to, to));
        let mut points = vec![from];
        let (mut apex, mut left, mut right) = (from, from, from);
        let (mut left_i, mut right_i) = (0usize, 0usize);
        let mut i = 1;
        while i < portals.len() {
            let (l, r) = portals[i];
            // The right side moves in (counterclockwise)?
            if cross2(apex, right, r) >= 0.0 {
                if same(apex, right) || cross2(apex, left, r) < 0.0 {
                    right = r;
                    right_i = i;
                } else {
                    // It crossed the left side: that corner is on the path,
                    // and the funnel starts again from it.
                    let at = left_i;
                    push_new(&mut points, left);
                    apex = left;
                    right = apex;
                    left_i = at;
                    right_i = at;
                    i = at + 1;
                    continue;
                }
            }
            // The left side moves in (clockwise)?
            if cross2(apex, left, l) <= 0.0 {
                if same(apex, left) || cross2(apex, right, l) > 0.0 {
                    left = l;
                    left_i = i;
                } else {
                    let at = right_i;
                    push_new(&mut points, right);
                    apex = right;
                    left = apex;
                    left_i = at;
                    right_i = at;
                    i = at + 1;
                    continue;
                }
            }
            i += 1;
        }
        if points.last().map_or(true, |p| !same(*p, to)) {
            points.push(to);
        }
        points
    }
}

/// Adds a corner unless it's where the path already is (one vertex can
/// end several shared edges in a row).
fn push_new(points: &mut Vec<[f32; 3]>, p: [f32; 3]) {
    if points.last().map_or(true, |last| !same(*last, p)) {
        points.push(p);
    }
}

fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    distance2(a, b) < 1e-6
}

/// Where the segment `p`–`q` crosses the segment `a`–`b`, seen from above:
/// the fraction along `p`–`q`, if they cross.
fn crossing(p: [f32; 3], q: [f32; 3], a: [f32; 3], b: [f32; 3]) -> Option<f32> {
    let r = [q[0] - p[0], q[1] - p[1]];
    let s = [b[0] - a[0], b[1] - a[1]];
    let denom = r[0] * s[1] - r[1] * s[0];
    if denom.abs() < 1e-9 {
        return None;
    }
    let ap = [a[0] - p[0], a[1] - p[1]];
    let t = (ap[0] * s[1] - ap[1] * s[0]) / denom;
    let u = (ap[0] * r[1] - ap[1] * r[0]) / denom;
    ((-1e-5..=1.0 + 1e-5).contains(&t) && (-1e-5..=1.0 + 1e-5).contains(&u)).then_some(t)
}

/// Seen from above: positive when `c` is to the left of the line from `a`
/// to `b` (counterclockwise).
fn cross2(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// The triangle's height under a point, if the point is inside it seen
/// from above.
fn height_in(a: [f32; 3], b: [f32; 3], c: [f32; 3], p: [f32; 3]) -> Option<f32> {
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-9 {
        return None;
    }
    let w1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let w2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    let w3 = 1.0 - w1 - w2;
    let eps = -1e-4;
    (w1 >= eps && w2 >= eps && w3 >= eps).then(|| w1 * a[2] + w2 * b[2] + w3 * c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An L-shaped corridor: a 100-wide strip going east, then north.
    ///
    /// ```text
    ///   6---7
    ///   |   |
    ///   4---5
    ///   |   |
    ///   2---3 (corner)
    /// 0-1...
    /// ```
    fn corridor_mesh() -> NavMesh {
        // Squares of 100: (0,0)-(100,100), (100,0)-(200,100),
        // (100,100)-(200,200), (100,200)-(200,300).
        let v = vec![
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [200.0, 0.0, 0.0],
            [0.0, 100.0, 0.0],
            [100.0, 100.0, 0.0],
            [200.0, 100.0, 0.0],
            [100.0, 200.0, 0.0],
            [200.0, 200.0, 0.0],
            [100.0, 300.0, 0.0],
            [200.0, 300.0, 0.0],
        ];
        let t = |vertices: [usize; 3], neighbors: [Option<usize>; 3]| NavTriangle {
            vertices,
            neighbors,
        };
        NavMesh {
            vertices: v,
            triangles: vec![
                // Square A (0,1,4,3): triangles 0 (0,1,4) and 1 (0,4,3).
                t([0, 1, 4], [None, Some(2), Some(1)]),
                t([0, 4, 3], [Some(0), None, None]),
                // Square B (1,2,5,4): 2 (1,5,4), 3 (1,2,5).
                t([1, 5, 4], [Some(3), Some(4), Some(0)]),
                t([1, 2, 5], [None, None, Some(2)]),
                // Square C (4,5,7,6): 4 (4,5,7), 5 (4,7,6).
                t([4, 5, 7], [Some(2), None, Some(5)]),
                t([4, 7, 6], [Some(4), Some(6), None]),
                // Square D (6,7,9,8): 6 (6,7,9)... shares 7-6 with 5.
                t([7, 6, 8], [Some(5), None, Some(7)]),
                t([7, 8, 9], [Some(6), None, None]),
            ],
            // A door across square C's first triangle.
            door_portals: HashMap::from([(4, FormId(0x904))]),
        }
    }

    #[test]
    fn a_path_turns_the_corner_at_the_inside_corner() {
        let mesh = corridor_mesh();
        assert_eq!(mesh.triangle_at([20.0, 50.0, 0.0]), Some(1));
        let path = mesh.path([20.0, 50.0, 0.0], [150.0, 280.0, 0.0]).unwrap();
        // From the start straight to the inside corner (100,100), then on
        // to the goal.
        assert_eq!(path.len(), 3, "{path:?}");
        assert!(same(path[1], [100.0, 100.0, 0.0]), "{path:?}");
        assert!(same(path[2], [150.0, 280.0, 0.0]));
        // In a straight line, no corners.
        let straight = mesh.path([150.0, 20.0, 0.0], [150.0, 280.0, 0.0]).unwrap();
        assert_eq!(straight.len(), 2, "{straight:?}");
    }

    #[test]
    fn a_path_names_the_doors_it_goes_through() {
        let mesh = corridor_mesh();
        // Through square C: the door, at its triangle's middle.
        let (_, doors) = mesh
            .path_with_doors([20.0, 50.0, 0.0], [150.0, 280.0, 0.0])
            .unwrap();
        assert_eq!(doors.len(), 1);
        assert_eq!(doors[0].0, FormId(0x904));
        assert!(same(doors[0].1, mesh.centroid(4)), "{:?}", doors[0].1);
        // A straight line (no search, `bUseStraightLineCheckFirst`) through
        // square C names the door too.
        let (line, doors) = mesh
            .path_with_doors([150.0, 20.0, 0.0], [150.0, 280.0, 0.0])
            .unwrap();
        assert_eq!(line.len(), 2, "{line:?}");
        assert_eq!(
            doors.iter().map(|d| d.0).collect::<Vec<_>>(),
            [FormId(0x904)]
        );
        // Within square A: none.
        let (_, none) = mesh
            .path_with_doors([20.0, 50.0, 0.0], [80.0, 20.0, 0.0])
            .unwrap();
        assert!(none.is_empty());
    }

    /// Four squares of 100, two by two, each as two triangles: from the
    /// bottom left to the top right by the bottom right (triangles 3, 6) or
    /// by the top left (1, 4).
    fn four_squares() -> NavMesh {
        let mut vertices = Vec::new();
        for y in 0..3 {
            for x in 0..3 {
                vertices.push([x as f32 * 100.0, y as f32 * 100.0, 0.0]);
            }
        }
        let t = |vertices: [usize; 3], neighbors: [Option<usize>; 3]| NavTriangle {
            vertices,
            neighbors,
        };
        NavMesh {
            vertices,
            triangles: vec![
                t([0, 1, 4], [None, Some(3), Some(1)]),
                t([0, 4, 3], [Some(0), Some(4), None]),
                t([1, 2, 5], [None, None, Some(3)]),
                t([1, 5, 4], [Some(2), Some(6), Some(0)]),
                t([3, 4, 7], [Some(1), Some(7), Some(5)]),
                t([3, 7, 6], [Some(4), None, None]),
                t([4, 5, 8], [Some(3), None, Some(7)]),
                t([4, 8, 7], [Some(6), None, Some(4)]),
            ],
            door_portals: HashMap::new(),
        }
    }

    #[test]
    fn a_straight_line_is_tried_first_and_avoid_nodes_cost_more() {
        let mesh = four_squares();
        // The straight line stays on the navmesh: no search.
        let p = mesh.path([20.0, 30.0, 0.0], [180.0, 170.0, 0.0]).unwrap();
        assert_eq!(p, vec![[20.0, 30.0, 0.0], [180.0, 170.0, 0.0]]);
        // Leaving the navmesh: no line.
        assert!(mesh
            .line_crossing([20.0, 30.0, 0.0], [20.0, 300.0, 0.0], 0, 5)
            .is_none());
        // A* takes one way round; a costly avoid node on its middle square
        // sends it the other way.
        let (start, goal) = (0, 7);
        let plain = mesh.corridor(start, goal, &[]).unwrap();
        let (busy, other) = if plain.contains(&3) { (3, 4) } else { (4, 3) };
        let node = crate::movement::AvoidNode {
            position: mesh.centroid(busy),
            radius: 10.0,
            cost: 2.0,
        };
        let round = mesh.corridor(start, goal, &[node]).unwrap();
        assert!(
            round.contains(&other) && !round.contains(&busy),
            "{round:?}"
        );
        assert!(mesh
            .path_avoiding([66.0, 33.0, 0.0], [133.0, 166.0, 0.0], &[node])
            .is_some());
    }

    #[test]
    fn schedules_cover_their_hours_and_days() {
        let night = Schedule {
            month: -1,
            day_of_week: -1,
            date: 0,
            hour: 22,
            duration: 8,
        };
        assert!(night.covers(0, 3, 1, 23.5));
        assert!(night.covers(0, 3, 1, 5.0));
        assert!(!night.covers(0, 3, 1, 12.0));
        let weekends = Schedule {
            day_of_week: 8,
            ..night
        };
        assert!(weekends.covers(0, 6, 1, 23.0) && !weekends.covers(0, 2, 1, 23.0));
        let any = Schedule {
            month: -1,
            day_of_week: -1,
            date: 0,
            hour: -1,
            duration: 0,
        };
        assert!(any.covers(5, 4, 20, 13.0));
    }
}
