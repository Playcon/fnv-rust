//! Cells and what's placed in them, read from a load order.
//!
//! A cell is one area of the game: an interior (a house, a vault) or one
//! square of an outdoor worldspace. Its contents are references: each points
//! at a base object (a static wall piece, a door, a light, a chair) and gives
//! it a position, rotation and scale. This crate resolves those references
//! against the load order, works out which ones the game shows when the cell
//! first loads, and turns their placement into transforms.
//!
//! ```no_run
//! use esm::{ActivePlugins, LoadOrder};
//!
//! let order = LoadOrder::from_data_dir("Data", &ActivePlugins::OfficialOnly)?;
//! let cell = world::find_cells(&order, "GSDocMitchellHouse")?[0];
//! let loaded = world::load_cell(&order, cell)?;
//! for object in &loaded.objects {
//!     println!("{:?} at {:?}", object.model, object.position);
//! }
//! # Ok::<(), world::Error>(())
//! ```

pub mod actor;
pub mod ai;
pub mod animation;
pub mod barter;
pub mod body_parts;
mod cell;
pub mod chargen;
pub mod combat;
pub mod combat_ai;
pub mod crime;
pub mod detection;
pub mod dialogue;
pub mod doors;
pub mod experience;
mod exterior;
pub mod face;
pub mod factions;
pub mod functions;
pub mod furniture;
pub mod grass;
pub mod idles;
mod image_space;
pub mod impacts;
pub mod items;
pub mod land;
pub mod leveled;
pub mod lip;
pub mod living;
pub mod lockpick;
pub mod locks;
pub mod lod;
pub mod look_ik;
pub mod magic;
pub mod map;
pub mod modifier;
pub mod more_functions;
pub mod movement;
pub mod music;
pub mod particles;
pub mod perks;
mod placement;
pub mod quest;
pub mod region;
pub mod reputation;
mod rotation;
pub mod sandbox;
pub mod save;
pub mod script_functions;
pub mod scripting;
pub mod social;
pub mod sound;
pub mod stats;
pub mod terminal;
pub mod tree;
pub mod vats;
pub mod vats_camera;
pub mod water;
pub mod weather;

use std::fmt;

use esm::{FormId, FourCC};

pub use actor::{actor_look, ActorLook, ActorPart, Face, Fighting};
pub use cell::{
    cell_info, describe_record, find_cells, interior_cells, CellInfo, CellSummary, Lighting,
    CELL_HAS_WATER, CELL_INTERIOR,
};
pub use exterior::{find_worldspace, square_of, worldspaces, WorldGrid, Worldspace};
pub use image_space::{Cinematic, Hdr, ImageSpace};
pub use land::{Land, LandTexture};
pub use placement::{
    enabled_now, is_marker, light_flags, load_cell, load_cell_now, made_placement, placement_of,
    resolve_emittance, Arrival, Disabled, Emittance, LeftOut, Light, LoadedCell, Part, PlacedLight,
    Placement, Primitive, Teleport, COLLISION_MARKER, OPEN_BY_DEFAULT,
};
pub use rotation::{is_tilted, AxisOrder, RotationConvention};

#[derive(Debug)]
pub enum Error {
    Esm(esm::Error),
    NoSuchRecord(FormId),
    NotACell {
        form_id: FormId,
        kind: FourCC,
    },
    /// A record of another type than the one wanted.
    NotA {
        form_id: FormId,
        kind: FourCC,
        wanted: &'static str,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Esm(e) => e.fmt(f),
            Error::NoSuchRecord(id) => write!(f, "no record has the form ID {id}"),
            Error::NotACell { form_id, kind } => {
                write!(f, "{form_id} is a {kind} record, not a cell")
            }
            Error::NotA {
                form_id,
                kind,
                wanted,
            } => write!(f, "{form_id} is a {kind} record, not a {wanted}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Esm(e) => Some(e),
            _ => None,
        }
    }
}

impl From<esm::Error> for Error {
    fn from(e: esm::Error) -> Self {
        Error::Esm(e)
    }
}
