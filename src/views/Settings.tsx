import { useEffect, useState } from "react";
import { api } from "../api";
import { fail, go, refresh, set, toast, useStore } from "../store";
import { restartToUpdate } from "../App";
import type { NovaSettings, StoreReport } from "../types";
import { Seg, Switch } from "../components/ui";
import * as I from "../components/Icons";

function Row({ title, sub, children }: { title: string; sub?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="set-row">
      <div className="grow">
        <div className="t">{title}</div>
        {sub && <div className="d">{sub}</div>}
      </div>
      {children}
    </div>
  );
}

function UpdateRow() {
  const u = useStore((x) => x.update);
  const [checking, setChecking] = useState(false);
  const st = u?.status;
  const sub =
    !st || st.state === "idle" ? "Not checked yet"
    : st.state === "checking" ? "Checking…"
    : st.state === "upToDate" ? `Up to date · checked ${new Date(st.checkedAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}`
    : st.state === "downloading" ? `Downloading ${st.version}… ${Math.round(st.progress * 100)}%`
    : st.state === "ready" ? `Nova ${st.version} is downloaded and verified${st.notes ? ` · ${st.notes}` : ""}`
    : `Couldn't update: ${st.message}`;
  return (
    <Row title={`Nova ${u?.current ?? ""}`} sub={<span style={{ color: st?.state === "error" ? "var(--warn)" : undefined }}>{sub}</span>}>
      {st?.state === "ready" ? (
        <button className="btn sm accent" onClick={restartToUpdate}>Restart to update</button>
      ) : (
        <button className="btn sm" disabled={checking || st?.state === "checking" || st?.state === "downloading"} onClick={async () => {
          setChecking(true);
          try {
            set({ update: await api.updateCheck() });
          } catch (e) {
            fail(e);
          }
          setChecking(false);
        }}>{checking ? <span className="spinner" /> : <I.IconRefresh size={14} />} Check now</button>
      )}
    </Row>
  );
}

export function SettingsView() {
  const s = useStore((x) => x.settings);
  const games = useStore((x) => x.games);
  const [info, setInfo] = useState<{ version: string; dataDir: string; laglessVersion: string } | null>(null);
  const [reports, setReports] = useState<StoreReport[] | null>(null);
  const [scanning, setScanning] = useState(false);
  useEffect(() => {
    api.appInfo().then(setInfo).catch(fail);
  }, []);
  if (!s) return null;
  const put = async (p: Partial<NovaSettings>) => {
    try {
      set({ settings: await api.setSettings(p) });
    } catch (e) {
      fail(e);
    }
  };
  const scan = async () => {
    setScanning(true);
    try {
      const r = await api.scanStores();
      setReports(r.reports);
      await refresh();
      toast(r.added ? `Added ${r.added} game${r.added === 1 ? "" : "s"}` : "Library is up to date", r.added ? "good" : "info");
    } catch (e) {
      fail(e);
    }
    setScanning(false);
  };
  const hidden = games.filter((g) => g.hidden).length;
  return (
    <div className="page" style={{ maxWidth: 880 }}>
      <div className="lib-head"><h1>Settings</h1></div>

      <div className="section" style={{ marginTop: 6 }}>
        <div className="section-head"><h2>Library</h2></div>
        <div className="card">
          <Row title="Find store games at startup" sub="Reads Steam, Epic Games and GOG manifests on this PC. Read-only and offline.">
            <Switch on={s.scanStoresOnStart} onChange={(v) => put({ scanStoresOnStart: v })} />
          </Row>
          <Row title="Scan store libraries now" sub={reports ? reports.map((r) => `${r.store}: ${r.detected ? `${r.found} found` : "not found"}`).join(" · ") : "Adds newly installed games; keeps your edits to existing ones"}>
            <button className="btn sm" onClick={scan} disabled={scanning}>{scanning ? <span className="spinner" /> : <I.IconRefresh size={14} />} Scan</button>
          </Row>
          <Row title="Show hidden games" sub={hidden ? `${hidden} hidden` : "No hidden games"}>
            <Switch on={s.showHidden} onChange={(v) => put({ showHidden: v })} />
          </Row>
          <Row title="Minimize Nova when a game starts">
            <Switch on={s.minimizeOnLaunch} onChange={(v) => put({ minimizeOnLaunch: v })} />
          </Row>
        </div>
      </div>

      <div className="section">
        <div className="section-head"><h2>Appearance</h2></div>
        <div className="card">
          <Row title="Cover size"><Seg value={s.cardSize} options={[[0, "Small"], [1, "Medium"], [2, "Large"]]} onChange={(v) => put({ cardSize: v })} /></Row>
          <Row title="Reduce motion" sub="Turns off animations and transitions"><Switch on={s.reduceMotion} onChange={(v) => put({ reduceMotion: v })} /></Row>
        </div>
      </div>

      <div className="section">
        <div className="section-head"><h2>Artwork</h2></div>
        <div className="card">
          <Row title="Download missing Steam artwork and info" sub="Off by default. When on, “Download artwork & info from Steam” appears in a Steam game's menu. Everything else works offline, using Steam's own cached art and images you choose.">
            <Switch on={s.onlineArt} onChange={(v) => put({ onlineArt: v })} />
          </Row>
        </div>
      </div>

      <div className="section">
        <div className="section-head"><h2>Audio streaming</h2></div>
        <div className="card">
          <Row title="Run lagless inside Nova" sub="Streams this PC's sound to your Mac or phone with the lowest latency the link allows, and measures your games' FPS.">
            <Switch on={s.laglessEnabled} onChange={(v) => put({ laglessEnabled: v })} />
          </Row>
          <Row title="Audio settings and devices"><button className="btn sm" onClick={() => go({ name: "audio" })}>Open <I.IconChevronRight size={13} /></button></Row>
        </div>
      </div>

      <div className="section">
        <div className="section-head"><h2>Safety</h2></div>
        <div className="card" style={{ padding: 18 }}>
          <div className="col dim" style={{ gap: 8, lineHeight: 1.55 }}>
            <div className="row" style={{ alignItems: "flex-start" }}><I.IconShield size={16} style={{ marginTop: 2, flex: "none" }} /><span>Imported folders are treated as untrusted. Scanning only reads file headers and never runs anything.</span></div>
            <div className="row" style={{ alignItems: "flex-start" }}><I.IconShield size={16} style={{ marginTop: 2, flex: "none" }} /><span>Games start with your normal rights. Administrator rights are only requested when you choose “Run as administrator” for a launch.</span></div>
            <div className="row" style={{ alignItems: "flex-start" }}><I.IconShield size={16} style={{ marginTop: 2, flex: "none" }} /><span>Disguised files, scripts, invalid signatures and programs that ask for admin rights are flagged before they run. Windows Defender and SmartScreen are never bypassed.</span></div>
            <div className="row" style={{ alignItems: "flex-start" }}><I.IconShield size={16} style={{ marginTop: 2, flex: "none" }} /><span>Removing a game deletes only Nova's library entry. Game files are never deleted, moved or modified.</span></div>
          </div>
        </div>
      </div>

      <div className="section">
        <div className="section-head"><h2>Updates</h2></div>
        <div className="card">
          <UpdateRow />
          <Row title="Update automatically" sub="Checks every few hours and downloads in the background. Updates are signed; anything that doesn't verify is discarded.">
            <Switch on={s.autoUpdate} onChange={(v) => put({ autoUpdate: v })} />
          </Row>
        </div>
      </div>

      <div className="section">
        <div className="section-head"><h2>About</h2></div>
        <div className="card">
          <Row title={`Nova ${info?.version ?? ""}`} sub={`lagless engine ${info?.laglessVersion ?? ""}`} />
          <Row title="Library data" sub={<span className="selectable">{info?.dataDir}</span>}>
            {info && <button className="btn sm" onClick={() => api.reveal(info.dataDir).catch(fail)}><I.IconFolder size={14} /> Open</button>}
          </Row>
        </div>
      </div>
    </div>
  );
}
