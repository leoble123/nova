//! Nova: a unified PC game library and launcher, with lagless built in.

pub mod art;
pub mod audio;
pub mod db;
pub mod launcher;
pub mod models;
pub mod pe;
pub mod safety;
pub mod scanner;
pub mod sources;
pub mod update;

use launcher::{Elevation, LaunchFail, Launcher, RunningGame};
use models::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub struct App {
    pub db: Arc<db::Db>,
    pub art: art::ArtStore,
    pub launcher: Arc<Launcher>,
    pub hub: Arc<audio::Hub>,
    pub updater: Arc<update::Updater>,
    pub data_dir: PathBuf,
}

type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    // anyhow's alternate form includes the cause chain.
    let s = format!("{err:#}");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

/// Fill in what's only known at runtime: is it installed, can it launch, is it running.
fn decorate(app: &App, mut g: Game) -> Game {
    g.running = app.launcher.is_running(g.id);
    let preferred = g.preferred_exe_id.and_then(|id| g.executables.iter().find(|e| e.id == id)).or(g.executables.first());
    let dir_ok = !g.install_dir.is_empty() && Path::new(&g.install_dir).is_dir();
    if !g.launch_uri.is_empty() {
        g.installed = dir_ok || g.install_dir.is_empty();
        if !g.installed {
            g.problem = "Not installed (the install folder is missing)".into();
        }
    } else {
        match preferred {
            None => g.problem = "No executable chosen".into(),
            Some(e) => match std::fs::metadata(&e.path) {
                Ok(m) if m.is_file() => g.installed = true,
                Ok(_) => g.problem = "The executable path points to a folder".into(),
                Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => g.problem = "Windows denied access to the game folder".into(),
                Err(_) => g.problem = if dir_ok { "The executable is missing".into() } else { "The game folder is missing (moved, deleted, or on a disconnected drive)".into() },
            },
        }
    }
    g
}

// ------------------------------------------------------------------ icons

/// Give a game an icon if it has none yet (or `force` after its executable changed): the
/// preferred executable's own icon, read as data. Never runs the file.
fn ensure_icon(app: &App, id: i64, force: bool) {
    let Ok(g) = app.db.get_game(id) else { return };
    if !force && !g.icon.is_empty() && g.icon != "-" {
        return;
    }
    let exe = g.preferred_exe_id.and_then(|p| g.executables.iter().find(|e| e.id == p)).or(g.executables.first());
    let found = exe.and_then(|e| app.art.put_exe_icon(id, Path::new(&e.path)));
    match found {
        Some(name) => {
            let _ = app.db.set_art(id, "icon", &name);
            app.art.remove(&g.icon);
        }
        // Keep an existing icon if the new executable has none; remember "none" otherwise,
        // so it isn't looked for again at every start.
        None if g.icon.is_empty() => {
            let _ = app.db.set_art(id, "icon", "-");
        }
        None => {}
    }
}

/// Games added before icons existed, or whose icon couldn't be read then.
fn backfill_icons(app: &App) -> usize {
    let Ok(games) = app.db.list_games() else { return 0 };
    let mut n = 0;
    for g in games.iter().filter(|g| g.icon.is_empty()) {
        ensure_icon(app, g.id, false);
        n += 1;
    }
    n
}

// ------------------------------------------------------------------ library

#[tauri::command(async)]
fn list_games(app: State<App>) -> R<Vec<Game>> {
    Ok(app.db.list_games().map_err(e)?.into_iter().map(|g| decorate(&app, g)).collect())
}

#[tauri::command(async)]
fn get_game(app: State<App>, id: i64) -> R<Game> {
    Ok(decorate(&app, app.db.get_game(id).map_err(e)?))
}

#[tauri::command(async)]
fn add_game(app: State<App>, game: NewGame) -> R<Game> {
    for x in &game.executables {
        let p = Path::new(&x.path);
        if !p.is_absolute() || !p.is_file() {
            return Err(format!("{} isn't an existing file", x.path));
        }
    }
    let id = app.db.add_game(&game, "").map_err(e)?;
    if !game.cover_path.is_empty() {
        if let Ok(name) = app.art.import_file(id, "cover", Path::new(&game.cover_path)) {
            let _ = app.db.set_art(id, "cover", &name);
        }
    }
    ensure_icon(&app, id, false);
    Ok(decorate(&app, app.db.get_game(id).map_err(e)?))
}

#[tauri::command(async)]
fn update_game(app: State<App>, id: i64, update: GameUpdate) -> R<Game> {
    if let Some(dir) = &update.working_dir {
        if !dir.trim().is_empty() && !Path::new(dir).is_dir() {
            return Err(format!("{dir} isn't an existing folder"));
        }
    }
    app.db.update_game(id, &update).map_err(e)?;
    if update.preferred_exe_id.is_some() {
        ensure_icon(&app, id, true);
    }
    Ok(decorate(&app, app.db.get_game(id).map_err(e)?))
}

/// Removes the library entry only. The game's own files are never touched.
#[tauri::command(async)]
fn remove_game(app: State<App>, id: i64) -> R<()> {
    if app.launcher.is_running(id) {
        return Err("Close the game before removing it from the library.".into());
    }
    let g = app.db.get_game(id).map_err(e)?;
    app.db.remove_game(id).map_err(e)?;
    app.art.remove(&g.cover);
    app.art.remove(&g.hero);
    app.art.remove(&g.icon);
    Ok(())
}

#[tauri::command(async)]
fn relocate_game(app: State<App>, id: i64, dir: String) -> R<Game> {
    let p = PathBuf::from(&dir);
    if !p.is_dir() {
        return Err(format!("{dir} isn't an existing folder"));
    }
    let g = app.db.get_game(id).map_err(e)?;
    let found = app.db.relocate(id, &p).map_err(e)?;
    ensure_icon(&app, id, false);
    if found == 0 && !g.executables.is_empty() {
        return Err(format!("Updated the folder, but {} wasn't found in it. Choose the executable on the Launch tab.", g.executables[0].path.rsplit(['\\', '/']).next().unwrap_or("the executable")));
    }
    Ok(decorate(&app, app.db.get_game(id).map_err(e)?))
}

#[tauri::command]
async fn scan_folder(app: State<'_, App>, path: String) -> R<scanner::ScanResult> {
    let p = PathBuf::from(&path);
    let mut r = tauri::async_runtime::spawn_blocking(move || scanner::scan(&p)).await.map_err(e)?.map_err(e)?;
    r.already_in_library = app.db.games_in_dir(&r.root).unwrap_or_default();
    Ok(r)
}

#[tauri::command]
async fn inspect_exe(path: String) -> R<scanner::Candidate> {
    tauri::async_runtime::spawn_blocking(move || scanner::inspect_one(Path::new(&path))).await.map_err(e)?.map_err(e)
}

#[tauri::command(async)]
fn add_executable(app: State<App>, game_id: i64, exe: NewExecutable) -> R<Game> {
    if !Path::new(&exe.path).is_file() {
        return Err(format!("{} isn't an existing file", exe.path));
    }
    app.db.add_executable(game_id, &exe).map_err(e)?;
    ensure_icon(&app, game_id, false);
    Ok(decorate(&app, app.db.get_game(game_id).map_err(e)?))
}

#[tauri::command(async)]
fn update_executable(app: State<App>, exe: Executable) -> R<Game> {
    if !Path::new(&exe.path).is_file() {
        return Err(format!("{} isn't an existing file", exe.path));
    }
    if !exe.working_dir.trim().is_empty() && !Path::new(&exe.working_dir).is_dir() {
        return Err(format!("{} isn't an existing folder", exe.working_dir));
    }
    app.db.update_executable(&exe).map_err(e)?;
    let preferred = app.db.get_game(exe.game_id).map(|g| g.preferred_exe_id == Some(exe.id)).unwrap_or(false);
    ensure_icon(&app, exe.game_id, preferred);
    Ok(decorate(&app, app.db.get_game(exe.game_id).map_err(e)?))
}

#[tauri::command(async)]
fn remove_executable(app: State<App>, game_id: i64, exe_id: i64) -> R<Game> {
    app.db.remove_executable(game_id, exe_id).map_err(e)?;
    Ok(decorate(&app, app.db.get_game(game_id).map_err(e)?))
}

#[tauri::command(async)]
fn set_art(app: State<App>, game_id: i64, kind: String, path: String) -> R<Game> {
    let g = app.db.get_game(game_id).map_err(e)?;
    let name = app.art.import_file(game_id, &kind, Path::new(&path)).map_err(e)?;
    app.db.set_art(game_id, &kind, &name).map_err(e)?;
    app.art.remove(if kind == "hero" { &g.hero } else { &g.cover });
    Ok(decorate(&app, app.db.get_game(game_id).map_err(e)?))
}

#[tauri::command(async)]
fn clear_art(app: State<App>, game_id: i64, kind: String) -> R<Game> {
    let g = app.db.get_game(game_id).map_err(e)?;
    app.db.set_art(game_id, &kind, "").map_err(e)?;
    app.art.remove(if kind == "hero" { &g.hero } else { &g.cover });
    Ok(decorate(&app, app.db.get_game(game_id).map_err(e)?))
}

/// Optional: fetch missing artwork for Steam games from Steam's CDN (off unless enabled).
#[tauri::command]
async fn fetch_online_art(app: State<'_, App>, game_id: i64) -> R<Game> {
    let g = app.db.get_game(game_id).map_err(e)?;
    if g.source != Source::Steam {
        return Err("Online artwork is only available for Steam games. Choose an image instead.".into());
    }
    let appid = g.source_id.clone();
    let dir = app.art.dir().to_path_buf();
    let need_cover = g.cover.is_empty();
    let need_hero = g.hero.is_empty();
    let (cover, hero, meta) = tauri::async_runtime::spawn_blocking(move || {
        let store = art::ArtStore::new(dir).ok();
        let c = store.as_ref().filter(|_| need_cover).and_then(|s| s.download(game_id, "cover", &sources::steam::cdn_cover_url(&appid)).ok());
        let h = store.as_ref().filter(|_| need_hero).and_then(|s| s.download(game_id, "hero", &sources::steam::cdn_hero_url(&appid)).ok());
        (c, h, sources::steam::store_details(&appid))
    })
    .await
    .map_err(e)?;
    if let Some((dev, publ, desc)) = &meta {
        let upd = GameUpdate {
            developer: g.developer.is_empty().then(|| dev.clone()),
            publisher: g.publisher.is_empty().then(|| publ.clone()),
            description: g.description.is_empty().then(|| desc.clone()),
            ..Default::default()
        };
        app.db.update_game(game_id, &upd).map_err(e)?;
    }
    if cover.is_none() && hero.is_none() && meta.is_none() {
        return Err("Couldn't reach Steam (offline?), or it has nothing more for this game.".into());
    }
    if let Some(c) = cover {
        app.art.remove(&g.cover);
        app.db.set_art(game_id, "cover", &c).map_err(e)?;
    }
    if let Some(h) = hero {
        app.art.remove(&g.hero);
        app.db.set_art(game_id, "hero", &h).map_err(e)?;
    }
    Ok(decorate(&app, app.db.get_game(game_id).map_err(e)?))
}

/// The icon of an executable as a data: URL (import preview). Read as data, never run.
#[tauri::command]
async fn exe_icon_preview(path: String) -> R<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = match pe::icon(Path::new(&path)) {
            Some(pe::Icon::Png(b)) | Some(pe::Icon::Ico(b)) => b,
            None => return Ok(String::new()),
        };
        art::data_url_bytes(&bytes).map_err(e)
    })
    .await
    .map_err(e)?
}

#[tauri::command]
async fn image_preview(path: String) -> R<String> {
    tauri::async_runtime::spawn_blocking(move || art::data_url(Path::new(&path))).await.map_err(e)?.map_err(e)
}

#[tauri::command(async)]
fn art_dir(app: State<App>) -> String {
    app.art.dir().to_string_lossy().to_string()
}

// ------------------------------------------------------------------ launching

#[tauri::command(async)]
fn launch_game(app: State<App>, id: i64, exe_id: Option<i64>, confirmed: bool, admin: bool) -> Result<RunningGame, LaunchFail> {
    let g = app.db.get_game(id).map_err(|err| LaunchFail { code: "missing-exe", message: e(err), warnings: vec![] })?;
    app.launcher.launch(&g, exe_id, confirmed, if admin { Elevation::UserRequestedAdmin } else { Elevation::Normal })
}

#[tauri::command(async)]
fn stop_game(app: State<App>, id: i64) -> bool {
    app.launcher.stop(id)
}

#[tauri::command(async)]
fn running_games(app: State<App>) -> Vec<RunningGame> {
    app.launcher.running()
}

#[tauri::command(async)]
fn sessions(app: State<App>, game_id: i64) -> R<Vec<Session>> {
    app.db.sessions(game_id, 50).map_err(e)
}

#[tauri::command(async)]
fn reveal(path: String) -> R<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("{path} doesn't exist"));
    }
    launcher::reveal(&p).map_err(e)
}

// ------------------------------------------------------------------ collections

#[tauri::command(async)]
fn list_collections(app: State<App>) -> R<Vec<Collection>> {
    app.db.list_collections().map_err(e)
}

#[tauri::command(async)]
fn create_collection(app: State<App>, name: String) -> R<i64> {
    app.db.create_collection(&name).map_err(e)
}

#[tauri::command(async)]
fn rename_collection(app: State<App>, id: i64, name: String) -> R<()> {
    app.db.rename_collection(id, &name).map_err(e)
}

#[tauri::command(async)]
fn delete_collection(app: State<App>, id: i64) -> R<()> {
    app.db.delete_collection(id).map_err(e)
}

#[tauri::command(async)]
fn set_in_collection(app: State<App>, collection_id: i64, game_id: i64, member: bool) -> R<()> {
    app.db.set_in_collection(collection_id, game_id, member).map_err(e)
}

// ------------------------------------------------------------------ stores

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanSummary {
    reports: Vec<sources::StoreReport>,
    added: usize,
}

pub fn scan_stores_blocking(app: &App) -> Vec<sources::StoreReport> {
    let mut reports = Vec::new();
    let scans: [(&str, fn() -> anyhow::Result<(PathBuf, Vec<sources::Found>)>); 3] = [("Steam", sources::steam::scan), ("Epic Games", sources::epic::scan), ("GOG", sources::gog::scan)];
    for (store, scan) in scans {
        let mut rep = sources::StoreReport { store: store.into(), ..Default::default() };
        match scan() {
            Ok((loc, found)) => {
                rep.detected = true;
                rep.location = loc.to_string_lossy().to_string();
                rep.found = found.len();
                for f in found {
                    match app.db.upsert_store_game(&f.game) {
                        Ok((id, new)) => {
                            if new {
                                rep.added += 1;
                            }
                            // Copy Steam's local artwork if we don't have any yet.
                            if let Ok(g) = app.db.get_game(id) {
                                if g.cover.is_empty() {
                                    if let Some(c) = &f.cover {
                                        if let Ok(n) = app.art.import_file(id, "cover", c) {
                                            let _ = app.db.set_art(id, "cover", &n);
                                        }
                                    }
                                }
                                if g.hero.is_empty() {
                                    if let Some(h) = &f.hero {
                                        if let Ok(n) = app.art.import_file(id, "hero", h) {
                                            let _ = app.db.set_art(id, "hero", &n);
                                        }
                                    }
                                }
                                if g.icon.is_empty() || g.icon == "-" {
                                    match &f.icon {
                                        Some(i) => {
                                            if let Ok(n) = app.art.import_file(id, "icon", i) {
                                                let _ = app.db.set_art(id, "icon", &n);
                                            }
                                        }
                                        None => ensure_icon(app, id, false),
                                    }
                                }
                            }
                        }
                        Err(err) => rep.error = e(err),
                    }
                }
            }
            Err(err) => rep.error = e(err),
        }
        reports.push(rep);
    }
    reports
}

#[tauri::command]
async fn scan_stores(handle: AppHandle) -> R<ScanSummary> {
    let h = handle.clone();
    let reports = tauri::async_runtime::spawn_blocking(move || scan_stores_blocking(&h.state::<App>())).await.map_err(e)?;
    let added = reports.iter().map(|r| r.added).sum();
    Ok(ScanSummary { reports, added })
}

// ------------------------------------------------------------------ settings

const DEFAULT_SETTINGS: &str = r#"{"autoUpdate":true,"scanStoresOnStart":true,"onlineArt":false,"laglessEnabled":true,"minimizeOnLaunch":false,"cardSize":1,"showHidden":false,"reduceMotion":false}"#;

fn nova_settings(app: &App) -> Value {
    let mut base: Value = serde_json::from_str(DEFAULT_SETTINGS).unwrap();
    if let Some(saved) = app.db.setting("nova").and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        if let (Some(b), Some(s)) = (base.as_object_mut(), saved.as_object()) {
            for (k, v) in s {
                b.insert(k.clone(), v.clone());
            }
        }
    }
    base
}

#[tauri::command(async)]
fn get_settings(app: State<App>) -> Value {
    nova_settings(&app)
}

#[tauri::command(async)]
fn set_settings(app: State<App>, settings: Value) -> R<Value> {
    let old = nova_settings(&app);
    let mut merged = old.clone();
    if let (Some(m), Some(s)) = (merged.as_object_mut(), settings.as_object()) {
        for (k, v) in s {
            m.insert(k.clone(), v.clone());
        }
    }
    app.db.set_setting("nova", &merged.to_string()).map_err(e)?;
    if old.get("laglessEnabled") != merged.get("laglessEnabled") {
        app.hub.set_enabled(merged.get("laglessEnabled").and_then(Value::as_bool).unwrap_or(true));
    }
    Ok(merged)
}

#[tauri::command(async)]
fn app_info(app: State<App>) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "dataDir": app.data_dir.to_string_lossy(),
        "laglessVersion": lagless::update::VERSION,
        "updates": app.updater.state(),
        "windows": cfg!(windows),
    })
}

// ------------------------------------------------------------------ updates

#[tauri::command(async)]
fn update_state(app: State<App>) -> Value {
    json!({ "current": update::VERSION, "status": app.updater.state() })
}

#[tauri::command(async)]
fn update_check(app: State<App>) -> Value {
    app.updater.check();
    json!({ "current": update::VERSION, "status": app.updater.state() })
}

/// Install the downloaded update now and start the new version. Running games keep running;
/// the time played so far is recorded first.
#[tauri::command(async)]
fn update_restart(handle: AppHandle) -> R<()> {
    let app = handle.state::<App>();
    app.launcher.shutdown();
    app.hub.shutdown();
    let exe = std::env::current_exe().map_err(e)?;
    match app.updater.install_into(None).map_err(e)? {
        Some(_) => {
            std::process::Command::new(&exe).args(["--after-update", &std::process::id().to_string()]).spawn().map_err(e)?;
            handle.exit(0);
            Ok(())
        }
        None => Err("No update is ready to install.".into()),
    }
}

// ------------------------------------------------------------------ lagless

#[tauri::command(async)]
fn audio_state(app: State<App>) -> Value {
    app.hub.state()
}
#[tauri::command(async)]
fn audio_set_volume(app: State<App>, volume: f32) -> R<()> {
    app.hub.set_volume(volume)
}
#[tauri::command(async)]
fn audio_set_muted(app: State<App>, muted: bool) -> R<()> {
    app.hub.set_muted(muted)
}
#[tauri::command(async)]
fn audio_set_paused(app: State<App>, paused: bool) -> R<()> {
    app.hub.set_paused(paused)
}
#[tauri::command(async)]
fn audio_set_mic(app: State<App>, on: bool) -> R<()> {
    app.hub.set_mic(on)
}
#[tauri::command(async)]
fn audio_set_device(app: State<App>, name: String, included: bool) {
    app.hub.set_device_included(&name, included)
}
#[tauri::command(async)]
fn audio_set_sync(app: State<App>, on: bool) {
    app.hub.set_sync(on)
}
#[tauri::command]
async fn audio_settings(app: State<'_, App>) -> R<Value> {
    // Listing audio devices can take a moment on Windows.
    let hub = app.hub.clone();
    tauri::async_runtime::spawn_blocking(move || hub.settings()).await.map_err(e)
}
#[tauri::command]
async fn audio_apply_settings(app: State<'_, App>, settings: Value) -> R<()> {
    let hub = app.hub.clone();
    tauri::async_runtime::spawn_blocking(move || hub.apply_settings(settings)).await.map_err(e)?
}
#[tauri::command(async)]
fn audio_allow_fps() -> R<()> {
    audio::allow_fps_counter()
}
#[tauri::command(async)]
fn audio_open_firewall() -> R<()> {
    audio::open_firewall()
}

// ------------------------------------------------------------------ overlays (FPS counter, room alerts)

fn overlay_geometry(app: &AppHandle, kind: &str, osd: &Value) -> Option<(tauri::LogicalPosition<f64>, tauri::LogicalSize<f64>)> {
    let main = app.get_webview_window("main")?;
    let mon = main.primary_monitor().ok().flatten().or_else(|| main.current_monitor().ok().flatten())?;
    let scale = mon.scale_factor();
    let (mx, my) = (mon.position().x as f64 / scale, mon.position().y as f64 / scale);
    let (mw, mh) = (mon.size().width as f64 / scale, mon.size().height as f64 / scale);
    if kind == "alert" {
        let (w, h) = (520.0, 96.0);
        return Some((tauri::LogicalPosition::new(mx + (mw - w) / 2.0, my + 28.0), tauri::LogicalSize::new(w, h)));
    }
    let size = [0.85, 1.0, 1.3][osd.get("size").and_then(Value::as_u64).unwrap_or(1).min(2) as usize];
    let (w, h) = match osd.get("style").and_then(Value::as_u64).unwrap_or(0) {
        1 => (380.0, 44.0),
        2 => (120.0, 56.0),
        _ => (280.0, 168.0),
    };
    let (w, h) = (w * size, h * size);
    let m = 16.0;
    let corner = osd.get("corner").and_then(Value::as_u64).unwrap_or(0);
    let x = if corner % 2 == 1 { mx + mw - w - m } else { mx + m };
    let y = if corner >= 2 { my + mh - h - m } else { my + m };
    Some((tauri::LogicalPosition::new(x, y), tauri::LogicalSize::new(w, h)))
}

fn ensure_overlay(app: &AppHandle, label: &str) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window(label) {
        return Some(w);
    }
    let w = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("overlay.html#{label}").into()))
        .title("Nova overlay")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        // WS_EX_NOACTIVATE: showing the counter can never take focus from a fullscreen game.
        .focusable(false)
        .shadow(false)
        .visible(false)
        .build()
        .ok()?;
    let _ = w.set_ignore_cursor_events(true); // clicks go through to the game
    Some(w)
}

fn sync_osd(app: &AppHandle, perf: &Value) {
    static LAST: std::sync::Mutex<Option<(bool, String)>> = std::sync::Mutex::new(None);
    let osd = perf.get("osd").cloned().unwrap_or(Value::Null);
    let want = osd.get("on").and_then(Value::as_bool).unwrap_or(false) && perf.get("live").and_then(Value::as_bool).unwrap_or(false);
    // Stats arrive several times a second; only move/show/hide the window when that changes.
    let key = (want, osd.to_string());
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(&key) {
            return;
        }
        *last = Some(key);
    }
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if want {
            if let (Some(w), Some((pos, size))) = (ensure_overlay(&app2, "osd"), overlay_geometry(&app2, "osd", &osd)) {
                let _ = w.set_size(size);
                let _ = w.set_position(pos);
                if !w.is_visible().unwrap_or(false) {
                    let _ = w.show();
                }
            }
        } else if let Some(w) = app2.get_webview_window("osd") {
            let _ = w.hide();
        }
    });
}

fn show_alert(app: &AppHandle) {
    // Each alert restarts the timer, so a second alert isn't hidden by the first one's.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let me = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let (Some(w), Some((pos, size))) = (ensure_overlay(&app2, "alert"), overlay_geometry(&app2, "alert", &Value::Null)) {
            let _ = w.set_size(size);
            let _ = w.set_position(pos);
            let _ = w.show();
            let w2 = w.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(7));
                if SEQ.load(std::sync::atomic::Ordering::SeqCst) == me {
                    let _ = w2.hide();
                }
            });
        }
    });
}

// ------------------------------------------------------------------ startup

fn data_dir(app: &AppHandle) -> PathBuf {
    if let Some(p) = std::env::var_os("NOVA_DATA_DIR") {
        return PathBuf::from(p);
    }
    app.path().app_local_data_dir().unwrap_or_else(|_| PathBuf::from("nova-data"))
}

pub fn run() {
    update::wait_for_previous();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = data_dir(&handle);
            let db = Arc::new(db::Db::open(&dir.join("library.db"))?);
            let art = art::ArtStore::new(dir.join("art"))?;
            let _ = app.asset_protocol_scope().allow_directory(art.dir(), true);
            let h1 = handle.clone();
            let launcher = Launcher::new(db.clone(), &dir, move |ev| {
                let _ = h1.emit("session", ev);
            });
            let settings_db = db.clone();
            let lagless_on = settings_db.setting("nova").and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v.get("laglessEnabled").and_then(Value::as_bool)).unwrap_or(true);
            let h2 = handle.clone();
            let emit: audio::Emit = Arc::new(move |name: &str, payload: Value| {
                match name {
                    "perf" => sync_osd(&h2, &payload),
                    "lagless-alert" => show_alert(&h2),
                    _ => {}
                }
                let _ = h2.emit(name, payload);
            });
            let l2 = launcher.clone();
            let hub = audio::Hub::start(lagless_on, emit, move |fps, low1| l2.perf_sample(fps, low1));
            let h4 = handle.clone();
            let updater = update::Updater::new(dir.join("update"), move |st| {
                let _ = h4.emit("update", json!({ "current": update::VERSION, "status": st }));
            });
            let db_for_updates = db.clone();
            updater.spawn_loop(move || {
                db_for_updates.setting("nova").and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v.get("autoUpdate").and_then(Value::as_bool)).unwrap_or(true)
            });
            app.manage(App { db, art, launcher, hub, updater, data_dir: dir });
            // Store libraries refresh in the background; the UI shows what's there meanwhile.
            let h3 = handle.clone();
            std::thread::spawn(move || {
                let state = h3.state::<App>();
                let on = nova_settings(&state).get("scanStoresOnStart").and_then(Value::as_bool).unwrap_or(true);
                let mut added = 0;
                if on {
                    let reports = scan_stores_blocking(&state);
                    added = reports.iter().map(|r| r.added).sum();
                }
                let icons = backfill_icons(&state);
                if on || icons > 0 {
                    let _ = h3.emit("library-changed", json!({ "added": added }));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.label() == "main" {
                    let app = window.app_handle();
                    for label in ["osd", "alert"] {
                        if let Some(w) = app.get_webview_window(label) {
                            let _ = w.destroy();
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_games,
            get_game,
            add_game,
            update_game,
            remove_game,
            scan_folder,
            relocate_game,
            inspect_exe,
            add_executable,
            update_executable,
            remove_executable,
            set_art,
            clear_art,
            fetch_online_art,
            art_dir,
            image_preview,
            exe_icon_preview,
            launch_game,
            stop_game,
            running_games,
            sessions,
            reveal,
            list_collections,
            create_collection,
            rename_collection,
            delete_collection,
            set_in_collection,
            scan_stores,
            get_settings,
            set_settings,
            app_info,
            audio_state,
            audio_set_volume,
            audio_set_muted,
            audio_set_paused,
            audio_set_mic,
            audio_set_device,
            audio_set_sync,
            audio_settings,
            audio_apply_settings,
            audio_allow_fps,
            audio_open_firewall,
            update_state,
            update_check,
            update_restart,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Nova");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(s) = handle.try_state::<App>() {
                s.launcher.shutdown();
                s.hub.shutdown();
                // Quitting with an update downloaded installs it; the next start is the new version.
                let _ = s.updater.install_into(None);
            }
        }
    });
}
