//! lagless, built in. Nova links the lagless engine as a library and runs it inside its own
//! process: the PC-audio sender, the Mac-mic link, the game stats monitor (FPS, frame times,
//! GPU/CPU) and lagless's local control API. Nova's UI replaces lagless's window entirely.
//!
//! Settings live in lagless's own `settings.json` (`%APPDATA%\Lagless`), so the standalone
//! lagless app and Nova share one configuration. If the standalone app is already running, Nova
//! doesn't start a second engine (two senders would fight over the same receivers); it drives
//! the running app through its control API instead.

use lagless::engine::{Phase, Shared, Status};
use lagless::gui::{Mode, Settings, SourceSel};
use lagless::perf::PerfSnapshot;
use lagless::playout::Volume;
use lagless::sender::{CodecChoice, Source};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub fn lagless_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NOVA_LAGLESS_DIR") {
        return Some(PathBuf::from(p));
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Some(base?.join("Lagless"))
}

fn load_settings() -> Settings {
    let s: Settings = lagless_dir().and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok()).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    s
}

fn save_settings(s: &Settings) {
    if let Some(d) = lagless_dir() {
        let _ = std::fs::create_dir_all(&d);
        let tmp = d.join("settings.json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(s).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, d.join("settings.json"));
        }
    }
}

/// Settings that only change how things look; changing them never restarts the audio.
fn needs_restart(a: &Settings, b: &Settings) -> bool {
    let core = |s: &Settings| {
        let mut s = s.clone();
        s.perf_osd = false;
        s.perf_osd_corner = 0;
        s.perf_osd_style = 0;
        s.perf_osd_size = 0;
        s.perf_osd_bg = 0.0;
        s.perf_osd_text = 0.0;
        s.perf_osd_color = false;
        s.perf_smoothing = 0;
        s.perf_show = false;
        s.volume = 0.0;
        s.excluded_devices.clear();
        s.sync_devices = false;
        s.auto_update = false;
        serde_json::to_string(&s).unwrap_or_default()
    };
    core(a) != core(b)
}

// ------------------------------------------------------------------ engines (as in lagless's gui.rs)

struct Engine {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

type Job = Box<dyn FnOnce(Arc<Shared>) -> anyhow::Result<()> + Send>;

impl Engine {
    fn spawn(name: &str, job: Job, gain: f32) -> Engine {
        let shared = Shared::new();
        shared.volume.set(gain); // before the first sample, so a low setting never blasts
        let sh = shared.clone();
        let handle = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                if let Err(e) = job(sh.clone()) {
                    sh.update(|st| st.phase = Phase::Error(format!("{e:#}")));
                }
            })
            .ok();
        Engine { shared, handle }
    }

    fn start(s: &Settings) -> Engine {
        let s = s.clone();
        let vol = s.volume;
        let job: Job = match s.mode {
            Mode::Receive => Box::new(move |sh| {
                lagless::receiver::run(
                    lagless::receiver::Options {
                        port: lagless::protocol::AUDIO_PORT,
                        device: s.out_device.clone(),
                        min_latency_ms: s.min_latency,
                        max_latency_ms: 120.0,
                        buffer_frames: s.buffer,
                        name: Some(s.rx_name.clone()).filter(|n| !n.trim().is_empty()),
                        null_output: std::env::var_os("LAGLESS_NULL_OUTPUT").is_some(),
                        spike_memory_secs: if s.lowest_latency { 4 } else { 20 },
                    },
                    sh,
                )
            }),
            Mode::Send => Box::new(move |sh| {
                let (source, device) = match &s.source {
                    SourceSel::Loopback(d) => (Source::Loopback, d.clone()),
                    SourceSel::Input(d) => (Source::Input, d.clone()),
                    SourceSel::Tone => (Source::Tone, None),
                };
                lagless::sender::run(
                    lagless::sender::Options {
                        to: Some(s.to.trim().to_string()).filter(|t| !t.is_empty()),
                        port: lagless::protocol::AUDIO_PORT,
                        name: None,
                        codec: s.quality,
                        frame_ms: s.frame_ms,
                        redundancy: s.fec,
                        bitrate_kbps: 320,
                        source,
                        device,
                        sim_loss: 0.0,
                        tone_rate: 48000,
                        multipath: s.multipath,
                        engine_boost: s.engine_boost,
                        voice_focus: false,
                        exclude: s.excluded_devices.clone(),
                        sync: s.sync_devices,
                        follow_system_volume: s.follow_windows_volume,
                    },
                    sh,
                )
            }),
        };
        Engine::spawn("lagless-engine", job, Volume::slider_to_gain(vol))
    }

    /// PC: plays the Mac's mic into a virtual cable that other apps use as their microphone.
    fn start_mic(s: &Settings) -> Result<Option<Engine>, String> {
        let s = s.clone();
        let job: Job = match s.mode {
            Mode::Receive if s.mic_send => Box::new(move |sh| {
                lagless::sender::run(
                    lagless::sender::Options {
                        to: None,
                        port: lagless::protocol::MIC_PORT,
                        name: None,
                        codec: CodecChoice::Auto,
                        frame_ms: 1.0,
                        redundancy: 1,
                        bitrate_kbps: 192,
                        source: Source::Input,
                        device: s.mic_device.clone(),
                        sim_loss: 0.0,
                        tone_rate: 48000,
                        multipath: s.multipath,
                        engine_boost: false,
                        voice_focus: s.voice_focus,
                        exclude: Vec::new(),
                        sync: false,
                        follow_system_volume: false,
                    },
                    sh,
                )
            }),
            Mode::Send if s.mic_receive => {
                let out = match s.mic_out.clone().or_else(find_virtual_cable) {
                    Some(o) => o,
                    None => return Err("no virtual cable".into()),
                };
                if default_output_name().as_deref() == Some(out.as_str()) {
                    return Err(format!("\"{out}\" is this PC's default output. Pick a different default speaker in Windows sound settings."));
                }
                Box::new(move |sh| {
                    lagless::receiver::run(
                        lagless::receiver::Options {
                            port: lagless::protocol::MIC_PORT,
                            device: Some(out),
                            min_latency_ms: 2.0,
                            max_latency_ms: 80.0,
                            buffer_frames: 32,
                            name: Some(format!("{} (mic)", lagless::net::hostname())),
                            null_output: std::env::var_os("LAGLESS_NULL_OUTPUT").is_some(),
                            spike_memory_secs: 4,
                        },
                        sh,
                    )
                })
            }
            _ => return Ok(None),
        };
        Ok(Some(Engine::spawn("lagless-mic", job, 1.0)))
    }

    fn stop(&mut self) {
        self.shared.stop.store(true, Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn device_names(output: bool) -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let list = if output { host.output_devices().map(|d| d.collect::<Vec<_>>()) } else { host.input_devices().map(|d| d.collect::<Vec<_>>()) };
    list.unwrap_or_default().iter().filter_map(|d| d.name().ok()).collect()
}

fn find_virtual_cable() -> Option<String> {
    let names = device_names(true);
    names.iter().find(|n| n.to_lowercase().contains("cable input")).or_else(|| names.iter().find(|n| n.to_lowercase().contains("vb-audio"))).cloned()
}

fn default_output_name() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host().default_output_device()?.name().ok()
}

// ------------------------------------------------------------------ the standalone app, if it's open

struct External {
    port: u16,
    token: String,
    last: Value,
    last_ok: Instant,
}

fn find_external() -> Option<External> {
    let text = std::fs::read_to_string(lagless_dir()?.join("control.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let pid = v.get("pid")?.as_u64()? as u32;
    if pid == std::process::id() {
        return None;
    }
    let mut sys = sysinfo::System::new().ok()?;
    let spid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[spid]), true);
    let p = sys.process(spid)?;
    if !p.name().to_string_lossy().to_lowercase().contains("lagless") {
        return None; // stale file; the pid now belongs to something else
    }
    Some(External { port: v.get("port")?.as_u64()? as u16, token: v.get("token")?.as_str()?.to_string(), last: Value::Null, last_ok: Instant::now() })
}

impl External {
    fn agent(&self) -> ureq::Agent {
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_millis(800))).http_status_as_error(false).build().into()
    }
    fn status(&mut self) -> bool {
        let r = self.agent().get(format!("http://127.0.0.1:{}/v1/status", self.port)).header("Authorization", format!("Bearer {}", self.token)).call();
        match r.ok().and_then(|mut r| r.body_mut().read_json::<Value>().ok()) {
            Some(v) => {
                self.last = v;
                self.last_ok = Instant::now();
                true
            }
            None => false,
        }
    }
    fn post(&self, path: &str, body: Value) -> Result<(), String> {
        self.agent()
            .post(format!("http://127.0.0.1:{}{path}", self.port))
            .header("Authorization", format!("Bearer {}", self.token))
            .send_json(body)
            .map(|_| ())
            .map_err(|e| format!("the lagless app didn't answer: {e}"))
    }
}

// ------------------------------------------------------------------ the hub

struct Inner {
    settings: Settings,
    enabled: bool,
    paused: bool,
    engine: Option<Engine>,
    mic: Option<Engine>,
    mic_problem: Option<String>,
    perf: Option<lagless::perf::Monitor>,
    snap: PerfSnapshot,
    snap_at: Option<Instant>,
    history: VecDeque<f32>,
    fps_shown: f32,
    control: Option<Arc<lagless::control::Control>>,
    external: Option<External>,
    unmute_to: f32,
    levels: [f32; 2],
    last_sound: Instant,
    alert: Option<(String, Instant)>,
}

pub struct Hub {
    inner: Mutex<Inner>,
    emit: Emit,
    on_perf: Box<dyn Fn(f32, f32) + Send + Sync>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PeerView {
    name: String,
    addr: String,
    link: &'static str,
    link_name: String,
    codec: String,
    lossless: bool,
    rtt_ms: f32,
    loss_pct: f32,
    latency_ms: f32,
    sync_ms: f32,
    playing: bool,
    included: bool,
}

fn phase_name(p: &Phase) -> &'static str {
    match p {
        Phase::Starting => "starting",
        Phase::Searching => "searching",
        Phase::Idle => "idle",
        Phase::Streaming => "streaming",
        Phase::Error(_) => "error",
    }
}

impl Hub {
    /// `enabled` comes from Nova's own settings (whether to run lagless inside Nova at all).
    pub fn start(enabled: bool, emit: Emit, on_perf: impl Fn(f32, f32) + Send + Sync + 'static) -> Arc<Hub> {
        let settings = load_settings();
        let external = find_external();
        let hub = Arc::new(Hub {
            inner: Mutex::new(Inner {
                unmute_to: if settings.volume > 0.0 { settings.volume } else { 0.5 },
                settings,
                enabled,
                paused: false,
                engine: None,
                mic: None,
                mic_problem: None,
                perf: None,
                snap: PerfSnapshot::default(),
                snap_at: None,
                history: VecDeque::new(),
                fps_shown: 0.0,
                control: None,
                external,
                levels: [0.0; 2],
                last_sound: Instant::now(),
                alert: None,
            }),
            emit,
            on_perf: Box::new(on_perf),
        });
        {
            let mut i = hub.inner.lock().unwrap();
            Self::bring_up(&mut i);
        }
        let h = hub.clone();
        std::thread::Builder::new().name("lagless-hub".into()).spawn(move || h.run()).ok();
        hub
    }

    fn bring_up(i: &mut Inner) {
        if i.external.is_some() {
            return;
        }
        // Game stats run whenever they're on, even with audio streaming switched off.
        let want_perf = i.settings.mode == Mode::Send && i.settings.perf_stats;
        if want_perf && i.perf.is_none() {
            i.perf = Some(lagless::perf::Monitor::start(|| {}));
        } else if !want_perf {
            i.perf = None;
        }
        if i.enabled && !i.paused {
            if i.engine.is_none() {
                i.engine = Some(Engine::start(&i.settings));
            }
            if i.mic.is_none() {
                match Engine::start_mic(&i.settings) {
                    Ok(m) => {
                        i.mic = m;
                        i.mic_problem = None;
                    }
                    Err(e) => i.mic_problem = Some(e),
                }
            }
            if i.control.is_none() {
                i.control = lagless::control::Control::start(lagless_dir(), || {});
            }
        }
    }

    fn take_down(i: &mut Inner, keep_control: bool) {
        i.mic = None;
        if let Some(mut e) = i.engine.take() {
            e.stop();
        }
        if !keep_control {
            if let Some(c) = i.control.take() {
                c.shutdown();
            }
        }
    }

    fn restart(i: &mut Inner) {
        save_settings(&i.settings);
        Self::take_down(i, true);
        Self::bring_up(i);
    }

    fn run(self: Arc<Self>) {
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let mut last_external_poll = Instant::now() - Duration::from_secs(1);
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let mut perf_out: Option<Value> = None;
            let mut alert: Option<String> = None;
            let mut toast: Option<String> = None;
            {
                let mut i = self.inner.lock().unwrap();
                if let Some(ext) = i.external.as_mut() {
                    if last_external_poll.elapsed() >= Duration::from_secs(1) {
                        last_external_poll = Instant::now();
                        if !ext.status() && ext.last_ok.elapsed() > Duration::from_secs(5) {
                            // The standalone app was closed: take over in-process.
                            i.external = None;
                            Self::bring_up(&mut i);
                            toast = Some("The lagless app was closed; Nova is streaming now.".into());
                        }
                    }
                }
                // Commands from other apps through lagless's control API.
                if let Some(c) = i.control.clone() {
                    for cmd in c.take_commands() {
                        Self::apply_command(&mut i, cmd);
                    }
                }
                if let Some(sh) = i.engine.as_ref().map(|e| e.shared.clone()) {
                    let e = &sh;
                    let lv = e.take_levels();
                    i.levels = [i.levels[0].max(lv[0]), i.levels[1].max(lv[1])];
                    let streaming = e.snapshot().phase == Phase::Streaming;
                    if lv[0].max(lv[1]) > 3e-4 || !streaming {
                        i.last_sound = Instant::now();
                    }
                    if let Some(ev) = e.take_events().pop() {
                        toast = Some(ev);
                    }
                    if let Some(a) = e.take_alerts().pop() {
                        alert = Some(a);
                    }
                }
                if let Some(m) = &i.mic {
                    let _ = m.shared.take_levels();
                    if let Some(ev) = m.shared.take_events().pop() {
                        toast = Some(format!("Mic: {ev}"));
                    }
                }
                if let (Some(c), Some(e)) = (i.control.clone(), i.engine.as_ref()) {
                    let st = lagless::control::AppState {
                        mode: if i.settings.mode == Mode::Send { "send" } else { "receive" },
                        paused: i.paused,
                        volume: i.settings.volume,
                        mic_enabled: if i.settings.mode == Mode::Send { i.settings.mic_receive } else { i.settings.mic_send },
                        mic_problem: i.mic_problem.clone(),
                    };
                    c.publish(st, &e.shared, i.mic.as_ref().map(|m| &m.shared));
                }
                // Game stats: pass to the Mac over the link, keep history for Nova's UI/overlay.
                let fresh = i.perf.as_ref().and_then(|m| m.take_fresh());
                if let Some(s) = fresh {
                    if let Some(e) = &i.engine {
                        e.shared.send_perf(s.to_json());
                    }
                    for p in &s.graph {
                        i.history.push_back(*p);
                    }
                    while i.history.len() > 240 {
                        i.history.pop_front();
                    }
                    let k = match i.settings.perf_smoothing {
                        0 => 0.6,
                        2 => 0.12,
                        _ => 0.3,
                    };
                    i.fps_shown = if i.fps_shown <= 0.0 || s.fps <= 0.0 { s.fps } else { i.fps_shown + (s.fps - i.fps_shown) * k };
                    if !s.game.is_empty() {
                        (self.on_perf)(s.fps, s.low1);
                    }
                    i.snap = s;
                    i.snap_at = Some(Instant::now());
                    perf_out = Some(Self::perf_json(&i));
                }
                if let Some(a) = &alert {
                    i.alert = Some((a.clone(), Instant::now()));
                }
                if i.alert.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(8)) {
                    i.alert = None;
                }
            }
            if let Some(p) = perf_out {
                (self.emit)("perf", p);
            }
            if let Some(a) = alert {
                (self.emit)("lagless-alert", json!({ "text": a }));
            }
            if let Some(t) = toast {
                (self.emit)("lagless-toast", json!({ "text": t }));
            }
            if last_emit.elapsed() >= Duration::from_millis(500) {
                last_emit = Instant::now();
                (self.emit)("audio-state", self.state());
            }
        }
    }

    /// lagless serializes its snapshot in snake_case; Nova's UI speaks camelCase.
    fn camel(v: Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .map(|(k, v)| {
                        let mut out = String::new();
                        let mut up = false;
                        for c in k.chars() {
                            if c == '_' {
                                up = true;
                            } else if up {
                                out.extend(c.to_uppercase());
                                up = false;
                            } else {
                                out.push(c);
                            }
                        }
                        (out, Self::camel(v))
                    })
                    .collect(),
            ),
            other => other,
        }
    }

    fn perf_json(i: &Inner) -> Value {
        let s = &i.snap;
        let live = i.snap_at.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) && !s.game.is_empty() && s.fps > 0.0;
        json!({
            "enabled": i.perf.is_some(),
            "live": live,
            "fpsShown": i.fps_shown,
            "snap": Self::camel(serde_json::to_value(s).unwrap_or(Value::Null)),
            "history": i.history.iter().copied().collect::<Vec<f32>>(),
            "osd": {
                "on": i.settings.perf_osd, "corner": i.settings.perf_osd_corner, "style": i.settings.perf_osd_style,
                "size": i.settings.perf_osd_size, "bg": i.settings.perf_osd_bg, "text": i.settings.perf_osd_text, "color": i.settings.perf_osd_color,
            },
        })
    }

    fn apply_command(i: &mut Inner, cmd: lagless::control::Command) {
        use lagless::control::Command;
        match cmd {
            Command::Volume(v) => Self::set_volume_inner(i, v),
            Command::Mute(m) => Self::set_muted_inner(i, m),
            Command::Mic(on) => {
                match i.settings.mode {
                    Mode::Receive => i.settings.mic_send = on,
                    Mode::Send => i.settings.mic_receive = on,
                }
                Self::restart_mic(i);
            }
            Command::Pause(p) => Self::set_paused_inner(i, p),
        }
    }

    fn restart_mic(i: &mut Inner) {
        save_settings(&i.settings);
        i.mic = None;
        if i.enabled && !i.paused {
            match Engine::start_mic(&i.settings) {
                Ok(m) => {
                    i.mic = m;
                    i.mic_problem = None;
                }
                Err(e) => i.mic_problem = Some(e),
            }
        }
    }

    fn set_volume_inner(i: &mut Inner, v: f32) {
        let v = v.clamp(0.0, 1.0);
        i.settings.volume = v;
        if v > 0.0 {
            i.unmute_to = v;
        }
        if let Some(e) = &i.engine {
            e.shared.volume.set(Volume::slider_to_gain(v));
        }
        save_settings(&i.settings);
    }

    fn set_muted_inner(i: &mut Inner, mute: bool) {
        let muted = i.settings.volume <= 0.0;
        if mute && !muted {
            let keep = i.settings.volume;
            Self::set_volume_inner(i, 0.0);
            i.unmute_to = keep;
        } else if !mute && muted {
            let v = i.unmute_to;
            Self::set_volume_inner(i, v);
        }
    }

    fn set_paused_inner(i: &mut Inner, pause: bool) {
        if pause == i.paused {
            return;
        }
        i.paused = pause;
        if pause {
            Self::take_down(i, true);
        } else {
            Self::bring_up(i);
        }
    }

    // ------------------------------------------------------------------ commands from Nova's UI

    pub fn state(&self) -> Value {
        let mut i = self.inner.lock().unwrap();
        let levels = std::mem::take(&mut i.levels);
        if let Some(ext) = &i.external {
            let v = &ext.last;
            return json!({
                "available": true, "external": true, "enabled": true,
                "paused": v.get("paused").and_then(Value::as_bool).unwrap_or(false),
                "mode": v.get("mode").cloned().unwrap_or(json!("send")),
                "phase": v.get("phase").cloned().unwrap_or(json!("starting")),
                "error": v.get("error").cloned().unwrap_or(Value::Null),
                "latencyMs": v.get("latencyMs").cloned().unwrap_or(json!(0)),
                "peerName": v.get("peer").cloned().unwrap_or(json!("")),
                "link": v.get("link").cloned().unwrap_or(json!("")),
                "linkName": v.get("linkName").cloned().unwrap_or(json!("")),
                "codec": v.get("codec").cloned().unwrap_or(json!("")),
                "sampleRate": v.get("sampleRate").cloned().unwrap_or(json!(0)),
                "lossless": v.get("lossless").cloned().unwrap_or(json!(true)),
                "rttMs": v.get("rttMs").cloned().unwrap_or(json!(0)),
                "lossPct": v.get("lossPct").cloned().unwrap_or(json!(0)),
                "bufferMs": v.get("bufferMs").cloned().unwrap_or(json!(0)),
                "volume": v.get("volume").cloned().unwrap_or(json!(1)),
                "muted": v.get("muted").cloned().unwrap_or(json!(false)),
                "peers": [], "levels": [0, 0], "silent": false,
                "mic": v.get("mic").cloned().unwrap_or(json!({})),
                "syncDevices": false,
                "perf": { "enabled": false, "live": false },
                "alert": Value::Null,
            });
        }
        let st: Status = i.engine.as_ref().map(|e| e.shared.snapshot()).unwrap_or_default();
        let mic: Option<Status> = i.mic.as_ref().map(|m| m.shared.snapshot());
        let phase = if !i.enabled {
            "off"
        } else if i.paused {
            "paused"
        } else {
            phase_name(&st.phase)
        };
        let error = match &st.phase {
            Phase::Error(e) if i.enabled && !i.paused => Some(e.clone()),
            _ => None,
        };
        let mut peers: Vec<PeerView> = st
            .peers
            .iter()
            .map(|p| PeerView {
                name: p.name.clone(),
                addr: p.addr.clone(),
                link: p.link.label(),
                link_name: p.link_name.clone(),
                codec: p.codec.clone(),
                lossless: p.lossless,
                rtt_ms: p.rtt_ms,
                loss_pct: p.loss_pct,
                latency_ms: p.latency_ms,
                sync_ms: p.sync_ms,
                playing: p.playing,
                included: true,
            })
            .collect();
        for name in &i.settings.excluded_devices {
            peers.push(PeerView { name: name.clone(), addr: String::new(), link: "", link_name: String::new(), codec: String::new(), lossless: false, rtt_ms: 0.0, loss_pct: 0.0, latency_ms: 0.0, sync_ms: 0.0, playing: false, included: false });
        }
        let silent = st.phase == Phase::Streaming && i.last_sound.elapsed() > Duration::from_secs(10);
        json!({
            "available": true, "external": false,
            "enabled": i.enabled, "paused": i.paused,
            "mode": if i.settings.mode == Mode::Send { "send" } else { "receive" },
            "phase": phase, "error": error,
            "latencyMs": st.latency_ms, "peerName": st.peer_name, "peerAddr": st.peer_addr,
            "link": if st.link_name.is_empty() { "" } else { st.link.label() }, "linkName": st.link_name,
            "deviceName": st.device_name, "codec": st.codec, "sampleRate": st.sample_rate, "lossless": st.lossless,
            "rttMs": st.rtt_ms, "lossPct": st.loss_pct, "bufferMs": st.buffer_ms, "targetMs": st.target_ms,
            "jitterMs": st.jitter_ms, "fixed": st.fixed, "lost": st.lost, "underruns": st.underruns, "fec": st.fec,
            "engineMs": st.engine_ms, "engineDefaultMs": st.engine_default_ms, "capturing": st.capturing,
            "systemVolume": st.system_volume,
            "peers": peers, "syncDevices": i.settings.sync_devices,
            "volume": i.settings.volume, "muted": i.settings.volume <= 0.0,
            "levels": levels, "silent": silent,
            "mic": {
                "enabled": if i.settings.mode == Mode::Send { i.settings.mic_receive } else { i.settings.mic_send },
                "active": mic.as_ref().is_some_and(|m| m.phase == Phase::Streaming),
                "latencyMs": mic.as_ref().map(|m| m.latency_ms).unwrap_or(0.0),
                "problem": i.mic_problem,
            },
            "perf": Self::perf_json(&i),
            "alert": i.alert.as_ref().map(|(t, _)| t.clone()),
        })
    }

    pub fn set_enabled(&self, on: bool) {
        let mut i = self.inner.lock().unwrap();
        i.enabled = on;
        if on {
            i.paused = false;
            Self::bring_up(&mut i);
        } else {
            Self::take_down(&mut i, false);
        }
    }

    pub fn set_volume(&self, v: f32) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if let Some(ext) = &i.external {
            return ext.post("/v1/volume", json!({ "volume": v.clamp(0.0, 1.0) }));
        }
        Self::set_volume_inner(&mut i, v);
        Ok(())
    }

    pub fn set_muted(&self, m: bool) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if let Some(ext) = &i.external {
            return ext.post("/v1/mute", json!({ "muted": m }));
        }
        Self::set_muted_inner(&mut i, m);
        Ok(())
    }

    pub fn set_paused(&self, p: bool) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if let Some(ext) = &i.external {
            return ext.post("/v1/pause", json!({ "paused": p }));
        }
        Self::set_paused_inner(&mut i, p);
        Ok(())
    }

    pub fn set_mic(&self, on: bool) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if let Some(ext) = &i.external {
            return ext.post("/v1/mic", json!({ "on": on }));
        }
        Self::apply_command(&mut i, lagless::control::Command::Mic(on));
        Ok(())
    }

    /// Include or exclude a receiver live, without interrupting the others.
    pub fn set_device_included(&self, name: &str, include: bool) {
        let mut i = self.inner.lock().unwrap();
        i.settings.excluded_devices.retain(|n| n != name);
        if !include {
            i.settings.excluded_devices.push(name.to_string());
        }
        if let Some(e) = &i.engine {
            *e.shared.exclude.lock().unwrap() = i.settings.excluded_devices.clone();
        }
        save_settings(&i.settings);
    }

    pub fn set_sync(&self, on: bool) {
        let mut i = self.inner.lock().unwrap();
        i.settings.sync_devices = on;
        if let Some(e) = &i.engine {
            e.shared.sync.store(on, Relaxed);
        }
        save_settings(&i.settings);
    }

    pub fn settings(&self) -> Value {
        let i = self.inner.lock().unwrap();
        json!({
            "settings": i.settings,
            "outputs": device_names(true),
            "inputs": device_names(false),
            "cable": find_virtual_cable(),
            "external": i.external.is_some(),
            "version": lagless::update::VERSION,
            "windows": cfg!(windows),
        })
    }

    pub fn apply_settings(&self, new: Value) -> Result<(), String> {
        let new: Settings = serde_json::from_value(new).map_err(|e| format!("invalid settings: {e}"))?;
        let mut i = self.inner.lock().unwrap();
        if i.external.is_some() {
            return Err("Close the lagless app to change its settings from Nova.".into());
        }
        let restart = needs_restart(&i.settings, &new);
        let mic_changed = i.settings.mic_receive != new.mic_receive || i.settings.mic_out != new.mic_out || i.settings.mic_send != new.mic_send;
        i.settings = new;
        if restart {
            Self::restart(&mut i);
        } else {
            save_settings(&i.settings);
            if mic_changed {
                Self::restart_mic(&mut i);
            }
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        let mut i = self.inner.lock().unwrap();
        i.perf = None;
        Self::take_down(&mut i, false);
    }
}

/// One admin prompt that lets Windows hand frame timing to non-admin apps (lagless's
/// "Allow the FPS counter"). Only ever run when the user clicks the button for it.
pub fn allow_fps_counter() -> Result<(), String> {
    #[cfg(windows)]
    {
        lagless::perf::allow_fps_counter().map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        Err("only needed on Windows".into())
    }
}

/// Replace the first-run firewall rule ("Private only") with one allowing every network.
pub fn open_firewall() -> Result<(), String> {
    #[cfg(windows)]
    {
        lagless::net::open_firewall().map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        Err("only needed on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_changes_dont_restart_audio() {
        let a = Settings::default();
        let mut b = a.clone();
        b.perf_osd = true;
        b.volume = 0.2;
        b.excluded_devices.push("Mac".into());
        assert!(!needs_restart(&a, &b));
        b.fec = 3;
        assert!(needs_restart(&a, &b));
    }

    #[test]
    fn perf_snapshot_keys_are_camel_case() {
        let v = Hub::camel(serde_json::to_value(PerfSnapshot::default()).unwrap());
        for k in ["frameMs", "gpuUtil", "low01", "cpuCoreMax", "fpsProblem"] {
            assert!(v.get(k).is_some(), "{k} missing in {v}");
        }
    }

    #[test]
    fn shares_lagless_settings_file() {
        let d = tempfile::tempdir().unwrap();
        std::env::set_var("NOVA_LAGLESS_DIR", d.path());
        let mut s = Settings::default();
        s.volume = 0.37;
        s.excluded_devices = vec!["Living room".into()];
        save_settings(&s);
        let back = load_settings();
        std::env::remove_var("NOVA_LAGLESS_DIR");
        assert_eq!(back.volume, 0.37);
        assert_eq!(back.excluded_devices, ["Living room"]);
    }
}
