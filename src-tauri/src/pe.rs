//! Reads facts out of Windows executables *as data*: architecture, GUI vs console, version-info
//! strings (product, company), whether the manifest demands administrator rights, whether it
//! carries a signature, and packer fingerprints. Nothing here ever runs the file.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeInfo {
    pub valid: bool,
    pub is_dll: bool,
    /// "x64", "x86", "arm64" or "".
    pub arch: String,
    pub gui: bool,
    pub console: bool,
    pub product_name: String,
    pub company_name: String,
    pub file_description: String,
    pub requires_admin: bool,
    pub has_signature: bool,
    /// UPX or similar packer section names.
    pub packed: bool,
}

const MAX_RSRC: u64 = 16 * 1024 * 1024;

pub fn inspect(path: &Path) -> PeInfo {
    inspect_inner(path).unwrap_or_default()
}

fn rd16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn rd32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn inspect_inner(path: &Path) -> Option<PeInfo> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = vec![0u8; 4096.min(len as usize)];
    f.read_exact(&mut head).ok()?;
    if head.get(0..2)? != b"MZ" {
        return None;
    }
    let pe = rd32(&head, 0x3c)? as usize;
    if pe > 2048 {
        // Header further out than usual: read enough to cover it.
        head = vec![0u8; (pe + 1024).min(len as usize)];
        f.seek(SeekFrom::Start(0)).ok()?;
        f.read_exact(&mut head).ok()?;
    }
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let machine = rd16(&head, coff)?;
    let sections = rd16(&head, coff + 2)? as usize;
    let opt_size = rd16(&head, coff + 16)? as usize;
    let characteristics = rd16(&head, coff + 18)?;
    let opt = coff + 20;
    let magic = rd16(&head, opt)?;
    let pe32plus = magic == 0x20b;
    let subsystem = rd16(&head, opt + 68)?;
    let dd = opt + if pe32plus { 112 } else { 96 };
    let n_dirs = rd32(&head, dd - 4).unwrap_or(0) as usize;
    let dir = |i: usize| -> (u32, u32) {
        if i >= n_dirs {
            return (0, 0);
        }
        (rd32(&head, dd + i * 8).unwrap_or(0), rd32(&head, dd + i * 8 + 4).unwrap_or(0))
    };
    let (rsrc_rva, _) = dir(2);
    let (_sec_off, sec_size) = dir(4); // security directory: file offset + size
    let mut info = PeInfo {
        valid: true,
        is_dll: characteristics & 0x2000 != 0,
        arch: match machine {
            0x8664 => "x64",
            0x14c => "x86",
            0xaa64 => "arm64",
            _ => "",
        }
        .into(),
        gui: subsystem == 2,
        console: subsystem == 3,
        has_signature: sec_size > 0,
        ..Default::default()
    };
    // Section table: find the section holding the resources, and look for packer names.
    let st = opt + opt_size;
    let mut rsrc: Option<(u64, u64)> = None;
    for i in 0..sections.min(96) {
        let s = st + i * 40;
        let Some(name) = head.get(s..s + 8) else { break };
        let name = String::from_utf8_lossy(name).trim_end_matches('\0').to_lowercase();
        if name.starts_with("upx") || name == ".aspack" || name == ".mpress1" || name == ".themida" {
            info.packed = true;
        }
        let vsize = rd32(&head, s + 8)?;
        let va = rd32(&head, s + 12)?;
        let raw_size = rd32(&head, s + 16)?;
        let raw_ptr = rd32(&head, s + 20)?;
        if rsrc_rva != 0 && rsrc_rva >= va && rsrc_rva < va + vsize.max(raw_size) {
            let start = raw_ptr as u64 + (rsrc_rva - va) as u64;
            let size = (raw_size as u64).saturating_sub((rsrc_rva - va) as u64).min(MAX_RSRC);
            rsrc = Some((start, size));
        }
    }
    if let Some((start, size)) = rsrc {
        if start < len && size > 0 {
            let size = size.min(len - start);
            let mut buf = vec![0u8; size as usize];
            if f.seek(SeekFrom::Start(start)).is_ok() && f.read_exact(&mut buf).is_ok() {
                info.product_name = version_string(&buf, "ProductName");
                info.company_name = version_string(&buf, "CompanyName");
                info.file_description = version_string(&buf, "FileDescription");
                info.requires_admin = find(&buf, b"requireAdministrator").is_some();
            }
        }
    }
    Some(info)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
}

/// A `String` entry of the VS_VERSIONINFO resource: {wLength, wValueLength, wType, szKey, pad, Value}.
fn version_string(buf: &[u8], key: &str) -> String {
    let k = utf16(key);
    let mut from = 0;
    while let Some(p) = find(&buf[from..], &k).map(|p| p + from) {
        from = p + 2;
        if p < 6 || p % 2 != 0 {
            continue;
        }
        let Some(value_words) = rd16(buf, p - 4) else { continue };
        let Some(kind) = rd16(buf, p - 2) else { continue };
        if kind != 1 || value_words == 0 || value_words > 512 {
            continue;
        }
        let mut v = p + k.len();
        v = (v + 3) & !3;
        let mut out = Vec::new();
        for i in 0..value_words as usize {
            match rd16(buf, v + i * 2) {
                Some(0) | None => break,
                Some(c) => out.push(c),
            }
        }
        let s = String::from_utf16_lossy(&out).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    String::new()
}

#[cfg(test)]
pub mod testpe {
    //! Builds tiny but structurally real PE files for tests.

    fn push16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn push32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    fn string_entry(key: &str, val: &str) -> Vec<u8> {
        let mut e = Vec::new();
        let kw: Vec<u16> = key.encode_utf16().chain([0]).collect();
        let vw: Vec<u16> = val.encode_utf16().chain([0]).collect();
        push16(&mut e, 0); // length (unused by the reader)
        push16(&mut e, vw.len() as u16);
        push16(&mut e, 1);
        for c in kw {
            push16(&mut e, c);
        }
        while e.len() % 4 != 0 {
            e.push(0);
        }
        for c in vw {
            push16(&mut e, c);
        }
        while e.len() % 4 != 0 {
            e.push(0);
        }
        e
    }

    /// gui: subsystem 2 vs 3. `extra_rsrc` is appended to the resource section (manifest text).
    pub fn build(gui: bool, product: &str, company: &str, extra_rsrc: &[u8], dll: bool) -> Vec<u8> {
        let mut rsrc = Vec::new();
        rsrc.extend(string_entry("CompanyName", company));
        rsrc.extend(string_entry("ProductName", product));
        rsrc.extend_from_slice(extra_rsrc);
        while rsrc.len() % 512 != 0 {
            rsrc.push(0);
        }
        let mut f = vec![0u8; 0x80];
        f[0] = b'M';
        f[1] = b'Z';
        f[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f.extend_from_slice(b"PE\0\0");
        push16(&mut f, 0x8664);
        push16(&mut f, 1); // sections
        push32(&mut f, 0);
        push32(&mut f, 0);
        push32(&mut f, 0);
        push16(&mut f, 240); // optional header size (PE32+)
        push16(&mut f, if dll { 0x2022 } else { 0x22 });
        let opt_start = f.len();
        push16(&mut f, 0x20b);
        f.resize(opt_start + 68, 0);
        push16(&mut f, if gui { 2 } else { 3 });
        f.resize(opt_start + 108, 0);
        push32(&mut f, 16); // NumberOfRvaAndSizes
        for i in 0..16 {
            if i == 2 {
                push32(&mut f, 0x1000);
                push32(&mut f, rsrc.len() as u32);
            } else {
                push32(&mut f, 0);
                push32(&mut f, 0);
            }
        }
        assert_eq!(f.len(), opt_start + 240);
        f.extend_from_slice(b".rsrc\0\0\0");
        push32(&mut f, rsrc.len() as u32);
        push32(&mut f, 0x1000);
        push32(&mut f, rsrc.len() as u32);
        push32(&mut f, 0x200);
        f.resize(f.len() + 16, 0);
        f.resize(0x200, 0);
        f.extend(rsrc);
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_synthetic_pe() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("game.exe");
        std::fs::write(&p, testpe::build(true, "Hollow Sky", "Moonlit Studio", b"<requestedExecutionLevel level=\"requireAdministrator\"/>", false)).unwrap();
        let i = inspect(&p);
        assert!(i.valid && i.gui && !i.console && !i.is_dll);
        assert_eq!(i.arch, "x64");
        assert_eq!(i.product_name, "Hollow Sky");
        assert_eq!(i.company_name, "Moonlit Studio");
        assert!(i.requires_admin);
        assert!(!i.has_signature);
    }

    #[test]
    fn non_pe_and_garbage_are_invalid_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [("a.exe", b"hello".to_vec()), ("b.exe", vec![]), ("c.exe", b"MZ\xff\xff\xff\xff".to_vec()), ("d.exe", { let mut v = vec![0u8; 64]; v[0] = b'M'; v[1] = b'Z'; v[0x3c] = 0xff; v[0x3d] = 0xff; v[0x3e] = 0xff; v })] {
            let p = dir.path().join(name);
            std::fs::write(&p, bytes).unwrap();
            assert!(!inspect(&p).valid, "{name}");
        }
        assert!(!inspect(&dir.path().join("missing.exe")).valid);
    }
}
