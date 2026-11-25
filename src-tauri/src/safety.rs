//! Warnings about executables before Nova registers or launches them. Imported folders are
//! treated as untrusted: these checks only *read* the file (headers, name, location, the
//! "downloaded from the internet" mark, the Authenticode signature). Nothing is executed, and
//! Windows' own protections (Defender, SmartScreen) stay fully in charge when a game starts.

use crate::pe::PeInfo;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub level: Level,
    pub code: &'static str,
    pub text: String,
}

fn w(level: Level, code: &'static str, text: impl Into<String>) -> Warning {
    Warning { level, code, text: text.into() }
}

pub const SCRIPT_EXTS: &[&str] = &["bat", "cmd", "sh", "ps1", "vbs", "vbe", "js", "jse", "wsf", "hta", "scr", "com", "pif"];

const BAD_WORDS: &[&str] = &["keygen", "crack", "activator", "kmspico", "kms_", "loader_", "injector", "trainer", "cheat", "hack", "miner"];

/// Cheap checks for every candidate in a scan.
pub fn quick(path: &Path, pe: &PeInfo, size: u64) -> Vec<Warning> {
    let mut out = Vec::new();
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let lower = name.to_lowercase();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();

    // Right-to-left override and friends: used to make "gnp.exe" display as "exe.png".
    if name.chars().any(|c| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')) {
        out.push(w(Level::Danger, "bidi-name", "The file name contains hidden direction-changing characters, a trick used to disguise programs as documents."));
    }
    // "readme.txt.exe", "cover.jpg.exe"
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    if let Some(inner) = Path::new(&stem).extension().map(|e| e.to_string_lossy().to_string()) {
        if ["txt", "pdf", "doc", "docx", "jpg", "jpeg", "png", "gif", "mp4", "mp3", "zip", "rar", "7z"].contains(&inner.as_str()) {
            out.push(w(Level::Danger, "double-ext", format!("Disguised extension: \"{name}\" looks like a .{inner} file but is a program.")));
        }
    }
    if SCRIPT_EXTS.contains(&ext.as_str()) {
        out.push(w(Level::Warn, "script", format!("This is a .{ext} script, not a game program. Scripts can run any command, so open it in a text editor and check it first.")));
    } else if ext == "exe" && !pe.valid {
        out.push(w(Level::Danger, "not-pe", "Named .exe but it isn't a valid Windows program. It may be damaged, or disguised."));
    }
    if pe.valid && pe.is_dll {
        out.push(w(Level::Warn, "dll", "This is a library (DLL) renamed as a program; it can't be launched on its own."));
    }
    if let Some(word) = BAD_WORDS.iter().find(|b| lower.contains(*b)) {
        out.push(w(Level::Warn, "name", format!("The name contains \"{}\". Tools like this are a common way malware spreads.", word.trim_matches('_'))));
    }
    if pe.requires_admin {
        out.push(w(Level::Warn, "admin", "Asks for administrator rights. Nova won't grant them unless you choose to, each time."));
    }
    if pe.packed {
        out.push(w(Level::Info, "packed", "Compressed with an executable packer. That's common for small tools but also used to hide malware from scanners."));
    }
    if pe.valid && ext == "exe" && size < 48 * 1024 && !lower.contains("launcher") {
        out.push(w(Level::Info, "tiny", "Very small for a game program; it's probably a launcher or helper."));
    }
    let full = path.to_string_lossy().to_lowercase().replace('/', "\\");
    if full.contains("\\windows\\system32") || full.contains("\\windows\\syswow64") || full.starts_with("c:\\windows\\") {
        out.push(w(Level::Danger, "system", "This is part of Windows itself, not a game."));
    } else if full.contains("\\appdata\\local\\temp\\") || full.contains("\\temp\\") {
        out.push(w(Level::Warn, "temp", "Located in a temporary folder. Games normally aren't run from there."));
    }
    if is_hidden(path) {
        out.push(w(Level::Warn, "hidden", "The file is marked hidden."));
    }
    out
}

/// Slower checks for the executables you're actually choosing (or launching).
pub fn deep(path: &Path, pe: &PeInfo) -> Vec<Warning> {
    let mut out = Vec::new();
    match zone_id(path) {
        Some(z) if z >= 3 => out.push(w(Level::Info, "motw", "Downloaded from the internet (Windows marked it). Windows SmartScreen may check it when it first runs.")),
        _ => {}
    }
    if pe.valid {
        match signature(path) {
            Sig::Valid => {}
            Sig::Invalid => out.push(w(Level::Danger, "bad-signature", "It carries a digital signature that doesn't check out: the file was changed after it was signed.")),
            Sig::Unsigned => out.push(w(Level::Info, "unsigned", "Not digitally signed. Common for indie and older games; it just means the publisher can't be verified.")),
            Sig::Unknown => {}
        }
    }
    out
}

pub fn worst(ws: &[Warning]) -> Option<Level> {
    ws.iter().map(|w| w.level).max()
}

/// The `Zone.Identifier` alternate data stream Windows attaches to downloads (3 = internet).
#[cfg(windows)]
fn zone_id(path: &Path) -> Option<u32> {
    let mut ads = path.as_os_str().to_owned();
    ads.push(":Zone.Identifier");
    let text = std::fs::read_to_string(ads).ok()?;
    text.lines().find_map(|l| l.trim().strip_prefix("ZoneId=")).and_then(|v| v.trim().parse().ok())
}

#[cfg(not(windows))]
fn zone_id(_path: &Path) -> Option<u32> {
    None
}

#[cfg(windows)]
fn is_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::metadata(path).map(|m| m.file_attributes() & 0x2 != 0).unwrap_or(false)
}

#[cfg(not(windows))]
fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

#[derive(Debug, PartialEq)]
pub enum Sig {
    Valid,
    Invalid,
    Unsigned,
    Unknown,
}

/// Authenticode check via WinVerifyTrust, offline only (no revocation lookups, cached URLs).
#[cfg(windows)]
pub fn signature(path: &Path) -> Sig {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::WinTrust::*;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let mut file = WINTRUST_FILE_INFO {
            cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: wide.as_ptr(),
            hFile: std::ptr::null_mut(),
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut data: WINTRUST_DATA = std::mem::zeroed();
        data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        data.Anonymous.pFile = &mut file;
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        data.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE;
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let r = WinVerifyTrust(std::ptr::null_mut(), &mut action, &mut data as *mut _ as *mut _);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(std::ptr::null_mut(), &mut action, &mut data as *mut _ as *mut _);
        const TRUST_E_NOSIGNATURE: i32 = 0x800B0100u32 as i32;
        const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B0003u32 as i32;
        const TRUST_E_BAD_DIGEST: i32 = 0x80096010u32 as i32;
        const CERT_E_CHAINING: i32 = 0x800B010Au32 as i32;
        const CERT_E_UNTRUSTEDROOT: i32 = 0x800B0109u32 as i32;
        match r {
            0 => Sig::Valid,
            TRUST_E_NOSIGNATURE | TRUST_E_SUBJECT_FORM_UNKNOWN => Sig::Unsigned,
            TRUST_E_BAD_DIGEST => Sig::Invalid,
            // Offline we may not be able to build the chain; that's not evidence of tampering.
            CERT_E_CHAINING | CERT_E_UNTRUSTEDROOT => Sig::Unknown,
            _ => Sig::Unknown,
        }
    }
}

#[cfg(not(windows))]
pub fn signature(_path: &Path) -> Sig {
    Sig::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::{inspect, testpe};

    fn codes(ws: &[Warning]) -> Vec<&'static str> {
        ws.iter().map(|w| w.code).collect()
    }

    #[test]
    fn flags_disguises_scripts_and_fakes() {
        let d = tempfile::tempdir().unwrap();
        let fake = d.path().join("cover.jpg.exe");
        std::fs::write(&fake, b"not a program").unwrap();
        let c = codes(&quick(&fake, &inspect(&fake), 13));
        assert!(c.contains(&"double-ext") && c.contains(&"not-pe"), "{c:?}");

        let bat = d.path().join("start.bat");
        std::fs::write(&bat, b"@echo off").unwrap();
        assert!(codes(&quick(&bat, &inspect(&bat), 9)).contains(&"script"));

        let rtlo = d.path().join("game\u{202e}gnp.exe");
        std::fs::write(&rtlo, testpe::build(true, "", "", &[], false)).unwrap();
        assert!(codes(&quick(&rtlo, &inspect(&rtlo), 100_000)).contains(&"bidi-name"));

        let kg = d.path().join("Keygen.exe");
        std::fs::write(&kg, testpe::build(true, "", "", &[], false)).unwrap();
        assert!(codes(&quick(&kg, &inspect(&kg), 100_000)).contains(&"name"));
    }

    #[test]
    fn clean_game_has_no_serious_warnings() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("HollowSky.exe");
        std::fs::write(&p, testpe::build(true, "Hollow Sky", "Moonlit", &[], false)).unwrap();
        let ws = quick(&p, &inspect(&p), 30_000_000);
        assert!(worst(&ws).is_none_or(|l| l == Level::Info), "{ws:?}");
    }
}
