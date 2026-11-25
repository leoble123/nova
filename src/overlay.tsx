import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import type { Perf } from "./types";
import "./styles.css";

/** The FPS counter window: transparent, click-through, always on top of borderless games. */
function Osd() {
  const [p, setP] = useState<Perf | null>(null);
  useEffect(() => {
    const un = listen<Perf>("perf", (e) => setP(e.payload));
    return () => void un.then((f) => f());
  }, []);
  if (!p?.live || !p.snap || !p.osd) return null;
  const o = p.osd;
  const s = p.snap;
  const fps = Math.round(p.fpsShown || s.fps);
  const color = o.color ? (fps >= 100 ? "#7ee2a5" : fps >= 55 ? "#f2f3f6" : fps >= 30 ? "#f6c46f" : "#ff8a82") : "#fff";
  const scale = [0.85, 1, 1.3][o.size] ?? 1;
  const style = { ["--ba" as string]: o.bg, ["--ta" as string]: o.text, transform: `scale(${scale})` };
  if (o.style === 2) return <div className="osd" style={style}><div className="fps" style={{ color }}>{fps}</div></div>;
  if (o.style === 1)
    return (
      <div className="osd" style={{ ...style, flexDirection: "row", alignItems: "baseline", gap: 12, whiteSpace: "nowrap" }}>
        <span className="fps" style={{ color, fontSize: 20 }}>{fps}</span>
        <span className="small">FPS · 1% {Math.round(s.low1)} · {s.frameMs.toFixed(1)} ms{s.gpuUtil != null ? ` · GPU ${Math.round(s.gpuUtil)}%` : ""} · CPU {Math.round(s.cpu)}%</span>
      </div>
    );
  return (
    <div className="osd" style={style}>
      <div className="row" style={{ alignItems: "baseline", gap: 8 }}><span className="fps" style={{ color }}>{fps}</span><span className="small">FPS</span></div>
      <div className="small">1% low {Math.round(s.low1)} · 0.1% {Math.round(s.low01)} · {s.frameMs.toFixed(1)} ms</div>
      {s.gpuUtil != null && <div className="small">GPU {Math.round(s.gpuUtil)}%{s.gpuTemp != null ? ` · ${Math.round(s.gpuTemp)}°C` : ""}{s.vramUsedGb != null ? ` · ${s.vramUsedGb.toFixed(1)} GB` : ""}</div>}
      <div className="small">CPU {Math.round(s.cpu)}% · core {Math.round(s.cpuCoreMax)}%</div>
      {s.verdict && <div className="small" style={{ color: "rgba(255,255,255,0.8)" }}>{s.verdict}</div>}
    </div>
  );
}

/** Room alerts from lagless on the Mac (doorbell, knocking, alarm…): shown over the game. */
function Alert() {
  const [text, setText] = useState("");
  useEffect(() => {
    const un = listen<{ text: string }>("lagless-alert", (e) => {
      setText(e.payload.text);
      setTimeout(() => setText(""), 6500);
    });
    return () => void un.then((f) => f());
  }, []);
  if (!text) return null;
  return (
    <div className="alert-banner">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="#f1b356" strokeWidth="2" strokeLinecap="round"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0" /></svg>
      {text}
    </div>
  );
}

document.body.style.background = "transparent";
createRoot(document.getElementById("root")!).render(location.hash === "#alert" ? <Alert /> : <Osd />);
