import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, on } from "./api";
import { back, fail, get, go, putGame, refresh, refreshCollections, set, toast, useStore } from "./store";
import type { AudioState, SessionEvent, UpdateInfo } from "./types";
import { Library } from "./views/Library";
import { GameDetails } from "./views/GameDetails";
import { ImportWizard } from "./views/ImportWizard";
import { SettingsView } from "./views/Settings";
import { AudioView } from "./views/Audio";
import { LaunchPromptModal } from "./launch";
import { Cover, fmtClock, fmtPlaytime, useNow, Confirm, useMenu } from "./components/ui";
import * as I from "./components/Icons";

function TitleBar() {
  const w = getCurrentWindow();
  return (
    <div className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <I.NovaMark size={15} /> NOVA
      </div>
      <div className="drag" data-tauri-drag-region />
      <button className="winbtn" aria-label="Minimize" onClick={() => w.minimize()}><I.IconMin size={14} /></button>
      <button className="winbtn" aria-label="Maximize" onClick={() => w.toggleMaximize()}><I.IconMax size={12} /></button>
      <button className="winbtn close" aria-label="Close" onClick={() => w.close()}><I.IconClose size={14} /></button>
    </div>
  );
}

function LaglessMini() {
  const a = useStore((s) => s.audio);
  const view = useStore((s) => s.view);
  if (!a) return null;
  const color = laglessColor(a);
  const line = laglessLine(a);
  return (
    <div className={`lagless-mini ${view.name === "audio" ? "active" : ""}`} onClick={() => go({ name: "audio" })} role="button" aria-label="lagless audio">
      <div className="row" style={{ gap: 8 }}>
        <span className="dot" style={{ color }} />
        <span style={{ fontSize: 11, fontWeight: 600, letterSpacing: "0.06em", color: "var(--dim)" }}>LAGLESS</span>
        {a.phase === "streaming" && a.peers.filter((p) => p.included).length > 1 && <span className="faint" style={{ marginLeft: "auto", fontSize: 11 }}>{a.peers.filter((p) => p.included).length} devices</span>}
      </div>
      <div className="row" style={{ marginTop: 6, alignItems: "baseline", gap: 6 }}>
        {a.phase === "streaming" && a.latencyMs > 0 ? (
          <>
            <span className="num display">{Math.round(a.latencyMs)}</span>
            <span className="dim" style={{ fontSize: 12 }}>ms</span>
          </>
        ) : (
          <span className="dim ellipsis" style={{ fontSize: 12.5 }}>{line}</span>
        )}
      </div>
      {a.phase === "streaming" && <div className="faint ellipsis" style={{ fontSize: 11.5, marginTop: 2 }}>{line}</div>}
    </div>
  );
}

export function laglessColor(a: AudioState) {
  if (a.phase === "error") return "var(--bad)";
  if (a.phase !== "streaming") return "var(--faint)";
  if (a.latencyMs > 45) return "var(--bad)";
  if (a.latencyMs > 25) return "var(--warn)";
  return "var(--good)";
}

export function laglessLine(a: AudioState): string {
  switch (a.phase) {
    case "off": return "Audio streaming is off";
    case "paused": return "Paused";
    case "starting": return "Starting…";
    case "searching": return a.mode === "send" ? "Looking for your Mac or phone…" : "Waiting for a PC…";
    case "idle": return `Connected to ${a.peerName || "receiver"} · nothing playing`;
    case "error": return a.error ?? "Something went wrong";
    case "streaming": {
      const n = a.peers.filter((p) => p.included && p.playing).length;
      const who = n > 1 ? `${n} devices` : a.peerName || "receiver";
      return `${who} · ${a.link || a.linkName}${a.lossless ? " · Lossless" : " · Opus"}`;
    }
  }
}

/** Restarting is safe mid-game: games keep running; time so far is saved. */
export function restartToUpdate() {
  api.updateRestart().catch(fail);
}

function UpdateCard() {
  const u = useStore((s) => s.update);
  const running = useStore((s) => s.running.length);
  const [ask, setAsk] = useState(false);
  if (u?.status.state !== "ready") return null;
  const v = u.status.version;
  return (
    <div className="update-card">
      <div className="row" style={{ gap: 8 }}><I.IconRefresh size={14} /><span style={{ fontWeight: 600 }}>Nova {v} is ready</span></div>
      <div className="faint" style={{ fontSize: 12, margin: "3px 0 9px" }}>Installs when you quit, or now.</div>
      <button className="btn sm accent" style={{ width: "100%" }} onClick={() => (running ? setAsk(true) : restartToUpdate())}>Restart to update</button>
      {ask && (
        <Confirm
          title={`Update to Nova ${v} now?`}
          body="Your game keeps running and the time played so far is saved, but playtime after the restart isn't counted for this session. You can also just quit Nova later; the update installs then."
          confirm="Restart now"
          onClose={() => setAsk(false)}
          onYes={restartToUpdate}
        />
      )}
    </div>
  );
}

function Sidebar() {
  const view = useStore((s) => s.view);
  const games = useStore((s) => s.games);
  const collections = useStore((s) => s.collections);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [del, setDel] = useState<{ id: number; name: string } | null>(null);
  const [renaming, setRenaming] = useState<{ id: number; name: string } | null>(null);
  const menu = useMenu();
  const rename = async () => {
    const r = renaming;
    setRenaming(null);
    if (!r || !r.name.trim()) return;
    try {
      await api.renameCollection(r.id, r.name);
      await refreshCollections();
    } catch (e) {
      fail(e);
    }
  };
  const visible = games.filter((g) => !g.hidden);
  const lib = view.name === "library";
  const navItem = (active: boolean, icon: React.ReactNode, label: string, onClick: () => void, count?: number, onContext?: (e: React.MouseEvent) => void) => (
    <button className={`nav ${active ? "active" : ""}`} onClick={onClick} onContextMenu={onContext}>
      {icon}
      <span className="ellipsis">{label}</span>
      {count !== undefined && <span className="count">{count}</span>}
    </button>
  );
  const create = async () => {
    const n = name.trim();
    setAdding(false);
    setName("");
    if (!n) return;
    try {
      const id = await api.createCollection(n);
      await refreshCollections();
      go({ name: "library", collection: id });
    } catch (e) {
      fail(e);
    }
  };
  return (
    <nav className="sidebar">
      {navItem(lib && !view.collection && !view.filter, <I.IconLibrary size={17} />, "Library", () => go({ name: "library" }), visible.length)}
      {navItem(lib && view.filter === "recent", <I.IconClock size={17} />, "Recently played", () => go({ name: "library", filter: "recent" }))}
      {navItem(lib && view.filter === "favorites", <I.IconStar size={17} />, "Favorites", () => go({ name: "library", filter: "favorites" }), visible.filter((g) => g.favorite).length)}
      {navItem(view.name === "audio", <I.IconAudio size={17} />, "Audio", () => go({ name: "audio" }))}

      <div className="nav-label">
        Collections
        <button className="btn ghost sm icon" style={{ width: 22, height: 22 }} aria-label="New collection" onClick={() => setAdding(true)}><I.IconPlus size={14} /></button>
      </div>
      {collections.map((c) =>
        renaming?.id === c.id ? (
          <div className="nav active" key={c.id}>
            <I.IconCollection size={16} />
            <input autoFocus value={renaming.name} onChange={(e) => setRenaming({ id: c.id, name: e.target.value })} onKeyDown={(e) => { if (e.key === "Enter") rename(); if (e.key === "Escape") setRenaming(null); }} onBlur={rename} />
          </div>
        ) : (
          <div key={c.id}>
            {navItem(lib && view.collection === c.id, <I.IconCollection size={16} />, c.name, () => go({ name: "library", collection: c.id }), c.gameCount, (e) =>
              menu.open(e, [
                { label: "Rename", icon: <I.IconEdit size={14} />, onClick: () => setRenaming({ id: c.id, name: c.name }) },
                { label: "Delete collection…", icon: <I.IconTrash size={14} />, danger: true, onClick: () => setDel({ id: c.id, name: c.name }) },
              ]),
            )}
          </div>
        ),
      )}
      {menu.el}
      {adding && (
        <div className="nav active">
          <I.IconCollection size={16} />
          <input autoFocus placeholder="Collection name" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") create(); if (e.key === "Escape") { setAdding(false); setName(""); } }} onBlur={create} />
        </div>
      )}
      {!collections.length && !adding && <div className="faint" style={{ fontSize: 12, padding: "2px 10px" }}>Group games your way.</div>}

      <div className="spacer" />
      <UpdateCard />
      <button className="btn" style={{ width: "100%", marginBottom: 6 }} onClick={() => set({ importOpen: true })}><I.IconPlus size={15} /> Add game</button>
      {navItem(view.name === "settings", <I.IconSettings size={17} />, "Settings", () => go({ name: "settings" }))}
      <LaglessMini />
      {del && (
        <Confirm
          title={`Delete “${del.name}”?`}
          body="The collection is removed. Its games stay in your library."
          confirm="Delete collection"
          danger
          onClose={() => setDel(null)}
          onYes={async () => {
            try {
              await api.deleteCollection(del.id);
              await refresh();
              if (get().view.name === "library" && (get().view as { collection?: number }).collection === del.id) go({ name: "library" });
            } catch (e) {
              fail(e);
            }
          }}
        />
      )}
    </nav>
  );
}

function NowPlaying() {
  const running = useStore((s) => s.running);
  const games = useStore((s) => s.games);
  const audio = useStore((s) => s.audio);
  const now = useNow(1000);
  const [confirm, setConfirm] = useState(false);
  const r = [...running].sort((a, b) => b.startedAt - a.startedAt)[0];
  if (!r) return null;
  const g = games.find((x) => x.id === r.gameId);
  const perf = audio?.perf;
  return (
    <div className="nowplaying glass" role="status">
      {g && <div className="thumb" onClick={() => go({ name: "game", id: g.id })} style={{ cursor: "pointer" }}><Cover game={g} badges={false} /></div>}
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="row" style={{ gap: 7 }}><span className="dot live" style={{ color: "var(--good)" }} /><span style={{ fontSize: 11, fontWeight: 600, letterSpacing: "0.08em", color: "var(--dim)" }}>NOW PLAYING{running.length > 1 ? ` · ${running.length}` : ""}</span></div>
        <div className="ellipsis" style={{ fontWeight: 600, fontSize: 14, marginTop: 2 }}>{r.title}</div>
      </div>
      {perf?.live && perf.fpsShown ? (
        <div className="np-stat"><div className="v">{Math.round(perf.fpsShown)}</div><div className="k">FPS</div></div>
      ) : null}
      <div className="np-stat"><div className="v">{fmtClock((now - r.startedAt) / 1000)}</div><div className="k">Session</div></div>
      <button className="btn sm" onClick={() => setConfirm(true)}><I.IconStop size={12} /> Stop</button>
      {confirm && (
        <Confirm
          title={`Close ${r.title}?`}
          body="This ends the game's processes immediately, like closing it from Task Manager. Unsaved progress will be lost."
          confirm="Close game"
          danger
          onClose={() => setConfirm(false)}
          onYes={() => api.stop(r.gameId)}
        />
      )}
    </div>
  );
}

function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className="toast glass">
          <span className="ic" style={{ color: { good: "var(--good)", bad: "var(--bad)", warn: "var(--warn)", info: "var(--accent)" }[t.kind] }}>
            {t.kind === "bad" || t.kind === "warn" ? <I.IconAlert size={16} /> : t.kind === "good" ? <I.IconCheck size={16} /> : <I.IconInfo size={16} />}
          </span>
          <span className="grow">{t.text}</span>
          {t.action && <button className="btn sm" onClick={t.action.run}>{t.action.label}</button>}
        </div>
      ))}
    </div>
  );
}

export default function App() {
  const view = useStore((s) => s.view);
  const importOpen = useStore((s) => s.importOpen);
  const prompt = useStore((s) => s.prompt);
  const reduce = useStore((s) => s.settings?.reduceMotion);

  useEffect(() => {
    api.getSettings().then((settings) => set({ settings }));
    refresh();
    api.audioState().then((audio) => set({ audio })).catch(() => {});
    api.updateState().then((update) => set({ update })).catch(() => {});
    const offs = [
      on<SessionEvent>("session", async (e) => {
        const running = await api.running();
        set({ running });
        try {
          putGame(await api.getGame(e.gameId));
        } catch {
          /* removed meanwhile */
        }
        const title = get().games.find((g) => g.id === e.gameId)?.title ?? "Game";
        if (e.state === "ended") toast(`${title} closed · ${fmtPlaytime(e.secs)} played`, "info");
        if (e.state === "failed") toast(e.message || `${title} didn't start`, "warn");
      }),
      on<AudioState>("audio-state", (audio) => set({ audio })),
      on<UpdateInfo>("update", (update) => {
        const was = get().update?.status.state;
        set({ update });
        if (update.status.state === "ready" && was !== "ready") toast(`Nova ${update.status.version} is ready to install`, "good");
      }),
      on<{ added: number }>("library-changed", (p) => {
        refresh();
        if (p.added) toast(`Found ${p.added} new game${p.added === 1 ? "" : "s"} in your store libraries`, "good");
      }),
      on<{ text: string }>("lagless-toast", (p) => toast(p.text, "info")),
      on<{ text: string }>("lagless-alert", (p) => toast(`Heard: ${p.text}`, "warn")),
    ];
    const key = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      const typing = t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT";
      if ((e.key === "f" && (e.ctrlKey || e.metaKey)) || (e.key === "/" && !typing)) {
        e.preventDefault();
        if (get().view.name !== "library") go({ name: "library" });
        setTimeout(() => document.getElementById("search")?.focus(), 30);
      }
      if (e.key === "Escape" && !typing && get().view.name === "game" && !get().importOpen && !get().prompt && !document.querySelector(".menu, .modal-back")) back();
      if ((e.key === "n" && (e.ctrlKey || e.metaKey))) {
        e.preventDefault();
        set({ importOpen: true });
      }
    };
    window.addEventListener("keydown", key);
    // Drop a game folder (or .exe) anywhere on the window to import it.
    const dd = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && e.payload.paths.length && !get().importOpen) {
        set({ importPath: e.payload.paths[0], importOpen: true });
      }
    });
    const ctx = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (!(t.tagName === "INPUT" || t.tagName === "TEXTAREA")) e.preventDefault();
    };
    window.addEventListener("contextmenu", ctx);
    return () => {
      offs.forEach((f) => f());
      dd.then((f) => f());
      window.removeEventListener("keydown", key);
      window.removeEventListener("contextmenu", ctx);
    };
  }, []);

  return (
    <div className={`shell ${reduce ? "reduce-motion" : ""}`}>
      <div className="ambient" />
      <TitleBar />
      <Sidebar />
      <main className="main">
        {view.name === "library" && <Library key={`${view.collection ?? ""}-${view.filter ?? ""}`} collection={view.collection} filter={view.filter} />}
        {view.name === "game" && <GameDetails key={view.id} id={view.id} />}
        {view.name === "audio" && <AudioView />}
        {view.name === "settings" && <SettingsView />}
        <NowPlaying />
      </main>
      {importOpen && <ImportWizard onClose={() => set({ importOpen: false })} />}
      {prompt && <LaunchPromptModal />}
      <Toasts />
    </div>
  );
}
