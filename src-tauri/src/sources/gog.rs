//! GOG: installed games are listed in the registry by GOG's installers. GOG games are DRM-free,
//! so they're launched directly from their own folder (no client needed).

use super::Found;
#[cfg(windows)]
use crate::models::{NewExecutable, NewGame, Source};
use std::path::PathBuf;

#[cfg(windows)]
pub fn scan() -> anyhow::Result<(PathBuf, Vec<Found>)> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\WOW6432Node\\GOG.com\\Games")
        .or_else(|_| hklm.open_subkey("SOFTWARE\\GOG.com\\Games"))
        .map_err(|_| anyhow::anyhow!("no GOG games are installed"))?;
    let mut out = Vec::new();
    for id in key.enum_keys().flatten() {
        let Ok(g) = key.open_subkey(&id) else { continue };
        let get = |n: &str| g.get_value::<String, _>(n).unwrap_or_default();
        let (name, path, exe) = (get("gameName"), get("path"), get("exe"));
        if name.is_empty() || path.is_empty() || get("dependsOn").trim() != "" {
            continue; // DLC entries depend on the base game
        }
        let working = get("workingDir");
        let args = get("launchParam");
        let exes = if exe.is_empty() { Vec::new() } else { vec![NewExecutable { path: exe, label: "Game".into(), args, working_dir: working }] };
        out.push(Found {
            game: NewGame { title: name, source: Some(Source::Gog), source_id: id.clone(), install_dir: path, executables: exes, ..Default::default() },
            cover: None,
            hero: None,
            icon: None,
        });
    }
    Ok((PathBuf::from("HKLM\\SOFTWARE\\GOG.com\\Games"), out))
}

#[cfg(not(windows))]
pub fn scan() -> anyhow::Result<(PathBuf, Vec<Found>)> {
    anyhow::bail!("GOG detection is only available on Windows")
}
