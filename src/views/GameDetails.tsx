import { useEffect, useRef, useState } from "react";
import { api, pickExe, pickFolder, pickImage } from "../api";
import { back, fail, go, putGame, refreshCollections, toast, useStore } from "../store";
import type { Executable, Game, Session } from "../types";
import { play } from "../launch";
import { Cover, GameIcon, fmtAgo, fmtPlaytime, heroUrl, Modal, sourceName, useMenu, useNow, fmtClock, type MenuItem } from "../components/ui";
import { RemoveConfirm } from "./Library";
import { PerfCard } from "./Audio";
import * as I from "../components/Icons";

function exeName(p: string) {
  return p.split(/[\\/]/).pop() ?? p;
}

function EditDetails({ g, onClose }: { g: Game; onClose: () => void }) {
  const [f, setF] = useState({ title: g.title, developer: g.developer, publisher: g.publisher, description: g.description, notes: g.notes });
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try {
      putGame(await api.updateGame(g.id, f));
      onClose();
    } catch (e) {
      fail(e);
    }
    setBusy(false);
  };
  const field = (k: keyof typeof f, label: string, multi = false) => (
    <label className="field">
      {label}
      {multi ? <textarea className="input" value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value })} /> : <input className="input" value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value })} />}
    </label>
  );
  return (
    <Modal title="Edit details" sub={g.title} onClose={onClose} foot={<><button className="btn ghost" onClick={onClose}>Cancel</button><button className="btn primary" disabled={busy || !f.title.trim()} onClick={save}>Save</button></>}>
      <div className="col" style={{ gap: 14 }}>
        {field("title", "Name")}
        <div className="row" style={{ gap: 12, alignItems: "stretch" }}>
          <div className="grow">{field("developer", "Developer")}</div>
          <div className="grow">{field("publisher", "Publisher")}</div>
        </div>
        {field("description", "Description", true)}
        {field("notes", "Notes", true)}
      </div>
    </Modal>
  );
}

function ExeEditor({ g, exe, onClose }: { g: Game; exe: Executable | null; onClose: () => void }) {
  const [f, setF] = useState<Omit<Executable, "id" | "gameId">>(exe ?? { path: "", label: "", args: "", workingDir: "" });
  const [warn, setWarn] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const browse = async () => {
    const p = await pickExe("Choose the game's executable", g.installDir || undefined);
    if (!p) return;
    setF((x) => ({ ...x, path: p }));
    try {
      const c = await api.inspectExe(p);
      setWarn(c.warnings.filter((w) => w.level !== "info").map((w) => w.text));
    } catch (e) {
      fail(e);
    }
  };
  const save = async () => {
    setBusy(true);
    try {
      putGame(exe ? await api.updateExecutable({ ...exe, ...f }) : await api.addExecutable(g.id, f));
      onClose();
    } catch (e) {
      fail(e);
    }
    setBusy(false);
  };
  return (
    <Modal title={exe ? "Edit executable" : "Add executable"} sub="The file is launched in place, from its own folder. Nothing is moved or copied." onClose={onClose} foot={<><button className="btn ghost" onClick={onClose}>Cancel</button><button className="btn primary" disabled={busy || !f.path} onClick={save}>{exe ? "Save" : "Add"}</button></>}>
      <div className="col" style={{ gap: 14 }}>
        <label className="field">
          Executable
          <div className="input-row">
            <input className="input" value={f.path} onChange={(e) => setF({ ...f, path: e.target.value })} placeholder="C:\Games\…\Game.exe" spellCheck={false} />
            <button className="btn" onClick={browse}>Browse…</button>
          </div>
        </label>
        {warn.length > 0 && <div className="callout warn"><I.IconAlert size={16} /><div>{warn.map((w, i) => <div key={i}>{w}</div>)}</div></div>}
        <label className="field">Label <span className="hint">Shown in the Play menu, e.g. “DirectX 11” or “Launcher”</span><input className="input" value={f.label} onChange={(e) => setF({ ...f, label: e.target.value })} placeholder={exeName(f.path) || "Game"} /></label>
        <label className="field">Launch arguments <span className="hint">Added after the game's own launch options</span><input className="input" value={f.args} onChange={(e) => setF({ ...f, args: e.target.value })} placeholder="-windowed -dx11" spellCheck={false} /></label>
        <label className="field">
          Working folder <span className="hint">Leave empty to use the game's working folder</span>
          <div className="input-row">
            <input className="input" value={f.workingDir} onChange={(e) => setF({ ...f, workingDir: e.target.value })} placeholder={g.workingDir || g.installDir || "Game folder"} spellCheck={false} />
            <button className="btn" onClick={async () => { const p = await pickFolder("Working folder", g.installDir || undefined); if (p) setF({ ...f, workingDir: p }); }}>Browse…</button>
          </div>
        </label>
      </div>
    </Modal>
  );
}

function LaunchTab({ g }: { g: Game }) {
  const [wd, setWd] = useState(g.workingDir);
  const [args, setArgs] = useState(g.launchArgs);
  const [editing, setEditing] = useState<Executable | null | "new">(null);
  useEffect(() => {
    setWd(g.workingDir);
    setArgs(g.launchArgs);
  }, [g.workingDir, g.launchArgs]);
  const dirty = wd !== g.workingDir || args !== g.launchArgs;
  const saveOpts = async () => {
    try {
      putGame(await api.updateGame(g.id, { workingDir: wd.trim(), launchArgs: args }));
      toast("Launch options saved", "good");
    } catch (e) {
      fail(e);
    }
  };
  const store = g.launchUri !== "";
  return (
    <div className="col" style={{ gap: 22 }}>
      {store && (
        <div className="callout info"><I.IconInfo size={16} /><span>{sourceName(g.source)} games start through the {sourceName(g.source)} client so sign-in, DRM and overlays keep working. Executables below can be launched directly from the Play menu.</span></div>
      )}
      <div>
        <div className="section-head" style={{ marginBottom: 10 }}>
          <h2>Executables</h2>
          <span className="faint" style={{ fontSize: 12.5 }}>{store ? "Direct launch options" : "The selected one starts when you press Play"}</span>
          <button className="btn sm more" onClick={() => setEditing("new")}><I.IconPlus size={13} /> Add</button>
        </div>
        <div className="card list">
          {g.executables.length === 0 && <div className="list-row"><span className="dim">No executable yet. Add the game's main .exe.</span></div>}
          {g.executables.map((x) => {
            const preferred = (g.preferredExeId ?? g.executables[0]?.id) === x.id;
            return (
              <div key={x.id} className="list-row exe-row">
                {!store && (
                  <button className={`radio ${preferred ? "on" : ""}`} aria-label="Use when pressing Play" onClick={async () => { try { putGame(await api.updateGame(g.id, { preferredExeId: x.id })); } catch (e) { fail(e); } }} />
                )}
                <div className="grow">
                  <div className="title">{x.label || exeName(x.path)} {preferred && !store && <span className="pill accent" style={{ marginLeft: 6 }}>Play</span>}</div>
                  <div className="path">{x.path}</div>
                  {(x.args || x.workingDir) && <div className="faint" style={{ fontSize: 12, marginTop: 2 }}>{x.args && <>Arguments: <span className="selectable">{x.args}</span> </>}{x.workingDir && <>· Runs in <span className="selectable">{x.workingDir}</span></>}</div>}
                </div>
                <button className="btn ghost sm icon" aria-label="Launch this" title="Launch this executable" onClick={() => play(g, { exeId: x.id })} disabled={g.running}><I.IconPlay size={12} /></button>
                <button className="btn ghost sm icon" aria-label="Show in folder" title="Show in folder" onClick={() => api.reveal(x.path).catch(fail)}><I.IconFolder size={14} /></button>
                <button className="btn ghost sm icon" aria-label="Edit" title="Edit" onClick={() => setEditing(x)}><I.IconEdit size={14} /></button>
                <button className="btn ghost sm icon" aria-label="Remove" title="Remove (the file is not deleted)" onClick={async () => { try { putGame(await api.removeExecutable(g.id, x.id)); } catch (e) { fail(e); } }}><I.IconTrash size={14} /></button>
              </div>
            );
          })}
        </div>
      </div>
      {!store && (
        <div className="card" style={{ padding: 18 }}>
          <div className="col" style={{ gap: 14 }}>
            <label className="field">
              Working folder <span className="hint">Where the game starts. Default: the game's root folder ({g.installDir || "—"}).</span>
              <div className="input-row">
                <input className="input" value={wd} onChange={(e) => setWd(e.target.value)} placeholder={g.installDir || "Game folder"} spellCheck={false} />
                <button className="btn" onClick={async () => { const p = await pickFolder("Working folder", g.installDir || undefined); if (p) setWd(p); }}>Browse…</button>
              </div>
            </label>
            <label className="field">
              Launch options <span className="hint">Passed to every executable of this game, exactly as typed</span>
              <input className="input" value={args} onChange={(e) => setArgs(e.target.value)} placeholder="-fullscreen -novid" spellCheck={false} />
            </label>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <button className="btn primary sm" disabled={!dirty} onClick={saveOpts}>Save launch options</button>
            </div>
          </div>
        </div>
      )}
      {editing && <ExeEditor g={g} exe={editing === "new" ? null : editing} onClose={() => setEditing(null)} />}
    </div>
  );
}

function ActivityTab({ g }: { g: Game }) {
  const [s, setS] = useState<Session[] | null>(null);
  useEffect(() => {
    api.sessions(g.id).then(setS).catch(fail);
  }, [g.id, g.playtimeSecs]);
  if (!s) return <div className="spinner" />;
  if (!s.length) return <div className="dim">No sessions yet. Playtime is tracked automatically while the game runs.</div>;
  return (
    <div className="card list">
      {s.map((x) => (
        <div key={x.id} className="list-row">
          <I.IconClock size={16} className="faint" />
          <div className="grow">
            <div className="title">{new Date(x.startedAt).toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })} <span className="dim" style={{ fontWeight: 400 }}>· {new Date(x.startedAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}</span></div>
          </div>
          {x.avgFps ? <span className="pill">{Math.round(x.avgFps)} FPS avg{x.low1 ? ` · ${Math.round(x.low1)} 1% low` : ""}</span> : null}
          <span className="mono" style={{ width: 90, textAlign: "right" }}>{fmtPlaytime(x.secs)}</span>
        </div>
      ))}
    </div>
  );
}

export function GameDetails({ id }: { id: number }) {
  const g = useStore((s) => s.games.find((x) => x.id === id));
  const loaded = useStore((s) => s.loaded);
  const collections = useStore((s) => s.collections);
  const running = useStore((s) => s.running.find((r) => r.gameId === id));
  const perf = useStore((s) => s.audio?.perf);
  const now = useNow(1000);
  const [tab, setTab] = useState<"overview" | "launch" | "activity">("overview");
  const [edit, setEdit] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [fetching, setFetching] = useState(false);
  const menu = useMenu();
  const playMenuRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    api.getGame(id).then(putGame).catch(() => {});
  }, [id]);
  if (!g) return loaded ? <div className="page"><div className="empty"><h2>This game isn't in your library</h2><button className="btn" onClick={() => go({ name: "library" })}>Back to library</button></div></div> : null;

  const hero = heroUrl(g);
  const safely = (f: () => Promise<Game>) => async () => {
    try {
      putGame(await f());
    } catch (e) {
      fail(e);
    }
  };
  const setArt = async (kind: "cover" | "hero" | "icon") => {
    const p = await pickImage(kind === "cover" ? "Choose a cover image (portrait works best)" : kind === "icon" ? "Choose an icon (square works best)" : "Choose a background image (wide works best)");
    if (!p) return;
    try {
      putGame(await api.setArt(g.id, kind, p));
    } catch (e) {
      fail(e);
    }
  };
  const fetchArt = async () => {
    setFetching(true);
    try {
      putGame(await api.fetchOnlineArt(g.id));
      toast("Cover art updated", "good");
    } catch (e) {
      fail(e);
    }
    setFetching(false);
  };
  const moreItems: MenuItem[] = [
    { label: "Edit details…", icon: <I.IconEdit size={15} />, onClick: () => setEdit(true) },
    { label: "Change cover…", icon: <I.IconImage size={15} />, onClick: () => setArt("cover") },
    { label: "Change background…", icon: <I.IconImage size={15} />, onClick: () => setArt("hero") },
    { label: "Change icon…", icon: <I.IconImage size={15} />, onClick: () => setArt("icon") },
    ...(g.cover ? [{ label: "Use generated cover", icon: <I.IconImage size={15} />, onClick: safely(() => api.clearArt(g.id, "cover")) }] : []),
    { label: fetching ? "Searching…" : "Find cover art online", icon: <I.IconRefresh size={15} />, onClick: fetchArt, disabled: fetching },
    { sep: true },
    ...(g.installDir ? [{ label: "Show in folder", icon: <I.IconFolder size={15} />, onClick: () => api.reveal(g.installDir).catch(fail) }] : []),
    {
      label: g.hidden ? "Unhide" : "Hide from library",
      icon: <I.IconEyeOff size={15} />,
      onClick: safely(() => api.updateGame(g.id, { hidden: !g.hidden })),
    },
    { label: "Remove from library…", icon: <I.IconTrash size={15} />, danger: true, onClick: () => setRemoving(true) },
  ];
  const playItems: MenuItem[] = [
    ...(g.launchUri ? [{ label: `Launch through ${sourceName(g.source)}`, icon: <I.IconPlay size={12} />, onClick: () => play(g) }] : []),
    ...g.executables.map((x) => ({ label: `${g.launchUri ? "Launch directly: " : ""}${x.label || exeName(x.path)}`, icon: <I.IconPlay size={12} />, onClick: () => play(g, { exeId: x.id }) })),
    ...(g.executables.length ? [{ sep: true }, { label: "Run as administrator…", icon: <I.IconShield size={14} />, onClick: () => play(g, { admin: true }) }] : []),
  ];
  const collectionItems: MenuItem[] = collections.length
    ? collections.map((c) => ({
        label: c.name,
        checked: g.collections.includes(c.id),
        onClick: async () => {
          try {
            await api.setInCollection(c.id, g.id, !g.collections.includes(c.id));
            putGame(await api.getGame(g.id));
            refreshCollections();
          } catch (e) {
            fail(e);
          }
        },
      }))
    : [{ label: "Create a collection from the sidebar first", disabled: true }];
  const livePerf = g.running && perf?.live;

  return (
    <div className="page">
      <div className="details">
        <div className="backdrop">
          {hero.url ? <div className={`bg ${hero.blur ? "blur" : ""}`} style={{ backgroundImage: `url("${hero.url}")` }} /> : <div className="bg" style={{ background: "radial-gradient(70% 100% at 70% 0%, rgba(154,168,255,0.16), transparent), #101217" }} />}
          <button className="btn glass sm back" onClick={back}><I.IconBack size={14} /> Back</button>
        </div>
        <div className="head">
          <div className="cover-wrap"><Cover game={g} badges={false} /></div>
          <div className="grow" style={{ paddingBottom: 4, minWidth: 0 }}>
            <div className="row" style={{ gap: 8 }}>
              <GameIcon game={g} size={22} />
              <span className="pill">{sourceName(g.source)}</span>
              {g.running ? <span className="pill good"><span className="dot live" /> Playing · {fmtClock((now - (running?.startedAt ?? now)) / 1000)}</span> : g.installed ? <span className="pill good">Installed</span> : <span className="pill warn">{g.problem || "Not installed"}</span>}
              {g.hidden && <span className="pill">Hidden</span>}
            </div>
            <h1>{g.title}</h1>
            <div className="meta-line">
              {g.developer && <span>{g.developer}</span>}
              {g.publisher && g.publisher !== g.developer && <><span className="faint">·</span><span>{g.publisher}</span></>}
              {!g.developer && !g.publisher && <span className="faint">No developer info · <a style={{ color: "var(--accent)", cursor: "pointer" }} onClick={() => setEdit(true)}>add it</a></span>}
            </div>
            <div className="actions">
              {g.running ? (
                <button className="btn lg" onClick={() => api.stop(g.id)} title="Ends the game's processes"><I.IconStop size={14} /> Stop</button>
              ) : (
                <div className="split">
                  <button className="btn primary lg" disabled={!g.installed} onClick={() => play(g)}><I.IconPlay size={16} /> Play</button>
                  <button ref={playMenuRef} className="btn primary lg" disabled={!g.installed || playItems.length === 0} aria-label="More ways to launch" onClick={() => playMenuRef.current && menu.openAt(playMenuRef.current, playItems)}><I.IconChevronDown size={16} /></button>
                </div>
              )}
              <button className="btn lg icon" style={{ width: 46 }} aria-label={g.favorite ? "Remove from favorites" : "Add to favorites"} onClick={safely(() => api.updateGame(g.id, { favorite: !g.favorite }))}>
                <I.IconStar size={18} filled={g.favorite} style={{ color: g.favorite ? "#ffd36e" : undefined }} />
              </button>
              <button className="btn lg icon" style={{ width: 46 }} aria-label="Collections" onClick={(e) => menu.openAt(e.currentTarget, collectionItems)}><I.IconCollection size={18} /></button>
              <button className="btn lg icon" style={{ width: 46 }} aria-label="More" onClick={(e) => menu.openAt(e.currentTarget, moreItems)}><I.IconMore size={18} /></button>
            </div>
          </div>
        </div>

        {!g.installed && g.problem && (
          <div style={{ padding: "0 44px", marginTop: 24 }}>
            <div className="callout warn">
              <I.IconAlert size={16} />
              <div className="grow">{g.problem}. {g.source === "manual" ? "If you moved the game, point Nova at its new location." : "Reinstall it with its store, or remove it from the library."}</div>
              {g.source === "manual" && <button className="btn sm" onClick={() => setTab("launch")}>Fix executable</button>}
              {g.source === "manual" && (
                <button className="btn sm" onClick={async () => { const p = await pickFolder("Where is the game now?"); if (!p) return; try { putGame(await api.relocate(g.id, p)); toast("Game folder updated", "good"); } catch (e) { fail(e); putGame(await api.getGame(g.id)); } }}>Locate folder</button>
              )}
            </div>
          </div>
        )}

        <div className="body">
          <div style={{ minWidth: 0 }}>
            <div className="stats">
              <div><div className="k">Playtime</div><div className="v">{fmtPlaytime(g.playtimeSecs)}</div></div>
              <div><div className="k">Last played</div><div className="v">{fmtAgo(g.lastPlayed)}</div></div>
              <div><div className="k">Added</div><div className="v">{fmtAgo(g.addedAt)}</div></div>
              <div><div className="k">Live FPS</div><div className="v">{livePerf && perf?.fpsShown ? Math.round(perf.fpsShown) : "—"}</div></div>
            </div>
            <div className="tabs" style={{ marginTop: 28 }}>
              {(["overview", "launch", "activity"] as const).map((t) => (
                <button key={t} className={tab === t ? "on" : ""} onClick={() => setTab(t)}>{{ overview: "Overview", launch: "Launch", activity: "Activity" }[t]}</button>
              ))}
            </div>
            {tab === "overview" && (
              <div className="col" style={{ gap: 20 }}>
                {livePerf && perf && <PerfCard perf={perf} title="Live performance · lagless" />}
                {g.description ? <div className="dim selectable" style={{ lineHeight: 1.65, whiteSpace: "pre-wrap" }}>{g.description}</div> : <div className="faint">No description. <a style={{ color: "var(--accent)", cursor: "pointer" }} onClick={() => setEdit(true)}>Add one</a></div>}
                {g.notes && <div className="card" style={{ padding: 16 }}><div className="faint" style={{ fontSize: 11.5, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 6 }}>Notes</div><div className="selectable" style={{ whiteSpace: "pre-wrap" }}>{g.notes}</div></div>}
              </div>
            )}
            {tab === "launch" && <LaunchTab g={g} />}
            {tab === "activity" && <ActivityTab g={g} />}
          </div>
          <aside className="col" style={{ gap: 16 }}>
            <div className="card" style={{ padding: 18 }}>
              <div className="kv">
                <div className="k">Source</div><div className="v">{sourceName(g.source)}{g.sourceId ? <span className="faint"> · {g.sourceId}</span> : null}</div>
                <div className="k">Location</div><div className="v">{g.installDir || "—"}</div>
                {g.launchUri === "" && <><div className="k">Starts in</div><div className="v">{g.workingDir || g.installDir || "—"}</div></>}
                {g.launchUri === "" && <><div className="k">Executable</div><div className="v">{(g.executables.find((x) => x.id === g.preferredExeId) ?? g.executables[0])?.path ?? "—"}</div></>}
                {g.launchArgs && <><div className="k">Options</div><div className="v">{g.launchArgs}</div></>}
              </div>
              {g.installDir && <button className="btn sm" style={{ marginTop: 16 }} onClick={() => api.reveal(g.installDir).catch(fail)}><I.IconFolder size={14} /> Open folder</button>}
            </div>
            {g.collections.length > 0 && (
              <div className="card" style={{ padding: 18 }}>
                <div className="faint" style={{ fontSize: 11.5, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 10 }}>Collections</div>
                <div className="row" style={{ flexWrap: "wrap", gap: 6 }}>
                  {collections.filter((c) => g.collections.includes(c.id)).map((c) => <button key={c.id} className="chip" onClick={() => go({ name: "library", collection: c.id })}>{c.name}</button>)}
                </div>
              </div>
            )}
          </aside>
        </div>
      </div>
      {menu.el}
      {edit && <EditDetails g={g} onClose={() => setEdit(false)} />}
      {removing && <RemoveConfirm game={g} onClose={() => setRemoving(false)} onDone={() => go({ name: "library" })} />}
    </div>
  );
}
