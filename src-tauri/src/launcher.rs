//! Starting games and knowing when they've closed.
//!
//! * Manual/GOG games: the executable is started in place, from its original folder, with the
//!   working directory set to the game's root folder (or a per-game / per-executable override).
//!   Nova runs it with the user's normal rights. Administrator rights are only ever requested
//!   when the user explicitly chooses "Run as administrator" for that launch.
//! * Steam/Epic games: started through the store client's URI so DRM and overlays work.
//!
//! "Running" means: the process Nova started is alive, OR any process in its Windows job object
//! is (children spawned by a launcher stub), OR any process whose executable lives inside the
//! game's folder is (games that hand off to another binary, store launches). Playtime is the
//! wall-clock time between the first and last moment the game was seen running.

use crate::db::Db;
use crate::models::{now_ms, Executable, Game};
use crate::safety::{self, Level, Warning};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchFail {
    /// missing-exe, missing-dir, needs-confirm, needs-admin, already-running, spawn-failed, no-target
    pub code: &'static str,
    pub message: String,
    pub warnings: Vec<Warning>,
}

fn fail(code: &'static str, message: impl Into<String>) -> LaunchFail {
    LaunchFail { code, message: message.into(), warnings: Vec::new() }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub game_id: i64,
    /// "started", "ended", "failed"
    pub state: &'static str,
    pub started_at: i64,
    pub secs: i64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningGame {
    pub game_id: i64,
    pub started_at: i64,
    pub title: String,
    pub elevated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Elevation {
    Normal,
    /// The user explicitly clicked "Run as administrator" for this launch.
    UserRequestedAdmin,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Direct { exe: PathBuf, args: String, cwd: PathBuf },
    Uri(String),
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub target: Target,
    /// Folders whose processes count as "the game is running".
    pub track_dirs: Vec<PathBuf>,
}

struct Run {
    started_at: i64,
    title: String,
    kill: Arc<AtomicBool>,
    fps: Arc<Mutex<Vec<(f32, f32)>>>,
    elevated: bool,
}

pub struct Launcher {
    db: Arc<Db>,
    runs: Mutex<HashMap<i64, Run>>,
    emit: Box<dyn Fn(SessionEvent) + Send + Sync>,
    recovery: PathBuf,
}

/// Resolve what to start, validating every path first so failures are clear, not cryptic.
pub fn plan(game: &Game, exe_id: Option<i64>) -> Result<Plan, LaunchFail> {
    let install = PathBuf::from(&game.install_dir);
    let chosen: Option<&Executable> = match exe_id {
        Some(id) => Some(game.executables.iter().find(|e| e.id == id).ok_or_else(|| fail("missing-exe", "That executable isn't registered for this game any more."))?),
        None if !game.launch_uri.is_empty() => None,
        None => Some(
            game.preferred_exe_id
                .and_then(|id| game.executables.iter().find(|e| e.id == id))
                .or_else(|| game.executables.first())
                .ok_or_else(|| fail("no-target", "No executable is set for this game. Open the game's settings and choose its executable."))?,
        ),
    };
    let mut track_dirs = Vec::new();
    if !game.install_dir.is_empty() && install.is_dir() {
        track_dirs.push(install.clone());
    }
    let Some(exe) = chosen else {
        if !game.install_dir.is_empty() && !install.is_dir() {
            return Err(fail("missing-dir", format!("The install folder {} is missing. The drive may be disconnected, or the game was uninstalled.", install.display())));
        }
        return Ok(Plan { target: Target::Uri(game.launch_uri.clone()), track_dirs });
    };
    let exe_path = PathBuf::from(&exe.path);
    match std::fs::metadata(&exe_path) {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err(fail("missing-exe", format!("{} is a folder, not a program.", exe_path.display()))),
        Err(e) => return Err(fail("missing-exe", format!("Can't find {}: {}. Choose the game's executable again if it moved.", exe_path.display(), crate::scanner::friendly_io(&e)))),
    }
    let cwd_s = [&exe.working_dir, &game.working_dir, &game.install_dir].into_iter().find(|s| !s.trim().is_empty()).cloned();
    let cwd = match cwd_s {
        Some(s) => PathBuf::from(s),
        None => exe_path.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    if !cwd.is_dir() {
        return Err(fail("missing-dir", format!("The working folder {} doesn't exist. Fix it in the game's settings.", cwd.display())));
    }
    if track_dirs.is_empty() {
        if let Some(p) = exe_path.parent() {
            track_dirs.push(p.to_path_buf());
        }
    }
    let args = [game.launch_args.trim(), exe.args.trim()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    Ok(Plan { target: Target::Direct { exe: exe_path, args, cwd }, track_dirs })
}

/// Split a launch-argument string the way Windows programs parse their command line.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn norm(p: &Path) -> String {
    let s = crate::scanner::dunce(p).to_string_lossy().replace('/', "\\");
    let s = s.trim_end_matches('\\').to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

pub fn path_inside(child: &Path, dir: &Path) -> bool {
    let (c, d) = (norm(child), norm(dir));
    !d.is_empty() && c.len() > d.len() && c.starts_with(&d) && c.as_bytes()[d.len()] == b'\\'
}

impl Launcher {
    pub fn new(db: Arc<Db>, data_dir: &Path, emit: impl Fn(SessionEvent) + Send + Sync + 'static) -> Arc<Launcher> {
        let l = Arc::new(Launcher { db, runs: Mutex::new(HashMap::new()), emit: Box::new(emit), recovery: data_dir.join("active-sessions.json") });
        l.recover();
        l
    }

    pub fn running(&self) -> Vec<RunningGame> {
        self.runs.lock().unwrap().iter().map(|(id, r)| RunningGame { game_id: *id, started_at: r.started_at, title: r.title.clone(), elevated: r.elevated }).collect()
    }

    pub fn is_running(&self, game_id: i64) -> bool {
        self.runs.lock().unwrap().contains_key(&game_id)
    }

    /// Ask a running game to close (terminates its processes; only ones inside its folder or job).
    pub fn stop(&self, game_id: i64) -> bool {
        match self.runs.lock().unwrap().get(&game_id) {
            Some(r) => {
                r.kill.store(true, Relaxed);
                true
            }
            None => false,
        }
    }

    /// FPS sample from the stats monitor; credited to the most recently started game.
    pub fn perf_sample(&self, fps: f32, low1: f32) {
        if fps <= 0.0 {
            return;
        }
        let runs = self.runs.lock().unwrap();
        if let Some(r) = runs.values().max_by_key(|r| r.started_at) {
            let mut f = r.fps.lock().unwrap();
            if f.len() < 200_000 {
                f.push((fps, low1));
            }
        }
    }

    pub fn launch(self: &Arc<Self>, game: &Game, exe_id: Option<i64>, confirmed: bool, elevation: Elevation) -> Result<RunningGame, LaunchFail> {
        if self.is_running(game.id) {
            return Err(fail("already-running", format!("{} is already running.", game.title)));
        }
        let plan = plan(game, exe_id)?;
        if let Target::Direct { exe, .. } = &plan.target {
            // Re-check the file right before running it: it may have changed since import.
            let pe = crate::pe::inspect(exe);
            let size = std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0);
            let mut ws = safety::quick(exe, &pe, size);
            ws.extend(safety::deep(exe, &pe));
            let serious: Vec<Warning> = ws.into_iter().filter(|w| w.level >= Level::Warn && !(w.code == "admin" && elevation == Elevation::UserRequestedAdmin)).collect();
            let key = ack_key(exe);
            if !serious.is_empty() && !confirmed && self.db.setting(&key).is_none() {
                return Err(LaunchFail { code: "needs-confirm", message: "Check these warnings before starting this program.".into(), warnings: serious });
            }
            if confirmed && !serious.is_empty() {
                let _ = self.db.set_setting(&key, "1");
            }
        }
        let started = Instant::now();
        let started_at = now_ms();
        let proc = spawn(&plan.target, elevation)?;
        let _ = self.db.mark_played(game.id, started_at);
        let run = Run { started_at, title: game.title.clone(), kill: Arc::new(AtomicBool::new(false)), fps: Arc::new(Mutex::new(Vec::new())), elevated: elevation == Elevation::UserRequestedAdmin };
        let (kill, fps) = (run.kill.clone(), run.fps.clone());
        self.runs.lock().unwrap().insert(game.id, run);
        self.save_recovery();
        (self.emit)(SessionEvent { game_id: game.id, state: "started", started_at, secs: 0, message: String::new() });
        let me = self.clone();
        let game_id = game.id;
        let is_uri = matches!(plan.target, Target::Uri(_));
        let dirs = plan.track_dirs.clone();
        std::thread::Builder::new()
            .name(format!("track-{game_id}"))
            .spawn(move || me.track(game_id, proc, dirs, is_uri, started, started_at, kill, fps))
            .map_err(|e| fail("spawn-failed", e.to_string()))?;
        Ok(RunningGame { game_id, started_at, title: game.title.clone(), elevated: elevation == Elevation::UserRequestedAdmin })
    }

    #[allow(clippy::too_many_arguments)]
    fn track(&self, game_id: i64, mut proc: Proc, dirs: Vec<PathBuf>, is_uri: bool, started: Instant, started_at: i64, kill: Arc<AtomicBool>, fps: Arc<Mutex<Vec<(f32, f32)>>>) {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut sys = System::new().ok();
        let me = sysinfo::get_current_pid().ok();
        let mut last_alive = Instant::now();
        let mut seen = !is_uri; // a direct launch is "seen" from the start
        let mut first_seen = if is_uri { None } else { Some(started) };
        let mut last_scan = Instant::now() - Duration::from_secs(10);
        let mut last_heartbeat = Instant::now();
        let mut dir_alive = false;
        // Store launches: the client may need to update or log in first.
        let appear_grace = Duration::from_secs(if is_uri { 180 } else { 0 });
        // A launcher stub that exits right as the game starts: keep looking for a moment.
        let handoff_grace = Duration::from_secs(if is_uri { 10 } else { 6 });
        let mut exit_code: Option<i32> = None;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let killing = kill.load(Relaxed);
            let own = proc.alive(&mut exit_code);
            if last_scan.elapsed() >= Duration::from_secs(2) || killing {
                last_scan = Instant::now();
                let mut matched = Vec::new();
                if let Some(sys) = sys.as_mut() {
                    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
                }
                for (pid, p) in sys.iter().flat_map(|s| s.processes()) {
                    if Some(*pid) == me {
                        continue;
                    }
                    if let Some(exe) = p.exe() {
                        if dirs.iter().any(|d| path_inside(exe, d)) {
                            matched.push(*pid);
                        }
                    }
                }
                dir_alive = !matched.is_empty();
                if killing {
                    proc.terminate();
                    for pid in matched {
                        if let Some(p) = sys.as_ref().and_then(|s| s.process(pid)) {
                            p.kill();
                        }
                    }
                }
            }
            let alive = own || dir_alive;
            if alive {
                last_alive = Instant::now();
                if !seen {
                    seen = true;
                    first_seen = Some(Instant::now());
                }
            }
            if !seen {
                if started.elapsed() > appear_grace || killing {
                    break;
                }
                continue;
            }
            if !alive && last_alive.elapsed() >= if killing { Duration::from_millis(0) } else { handoff_grace } {
                break;
            }
            if last_heartbeat.elapsed() > Duration::from_secs(30) {
                last_heartbeat = Instant::now();
                self.save_recovery();
            }
        }
        proc.close();
        let ended_at = now_ms();
        let secs = match first_seen {
            // Count up to the last time the game was seen, not the grace period after it.
            Some(t) => last_alive.saturating_duration_since(t).as_secs() as i64,
            None => 0,
        };
        let samples = std::mem::take(&mut *fps.lock().unwrap());
        let (avg, low) = if samples.len() >= 10 {
            let n = samples.len() as f64;
            (Some(samples.iter().map(|s| s.0 as f64).sum::<f64>() / n), Some(samples.iter().map(|s| s.1 as f64).sum::<f64>() / n))
        } else {
            (None, None)
        };
        self.runs.lock().unwrap().remove(&game_id);
        self.save_recovery();
        if !seen {
            (self.emit)(SessionEvent { game_id, state: "failed", started_at, secs: 0, message: "The game didn't start. Check that the store client is signed in.".into() });
            return;
        }
        let real_end = started_at + secs * 1000;
        let _ = self.db.record_session(game_id, started_at, real_end.min(ended_at), avg, low);
        let message = match exit_code {
            Some(code) if code != 0 && secs < 5 && !kill.load(Relaxed) => format!("The game closed right away (exit code {code}). Check its working folder and launch options."),
            _ => String::new(),
        };
        (self.emit)(SessionEvent { game_id, state: if message.is_empty() { "ended" } else { "failed" }, started_at, secs, message });
    }

    // ---- crash safety: if Nova itself is closed or crashes mid-game, the session isn't lost.

    fn save_recovery(&self) {
        let now = now_ms();
        let map: HashMap<String, (i64, i64)> = self.runs.lock().unwrap().iter().map(|(id, r)| (id.to_string(), (r.started_at, now))).collect();
        let tmp = self.recovery.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_string(&map).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.recovery);
        }
    }

    fn recover(&self) {
        let Ok(text) = std::fs::read_to_string(&self.recovery) else { return };
        if let Ok(map) = serde_json::from_str::<HashMap<String, (i64, i64)>>(&text) {
            for (id, (start, last_seen)) in map {
                if let Ok(id) = id.parse::<i64>() {
                    if last_seen > start {
                        let _ = self.db.record_session(id, start, last_seen, None, None);
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&self.recovery);
    }

    /// Called on quit: games keep running, the time so far is recorded.
    pub fn shutdown(&self) {
        let now = now_ms();
        for (id, r) in self.runs.lock().unwrap().drain() {
            let _ = self.db.record_session(id, r.started_at, now, None, None);
        }
        let _ = std::fs::remove_file(&self.recovery);
    }
}

fn ack_key(exe: &Path) -> String {
    let m = std::fs::metadata(exe).ok();
    let size = m.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    // A changed file (update, tampering) gets asked about again.
    format!("ack:{}:{size}:{mtime}", norm(exe))
}

// ------------------------------------------------------------------ processes

enum Proc {
    None,
    Child(std::process::Child, Option<job::Job>),
    #[cfg(windows)]
    Handle(job::OwnedHandle),
}

impl Proc {
    fn alive(&mut self, code: &mut Option<i32>) -> bool {
        match self {
            Proc::None => false,
            Proc::Child(c, j) => {
                let own = match c.try_wait() {
                    Ok(Some(st)) => {
                        if code.is_none() {
                            *code = st.code();
                        }
                        false
                    }
                    Ok(None) => true,
                    Err(_) => false,
                };
                own || j.as_ref().is_some_and(|j| j.active() > 0)
            }
            #[cfg(windows)]
            Proc::Handle(h) => h.alive(),
        }
    }

    fn terminate(&mut self) {
        match self {
            Proc::None => {}
            Proc::Child(c, j) => {
                if let Some(j) = j {
                    j.terminate();
                }
                let _ = c.kill();
            }
            #[cfg(windows)]
            Proc::Handle(h) => h.terminate(),
        }
    }

    fn close(self) {
        // Dropping the job handle doesn't kill anything (no KILL_ON_JOB_CLOSE).
    }
}

fn spawn(t: &Target, elevation: Elevation) -> Result<Proc, LaunchFail> {
    match t {
        Target::Uri(uri) => {
            open_uri(uri).map_err(|e| fail("spawn-failed", format!("Couldn't hand the game to its store client: {e}")))?;
            Ok(Proc::None)
        }
        Target::Direct { exe, args, cwd } => {
            if elevation == Elevation::UserRequestedAdmin {
                return spawn_elevated(exe, args, cwd);
            }
            let mut cmd = std::process::Command::new(exe);
            cmd.current_dir(cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NEW_CONSOLE: u32 = 0x10;
                const NORMAL_PRIORITY_CLASS: u32 = 0x20;
                const CREATE_UNICODE_ENVIRONMENT: u32 = 0x400;
                cmd.creation_flags(CREATE_NEW_CONSOLE | NORMAL_PRIORITY_CLASS | CREATE_UNICODE_ENVIRONMENT);
                if !args.is_empty() {
                    cmd.raw_arg(args); // exactly as typed, like a shortcut's target field
                }
                // Scripts (.bat/.cmd) are run by cmd.exe; std handles that, with its own escaping.
            }
            #[cfg(not(windows))]
            cmd.args(split_args(args));
            match cmd.spawn() {
                Ok(child) => {
                    let j = job::Job::for_child(&child);
                    Ok(Proc::Child(child, j))
                }
                Err(e) if e.raw_os_error() == Some(740) => Err(fail(
                    "needs-admin",
                    "This game asks for administrator rights. Nova runs games with your normal rights; you can run it as administrator for this launch if you trust it.",
                )),
                Err(e) => Err(fail("spawn-failed", format!("Windows couldn't start {}: {}", exe.display(), e))),
            }
        }
    }
}

#[cfg(windows)]
fn spawn_elevated(exe: &Path, args: &str, cwd: &Path) -> Result<Proc, LaunchFail> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let w = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain([0]).collect() };
    let (verb, file, params, dir) = (w("runas".as_ref()), w(exe.as_os_str()), w(args.as_ref()), w(cwd.as_os_str()));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.lpDirectory = dir.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err(fail("spawn-failed", "Starting as administrator was cancelled."));
        }
        Ok(Proc::Handle(job::OwnedHandle(info.hProcess)))
    }
}

#[cfg(not(windows))]
fn spawn_elevated(_exe: &Path, _args: &str, _cwd: &Path) -> Result<Proc, LaunchFail> {
    Err(fail("spawn-failed", "Running as administrator is only available on Windows."))
}

pub fn open_uri(uri: &str) -> std::io::Result<()> {
    // Only store-client schemes are ever opened this way.
    let allowed = ["steam://", "com.epicgames.launcher://", "goggalaxy://"];
    if !allowed.iter().any(|a| uri.starts_with(a)) {
        return Err(std::io::Error::other("unsupported launch address"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let op: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain([0]).collect();
        let file: Vec<u16> = std::ffi::OsStr::new(uri).encode_wide().chain([0]).collect();
        let r = unsafe { ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
        if (r as isize) <= 32 {
            return Err(std::io::Error::other("the store client isn't installed"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(uri).spawn().map(|_| ())
    }
}

/// Show a folder (or select a file) in Explorer. Never opens/executes the file itself.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("explorer.exe");
        if path.is_file() {
            c.raw_arg(format!("/select,\"{}\"", path.display()));
        } else {
            c.arg(path);
        }
        c.spawn().map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let dir = if path.is_file() { path.parent().unwrap_or(path) } else { path };
        std::process::Command::new("xdg-open").arg(dir).spawn().map(|_| ())
    }
}

#[cfg(windows)]
mod job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};

    /// A job object holding the game and every process it starts (unless they break away).
    pub struct Job(HANDLE);
    unsafe impl Send for Job {}

    impl Job {
        pub fn for_child(child: &std::process::Child) -> Option<Job> {
            use std::os::windows::io::AsRawHandle;
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                // Let games that need to escape the job do so; never kill on close.
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
                SetInformationJobObject(h, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of_val(&info) as u32);
                if AssignProcessToJobObject(h, child.as_raw_handle() as HANDLE) == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(Job(h))
            }
        }

        pub fn active(&self) -> u32 {
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                let ok = QueryInformationJobObject(self.0, JobObjectBasicAccountingInformation, &mut info as *mut _ as *mut _, std::mem::size_of_val(&info) as u32, std::ptr::null_mut());
                if ok == 0 {
                    0
                } else {
                    info.ActiveProcesses
                }
            }
        }

        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub struct OwnedHandle(pub HANDLE);
    unsafe impl Send for OwnedHandle {}

    impl OwnedHandle {
        pub fn alive(&self) -> bool {
            unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
        }
        pub fn terminate(&self) {
            unsafe {
                TerminateProcess(self.0, 1);
            }
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(not(windows))]
mod job {
    pub struct Job;
    impl Job {
        pub fn for_child(_c: &std::process::Child) -> Option<Job> {
            None
        }
        pub fn active(&self) -> u32 {
            0
        }
        pub fn terminate(&self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewExecutable, NewGame};

    #[test]
    fn splits_like_windows() {
        assert_eq!(split_args(r#"-windowed -res "1920 1080"  ""  x"#), vec!["-windowed", "-res", "1920 1080", "", "x"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn inside_checks_boundaries() {
        assert!(path_inside(Path::new("/g/Game/bin/a.exe"), Path::new("/g/Game")));
        assert!(!path_inside(Path::new("/g/Game2/a.exe"), Path::new("/g/Game")));
        assert!(!path_inside(Path::new("/g/Game"), Path::new("/g/Game")));
        assert!(!path_inside(Path::new("/x"), Path::new("")));
    }

    fn game_with(dir: &Path, exe: &str, wd: &str) -> Game {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .add_game(
                &NewGame {
                    title: "T".into(),
                    install_dir: dir.to_string_lossy().into(),
                    working_dir: wd.into(),
                    launch_args: "-a".into(),
                    executables: vec![NewExecutable { path: exe.into(), args: "-b".into(), ..Default::default() }],
                    ..Default::default()
                },
                "",
            )
            .unwrap();
        db.get_game(id).unwrap()
    }

    #[test]
    fn plan_uses_root_as_working_dir_and_validates() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("bin")).unwrap();
        let exe = d.path().join("bin/game.exe");
        std::fs::write(&exe, b"x").unwrap();
        let p = plan(&game_with(d.path(), &exe.to_string_lossy(), ""), None).unwrap();
        assert_eq!(p.target, Target::Direct { exe: exe.clone(), args: "-a -b".into(), cwd: d.path().to_path_buf() });

        let bad_wd = plan(&game_with(d.path(), &exe.to_string_lossy(), "/definitely/not/here"), None).unwrap_err();
        assert_eq!(bad_wd.code, "missing-dir");
        let missing = plan(&game_with(d.path(), &d.path().join("gone.exe").to_string_lossy(), ""), None).unwrap_err();
        assert_eq!(missing.code, "missing-exe");
    }

    /// End to end on a real process: launch, see it running, see it exit, playtime recorded.
    /// The "game" is a launcher stub that starts the real binary (in the game folder) and exits
    /// at once, so this also covers hand-off detection by folder.
    #[cfg(unix)]
    #[test]
    fn tracks_handoff_and_records_playtime() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Handoff Game");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::copy("/bin/sleep", root.join("bin/realgame")).unwrap();
        let stub = root.join("launcher.sh");
        std::fs::write(&stub, "#!/bin/sh\ncd \"$(dirname \"$0\")\"\n./bin/realgame 4 &\nexit 0\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let id = db
            .add_game(&NewGame { title: "Handoff".into(), install_dir: root.to_string_lossy().into(), executables: vec![NewExecutable { path: stub.to_string_lossy().into(), ..Default::default() }], ..Default::default() }, "")
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let l = Launcher::new(db.clone(), d.path(), move |e| ev.lock().unwrap().push(e));
        let g = db.get_game(id).unwrap();
        // .sh triggers the script warning; the user confirms once.
        assert_eq!(l.launch(&g, None, false, Elevation::Normal).unwrap_err().code, "needs-confirm");
        l.launch(&g, None, true, Elevation::Normal).unwrap();
        assert!(l.is_running(id));
        assert_eq!(l.launch(&g, None, true, Elevation::Normal).unwrap_err().code, "already-running");
        let t = Instant::now();
        while l.is_running(id) && t.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(!l.is_running(id));
        let after = db.get_game(id).unwrap();
        assert!((3..=6).contains(&after.playtime_secs), "playtime {}", after.playtime_secs);
        assert!(after.last_played.is_some());
        let states: Vec<_> = events.lock().unwrap().iter().map(|e| e.state).collect();
        assert_eq!(states, ["started", "ended"]);
        // Second launch: confirmation remembered for the unchanged file.
        l.launch(&db.get_game(id).unwrap(), None, false, Elevation::Normal).unwrap();
        assert!(l.stop(id));
        let t = Instant::now();
        while l.is_running(id) && t.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!l.is_running(id), "stop should end the session");
    }

    #[test]
    fn recovers_sessions_after_crash() {
        let d = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let id = db.add_game(&NewGame { title: "R".into(), ..Default::default() }, "").unwrap();
        std::fs::write(d.path().join("active-sessions.json"), format!("{{\"{id}\": [1000, 121000]}}")).unwrap();
        let _l = Launcher::new(db.clone(), d.path(), |_| {});
        assert_eq!(db.get_game(id).unwrap().playtime_secs, 120);
        assert!(!d.path().join("active-sessions.json").exists());
    }
}
