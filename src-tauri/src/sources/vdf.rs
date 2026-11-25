//! Minimal parser for Valve's text KeyValues format (libraryfolders.vdf, appmanifest_*.acf).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Str(String),
    Obj(BTreeMap<String, Kv>),
}

impl Kv {
    pub fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            // Keys are case-insensitive in practice ("AppState" vs "appstate").
            Kv::Obj(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::Str(_) => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Kv::Str(s) => Some(s),
            Kv::Obj(_) => None,
        }
    }

    pub fn entries(&self) -> Vec<(&String, &Kv)> {
        match self {
            Kv::Obj(m) => m.iter().collect(),
            Kv::Str(_) => Vec::new(),
        }
    }
}

fn tokens(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = src.chars().peekable();
    while let Some(&c) = it.peek() {
        match c {
            '"' => {
                it.next();
                let mut s = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '\\' => match it.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        '"' => break,
                        c => s.push(c),
                    }
                }
                out.push(Tok::S(s));
            }
            '{' => {
                it.next();
                out.push(Tok::Open);
            }
            '}' => {
                it.next();
                out.push(Tok::Close);
            }
            '/' => {
                // // comment to end of line
                while let Some(c) = it.next() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {
                it.next();
            }
            _ => {
                // Unquoted token.
                let mut s = String::new();
                while let Some(&c) = it.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    s.push(c);
                    it.next();
                }
                out.push(Tok::S(s));
            }
        }
    }
    out
}

#[derive(Debug)]
enum Tok {
    S(String),
    Open,
    Close,
}

pub fn parse(src: &str) -> Kv {
    let toks = tokens(src);
    let mut i = 0;
    Kv::Obj(obj(&toks, &mut i))
}

fn obj(t: &[Tok], i: &mut usize) -> BTreeMap<String, Kv> {
    let mut m = BTreeMap::new();
    while *i < t.len() {
        match &t[*i] {
            Tok::Close => {
                *i += 1;
                return m;
            }
            Tok::Open => {
                *i += 1; // stray brace; skip
            }
            Tok::S(k) => {
                *i += 1;
                match t.get(*i) {
                    Some(Tok::S(v)) => {
                        *i += 1;
                        m.insert(k.clone(), Kv::Str(v.clone()));
                    }
                    Some(Tok::Open) => {
                        *i += 1;
                        let inner = obj(t, i);
                        m.insert(k.clone(), Kv::Obj(inner));
                    }
                    _ => {}
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_and_escaped() {
        let kv = parse(
            r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "400" "123" }
	}
	// comment
	"1" { "path" "D:\\SteamLibrary" }
}"#,
        );
        let lf = kv.get("LibraryFolders").unwrap();
        assert_eq!(lf.get("0").unwrap().str("path"), Some("C:\\Program Files (x86)\\Steam"));
        assert_eq!(lf.get("1").unwrap().str("path"), Some("D:\\SteamLibrary"));
        assert_eq!(lf.get("0").unwrap().get("apps").unwrap().str("400"), Some("123"));
    }

    #[test]
    fn survives_garbage() {
        let _ = parse("}}}{{\"a\"");
        let _ = parse("");
        let _ = parse("\"unterminated");
    }
}
