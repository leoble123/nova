//! Data shapes shared between the database, the backend services and the UI (camelCase JSON).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Manual,
    Steam,
    Epic,
    Gog,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Manual => "manual",
            Source::Steam => "steam",
            Source::Epic => "epic",
            Source::Gog => "gog",
        }
    }

    pub fn parse(s: &str) -> Source {
        match s {
            "steam" => Source::Steam,
            "epic" => Source::Epic,
            "gog" => Source::Gog,
            _ => Source::Manual,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Executable {
    pub id: i64,
    pub game_id: i64,
    /// Absolute path to the .exe, in the game's original folder. Never moved.
    pub path: String,
    pub label: String,
    pub args: String,
    /// Overrides the game's working directory for this executable ("" = use the game's).
    pub working_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub title: String,
    pub source: Source,
    /// Store id: Steam appid, Epic AppName, GOG product id. "" for manual games.
    pub source_id: String,
    pub developer: String,
    pub publisher: String,
    pub description: String,
    pub install_dir: String,
    /// Working directory for launches ("" = the install dir, i.e. the game's root folder).
    pub working_dir: String,
    pub launch_args: String,
    /// Store games launch through their client (steam://, com.epicgames.launcher://, ...).
    pub launch_uri: String,
    pub preferred_exe_id: Option<i64>,
    /// File name inside the art cache, "" = none (the UI draws a typographic cover).
    pub cover: String,
    pub hero: String,
    pub favorite: bool,
    pub hidden: bool,
    pub added_at: i64,
    pub last_played: Option<i64>,
    pub playtime_secs: i64,
    pub notes: String,
    /// The game's icon in the art cache ("" = not looked for yet, "-" = none).
    pub icon: String,
    pub executables: Vec<Executable>,
    pub collections: Vec<i64>,
    // ---- computed when listing, not stored
    pub installed: bool,
    /// Why it can't be launched right now ("" = it can).
    pub problem: String,
    pub running: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameUpdate {
    pub title: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub install_dir: Option<String>,
    pub working_dir: Option<String>,
    pub launch_args: Option<String>,
    pub preferred_exe_id: Option<i64>,
    pub favorite: Option<bool>,
    pub hidden: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub game_count: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub game_id: i64,
    pub started_at: i64,
    pub ended_at: i64,
    pub secs: i64,
    pub avg_fps: Option<f64>,
    pub low1: Option<f64>,
}

/// A new game, from the import wizard or a store scan.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NewGame {
    pub title: String,
    pub source: Option<Source>,
    pub source_id: String,
    pub developer: String,
    pub publisher: String,
    pub description: String,
    pub install_dir: String,
    pub working_dir: String,
    pub launch_args: String,
    pub launch_uri: String,
    /// Executables to register; the first is the preferred one unless `preferred` says otherwise.
    pub executables: Vec<NewExecutable>,
    pub preferred: usize,
    /// A local image to use as the cover (copied into the art cache).
    pub cover_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NewExecutable {
    pub path: String,
    pub label: String,
    pub args: String,
    pub working_dir: String,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
