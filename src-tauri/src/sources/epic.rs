//! Epic Games Launcher: one JSON `.item` manifest per installed app under
//! `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`. Launches go through the Epic client
//! (`com.epicgames.launcher://apps/...?action=launch`), which handles login and ownership.

use super::Found;
use crate::models::{NewExecutable, NewGame, Source};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
#[serde(default)]
struct Item {
    #[serde(rename = "DisplayName")]
    display_name: String,
    #[serde(rename = "InstallLocation")]
    install_location: String,
    #[serde(rename = "LaunchExecutable")]
    launch_executable: String,
    #[serde(rename = "AppName")]
    app_name: String,
    #[serde(rename = "MainGameAppName")]
    main_game_app_name: String,
    #[serde(rename = "CatalogNamespace")]
    catalog_namespace: String,
    #[serde(rename = "CatalogItemId")]
    catalog_item_id: String,
    #[serde(rename = "bIsIncompleteInstall")]
    incomplete: bool,
    #[serde(rename = "AppCategories")]
    categories: Vec<String>,
}

pub fn manifests_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NOVA_EPIC_MANIFESTS") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
    let p = base.join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests");
    p.is_dir().then_some(p)
}

pub fn scan() -> anyhow::Result<(PathBuf, Vec<Found>)> {
    let dir = manifests_dir().ok_or_else(|| anyhow::anyhow!("the Epic Games Launcher isn't installed"))?;
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir)?.flatten() {
        if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("item")) {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if let Some(f) = parse_item(&text) {
                    out.push(f);
                }
            }
        }
    }
    Ok((dir, out))
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn parse_item(text: &str) -> Option<Found> {
    let it: Item = serde_json::from_str(text).ok()?;
    if it.incomplete || it.display_name.trim().is_empty() || it.app_name.is_empty() {
        return None;
    }
    // DLC and add-ons have their own manifest pointing at the main game.
    if !it.main_game_app_name.is_empty() && it.main_game_app_name != it.app_name {
        return None;
    }
    if !it.categories.is_empty() && !it.categories.iter().any(|c| c == "games") {
        return None; // engines, plugins
    }
    let uri = format!("com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true", enc(&it.catalog_namespace), enc(&it.catalog_item_id), enc(&it.app_name));
    let exe = (!it.launch_executable.is_empty()).then(|| Path::new(&it.install_location).join(&it.launch_executable));
    Some(Found {
        game: NewGame {
            title: it.display_name.trim().to_string(),
            source: Some(Source::Epic),
            source_id: it.app_name.clone(),
            install_dir: it.install_location.clone(),
            launch_uri: uri,
            executables: exe.map(|p| vec![NewExecutable { path: p.to_string_lossy().to_string(), label: "Game".into(), ..Default::default() }]).unwrap_or_default(),
            ..Default::default()
        },
        cover: None,
        hero: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_games_and_skips_dlc_and_partial() {
        let game = r#"{"DisplayName":"Hollow Sky","InstallLocation":"D:\\Epic\\HollowSky","LaunchExecutable":"Bin/HollowSky.exe",
            "AppName":"Falcon","MainGameAppName":"Falcon","CatalogNamespace":"ns1","CatalogItemId":"item1","bIsIncompleteInstall":false,"AppCategories":["public","games","applications"]}"#;
        let f = parse_item(game).unwrap();
        assert_eq!(f.game.title, "Hollow Sky");
        assert_eq!(f.game.launch_uri, "com.epicgames.launcher://apps/ns1%3Aitem1%3AFalcon?action=launch&silent=true");
        assert_eq!(f.game.executables.len(), 1);
        let dlc = game.replace("\"MainGameAppName\":\"Falcon\"", "\"MainGameAppName\":\"Other\"");
        assert!(parse_item(&dlc).is_none());
        let partial = game.replace("\"bIsIncompleteInstall\":false", "\"bIsIncompleteInstall\":true");
        assert!(parse_item(&partial).is_none());
        assert!(parse_item("{not json").is_none());
    }
}
