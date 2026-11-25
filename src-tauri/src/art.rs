//! Cover and hero artwork, cached as copies in Nova's own data folder so the library renders
//! offline and never depends on (or locks) files inside game folders.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 25 * 1024 * 1024;

pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.len() > 6 && bytes.starts_with(&[0, 0, 1, 0]) && bytes[4..6] != [0, 0] {
        Some("ico")
    } else {
        None
    }
}

/// A local image as a data: URL, for previewing a cover before it's imported.
pub fn data_url(path: &Path) -> Result<String> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_BYTES {
        bail!("that image is too large (over 25 MB)");
    }
    data_url_bytes(&std::fs::read(path)?)
}

pub fn data_url_bytes(bytes: &[u8]) -> Result<String> {
    let ext = sniff(bytes).context("that file isn't a PNG, JPEG, WebP or GIF image")?;
    let mime = match ext {
        "jpg" => "jpeg",
        "ico" => "x-icon",
        e => e,
    };
    Ok(format!("data:image/{mime};base64,{}", b64(bytes)))
}

fn b64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(T[(n >> 18) as usize & 63] as char);
        o.push(T[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        o.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    o
}

pub struct ArtStore {
    dir: PathBuf,
}

impl ArtStore {
    pub fn new(dir: PathBuf) -> Result<ArtStore> {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(ArtStore { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Store image bytes; returns the cache file name.
    pub fn put(&self, game_id: i64, kind: &str, bytes: &[u8]) -> Result<String> {
        if bytes.len() as u64 > MAX_BYTES {
            bail!("that image is too large (over 25 MB)");
        }
        let ext = sniff(bytes).context("that file isn't a PNG, JPEG, WebP or GIF image")?;
        let name = format!("{game_id}-{kind}-{}.{ext}", crate::models::now_ms());
        let tmp = self.dir.join(format!("{name}.part"));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, self.dir.join(&name))?;
        Ok(name)
    }

    pub fn import_file(&self, game_id: i64, kind: &str, src: &Path) -> Result<String> {
        let len = std::fs::metadata(src).with_context(|| format!("can't read {}", src.display()))?.len();
        if len > MAX_BYTES {
            bail!("that image is too large (over 25 MB)");
        }
        self.put(game_id, kind, &std::fs::read(src)?)
    }

    pub fn download(&self, game_id: i64, kind: &str, url: &str) -> Result<String> {
        let resp = ureq::get(url).config().timeout_global(Some(std::time::Duration::from_secs(15))).build().call()?;
        let bytes = resp.into_body().with_config().limit(MAX_BYTES).read_to_vec()?;
        self.put(game_id, kind, &bytes)
    }

    /// Extract an executable's own icon (read as data, never run) into the cache.
    pub fn put_exe_icon(&self, game_id: i64, exe: &Path) -> Option<String> {
        let bytes = match crate::pe::icon(exe)? {
            crate::pe::Icon::Png(b) | crate::pe::Icon::Ico(b) => b,
        };
        self.put(game_id, "icon", &bytes).ok()
    }

    /// Old cover files are deleted only from Nova's own cache folder.
    pub fn remove(&self, name: &str) {
        if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
            return;
        }
        let _ = std::fs::remove_file(self.dir.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(b64(b"Man"), "TWFu");
        assert_eq!(b64(b"Ma"), "TWE=");
        assert_eq!(b64(b"M"), "TQ==");
    }

    #[test]
    fn accepts_images_rejects_others() {
        let d = tempfile::tempdir().unwrap();
        let s = ArtStore::new(d.path().join("art")).unwrap();
        let name = s.put(1, "cover", &[0x89, b'P', b'N', b'G', 0, 0]).unwrap();
        assert!(name.ends_with(".png") && s.dir().join(&name).is_file());
        assert!(s.put(1, "cover", b"MZ\x90\0 definitely a program").is_err());
        s.remove("../../etc/passwd"); // ignored
        s.remove(&name);
        assert!(!s.dir().join(&name).exists());
    }
}
