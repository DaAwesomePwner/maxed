//! Local asset catalog boundary. Shared contracts live in `maxed-types`.

#![forbid(unsafe_code)]

mod cache;
mod catalog;
mod error;
mod reader;
mod reference_ids;
mod rules;
pub mod sprite_delivery;
mod sprites;
mod tiles;
mod toast_icons;
pub use toast_icons::{cached_toast_portal_icon, local_toast_icon_path, toast_portal_icon};
mod xml;
pub use cache::{
    client_fingerprint, load_or_extract, prune_unused_generations, AssetPaths, AssetProgress,
    CachedCatalog,
};
pub use catalog::{build_catalog, build_catalog_with_sheets, parse_equipment, validate_catalog};
pub use sprites::parse_sprites;
pub mod unity;

pub use error::AssetError;
pub use reader::checked_span;
pub use reference_ids::{build_reference_id_lists, ReferenceIdLists};
pub use unity::{
    inspect_resources, read_resources, ExtractedAssets, ResourceInfo, TextAsset, TextureAsset,
};
pub use unity::{
    locate_enchant_pips, open_sheets, read_text_resources, EnchantPips, SheetInfo, SheetReader,
};
