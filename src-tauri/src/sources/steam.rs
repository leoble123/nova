//! Steam: libraries from `steamapps/libraryfolders.vdf`, games from `appmanifest_<id>.acf`,
//! artwork from Steam's own local cache (`appcache/librarycache`). Launches go through the
//! Steam client (`steam://rungameid/<id>`) so DRM, overlays and cloud saves keep working.

use super::vdf;
use super::Found;
use crate::models::{NewGame, Source};
use std::path::{Path, PathBuf};

pub fn steam_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NOVA_STEAM_PATH") {
        return Some(PathBuf::from(p));
    }
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(k) = hkcu.open_subkey("Software\\Valve\\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                let p = PathBuf::from(p.replace('/', "\\"));
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(k) = hklm.open_subkey("SOFTWARE\\WOW6432Node\\Valve\\Steam") {
            if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                let p = PathBuf::from(p);
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
        let def = PathBuf::from("C:\\Program Files (x86)\\Steam");
        if def.is_dir() {
            return Some(def);
        }
    }
    None
}

/// Tools and runtimes Steam installs alongside games.
fn is_tool(appid: &str, name: &str) -> bool {
    const TOOL_IDS: &[&str] = &["228980", "1070560", "1391110", "1628350", "1493710", "961940", "1113280", "1245040", "1420170", "250820", "1826330"];
    let n = name.to_lowercase();
    TOOL_IDS.contains(&appid) || n.starts_with("proton") || n.contains("steamworks common") || n.contains("steam linux runtime") || n.contains("steamvr") || n.ends_with(" dedicated server") || n.ends_with(" sdk")
}

pub fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let file = root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(file) {
        let kv = vdf::parse(&text);
        if let Some(lf) = kv.get("libraryfolders") {
            for (_, entry) in lf.entries() {
                // Newer format: { "path" "..." }; older: "1" "D:\\SteamLibrary"
                let p = match entry {
                    vdf::Kv::Obj(_) => entry.str("path").map(str::to_string),
                    vdf::Kv::Str(s) => Some(s.clone()),
                };
                if let Some(p) = p {
                    let p = PathBuf::from(p);
                    if !out.iter().any(|o| same_path(o, &p)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase().trim_end_matches(['\\', '/']) == b.to_string_lossy().to_lowercase().trim_end_matches(['\\', '/'])
}

pub fn scan() -> anyhow::Result<(PathBuf, Vec<Found>)> {
    let root = steam_root().ok_or_else(|| anyhow::anyhow!("Steam isn't installed"))?;
    let mut out = Vec::new();
    for lib in libraries(&root) {
        let apps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue }; // library on a drive that's not connected
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            if let Some(f) = parse_manifest(&text, &apps, &root) {
                out.push(f);
            }
        }
    }
    Ok((root, out))
}

pub fn parse_manifest(text: &str, steamapps: &Path, root: &Path) -> Option<Found> {
    let kv = vdf::parse(text);
    let st = kv.get("AppState")?;
    let appid = st.str("appid")?.trim().to_string();
    let name = st.str("name").unwrap_or("").trim().to_string();
    let installdir = st.str("installdir").unwrap_or("").to_string();
    if appid.is_empty() || name.is_empty() || is_tool(&appid, &name) {
        return None;
    }
    let install_dir = steamapps.join("common").join(&installdir);
    let (cover, hero) = local_art(root, &appid);
    Some(Found {
        game: NewGame {
            title: name,
            source: Some(Source::Steam),
            source_id: appid.clone(),
            install_dir: install_dir.to_string_lossy().to_string(),
            launch_uri: format!("steam://rungameid/{appid}"),
            ..Default::default()
        },
        cover,
        hero,
    })
}

/// Steam keeps library artwork locally; two layouts exist (flat files, and per-app folders).
pub fn local_art(root: &Path, appid: &str) -> (Option<PathBuf>, Option<PathBuf>) {
    let cache = root.join("appcache").join("librarycache");
    let flat = |suffix: &str| Some(cache.join(format!("{appid}_{suffix}"))).filter(|p| p.is_file());
    let nested = |stem: &str| -> Option<PathBuf> {
        let dir = cache.join(appid);
        let direct = dir.join(format!("{stem}.jpg"));
        if direct.is_file() {
            return Some(direct);
        }
        // Newer clients put files in hashed sub-folders.
        walkdir::WalkDir::new(&dir).max_depth(2).into_iter().flatten().map(|e| e.into_path()).find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == stem) && p.is_file())
    };
    let cover = flat("library_600x900_2x.jpg").or_else(|| flat("library_600x900.jpg")).or_else(|| nested("library_600x900_2x")).or_else(|| nested("library_600x900"));
    let hero = flat("library_hero.jpg").or_else(|| nested("library_hero"));
    (cover, hero)
}

/// Developer, publisher and short description from the Steam store (optional, online).
pub fn store_details(appid: &str) -> Option<(String, String, String)> {
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english");
    let mut resp = ureq::get(&url).config().timeout_global(Some(std::time::Duration::from_secs(12))).build().call().ok()?;
    let v: serde_json::Value = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_json().ok()?;
    parse_details(&v, appid)
}

pub fn parse_details(v: &serde_json::Value, appid: &str) -> Option<(String, String, String)> {
    let d = v.get(appid)?.get("data")?;
    let join = |k: &str| d.get(k).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let desc = d.get("short_description").and_then(|x| x.as_str()).unwrap_or("").replace("&quot;", "\"").replace("&amp;", "&");
    Some((join("developers"), join("publishers"), desc))
}

pub fn cdn_cover_url(appid: &str) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900_2x.jpg")
}

pub fn cdn_hero_url(appid: &str) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_hero.jpg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_store_details() {
        let v: serde_json::Value = serde_json::from_str(r#"{"620":{"success":true,"data":{"developers":["Valve"],"publishers":["Valve"],"short_description":"Sequel &amp; more"}}}"#).unwrap();
        assert_eq!(parse_details(&v, "620"), Some(("Valve".into(), "Valve".into(), "Sequel & more".into())));
        assert_eq!(parse_details(&v, "1"), None);
    }

    #[test]
    fn reads_libraries_manifests_and_art() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Steam");
        let lib2 = d.path().join("Lib2");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::create_dir_all(lib2.join("steamapps")).unwrap();
        std::fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} }}", root.display().to_string().replace('\\', "\\\\"), lib2.display().to_string().replace('\\', "\\\\")),
        )
        .unwrap();
        std::fs::write(root.join("steamapps/appmanifest_400.acf"), "\"AppState\" { \"appid\" \"400\" \"name\" \"Portal\" \"installdir\" \"Portal\" \"StateFlags\" \"4\" }").unwrap();
        std::fs::write(lib2.join("steamapps/appmanifest_228980.acf"), "\"AppState\" { \"appid\" \"228980\" \"name\" \"Steamworks Common Redistributables\" \"installdir\" \"x\" }").unwrap();
        std::fs::write(lib2.join("steamapps/appmanifest_620.acf"), "\"AppState\" { \"appid\" \"620\" \"name\" \"Portal 2\" \"installdir\" \"Portal 2\" }").unwrap();
        std::fs::create_dir_all(root.join("appcache/librarycache/620/abc")).unwrap();
        std::fs::write(root.join("appcache/librarycache/400_library_600x900.jpg"), b"jpg").unwrap();
        std::fs::write(root.join("appcache/librarycache/620/abc/library_600x900.jpg"), b"jpg").unwrap();
        std::env::set_var("NOVA_STEAM_PATH", &root);
        let (_, found) = scan().unwrap();
        std::env::remove_var("NOVA_STEAM_PATH");
        let mut names: Vec<_> = found.iter().map(|f| f.game.title.clone()).collect();
        names.sort();
        assert_eq!(names, ["Portal", "Portal 2"]);
        let p2 = found.iter().find(|f| f.game.source_id == "620").unwrap();
        assert!(p2.cover.is_some());
        assert_eq!(p2.game.launch_uri, "steam://rungameid/620");
        assert!(p2.game.install_dir.ends_with("Portal 2"));
    }
}
