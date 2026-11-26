import { useEffect, useRef, useState } from "react";
import { api, errText } from "../api";
import { fail, set, toast, useStore } from "../store";
import type { AudioSettingsInfo, AudioState, LaglessSettings, Perf } from "../types";
import { Seg, Slider, Switch } from "../components/ui";
import { laglessColor, laglessLine } from "../App";
import * as I from "../components/Icons";

function FrameGraph({ points, height = 72 }: { points: number[]; height?: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    c.width = w * dpr;
    c.height = height * dpr;
    const g = c.getContext("2d")!;
    g.scale(dpr, dpr);
    g.clearRect(0, 0, w, height);
    const pts = points.slice(-160);
    if (pts.length < 2) return;
    const sorted = [...pts].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)] || 16;
    const top = Math.max(median * 2.4, ...pts.map((p) => Math.min(p, median * 4)));
    const step = w / (pts.length - 1);
    g.strokeStyle = "rgba(255,255,255,0.06)";
    g.lineWidth = 1;
    g.beginPath();
    g.moveTo(0, height - (median / top) * height);
    g.lineTo(w, height - (median / top) * height);
    g.stroke();
    const grad = g.createLinearGradient(0, 0, 0, height);
    grad.addColorStop(0, "rgba(154,168,255,0.35)");
    grad.addColorStop(1, "rgba(154,168,255,0)");
    g.beginPath();
    pts.forEach((p, i) => {
      const y = height - (Math.min(p, top) / top) * (height - 4);
      i ? g.lineTo(i * step, y) : g.moveTo(0, y);
    });
    g.lineTo(w, height);
    g.lineTo(0, height);
    g.fillStyle = grad;
    g.fill();
    g.beginPath();
    pts.forEach((p, i) => {
      const y = height - (Math.min(p, top) / top) * (height - 4);
      i ? g.lineTo(i * step, y) : g.moveTo(0, y);
    });
    g.strokeStyle = "rgba(205,212,255,0.9)";
    g.lineWidth = 1.5;
    g.stroke();
    // Stutters: frames far above the median.
    pts.forEach((p, i) => {
      if (p > median * 2.5 && p > 25) {
        g.fillStyle = "#ff6f66";
        g.beginPath();
        g.arc(i * step, height - (Math.min(p, top) / top) * (height - 4), 2.5, 0, Math.PI * 2);
        g.fill();
      }
    });
  }, [points, height]);
  return <canvas ref={ref} className="graph" style={{ height }} />;
}

function verdictColor(v: string) {
  const l = v.toLowerCase();
  if (l.includes("gpu")) return "var(--good)";
  if (l.includes("cpu")) return "var(--warn)";
  if (l.includes("cap")) return "var(--accent)";
  return "var(--dim)";
}

/** Game stats from lagless's perf monitor (ETW frame timing, NVML, sysinfo). */
export function PerfCard({ perf, title = "Game stats" }: { perf: Perf; title?: string }) {
  const s = perf.snap;
  const [asking, setAsking] = useState(false);
  if (!s) return null;
  const needsPermission = s.fpsProblem === "needs permission";
  const fmt = (v: number | null | undefined, u: string, d = 0) => (v === null || v === undefined ? "—" : `${v.toFixed(d)}${u}`);
  return (
    <div className="card" style={{ padding: 20 }}>
      <div className="row" style={{ marginBottom: 14 }}>
        <I.IconGauge size={16} className="dim" />
        <span style={{ fontWeight: 600 }}>{title}</span>
        {perf.live && s.game && <span className="pill good" style={{ marginLeft: "auto" }}><span className="dot live" /> {s.game}</span>}
      </div>
      {perf.live ? (
        <>
          <div className="row" style={{ alignItems: "flex-end", gap: 28 }}>
            <div>
              <div className="fps-big display">{Math.round(perf.fpsShown || s.fps)}</div>
              <div className="faint" style={{ fontSize: 11.5, fontWeight: 600, letterSpacing: "0.06em" }}>FPS</div>
            </div>
            <div className="col" style={{ gap: 2 }}>
              <span className="mono">{Math.round(s.low1)} <span className="faint">1% low</span></span>
              <span className="mono">{Math.round(s.low01)} <span className="faint">0.1% low</span></span>
              <span className="mono">{s.frameMs.toFixed(1)} ms <span className="faint">frame</span></span>
            </div>
            <div className="grow" />
            {s.stutters > 0 && <span className="pill warn">{s.stutters} stutter{s.stutters === 1 ? "" : "s"}</span>}
          </div>
          <div style={{ marginTop: 14 }}><FrameGraph points={perf.history ?? []} /></div>
          {s.verdict && (
            <div className="row" style={{ marginTop: 12, alignItems: "flex-start" }}>
              <span className="dot" style={{ color: verdictColor(s.verdict), marginTop: 6 }} />
              <div><div style={{ fontWeight: 600 }}>{s.verdict}</div>{s.tip && <div className="dim" style={{ fontSize: 12.5 }}>{s.tip}</div>}</div>
            </div>
          )}
          {s.lastStutter && <div className="faint" style={{ fontSize: 12.5, marginTop: 8 }}>Last stutter: {s.lastStutter}</div>}
        </>
      ) : needsPermission ? (
        <div className="col" style={{ gap: 10 }}>
          <div className="dim">Windows only lets administrators read frame timing. One prompt fixes it for good (then sign out of Windows and back in once).</div>
          <div><button className="btn sm" disabled={asking} onClick={async () => { setAsking(true); try { await api.allowFps(); toast("Done. Sign out of Windows and back in once to finish.", "good"); } catch (e) { fail(e); } setAsking(false); }}><I.IconShield size={14} /> Allow the FPS counter</button></div>
        </div>
      ) : (
        <div className="dim">{s.fpsProblem || "No game running. Start one in borderless or windowed mode and its frame rate shows up here and on your Mac."}</div>
      )}
      <div className="kv" style={{ marginTop: 16, gridTemplateColumns: "90px 1fr" }}>
        {s.gpuName && <><div className="k">GPU</div><div className="v" style={{ wordBreak: "normal" }}>{fmt(s.gpuUtil, "%")} · {fmt(s.gpuTemp, "°C")} · {fmt(s.gpuClockMhz, " MHz")}{s.vramUsedGb != null ? ` · ${s.vramUsedGb.toFixed(1)}/${s.vramTotalGb?.toFixed(0)} GB` : ""}{s.gpuLimit ? <span style={{ color: "var(--warn)" }}> · {s.gpuLimit}</span> : null}</div></>}
        <div className="k">CPU</div><div className="v" style={{ wordBreak: "normal" }}>{s.cpu.toFixed(0)}% · busiest core {s.cpuCoreMax.toFixed(0)}%</div>
        <div className="k">Memory</div><div className="v" style={{ wordBreak: "normal" }}>{s.ramUsedGb.toFixed(1)} / {s.ramTotalGb.toFixed(0)} GB</div>
      </div>
    </div>
  );
}

function Row({ icon, title, sub, children }: { icon?: React.ReactNode; title: React.ReactNode; sub?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="set-row">
      {icon && <span className="dim" style={{ display: "grid" }}>{icon}</span>}
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="t">{title}</div>
        {sub && <div className="d">{sub}</div>}
      </div>
      {children}
    </div>
  );
}

function deviceIcon(name: string) {
  const n = name.toLowerCase();
  if (n.includes("iphone") || n.includes("pixel") || n.includes("phone") || n.includes("galaxy") || n.includes("android")) return <I.IconPhone size={17} />;
  return <I.IconLaptop size={17} />;
}

function Hero({ a }: { a: AudioState }) {
  const [vol, setVol] = useState(a.volume);
  const dragging = useRef(false);
  useEffect(() => {
    if (!dragging.current) setVol(a.volume);
  }, [a.volume]);
  const color = laglessColor(a);
  const glow = a.phase === "streaming" ? (a.latencyMs > 45 ? "rgba(255,111,102,0.18)" : a.latencyMs > 25 ? "rgba(241,179,86,0.16)" : "rgba(91,211,137,0.16)") : "rgba(154,168,255,0.08)";
  const level = Math.min(1, Math.max(a.levels?.[0] ?? 0, a.levels?.[1] ?? 0) ** 0.5);
  return (
    <div className="audio-hero card" style={{ ["--glow" as string]: glow }}>
      <div className="row" style={{ position: "relative", alignItems: "flex-start" }}>
        <div className="grow">
          <div className="row" style={{ gap: 8 }}>
            <span className="dot" style={{ color }} />
            <span className="dim" style={{ fontSize: 12, fontWeight: 600, letterSpacing: "0.06em" }}>{a.mode === "send" ? "STREAMING THIS PC'S AUDIO" : "RECEIVING"}</span>
          </div>
          <div className="big-num display" style={{ marginTop: 14 }}>
            {a.phase === "streaming" && a.latencyMs > 0 ? <>{Math.round(a.latencyMs)}<small>ms</small></> : <span style={{ fontSize: 44, color: "var(--dim)" }}>{a.phase === "off" ? "Off" : a.phase === "paused" ? "Paused" : a.phase === "error" ? "Problem" : "—"}</span>}
          </div>
          <div className="dim" style={{ marginTop: 10, fontSize: 14 }}>{laglessLine(a)}</div>
          {a.silent && <div style={{ color: "var(--warn)", fontSize: 12.5, marginTop: 6 }}>Streaming silence. Check that the right playback device is captured (settings below).</div>}
          {a.external && <div className="pill accent" style={{ marginTop: 10 }}><I.IconLink size={12} /> Controlling the lagless app</div>}
        </div>
        <div className="col" style={{ alignItems: "flex-end" }}>
          {!a.external && (
            <div className="row">
              <span className="dim" style={{ fontSize: 12.5 }}>Stream audio</span>
              <Switch label="Stream audio" on={a.enabled} onChange={async (on) => set({ settings: await api.setSettings({ laglessEnabled: on }) })} />
            </div>
          )}
          {a.enabled && a.phase !== "off" && (
            <button className="btn sm" onClick={() => api.audioPaused(!a.paused).catch(fail)}>{a.paused ? <><I.IconPlay size={11} /> Resume</> : "Pause"}</button>
          )}
        </div>
      </div>
      {a.enabled && (
        <div className="row" style={{ marginTop: 28, gap: 14, position: "relative" }}>
          <button className="btn ghost icon" aria-label={a.muted ? "Unmute" : "Mute"} onClick={() => api.audioMuted(!a.muted).catch(fail)}>{a.muted ? <I.IconMute size={18} /> : <I.IconVolume size={18} />}</button>
          <div className="grow">
            <Slider
              label="Stream volume"
              value={vol}
              onChange={(v) => {
                dragging.current = true;
                setVol(v);
                api.audioVolume(v).catch(() => {});
              }}
              onCommit={() => (dragging.current = false)}
            />
            <div className="meter" style={{ marginTop: 6 }}><div style={{ width: `${level * 100}%` }} /></div>
          </div>
          <span className="mono dim" style={{ width: 44, textAlign: "right" }}>{Math.round(vol * 100)}%</span>
        </div>
      )}
    </div>
  );
}

function Settings({ info, onChange }: { info: AudioSettingsInfo; onChange: (s: LaglessSettings) => void }) {
  const s = info.settings;
  const [busy, setBusy] = useState(false);
  const put = async (patch: Partial<LaglessSettings>) => {
    const next = { ...s, ...patch } as LaglessSettings;
    onChange(next);
    setBusy(true);
    try {
      await api.audioApply(next);
    } catch (e) {
      toast(errText(e), "bad");
    }
    setBusy(false);
  };
  const src = typeof s.source === "string" ? "Tone" : "Loopback" in s.source ? `out:${s.source.Loopback ?? ""}` : `in:${s.source.Input ?? ""}`;
  return (
    <div className="settings-grid">
      <div className="card">
        <Row title="Sound to stream" sub="What lagless captures and sends">
          <select className="select" style={{ width: 230 }} value={src} disabled={busy || info.external} onChange={(e) => {
            const v = e.target.value;
            put({ source: v === "Tone" ? "Tone" : v.startsWith("out:") ? { Loopback: v.slice(4) || null } : { Input: v.slice(3) || null } });
          }}>
            <option value="out:">Everything this PC plays</option>
            {info.outputs.map((o) => <option key={"o" + o} value={`out:${o}`}>Playback: {o}</option>)}
            {info.inputs.map((o) => <option key={"i" + o} value={`in:${o}`}>Input: {o}</option>)}
            <option value="Tone">Test tone</option>
          </select>
        </Row>
        <Row title="Quality" sub="Auto stays lossless and drops to Opus only if the link loses audio">
          <Seg value={s.quality} options={[["Auto", "Auto"], ["Pcm24", "24-bit"], ["Pcm16", "16-bit"], ["Opus", "Opus"]]} onChange={(v) => put({ quality: v })} />
        </Row>
        <Row title="Loss protection" sub="Copies of earlier frames in each packet; repairs drops with no added delay">
          <Seg value={s.fec} options={[[0, "Off"], [1, "1"], [2, "2"], [3, "3"]]} onChange={(v) => put({ fec: v })} />
        </Row>
        <Row title="Use two links at once" sub="Send over Wi-Fi and cable together when both are connected"><Switch on={s.multipath} onChange={(v) => put({ multipath: v })} /></Row>
        <Row title="Low-latency Windows audio" sub="Runs the Windows audio engine at its shortest period (about 2.7 ms instead of 10)"><Switch on={s.engine_boost} onChange={(v) => put({ engine_boost: v })} /></Row>
        <Row title="Follow the Windows volume" sub="The volume keys and mute also set the stream's level"><Switch on={s.follow_windows_volume} onChange={(v) => put({ follow_windows_volume: v })} /></Row>
        <Row title="Receiver address" sub="Optional. Leave empty to find receivers automatically">
          <input className="input" style={{ width: 180 }} defaultValue={s.to} placeholder="Automatic" onBlur={(e) => e.target.value !== s.to && put({ to: e.target.value })} spellCheck={false} />
        </Row>
        {info.windows && (
          <Row title="Firewall" sub="If devices on public networks can't connect, allow lagless on every network (one admin prompt)">
            <button className="btn sm" onClick={async () => { try { await api.openFirewall(); toast("Firewall rule updated", "good"); } catch (e) { fail(e); } }}><I.IconShield size={13} /> Allow</button>
          </Row>
        )}
      </div>
      <div className="col" style={{ gap: 20 }}>
        <div className="card">
          <Row title="Game stats" sub="FPS, frame times, GPU and CPU while you play; also shown on your Mac"><Switch on={s.perf_stats} onChange={(v) => put({ perf_stats: v })} /></Row>
          <Row title="FPS readout" sub="How calm the number is">
            <Seg value={s.perf_smoothing} options={[[0, "Responsive"], [1, "Balanced"], [2, "Smooth"]]} onChange={(v) => put({ perf_smoothing: v })} />
          </Row>
        </div>
        <div className="card">
          <Row title="On-screen FPS counter" sub="Click-through, over borderless and windowed games"><Switch on={s.perf_osd} onChange={(v) => put({ perf_osd: v })} /></Row>
          {s.perf_osd && (
            <>
              <Row title="Style"><Seg value={s.perf_osd_style} options={[[0, "Full"], [1, "Compact"], [2, "FPS only"]]} onChange={(v) => put({ perf_osd_style: v })} /></Row>
              <Row title="Size"><Seg value={s.perf_osd_size} options={[[0, "S"], [1, "M"], [2, "L"]]} onChange={(v) => put({ perf_osd_size: v })} /></Row>
              <Row title="Corner"><Seg value={s.perf_osd_corner} options={[[0, "↖"], [1, "↗"], [2, "↙"], [3, "↘"]]} onChange={(v) => put({ perf_osd_corner: v })} /></Row>
              <Row title="Background"><div style={{ width: 160 }}><Slider value={s.perf_osd_bg} onChange={(v) => onChange({ ...s, perf_osd_bg: v })} onCommit={(v) => put({ perf_osd_bg: v })} /></div></Row>
              <Row title="Color-coded FPS"><Switch on={s.perf_osd_color} onChange={(v) => put({ perf_osd_color: v })} /></Row>
            </>
          )}
        </div>
        <div className="faint" style={{ fontSize: 12 }}>lagless {info.version} engine, running inside Nova. Settings are shared with the standalone lagless app.</div>
      </div>
    </div>
  );
}

export function AudioView() {
  const a = useStore((s) => s.audio);
  const [info, setInfo] = useState<AudioSettingsInfo | null>(null);
  const [details, setDetails] = useState(false);
  useEffect(() => {
    api.audioSettings().then(setInfo).catch(fail);
  }, [a?.external]);
  if (!a) return <div className="page"><div className="spinner lg" /></div>;
  const peers = a.peers ?? [];
  const mic = a.mic ?? {};
  return (
    <div className="page">
      <div className="lib-head">
        <h1>Audio</h1>
        <span className="count">powered by lagless</span>
        <div className="grow" />
        {!a.external && (
          <button className="btn sm" title="Closes the library window and its browser processes; lagless keeps running from the tray" onClick={() => api.laglessOnly().catch(fail)}>
            Switch to lagless only
          </button>
        )}
      </div>
      <Hero a={a} />
      {a.error && a.phase === "error" && <div className="callout bad" style={{ marginTop: 16 }}><I.IconAlert size={16} /><span className="selectable">{a.error}</span></div>}

      <div className="settings-grid" style={{ marginTop: 20 }}>
        <div className="col" style={{ gap: 20 }}>
          <div className="card">
            <div className="set-row" style={{ minHeight: 44 }}><span className="t">Devices</span><span className="faint" style={{ marginLeft: "auto", fontSize: 12 }}>{peers.filter((p) => p.included).length || ""}</span></div>
            {peers.length === 0 && <Row icon={<I.IconLaptop size={17} />} title={<span className="dim">Looking for devices…</span>} sub="Every Mac, phone or tablet running lagless shows up here" />}
            {peers.map((p) => (
              <Row
                key={p.name + p.addr}
                icon={deviceIcon(p.name)}
                title={<span style={{ color: p.included ? undefined : "var(--dim)" }}>{p.name}</span>}
                sub={!p.included ? "Not streaming · switch on to add" : p.playing ? `${p.link} · ${p.lossless ? "Lossless" : "Opus"} · ${p.latencyMs > 0 ? `${Math.round(p.latencyMs)} ms` : "…"}${p.syncMs > 0.5 ? ` · +${Math.round(p.syncMs)} ms sync` : ""}` : `${p.link} · connecting…`}
              >
                <Switch label={`Stream to ${p.name}`} on={p.included} disabled={a.external} onChange={(v) => api.audioDevice(p.name, v).catch(fail)} />
              </Row>
            ))}
            {peers.length >= 2 && (
              <Row icon={<I.IconSync size={17} />} title="Play in sync" sub="Speakers in one room won't echo; faster devices wait a little">
                <Switch on={a.syncDevices} disabled={a.external} onChange={(v) => api.audioSync(v).catch(fail)} />
              </Row>
            )}
          </div>
          {a.mode === "send" && (
            <div className="card">
              <Row icon={<I.IconMic size={17} />} title="Use the Mac's mic on this PC" sub={mic.problem === "no virtual cable" ? "Needs VB-Audio Virtual Cable (free). Then pick “CABLE Output” as the mic in Discord or your game." : mic.problem ? mic.problem : mic.active ? `Live · ${Math.round(mic.latencyMs ?? 0)} ms` : mic.enabled ? "Waiting for the Mac to send its mic…" : "Plays it into a virtual cable that apps use as a microphone"}>
                <Switch on={!!mic.enabled} onChange={(v) => api.audioMic(v).catch(fail)} />
              </Row>
            </div>
          )}
          <div className="card">
            <button className="set-row" style={{ width: "100%", background: "none", border: 0, cursor: "pointer", textAlign: "left" }} onClick={() => setDetails(!details)}>
              <span className="t grow">Details</span>{details ? <I.IconChevronDown size={15} /> : <I.IconChevronRight size={15} />}
            </button>
            {details && (
              <div style={{ padding: "4px 18px 18px" }}>
                <div className="kv">
                  <div className="k">Link</div><div className="v">{a.linkName || "—"} {a.link && <span className="faint">({a.link})</span>}</div>
                  <div className="k">Format</div><div className="v">{a.codec || "—"}{a.sampleRate ? ` · ${(a.sampleRate / 1000).toFixed(1)} kHz` : ""}</div>
                  <div className="k">Round trip</div><div className="v">{a.rttMs ? `${a.rttMs.toFixed(2)} ms` : "—"}</div>
                  <div className="k">Buffer</div><div className="v">{a.bufferMs ? `${a.bufferMs.toFixed(1)} ms` : "—"}{a.targetMs ? ` (target ${a.targetMs.toFixed(1)})` : ""}</div>
                  <div className="k">Loss</div><div className="v">{(a.lossPct ?? 0).toFixed(1)}% · {a.fixed ?? 0} repaired · {a.lost ?? 0} lost · {a.underruns ?? 0} rebuffers</div>
                  {a.engineMs ? <><div className="k">Windows audio</div><div className="v">{a.engineMs.toFixed(1)} ms period (default {a.engineDefaultMs?.toFixed(1)} ms)</div></> : null}
                  {a.deviceName && <><div className="k">Capturing</div><div className="v">{a.deviceName}</div></>}
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="col" style={{ gap: 20 }}>{a.perf?.snap ? <PerfCard perf={a.perf} /> : <div className="card" style={{ padding: 20 }}><div className="row"><I.IconGauge size={16} className="dim" /><span style={{ fontWeight: 600 }}>Game stats</span></div><div className="dim" style={{ marginTop: 10 }}>{a.perf?.enabled ? "Waiting for the first reading…" : "Off. Turn on Game stats below."}</div></div>}</div>
      </div>

      <div className="section">
        <div className="section-head"><h2>Settings</h2></div>
        {info ? <Settings info={info} onChange={(settings) => setInfo({ ...info, settings })} /> : <div className="spinner" />}
        {info?.external && <div className="callout info" style={{ marginTop: 14 }}><I.IconInfo size={16} /><span>The standalone lagless app is open, so it's doing the streaming. Close it and Nova takes over automatically; then these settings can be changed here.</span></div>}
      </div>
    </div>
  );
}
