//! Inspects a folder the user points at and proposes which executable is the game.
//!
//! Strictly read-only: files are listed and their headers read (pe.rs). Nothing found here is
//! ever executed, extracted or modified. Unreadable sub-folders are counted and skipped.

use crate::pe::{self, PeInfo};
use crate::safety::{self, Warning};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_DEPTH: usize = 6;
const MAX_ENTRIES: usize = 60_000;
const DEEP_CHECKS: usize = 8;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Game,
    Launcher,
    Tool,
    Installer,
    Redist,
    Crash,
    AntiCheat,
    Script,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    /// Relative to the scanned folder, with `\` on Windows.
    pub rel_path: String,
    pub name: String,
    pub size: u64,
    pub depth: usize,
    pub role: Role,
    pub score: i32,
    pub pe: PeInfo,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub root: String,
    pub suggested_title: String,
    pub developer: String,
    pub publisher: String,
    /// "Unity", "Unreal Engine", "Godot", "Ren'Py", "RPG Maker", "GameMaker" or "".
    pub engine: String,
    pub candidates: Vec<Candidate>,
    pub files_seen: usize,
    pub total_size: u64,
    pub inaccessible: usize,
    pub truncated: bool,
    /// Games already in the library for this folder.
    pub already_in_library: Vec<(i64, String)>,
}

/// Folder names that never hold the game's own executable.
const SKIP_DIRS: &[&str] = &["$recycle.bin", "system volume information", ".git", "__macosx", "node_modules", "windows", "$windows.~bt"];
const REDIST_DIRS: &[&str] = &["_commonredist", "redist", "redistributables", "directx", "vcredist", "dotnet", "prerequisites", "_redist", "support", "installers", "__installer", "commonredist"];

fn role_of(name: &str, rel_dirs: &str, pe: &PeInfo, ext: &str) -> Role {
    let n = name.to_lowercase();
    if safety::SCRIPT_EXTS.contains(&ext) {
        return Role::Script;
    }
    let has = |ws: &[&str]| ws.iter().any(|w| n.contains(w));
    if REDIST_DIRS.iter().any(|d| rel_dirs.split(['\\', '/']).any(|p| p == *d))
        || has(&["vcredist", "vc_redist", "dxsetup", "dxwebsetup", "dotnet", "ndp4", "physx", "oalinst", "directx", "xnafx", "ue4prereq", "ueprereq", "uerequisites"])
    {
        return Role::Redist;
    }
    if n.starts_with("unins") || has(&["setup", "install", "uninst", "update", "patcher"]) {
        return Role::Installer;
    }
    if has(&["crash", "report", "bugsplat", "sentry", "errorhandler", "unitycrashhandler"]) {
        return Role::Crash;
    }
    if has(&["easyanticheat", "battleye", "_be.exe", "beservice", "eac_launcher", "start_protected_game", "vanguard", "nprotect", "xigncode"]) {
        return Role::AntiCheat;
    }
    if has(&["config", "settings", "setting", "editor", "server", "dedicated", "benchmark", "modmanager", "mod manager", "tool", "cefsharp", "helper", "overlay", "webhelper", "dxdiag"])
        || (pe.console && !pe.gui)
    {
        return Role::Tool;
    }
    if has(&["launcher"]) {
        return Role::Launcher;
    }
    Role::Game
}

/// Lowercase alphanumeric words, for comparing names like "Hollow_Sky-v1.2" and "HollowSky.exe".
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// "Hollow_Sky.v1.2.3-GOG [Repack]" -> "Hollow Sky"
pub fn clean_title(raw: &str) -> String {
    let mut s = raw.to_string();
    // Drop bracketed tags.
    for (o, c) in [('[', ']'), ('(', ')'), ('{', '}')] {
        while let (Some(a), Some(b)) = (s.find(o), s.find(c)) {
            if b > a {
                s.replace_range(a..=b, " ");
            } else {
                break;
            }
        }
    }
    let junk = ["gog", "steam", "repack", "portable", "win64", "win32", "x64", "x86", "windows", "pc", "multi", "dlc", "goty", "shipping", "release"];
    let is_version = |w: &str| {
        let lw = w.to_lowercase();
        let body = lw.strip_prefix('v').unwrap_or(&lw);
        !body.is_empty() && body.chars().next().is_some_and(|c| c.is_ascii_digit()) && body.chars().all(|c| c.is_ascii_digit() || c == '.')
    };
    let keep = |w: &str| !w.is_empty() && !junk.contains(&w.to_lowercase().as_str()) && !is_version(w);
    let mut words: Vec<String> = Vec::new();
    for tok in s.replace('_', " ").split_whitespace() {
        // "1.2.3-GOG", "Hollow.Sky", "Half-Life": versions and tags go, real names stay.
        let parts: Vec<String> = tok
            .split('-')
            .filter(|p| keep(p))
            .map(|p| if is_version(p) { String::new() } else { p.split('.').filter(|x| keep(x)).collect::<Vec<_>>().join(" ") })
            .filter(|p| !p.is_empty())
            .collect();
        if !parts.is_empty() {
            words.push(parts.join("-"));
        }
    }
    let out = words.join(" ").trim_matches(['-', ' ']).to_string();
    // Split "HollowSky" camel case only when there were no spaces at all.
    if !out.contains(' ') && out.chars().any(|c| c.is_lowercase()) {
        let mut spaced = String::new();
        let chars: Vec<char> = out.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
                spaced.push(' ');
            }
            spaced.push(c);
        }
        if !spaced.is_empty() {
            return spaced;
        }
    }
    if out.is_empty() {
        raw.trim().to_string()
    } else {
        out
    }
}

pub fn scan(root: &Path) -> anyhow::Result<ScanResult> {
    let meta = std::fs::metadata(root).map_err(|e| anyhow::anyhow!("can't open {}: {}", root.display(), friendly_io(&e)))?;
    if !meta.is_dir() {
        anyhow::bail!("{} isn't a folder", root.display());
    }
    let root = dunce(root);
    let folder_name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let folder_key = squash(&clean_title(&folder_name));

    let mut files_seen = 0usize;
    let mut total_size = 0u64;
    let mut inaccessible = 0usize;
    let mut truncated = false;
    let mut found: Vec<(PathBuf, u64, usize)> = Vec::new();
    let mut dirs: HashSet<String> = HashSet::new();
    let mut flags = EngineFlags::default();

    let walker = WalkDir::new(&root).follow_links(false).max_depth(MAX_DEPTH).into_iter().filter_entry(|e| {
        !(e.file_type().is_dir() && e.depth() > 0 && SKIP_DIRS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str()))
    });
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                inaccessible += 1;
                continue;
            }
        };
        files_seen += 1;
        if files_seen > MAX_ENTRIES {
            truncated = true;
            break;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if entry.file_type().is_dir() {
            if entry.depth() == 1 {
                dirs.insert(name.clone());
            }
            flags.dir(&name, entry.depth());
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        total_size += size;
        flags.file(&name, entry.depth());
        let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        if ext == "exe" || (["bat", "cmd"].contains(&ext.as_str()) && entry.depth() <= 2) {
            found.push((entry.path().to_path_buf(), size, entry.depth() - 1));
        }
    }

    let unity_data: HashSet<String> = dirs.iter().filter_map(|d| d.strip_suffix("_data").map(squash)).collect();
    let mut candidates: Vec<Candidate> = found
        .into_iter()
        .map(|(path, size, depth)| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let rel = path.strip_prefix(&root).unwrap_or(&path);
            let rel_path = rel.to_string_lossy().to_string();
            let rel_dirs = rel.parent().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
            let pe = if ext == "exe" { pe::inspect(&path) } else { PeInfo::default() };
            let role = role_of(&name, &rel_dirs, &pe, &ext);
            let warnings = safety::quick(&path, &pe, size);
            let stem_key = squash(path.file_stem().unwrap().to_string_lossy().as_ref());
            let mut score = match role {
                Role::Game => 40,
                Role::Launcher => 25,
                Role::Script => 5,
                Role::Tool => -10,
                Role::AntiCheat => -5,
                Role::Installer | Role::Crash => -40,
                Role::Redist => -60,
            };
            if pe.gui {
                score += 15;
            }
            if pe.arch == "x64" {
                score += 4;
            }
            if !folder_key.is_empty() && !stem_key.is_empty() {
                let shipping = stem_key.trim_end_matches("win64shipping").trim_end_matches("win32shipping");
                if stem_key == folder_key || shipping == folder_key {
                    score += 30;
                } else if folder_key.contains(shipping) || shipping.contains(&folder_key) {
                    score += 18;
                }
            }
            let product = squash(&pe.product_name);
            if !product.is_empty() && (product == folder_key || product.contains(&folder_key) && folder_key.len() > 3) {
                score += 10;
            }
            if depth == 0 && unity_data.contains(&stem_key) {
                score += 50; // Unity: Game.exe sits next to Game_Data
            }
            if stem_key.ends_with("win64shipping") || stem_key.ends_with("win32shipping") {
                score += 12; // Unreal's real binary; the root stub (if any) gets the depth bonus
            }
            score -= 6 * depth as i32;
            score += ((size.max(1) as f64).log2() as i32 - 16).clamp(0, 12);
            if let Some(worst) = safety::worst(&warnings) {
                score -= match worst {
                    safety::Level::Danger => 60,
                    safety::Level::Warn => 8,
                    safety::Level::Info => 0,
                };
            }
            Candidate { path: path.to_string_lossy().to_string(), rel_path, name, size, depth, role, score, pe, warnings }
        })
        .collect();
    // Unreal: when a root stub "Game.exe" exists next to "Game/Binaries/Win64/Game-Win64-Shipping.exe",
    // the stub is what the store launches; prefer it.
    let shipping_keys: Vec<String> = candidates
        .iter()
        .filter(|c| squash(&c.name).ends_with("win64shippingexe"))
        .map(|c| squash(&c.name).trim_end_matches("win64shippingexe").to_string())
        .collect();
    for c in candidates.iter_mut().filter(|c| c.depth == 0) {
        let key = squash(c.name.trim_end_matches(".exe").trim_end_matches(".EXE"));
        if shipping_keys.contains(&key) {
            c.score += 20;
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.depth.cmp(&b.depth)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    for c in candidates.iter_mut().take(DEEP_CHECKS) {
        let p = PathBuf::from(&c.path);
        c.warnings.extend(safety::deep(&p, &c.pe));
    }

    let best = candidates.first();
    let pe_title = best.map(|c| c.pe.product_name.clone()).unwrap_or_default();
    let mut suggested_title = clean_title(&folder_name);
    // A generic folder ("Game", "bin", "x64") says little; the exe's product name says more.
    if ["game", "bin", "x64", "win64", "binaries", "app", "files", "data"].contains(&suggested_title.to_lowercase().as_str()) && !pe_title.is_empty() {
        suggested_title = pe_title;
    }
    let company = best.map(|c| c.pe.company_name.clone()).unwrap_or_default();
    Ok(ScanResult {
        root: root.to_string_lossy().to_string(),
        suggested_title,
        developer: company.clone(),
        publisher: company,
        engine: flags.engine(&unity_data),
        candidates,
        files_seen,
        total_size,
        inaccessible,
        truncated,
        already_in_library: Vec::new(),
    })
}

#[derive(Default)]
struct EngineFlags {
    unity_player: bool,
    unreal: bool,
    godot_pck: bool,
    renpy: bool,
    rpgmaker: bool,
    gamemaker: bool,
}

impl EngineFlags {
    fn dir(&mut self, name: &str, depth: usize) {
        if depth <= 2 && name == "engine" {
            self.unreal = true;
        }
        if depth == 1 && name == "renpy" {
            self.renpy = true;
        }
        if depth == 1 && (name == "www" || name == "js") {
            self.rpgmaker |= name == "www";
        }
    }
    fn file(&mut self, name: &str, depth: usize) {
        if depth <= 2 {
            self.unity_player |= name == "unityplayer.dll";
            self.godot_pck |= name.ends_with(".pck");
            self.gamemaker |= name == "data.win";
            self.rpgmaker |= name == "rpg_rt.exe" || name == "rpg_core.js";
        }
    }
    fn engine(&self, unity_data: &HashSet<String>) -> String {
        if self.unity_player || !unity_data.is_empty() {
            "Unity"
        } else if self.unreal {
            "Unreal Engine"
        } else if self.renpy {
            "Ren'Py"
        } else if self.rpgmaker {
            "RPG Maker"
        } else if self.gamemaker {
            "GameMaker"
        } else if self.godot_pck {
            "Godot"
        } else {
            ""
        }
        .into()
    }
}

/// Inspect a single executable the user picked by hand (Browse...).
pub fn inspect_one(path: &Path) -> anyhow::Result<Candidate> {
    let meta = std::fs::metadata(path).map_err(|e| anyhow::anyhow!("can't open {}: {}", path.display(), friendly_io(&e)))?;
    if !meta.is_file() {
        anyhow::bail!("{} isn't a file", path.display());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let pe = if ext == "exe" { pe::inspect(path) } else { PeInfo::default() };
    let mut warnings = safety::quick(path, &pe, meta.len());
    warnings.extend(safety::deep(path, &pe));
    Ok(Candidate {
        path: path.to_string_lossy().to_string(),
        rel_path: name.clone(),
        role: role_of(&name, "", &pe, &ext),
        name,
        size: meta.len(),
        depth: 0,
        score: 0,
        pe,
        warnings,
    })
}

pub fn friendly_io(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it doesn't exist (moved, deleted, or on a drive that isn't connected)".into(),
        std::io::ErrorKind::PermissionDenied => "Windows denied access to it".into(),
        _ => e.to_string(),
    }
}

/// Strip Windows' `\\?\` prefix so paths stay readable and comparable.
pub fn dunce(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::testpe::build;

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn big_gui(product: &str) -> Vec<u8> {
        let mut b = build(true, product, "Moonlit Studio", &[], false);
        b.resize(3_000_000, 0);
        b
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("Hollow_Sky.v1.2.3-GOG [Repack]"), "Hollow Sky");
        assert_eq!(clean_title("HollowSky"), "Hollow Sky");
        assert_eq!(clean_title("Hades II (Early Access)"), "Hades II");
        assert_eq!(clean_title("DOOM"), "DOOM");
    }

    #[test]
    fn picks_unity_game_over_helpers_and_never_runs_anything() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Hollow Sky");
        put(&root, "HollowSky.exe", &big_gui("Hollow Sky"));
        put(&root, "HollowSky_Data/level0", b"x");
        put(&root, "UnityPlayer.dll", b"x");
        put(&root, "UnityCrashHandler64.exe", &build(true, "", "", &[], false));
        put(&root, "unins000.exe", &build(true, "", "", &[], false));
        put(&root, "_CommonRedist/vcredist/vc_redist.x64.exe", &build(true, "", "", &[], false));
        put(&root, "tools/ModTool.exe", &build(false, "", "", &[], false));
        let r = scan(&root).unwrap();
        assert_eq!(r.engine, "Unity");
        assert_eq!(r.candidates[0].name, "HollowSky.exe");
        assert_eq!(r.candidates[0].role, Role::Game);
        assert_eq!(r.suggested_title, "Hollow Sky");
        assert_eq!(r.developer, "Moonlit Studio");
        let role = |n: &str| r.candidates.iter().find(|c| c.name == n).unwrap().role.clone();
        assert_eq!(role("UnityCrashHandler64.exe"), Role::Crash);
        assert_eq!(role("unins000.exe"), Role::Installer);
        assert_eq!(role("vc_redist.x64.exe"), Role::Redist);
        assert_eq!(role("ModTool.exe"), Role::Tool);
        // Nothing was modified: the "executables" are still the same bytes.
        assert_eq!(std::fs::read(root.join("HollowSky.exe")).unwrap().len(), 3_000_000);
    }

    #[test]
    fn unreal_prefers_root_stub() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Starfall");
        put(&root, "Starfall.exe", &build(true, "Starfall", "", &[], false));
        put(&root, "Starfall/Binaries/Win64/Starfall-Win64-Shipping.exe", &big_gui("Starfall"));
        put(&root, "Engine/Binaries/ThirdParty/CrashReportClient.exe", &build(true, "", "", &[], false));
        let r = scan(&root).unwrap();
        assert_eq!(r.engine, "Unreal Engine");
        assert_eq!(r.candidates[0].name, "Starfall.exe", "{:#?}", r.candidates.iter().map(|c| (&c.name, c.score)).collect::<Vec<_>>());
        assert_eq!(r.candidates[1].name, "Starfall-Win64-Shipping.exe");
    }

    #[test]
    fn disguised_file_sinks_to_bottom() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Quiet Lake");
        put(&root, "QuietLake.exe", &big_gui("Quiet Lake"));
        put(&root, "QuietLake.txt.exe", b"junk");
        let r = scan(&root).unwrap();
        assert_eq!(r.candidates[0].name, "QuietLake.exe");
        assert!(r.candidates.last().unwrap().warnings.iter().any(|w| w.code == "double-ext"));
    }

    #[test]
    fn missing_and_file_paths_are_errors_not_panics() {
        let d = tempfile::tempdir().unwrap();
        assert!(scan(&d.path().join("nope")).is_err());
        put(d.path(), "f.exe", b"x");
        assert!(scan(&d.path().join("f.exe")).is_err());
        assert!(inspect_one(&d.path().join("f.exe")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_subfolders_are_counted_and_skipped() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc_geteuid() } == 0 {
            return; // root can read everything; nothing to test
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Locked");
        put(&root, "Game.exe", &big_gui("Locked"));
        put(&root, "secret/x.exe", b"x");
        std::fs::set_permissions(root.join("secret"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let r = scan(&root).unwrap();
        std::fs::set_permissions(root.join("secret"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(r.inaccessible, 1);
        assert_eq!(r.candidates.len(), 1);
    }

    #[cfg(unix)]
    extern "C" {
        #[link_name = "geteuid"]
        fn libc_geteuid() -> u32;
    }
}
