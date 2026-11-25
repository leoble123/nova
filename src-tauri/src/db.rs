//! The local library database (SQLite, one file in the app's data folder).
//!
//! Removing a game here only deletes rows. Nothing in this module (or anywhere in Nova) deletes
//! files from a game's folder.

use crate::models::*;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA_VERSION: i64 = 2;

const MIGRATION_1: &str = r#"
CREATE TABLE games (
    id               INTEGER PRIMARY KEY,
    title            TEXT NOT NULL,
    source           TEXT NOT NULL DEFAULT 'manual',
    source_id        TEXT NOT NULL DEFAULT '',
    developer        TEXT NOT NULL DEFAULT '',
    publisher        TEXT NOT NULL DEFAULT '',
    description      TEXT NOT NULL DEFAULT '',
    install_dir      TEXT NOT NULL DEFAULT '',
    working_dir      TEXT NOT NULL DEFAULT '',
    launch_args      TEXT NOT NULL DEFAULT '',
    launch_uri       TEXT NOT NULL DEFAULT '',
    preferred_exe_id INTEGER,
    cover            TEXT NOT NULL DEFAULT '',
    hero             TEXT NOT NULL DEFAULT '',
    favorite         INTEGER NOT NULL DEFAULT 0,
    hidden           INTEGER NOT NULL DEFAULT 0,
    added_at         INTEGER NOT NULL,
    last_played      INTEGER,
    playtime_secs    INTEGER NOT NULL DEFAULT 0,
    notes            TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX games_store ON games(source, source_id) WHERE source != 'manual';
CREATE TABLE executables (
    id          INTEGER PRIMARY KEY,
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    label       TEXT NOT NULL DEFAULT '',
    args        TEXT NOT NULL DEFAULT '',
    working_dir TEXT NOT NULL DEFAULT ''
);
CREATE INDEX executables_game ON executables(game_id);
CREATE TABLE collections (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE collection_games (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    PRIMARY KEY (collection_id, game_id)
);
CREATE TABLE sessions (
    id         INTEGER PRIMARY KEY,
    game_id    INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER NOT NULL,
    secs       INTEGER NOT NULL,
    avg_fps    REAL,
    low1       REAL
);
CREATE INDEX sessions_game ON sessions(game_id, started_at);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const GAME_COLS: &str = "id, title, source, source_id, developer, publisher, description, install_dir, working_dir, \
     launch_args, launch_uri, preferred_exe_id, cover, hero, favorite, hidden, added_at, last_played, playtime_secs, notes, icon";

fn game_from_row(r: &Row) -> rusqlite::Result<Game> {
    Ok(Game {
        id: r.get(0)?,
        title: r.get(1)?,
        source: Source::parse(&r.get::<_, String>(2)?),
        source_id: r.get(3)?,
        developer: r.get(4)?,
        publisher: r.get(5)?,
        description: r.get(6)?,
        install_dir: r.get(7)?,
        working_dir: r.get(8)?,
        launch_args: r.get(9)?,
        launch_uri: r.get(10)?,
        preferred_exe_id: r.get(11)?,
        cover: r.get(12)?,
        hero: r.get(13)?,
        favorite: r.get::<_, i64>(14)? != 0,
        hidden: r.get::<_, i64>(15)? != 0,
        added_at: r.get(16)?,
        last_played: r.get(17)?,
        playtime_secs: r.get(18)?,
        notes: r.get(19)?,
        icon: r.get(20)?,
        executables: Vec::new(),
        collections: Vec::new(),
        installed: false,
        problem: String::new(),
        running: false,
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        // WAL: a crash mid-write can't corrupt the library, and reads never block on writes.
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            bail!("the library was created by a newer version of Nova (schema {version})");
        }
        if version < 1 {
            conn.execute_batch(&format!("BEGIN; {MIGRATION_1} PRAGMA user_version = 1; COMMIT;"))?;
        }
        if version < 2 {
            // Game icons: a file in the art cache, "" = not looked for yet, "-" = none found.
            conn.execute_batch("BEGIN; ALTER TABLE games ADD COLUMN icon TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 2; COMMIT;")?;
        }
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves a usable connection; keep going.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ------------------------------------------------------------------ games

    pub fn list_games(&self) -> Result<Vec<Game>> {
        let c = self.c();
        let mut games: Vec<Game> = c
            .prepare(&format!("SELECT {GAME_COLS} FROM games ORDER BY title COLLATE NOCASE"))?
            .query_map([], game_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        let mut exes = c.prepare("SELECT id, game_id, path, label, args, working_dir FROM executables ORDER BY id")?;
        let exes: Vec<Executable> = exes.query_map([], exe_from_row)?.collect::<rusqlite::Result<_>>()?;
        let mut cols = c.prepare("SELECT collection_id, game_id FROM collection_games")?;
        let cols: Vec<(i64, i64)> = cols.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let index: std::collections::HashMap<i64, usize> = games.iter().enumerate().map(|(i, g)| (g.id, i)).collect();
        for e in exes {
            if let Some(&i) = index.get(&e.game_id) {
                games[i].executables.push(e);
            }
        }
        for (cid, gid) in cols {
            if let Some(&i) = index.get(&gid) {
                games[i].collections.push(cid);
            }
        }
        Ok(games)
    }

    pub fn get_game(&self, id: i64) -> Result<Game> {
        let c = self.c();
        let mut g = c
            .query_row(&format!("SELECT {GAME_COLS} FROM games WHERE id = ?1"), [id], game_from_row)
            .optional()?
            .context("that game isn't in the library any more")?;
        g.executables = c
            .prepare("SELECT id, game_id, path, label, args, working_dir FROM executables WHERE game_id = ?1 ORDER BY id")?
            .query_map([id], exe_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        g.collections = c
            .prepare("SELECT collection_id FROM collection_games WHERE game_id = ?1")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(g)
    }

    pub fn find_store_game(&self, source: Source, source_id: &str) -> Result<Option<i64>> {
        Ok(self
            .c()
            .query_row("SELECT id FROM games WHERE source = ?1 AND source_id = ?2", params![source.as_str(), source_id], |r| r.get(0))
            .optional()?)
    }

    /// Games already registered for a folder (to warn about importing the same game twice).
    pub fn games_in_dir(&self, dir: &str) -> Result<Vec<(i64, String)>> {
        let c = self.c();
        let mut st = c.prepare("SELECT id, title FROM games WHERE install_dir = ?1 COLLATE NOCASE")?;
        let r = st.query_map([dir], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(r)
    }

    pub fn add_game(&self, g: &NewGame, cover: &str) -> Result<i64> {
        let title = g.title.trim();
        if title.is_empty() {
            bail!("the game needs a name");
        }
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO games (title, source, source_id, developer, publisher, description, install_dir, working_dir, launch_args, launch_uri, cover, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                title,
                g.source.unwrap_or(Source::Manual).as_str(),
                g.source_id,
                g.developer.trim(),
                g.publisher.trim(),
                g.description,
                g.install_dir,
                g.working_dir,
                g.launch_args,
                g.launch_uri,
                cover,
                now_ms()
            ],
        )?;
        let id = tx.last_insert_rowid();
        let mut preferred = None;
        for (i, e) in g.executables.iter().enumerate() {
            tx.execute(
                "INSERT INTO executables (game_id, path, label, args, working_dir) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, e.path, e.label, e.args, e.working_dir],
            )?;
            if i == g.preferred || preferred.is_none() {
                preferred = Some(tx.last_insert_rowid());
            }
        }
        tx.execute("UPDATE games SET preferred_exe_id = ?1 WHERE id = ?2", params![preferred, id])?;
        tx.commit()?;
        Ok(id)
    }

    /// Store scans: refresh what the store knows (path, name) but keep the user's own edits
    /// (cover, notes, playtime, collections).
    pub fn upsert_store_game(&self, g: &NewGame) -> Result<(i64, bool)> {
        let source = g.source.unwrap_or(Source::Manual);
        if let Some(id) = self.find_store_game(source, &g.source_id)? {
            self.c().execute(
                "UPDATE games SET install_dir = ?1, launch_uri = ?2,
                    developer = CASE WHEN developer = '' THEN ?3 ELSE developer END,
                    publisher = CASE WHEN publisher = '' THEN ?4 ELSE publisher END
                 WHERE id = ?5",
                params![g.install_dir, g.launch_uri, g.developer, g.publisher, id],
            )?;
            return Ok((id, false));
        }
        Ok((self.add_game(g, "")?, true))
    }

    pub fn update_game(&self, id: i64, u: &GameUpdate) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let set = |col: &str, v: &dyn rusqlite::ToSql| -> Result<()> {
            tx.execute(&format!("UPDATE games SET {col} = ?1 WHERE id = ?2"), params![v, id])?;
            Ok(())
        };
        if let Some(t) = &u.title {
            if t.trim().is_empty() {
                bail!("the game needs a name");
            }
            set("title", &t.trim())?;
        }
        if let Some(v) = &u.developer {
            set("developer", &v.trim())?;
        }
        if let Some(v) = &u.publisher {
            set("publisher", &v.trim())?;
        }
        if let Some(v) = &u.description {
            set("description", v)?;
        }
        if let Some(v) = &u.install_dir {
            set("install_dir", v)?;
        }
        if let Some(v) = &u.working_dir {
            set("working_dir", v)?;
        }
        if let Some(v) = &u.launch_args {
            set("launch_args", v)?;
        }
        if let Some(v) = &u.notes {
            set("notes", v)?;
        }
        if let Some(v) = u.favorite {
            set("favorite", &(v as i64))?;
        }
        if let Some(v) = u.hidden {
            set("hidden", &(v as i64))?;
        }
        if let Some(e) = u.preferred_exe_id {
            let ok: i64 = tx.query_row("SELECT COUNT(*) FROM executables WHERE id = ?1 AND game_id = ?2", params![e, id], |r| r.get(0))?;
            if ok == 0 {
                bail!("that executable doesn't belong to this game");
            }
            set("preferred_exe_id", &e)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_art(&self, id: i64, kind: &str, file: &str) -> Result<()> {
        let col = match kind {
            "hero" => "hero",
            "icon" => "icon",
            _ => "cover",
        };
        self.c().execute(&format!("UPDATE games SET {col} = ?1 WHERE id = ?2"), params![file, id])?;
        Ok(())
    }

    /// Deletes the library entry only. The game's files are never touched.
    pub fn remove_game(&self, id: i64) -> Result<()> {
        self.c().execute("DELETE FROM games WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The game folder moved: point the game, its executables and working folders at the new
    /// place, keeping each file's position relative to the folder. Returns how many executables
    /// were found at their new location.
    pub fn relocate(&self, id: i64, new_dir: &Path) -> Result<usize> {
        let g = self.get_game(id)?;
        let old = PathBuf::from(&g.install_dir);
        let map = |p: &str| -> Option<String> {
            let rel = Path::new(p).strip_prefix(&old).ok()?;
            Some(new_dir.join(rel).to_string_lossy().to_string())
        };
        let mut c = self.c();
        let tx = c.transaction()?;
        let mut found = 0;
        for e in &g.executables {
            if let Some(np) = map(&e.path) {
                if Path::new(&np).is_file() {
                    found += 1;
                }
                let wd = if e.working_dir.is_empty() { String::new() } else { map(&e.working_dir).unwrap_or_else(|| e.working_dir.clone()) };
                tx.execute("UPDATE executables SET path = ?1, working_dir = ?2 WHERE id = ?3", params![np, wd, e.id])?;
            }
        }
        let wd = if g.working_dir.is_empty() { String::new() } else { map(&g.working_dir).unwrap_or(g.working_dir.clone()) };
        tx.execute("UPDATE games SET install_dir = ?1, working_dir = ?2 WHERE id = ?3", params![new_dir.to_string_lossy(), wd, id])?;
        tx.commit()?;
        Ok(found)
    }

    // ------------------------------------------------------------------ executables

    pub fn add_executable(&self, game_id: i64, e: &NewExecutable) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO executables (game_id, path, label, args, working_dir) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![game_id, e.path, e.label, e.args, e.working_dir],
        )?;
        let id = c.last_insert_rowid();
        c.execute("UPDATE games SET preferred_exe_id = ?1 WHERE id = ?2 AND preferred_exe_id IS NULL", params![id, game_id])?;
        Ok(id)
    }

    pub fn update_executable(&self, e: &Executable) -> Result<()> {
        let n = self.c().execute(
            "UPDATE executables SET path = ?1, label = ?2, args = ?3, working_dir = ?4 WHERE id = ?5 AND game_id = ?6",
            params![e.path, e.label, e.args, e.working_dir, e.id, e.game_id],
        )?;
        if n == 0 {
            bail!("that executable isn't registered any more");
        }
        Ok(())
    }

    pub fn remove_executable(&self, game_id: i64, exe_id: i64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM executables WHERE id = ?1 AND game_id = ?2", params![exe_id, game_id])?;
        // Keep a preferred executable if any remain.
        tx.execute(
            "UPDATE games SET preferred_exe_id = (SELECT MIN(id) FROM executables WHERE game_id = ?1)
             WHERE id = ?1 AND (preferred_exe_id IS NULL OR preferred_exe_id = ?2)",
            params![game_id, exe_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------------ collections

    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT c.id, c.name, COUNT(cg.game_id) FROM collections c LEFT JOIN collection_games cg ON cg.collection_id = c.id
             GROUP BY c.id ORDER BY c.sort, c.name COLLATE NOCASE",
        )?;
        let r = st.query_map([], |r| Ok(Collection { id: r.get(0)?, name: r.get(1)?, game_count: r.get(2)? }))?.collect::<rusqlite::Result<_>>()?;
        Ok(r)
    }

    pub fn create_collection(&self, name: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            bail!("the collection needs a name");
        }
        let c = self.c();
        c.execute("INSERT INTO collections (name, sort) VALUES (?1, (SELECT COALESCE(MAX(sort), 0) + 1 FROM collections))", [name])
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => anyhow::anyhow!("there's already a collection called \"{name}\""),
                e => e.into(),
            })?;
        Ok(c.last_insert_rowid())
    }

    pub fn rename_collection(&self, id: i64, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            bail!("the collection needs a name");
        }
        self.c().execute("UPDATE collections SET name = ?1 WHERE id = ?2", params![name.trim(), id])?;
        Ok(())
    }

    pub fn delete_collection(&self, id: i64) -> Result<()> {
        self.c().execute("DELETE FROM collections WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_in_collection(&self, collection_id: i64, game_id: i64, member: bool) -> Result<()> {
        let c = self.c();
        if member {
            c.execute("INSERT OR IGNORE INTO collection_games (collection_id, game_id) VALUES (?1, ?2)", params![collection_id, game_id])?;
        } else {
            c.execute("DELETE FROM collection_games WHERE collection_id = ?1 AND game_id = ?2", params![collection_id, game_id])?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ sessions / playtime

    pub fn record_session(&self, game_id: i64, started_at: i64, ended_at: i64, avg_fps: Option<f64>, low1: Option<f64>) -> Result<i64> {
        let secs = ((ended_at - started_at) / 1000).max(0);
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO sessions (game_id, started_at, ended_at, secs, avg_fps, low1) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![game_id, started_at, ended_at, secs, avg_fps, low1],
        )?;
        tx.execute("UPDATE games SET playtime_secs = playtime_secs + ?1, last_played = ?2 WHERE id = ?3", params![secs, ended_at, game_id])?;
        tx.commit()?;
        Ok(secs)
    }

    pub fn mark_played(&self, game_id: i64, at: i64) -> Result<()> {
        self.c().execute("UPDATE games SET last_played = ?1 WHERE id = ?2", params![at, game_id])?;
        Ok(())
    }

    pub fn sessions(&self, game_id: i64, limit: i64) -> Result<Vec<Session>> {
        let c = self.c();
        let mut st = c.prepare("SELECT id, game_id, started_at, ended_at, secs, avg_fps, low1 FROM sessions WHERE game_id = ?1 ORDER BY started_at DESC LIMIT ?2")?;
        let r = st
            .query_map(params![game_id, limit], |r| {
                Ok(Session { id: r.get(0)?, game_id: r.get(1)?, started_at: r.get(2)?, ended_at: r.get(3)?, secs: r.get(4)?, avg_fps: r.get(5)?, low1: r.get(6)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(r)
    }

    // ------------------------------------------------------------------ settings

    pub fn setting(&self, key: &str) -> Option<String> {
        self.c().query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.c().execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }
}

fn exe_from_row(r: &Row) -> rusqlite::Result<Executable> {
    Ok(Executable { id: r.get(0)?, game_id: r.get(1)?, path: r.get(2)?, label: r.get(3)?, args: r.get(4)?, working_dir: r.get(5)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manual(title: &str, exes: &[&str]) -> NewGame {
        NewGame {
            title: title.into(),
            install_dir: "C:\\Games\\X".into(),
            executables: exes.iter().map(|p| NewExecutable { path: p.to_string(), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn add_list_and_remove_keeps_nothing_dangling() {
        let db = Db::open_in_memory().unwrap();
        let id = db.add_game(&manual("Alpha", &["C:\\Games\\X\\a.exe", "C:\\Games\\X\\b.exe"]), "").unwrap();
        let g = db.get_game(id).unwrap();
        assert_eq!(g.executables.len(), 2);
        assert_eq!(g.preferred_exe_id, Some(g.executables[0].id));
        let col = db.create_collection("Favourites").unwrap();
        db.set_in_collection(col, id, true).unwrap();
        db.record_session(id, 1_000, 61_000, Some(120.0), None).unwrap();
        assert_eq!(db.get_game(id).unwrap().playtime_secs, 60);
        db.remove_game(id).unwrap();
        assert!(db.list_games().unwrap().is_empty());
        assert_eq!(db.list_collections().unwrap()[0].game_count, 0);
    }

    #[test]
    fn preferred_executable_follows_removal_and_validates() {
        let db = Db::open_in_memory().unwrap();
        let other = db.add_game(&manual("Other", &["o.exe"]), "").unwrap();
        let id = db.add_game(&NewGame { preferred: 1, ..manual("Beta", &["a.exe", "b.exe"]) }, "").unwrap();
        let g = db.get_game(id).unwrap();
        assert_eq!(g.preferred_exe_id, Some(g.executables[1].id));
        // Someone else's executable can't become this game's preferred one.
        let foreign = db.get_game(other).unwrap().executables[0].id;
        assert!(db.update_game(id, &GameUpdate { preferred_exe_id: Some(foreign), ..Default::default() }).is_err());
        db.remove_executable(id, g.executables[1].id).unwrap();
        assert_eq!(db.get_game(id).unwrap().preferred_exe_id, Some(g.executables[0].id));
    }

    #[test]
    fn store_upsert_keeps_user_edits() {
        let db = Db::open_in_memory().unwrap();
        let g = NewGame { title: "Portal".into(), source: Some(Source::Steam), source_id: "400".into(), install_dir: "D:\\a".into(), ..Default::default() };
        let (id, new) = db.upsert_store_game(&g).unwrap();
        assert!(new);
        db.update_game(id, &GameUpdate { title: Some("Portal (mine)".into()), ..Default::default() }).unwrap();
        let (id2, new2) = db.upsert_store_game(&NewGame { install_dir: "E:\\b".into(), ..g }).unwrap();
        assert_eq!((id, false), (id2, new2));
        let got = db.get_game(id).unwrap();
        assert_eq!(got.title, "Portal (mine)");
        assert_eq!(got.install_dir, "E:\\b");
    }

    #[test]
    fn relocate_rewrites_paths_inside_old_folder() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("old");
        let new = d.path().join("new");
        std::fs::create_dir_all(new.join("bin")).unwrap();
        std::fs::write(new.join("bin/g.exe"), b"x").unwrap();
        let db = Db::open_in_memory().unwrap();
        let id = db
            .add_game(&NewGame { title: "M".into(), install_dir: old.to_string_lossy().into(), executables: vec![NewExecutable { path: old.join("bin/g.exe").to_string_lossy().into(), ..Default::default() }], ..Default::default() }, "")
            .unwrap();
        assert_eq!(db.relocate(id, &new).unwrap(), 1);
        let g = db.get_game(id).unwrap();
        assert_eq!(g.install_dir, new.to_string_lossy());
        assert_eq!(g.executables[0].path, new.join("bin/g.exe").to_string_lossy());
    }

    #[test]
    fn collections_reject_duplicates_and_empty_names() {
        let db = Db::open_in_memory().unwrap();
        db.create_collection("RPG").unwrap();
        assert!(db.create_collection("rpg").is_err());
        assert!(db.create_collection("  ").is_err());
    }

    #[test]
    fn migrates_version_1_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("old.db");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(&format!("{MIGRATION_1} PRAGMA user_version = 1;")).unwrap();
            c.execute("INSERT INTO games (title, added_at) VALUES ('Old', 1)", []).unwrap();
        }
        let db = Db::open(&p).unwrap();
        let g = &db.list_games().unwrap()[0];
        assert_eq!((g.title.as_str(), g.icon.as_str()), ("Old", ""));
    }

    #[test]
    fn reopening_file_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lib.db");
        let id = Db::open(&p).unwrap().add_game(&manual("Keep", &["k.exe"]), "").unwrap();
        assert_eq!(Db::open(&p).unwrap().get_game(id).unwrap().title, "Keep");
    }
}
