pub mod activities;
pub mod album;
pub mod animal;
pub mod face;
pub mod config;
pub mod dedup;
pub mod error;
pub mod image_open;
pub mod importer;
pub mod metadata;
pub mod storage;
pub mod web;

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "models/"]
struct EmbeddedModels;

/// Return a copy of the named model's bytes if it was compiled into the binary.
pub fn get_embedded_model(name: &str) -> Option<Vec<u8>> {
    EmbeddedModels::get(name).map(|f| f.data.into_owned())
}
