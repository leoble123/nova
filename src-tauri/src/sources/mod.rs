//! Finds games installed by store clients by reading the clients' own manifest files. Read-only:
//! nothing is launched, and the clients' files are never written.

pub mod epic;
pub mod gog;
pub mod steam;
pub mod vdf;

use crate::models::NewGame;
use serde::Serialize;
use std::path::PathBuf;

/// What a scan found for one store: games, plus local artwork to copy into the cache.
pub struct Found {
    pub game: NewGame,
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoreReport {
    pub store: String,
    pub detected: bool,
    pub location: String,
    pub found: usize,
    pub added: usize,
    pub error: String,
}
