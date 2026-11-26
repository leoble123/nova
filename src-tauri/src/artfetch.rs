//! Finding cover art online for games that have none locally.
//!
//! * Steam games: Steam's store service lists each app's library images (portrait cover,
//!   background), including the hashed file names newer games use.
//! * Epic games: the Epic launcher's own catalog cache on this PC has portrait covers
//!   ("DieselGameBoxTall") and wide art; read locally, no network needed to find them.
//! * Anything else (and Epic games missing from that cache): Steam store search by name. Only
//!   an exact name match is accepted, so a game never gets someone else's cover.
//!
//! No API keys. Requests only carry the game's name or Steam app id.

use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const IMAGE_BASE: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";

#[derive(Debug, Default, PartialEq)]
pub struct ArtUrls {
    pub steam_appid: Option<String>,
    pub cover: Vec<String>,
    pub hero: Vec<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(12))).user_agent("Nova").build().into()
}

fn get_json(url: &str) -> Option<Value> {
    agent().get(url).call().ok()?.body_mut().with_config().limit(4 * 1024 * 1024).read_json().ok()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// "Alan Wake 2™: Deluxe" and "alan wake 2 deluxe" compare equal.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Image URLs from a Steam store `GetItems` response.
pub fn parse_steam_assets(v: &Value) -> (Vec<String>, Vec<String>) {
    let Some(a) = v.pointer("/response/store_items/0/assets") else { return (vec![], vec![]) };
    let fmt = a.get("asset_url_format").and_then(Value::as_str).unwrap_or("");
    if fmt.is_empty() {
        return (vec![], vec![]);
    }
    let url = |k: &str| a.get(k).and_then(Value::as_str).map(|f| format!("{IMAGE_BASE}{}", fmt.replace("${FILENAME}", f)));
    let cover = ["library_capsule_2x", "library_capsule"].iter().filter_map(|k| url(k)).collect();
    let hero = ["library_hero", "library_hero_2x"].iter().filter_map(|k| url(k)).collect();
    (cover, hero)
}

pub fn steam_assets(appid: &str) -> (Vec<String>, Vec<String>) {
    let input = format!(r#"{{"ids":[{{"appid":{appid}}}],"context":{{"language":"english","country_code":"US"}},"data_request":{{"include_assets":true}}}}"#);
    let mut found = get_json(&format!("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={}", enc(&input))).map(|v| parse_steam_assets(&v)).unwrap_or_default();
    // Older apps: the classic CDN paths still work when the service has nothing.
    if found.0.is_empty() {
        found.0 = vec![format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900_2x.jpg"), format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg")];
    }
    if found.1.is_empty() {
        found.1 = vec![format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_hero.jpg")];
    }
    found
}

/// Steam app id for a game name, only on an exact (normalized) name match.
pub fn parse_search(v: &Value, title: &str) -> Option<String> {
    let want = norm(title);
    if want.len() < 2 {
        return None;
    }
    v.get("items")?.as_array()?.iter().find(|i| i.get("name").and_then(Value::as_str).is_some_and(|n| norm(n) == want)).and_then(|i| i.get("id")).map(|id| id.to_string().trim_matches('"').to_string())
}

pub fn steam_search(title: &str) -> Option<String> {
    let v = get_json(&format!("https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US", enc(title)))?;
    parse_search(&v, title)
}

// ------------------------------------------------------------------ Epic (local catalog cache)

fn epic_catalog_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NOVA_EPIC_CATALOG") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
    Some(base.join("Epic").join("EpicGamesLauncher").join("Data").join("Catalog").join("catcache.bin"))
}

fn b64decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Cover and wide art for one Epic catalog item, from the launcher's catcache.bin contents.
pub fn parse_epic_catalog(text: &str, namespace: &str, item: &str) -> ArtUrls {
    let json = if text.trim_start().starts_with('[') { Some(text.as_bytes().to_vec()) } else { b64decode(text) };
    let Some(items) = json.and_then(|j| serde_json::from_slice::<Value>(&j).ok()) else { return ArtUrls::default() };
    let Some(entry) = items.as_array().and_then(|a| a.iter().find(|e| e.get("id").and_then(Value::as_str) == Some(item) && (namespace.is_empty() || e.get("namespace").and_then(Value::as_str) == Some(namespace)))) else {
        return ArtUrls::default();
    };
    let imgs: Vec<(&str, &str)> = entry.get("keyImages").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| Some((i.get("type")?.as_str()?, i.get("url")?.as_str()?))).collect()).unwrap_or_default();
    let pick = |types: &[&str]| types.iter().filter_map(|t| imgs.iter().find(|(k, _)| k == t).map(|(_, u)| u.to_string())).collect::<Vec<_>>();
    ArtUrls { steam_appid: None, cover: pick(&["DieselGameBoxTall", "OfferImageTall"]), hero: pick(&["DieselGameBox", "OfferImageWide", "DieselStoreFrontWide"]) }
}

/// (namespace, catalog item) from an Epic launch URI.
pub fn epic_ids(launch_uri: &str) -> Option<(String, String)> {
    let rest = launch_uri.strip_prefix("com.epicgames.launcher://apps/")?;
    let rest = rest.split('?').next()?;
    let mut parts = rest.split("%3A");
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

pub fn epic_art(launch_uri: &str) -> ArtUrls {
    let Some((ns, item)) = epic_ids(launch_uri) else { return ArtUrls::default() };
    let Some(text) = epic_catalog_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return ArtUrls::default() };
    parse_epic_catalog(&text, &ns, &item)
}

/// Where to get art for a game. Network lookups happen here; downloads happen in the caller.
pub fn find(source: crate::models::Source, source_id: &str, launch_uri: &str, title: &str) -> ArtUrls {
    use crate::models::Source;
    let mut out = ArtUrls::default();
    if source == Source::Epic {
        out = epic_art(launch_uri);
    }
    let appid = if source == Source::Steam { Some(source_id.to_string()) } else { steam_search(title) };
    if let Some(id) = appid {
        let (c, h) = steam_assets(&id);
        out.cover.extend(c);
        out.hero.extend(h);
        out.steam_appid = Some(id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_assets_use_the_url_format() {
        let v: Value = serde_json::from_str(r#"{"response":{"store_items":[{"assets":{"asset_url_format":"steam/apps/42/${FILENAME}?t=1","library_capsule":"abc/library_600x900.jpg","library_capsule_2x":"abc/library_600x900_2x.jpg","library_hero":"library_hero.jpg"}}]}}"#).unwrap();
        let (c, h) = parse_steam_assets(&v);
        assert_eq!(c[0], "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/42/abc/library_600x900_2x.jpg?t=1");
        assert_eq!(h, ["https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/42/library_hero.jpg?t=1"]);
        assert_eq!(parse_steam_assets(&serde_json::json!({})), (vec![], vec![]));
    }

    #[test]
    fn search_only_accepts_exact_names() {
        let v: Value = serde_json::from_str(r#"{"items":[{"name":"Hollow Knight: Silksong","id":1030300},{"name":"Hollow Knight™","id":367520}]}"#).unwrap();
        assert_eq!(parse_search(&v, "Hollow Knight").as_deref(), Some("367520"));
        assert_eq!(parse_search(&v, "Hollow"), None);
        assert_eq!(parse_search(&v, "Tiny Quest"), None);
    }

    #[test]
    fn reads_epic_catalog_cache() {
        let json = r#"[{"id":"item1","namespace":"ns1","keyImages":[{"type":"Thumbnail","url":"t"},{"type":"DieselGameBoxTall","url":"https://cdn/tall.jpg"},{"type":"DieselGameBox","url":"https://cdn/wide.jpg"}]}]"#;
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let b = json.as_bytes();
            let mut o = String::new();
            for c in b.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                for i in 0..4 {
                    o.push(if i <= c.len() { T[(n >> (18 - 6 * i)) as usize & 63] as char } else { '=' });
                }
            }
            o
        };
        let a = parse_epic_catalog(&b64, "ns1", "item1");
        assert_eq!(a.cover, ["https://cdn/tall.jpg"]);
        assert_eq!(a.hero, ["https://cdn/wide.jpg"]);
        assert_eq!(parse_epic_catalog(json, "ns1", "nope"), ArtUrls::default());
        assert_eq!(epic_ids("com.epicgames.launcher://apps/ns1%3Aitem1%3AFalcon?action=launch").unwrap(), ("ns1".into(), "item1".into()));
    }

    /// Real network: run with NOVA_NET_TESTS=1.
    #[test]
    fn live_lookups() {
        if std::env::var_os("NOVA_NET_TESTS").is_none() {
            return;
        }
        assert_eq!(steam_search("Hollow Knight").as_deref(), Some("367520"));
        let (c, h) = steam_assets("1145360");
        assert!(c[0].contains("library_600x900") && !h.is_empty(), "{c:?} {h:?}");
    }
}
