import { useEffect, useMemo, useState } from "react";
import { api, errText, pickExe, pickFolder, pickImage } from "../api";
import { get, go, putGame, refresh, set, toast } from "../store";
import type { Candidate, Game, Role, ScanResult, StoreReport, Warning } from "../types";
import { Cover, fmtBytes } from "../components/ui";
import * as I from "../components/Icons";

type Step = { s: "choose" } | { s: "scanning"; path: string } | { s: "review"; scan: ScanResult; single: boolean } | { s: "stores"; busy: boolean; reports: StoreReport[]; added: number };

const roleLabel: Record<Role, string> = { game: "Game", launcher: "Launcher", tool: "Tool", installer: "Installer", redist: "Redistributable", crash: "Crash reporter", anticheat: "Anti-cheat", script: "Script" };
const likely = (c: Candidate) => (c.role === "game" || c.role === "launcher" || c.role === "script") && c.score > 0;

function worst(ws: Warning[]) {
  return ws.some((w) => w.level === "danger") ? "danger" : ws.some((w) => w.level === "warn") ? "warn" : ws.length ? "info" : "";
}

function CandidateRow({ c, preferred, included, onPrefer, onInclude }: { c: Candidate; preferred: boolean; included: boolean; onPrefer: () => void; onInclude: () => void }) {
  const [open, setOpen] = useState(false);
  const w = worst(c.warnings);
  const serious = c.warnings.filter((x) => x.level !== "info");
  return (
    <div className={`cand ${preferred ? "chosen" : ""}`}>
      <button className={`radio ${preferred ? "on" : ""}`} aria-label={`Launch ${c.name} with Play`} onClick={onPrefer} style={{ marginTop: 2 }} />
      <div className="grow" style={{ minWidth: 0 }} onClick={onPrefer}>
        <div className="row" style={{ gap: 8 }}>
          <span className="n ellipsis">{c.name}</span>
          <span className={`pill ${c.role === "game" ? "accent" : ""}`}>{roleLabel[c.role]}</span>
          {c.pe.arch && <span className="faint" style={{ fontSize: 11.5 }}>{c.pe.arch}</span>}
          <span className="faint" style={{ fontSize: 11.5 }}>{fmtBytes(c.size)}</span>
          {w === "danger" && <span className="pill bad"><I.IconAlert size={12} /> Unsafe</span>}
          {w === "warn" && <span className="pill warn"><I.IconAlert size={12} /> Check</span>}
        </div>
        <div className="p">{c.relPath}{c.pe.productName ? <span> · {c.pe.productName}</span> : null}</div>
        {(serious.length > 0 || open) && (
          <div className="warns">
            {(open ? c.warnings : serious).map((x, i) => (
              <div key={i} className={`warn-line ${x.level}`}>{x.level === "info" ? <I.IconInfo size={14} /> : <I.IconAlert size={14} />}<span>{x.text}</span></div>
            ))}
          </div>
        )}
        {!open && c.warnings.length > serious.length && (
          <button className="btn ghost sm" style={{ marginTop: 4, marginLeft: -10 }} onClick={(e) => { e.stopPropagation(); setOpen(true); }}>{c.warnings.length - serious.length} note{c.warnings.length - serious.length === 1 ? "" : "s"}</button>
        )}
      </div>
      <button className={`check ${included || preferred ? "on" : ""}`} disabled={preferred} title="Also add to this game's executables" aria-label={`Also register ${c.name}`} onClick={onInclude} style={{ marginTop: 2, opacity: preferred ? 0.5 : 1 }}>
        {(included || preferred) && <I.IconCheck size={12} strokeWidth={3} />}
      </button>
    </div>
  );
}

function Review({ scan, single, onBack, onDone }: { scan: ScanResult; single: boolean; onBack: () => void; onDone: (g: Game) => void }) {
  const [title, setTitle] = useState(scan.suggestedTitle);
  const [developer, setDeveloper] = useState(scan.developer);
  const [publisher, setPublisher] = useState(scan.publisher);
  const [installDir, setInstallDir] = useState(scan.root);
  const [workingDir, setWorkingDir] = useState("");
  const [args, setArgs] = useState("");
  const [cover, setCover] = useState("");
  const [coverUrl, setCoverUrl] = useState("");
  useEffect(() => {
    if (!cover) return setCoverUrl("");
    api.imagePreview(cover).then(setCoverUrl).catch((e) => { toast(errText(e), "bad"); setCover(""); });
  }, [cover]);
  const [cands, setCands] = useState<Candidate[]>(scan.candidates);
  const [pref, setPref] = useState<string>(scan.candidates[0]?.path ?? "");
  const [extra, setExtra] = useState<Set<string>>(new Set());
  const [showAll, setShowAll] = useState(false);
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);
  const main = cands.filter(likely);
  const others = cands.filter((c) => !likely(c));
  const prefC = cands.find((c) => c.path === pref);
  const prefWorst = prefC ? worst(prefC.warnings) : "";
  useEffect(() => setAck(false), [pref]);
  // If the best pick is in "others" (e.g. only tools found), show them.
  useEffect(() => {
    if (prefC && !likely(prefC)) setShowAll(true);
  }, [prefC]);

  const browseExe = async () => {
    const p = await pickExe("Choose the game's executable", installDir);
    if (!p) return;
    try {
      const c = await api.inspectExe(p);
      setCands((l) => [c, ...l.filter((x) => x.path !== c.path)]);
      setPref(c.path);
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  const add = async () => {
    if (!prefC) return;
    setBusy(true);
    const exes = [prefC, ...cands.filter((c) => extra.has(c.path) && c.path !== pref)];
    try {
      const g = await api.addGame({
        title: title.trim(),
        developer,
        publisher,
        description: "",
        installDir,
        workingDir: workingDir.trim(),
        launchArgs: args,
        executables: exes.map((c) => ({ path: c.path, label: c === prefC ? "" : roleLabel[c.role] === "Game" ? c.name.replace(/\.exe$/i, "") : roleLabel[c.role], args: "", workingDir: "" })),
        preferred: 0,
        coverPath: cover,
      });
      onDone(g);
    } catch (e) {
      toast(errText(e), "bad");
    }
    setBusy(false);
  };

  const preview: Game = { id: 0, title: title || "Untitled", source: "manual", sourceId: "", developer, publisher, description: "", installDir, workingDir, launchArgs: args, launchUri: "", preferredExeId: null, cover: "", hero: "", favorite: false, hidden: false, addedAt: 0, lastPlayed: null, playtimeSecs: 0, notes: "", executables: [], collections: [], installed: true, problem: "", running: false };

  return (
    <>
      <div className="modal-body">
        <div className="row" style={{ gap: 24, alignItems: "flex-start" }}>
          <div style={{ width: 150, flex: "none" }}>
            {cover ? (
              <div className="cover">{coverUrl ? <img src={coverUrl} alt="" /> : <div className="spinner" style={{ margin: 20 }} />}</div>
            ) : (
              <Cover game={preview} badges={false} />
            )}
            <button className="btn sm" style={{ width: "100%", marginTop: 10 }} onClick={async () => { const p = await pickImage("Choose a cover image"); if (p) setCover(p); }}><I.IconImage size={14} /> {cover ? "Change" : "Choose"} cover</button>
          </div>
          <div className="grow col" style={{ gap: 12 }}>
            <label className="field">Name<input className="input" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus /></label>
            <div className="row" style={{ gap: 12 }}>
              <label className="field grow">Developer<input className="input" value={developer} onChange={(e) => setDeveloper(e.target.value)} placeholder="Optional" /></label>
              <label className="field grow">Publisher<input className="input" value={publisher} onChange={(e) => setPublisher(e.target.value)} placeholder="Optional" /></label>
            </div>
            <div className="faint" style={{ fontSize: 12.5 }}>
              {single ? "Single executable" : `${scan.filesSeen.toLocaleString()} files · ${fmtBytes(scan.totalSize)}`}
              {scan.engine && <> · {scan.engine}</>}
              {scan.inaccessible > 0 && <span style={{ color: "var(--warn)" }}> · {scan.inaccessible} folder{scan.inaccessible === 1 ? "" : "s"} couldn't be read</span>}
              {scan.truncated && <span style={{ color: "var(--warn)" }}> · very large folder, scan stopped early</span>}
            </div>
          </div>
        </div>

        {scan.alreadyInLibrary.length > 0 && (
          <div className="callout warn" style={{ marginTop: 18 }}><I.IconInfo size={16} /><span>This folder is already in your library as {scan.alreadyInLibrary.map(([, t]) => `“${t}”`).join(", ")}. You can still add it again (e.g. for a different executable).</span></div>
        )}

        <div className="section-head" style={{ marginTop: 26, marginBottom: 8 }}>
          <h2 style={{ fontSize: 14 }}>Which program is the game?</h2>
          <span className="faint" style={{ fontSize: 12 }}>Nothing has been run. ✓ also registers extras like a launcher or DX11 mode.</span>
          <button className="btn sm more" onClick={browseExe}>Browse…</button>
        </div>
        {cands.length === 0 ? (
          <div className="callout info"><I.IconInfo size={16} /><span>No programs found in this folder. Use Browse… to pick the executable yourself.</span></div>
        ) : (
          <div className="col" style={{ gap: 2 }}>
            {(main.length ? main : []).map((c) => (
              <CandidateRow key={c.path} c={c} preferred={c.path === pref} included={extra.has(c.path)} onPrefer={() => setPref(c.path)} onInclude={() => setExtra((s) => { const n = new Set(s); n.has(c.path) ? n.delete(c.path) : n.add(c.path); return n; })} />
            ))}
            {others.length > 0 && (
              <>
                <button className="btn ghost sm" style={{ alignSelf: "flex-start", marginTop: 4 }} onClick={() => setShowAll(!showAll)}>
                  {showAll ? <I.IconChevronDown size={14} /> : <I.IconChevronRight size={14} />} {others.length} other program{others.length === 1 ? "" : "s"} (tools, installers, crash reporters)
                </button>
                {showAll && others.map((c) => (
                  <CandidateRow key={c.path} c={c} preferred={c.path === pref} included={extra.has(c.path)} onPrefer={() => setPref(c.path)} onInclude={() => setExtra((s) => { const n = new Set(s); n.has(c.path) ? n.delete(c.path) : n.add(c.path); return n; })} />
                ))}
              </>
            )}
          </div>
        )}

        <div className="row" style={{ gap: 12, marginTop: 22, alignItems: "flex-start" }}>
          <label className="field grow">
            Game folder <span className="hint">Its processes count as “playing”</span>
            <div className="input-row">
              <input className="input" value={installDir} onChange={(e) => setInstallDir(e.target.value)} spellCheck={false} />
              <button className="btn" onClick={async () => { const p = await pickFolder("Game folder", installDir); if (p) setInstallDir(p); }}>…</button>
            </div>
          </label>
          <label className="field grow">
            Working folder <span className="hint">Default: the game folder</span>
            <div className="input-row">
              <input className="input" value={workingDir} onChange={(e) => setWorkingDir(e.target.value)} placeholder={installDir} spellCheck={false} />
              <button className="btn" onClick={async () => { const p = await pickFolder("Working folder", installDir); if (p) setWorkingDir(p); }}>…</button>
            </div>
          </label>
        </div>
        <label className="field" style={{ marginTop: 12 }}>Launch options <span className="hint">Optional, passed exactly as typed</span><input className="input" value={args} onChange={(e) => setArgs(e.target.value)} placeholder="-windowed" spellCheck={false} /></label>

        {prefWorst === "danger" && (
          <div className="callout bad" style={{ marginTop: 18 }}>
            <I.IconAlert size={16} />
            <div className="grow">
              The selected program has serious warnings. Nova won't run it until you confirm, and you'll be asked again before it starts.
              <label className="row" style={{ marginTop: 10, gap: 8, cursor: "pointer", color: "var(--text)" }}>
                <button className={`check ${ack ? "on" : ""}`} onClick={() => setAck(!ack)} aria-label="I understand">{ack && <I.IconCheck size={12} strokeWidth={3} />}</button>
                I understand and want to add it anyway
              </label>
            </div>
          </div>
        )}
      </div>
      <div className="modal-foot">
        <button className="btn ghost" onClick={onBack} style={{ marginRight: "auto" }}><I.IconBack size={14} /> Back</button>
        <span className="faint" style={{ fontSize: 12 }}>Files stay where they are. Nothing is copied or moved.</span>
        <button className="btn primary" disabled={busy || !prefC || !title.trim() || !installDir || (prefWorst === "danger" && !ack)} onClick={add}>
          {busy ? <span className="spinner" /> : <I.IconPlus size={15} />} Add to library
        </button>
      </div>
    </>
  );
}

export function ImportWizard({ onClose }: { onClose: () => void }) {
  const [step, setStep] = useState<Step>({ s: "choose" });

  const scanFolder = async (path: string) => {
    setStep({ s: "scanning", path });
    try {
      const scan = await api.scanFolder(path);
      setStep({ s: "review", scan, single: false });
    } catch (e) {
      toast(errText(e), "bad");
      setStep({ s: "choose" });
    }
  };
  const chooseFolder = async () => {
    const p = await pickFolder("Choose the game's folder");
    if (p) scanFolder(p);
  };
  const chooseExe = async () => {
    const p = await pickExe("Choose the game's executable");
    if (!p) return;
    try {
      const c = await api.inspectExe(p);
      const parts = p.split(/[\\/]/);
      parts.pop();
      const root = parts.join(p.includes("\\") ? "\\" : "/");
      const folder = parts[parts.length - 1] ?? "";
      const generic = /^(bin|binaries|win64|win32|x64|x86|game)$/i.test(folder);
      setStep({
        s: "review",
        single: true,
        scan: { root, suggestedTitle: c.pe.productName || (generic ? c.name.replace(/\.exe$/i, "") : folder) || c.name, developer: c.pe.companyName, publisher: c.pe.companyName, engine: "", candidates: [c], filesSeen: 1, totalSize: c.size, inaccessible: 0, truncated: false, alreadyInLibrary: [] },
      });
    } catch (e) {
      toast(errText(e), "bad");
    }
  };
  const scanStores = async () => {
    setStep({ s: "stores", busy: true, reports: [], added: 0 });
    try {
      const r = await api.scanStores();
      await refresh();
      setStep({ s: "stores", busy: false, reports: r.reports, added: r.added });
    } catch (e) {
      toast(errText(e), "bad");
      setStep({ s: "choose" });
    }
  };

  // Opened by dropping a folder or .exe onto the window.
  useEffect(() => {
    const p = get().importPath;
    if (!p) return;
    set({ importPath: null });
    if (/\.(exe|bat|cmd)$/i.test(p)) {
      api.inspectExe(p).then(() => scanFolder(p.replace(/[\\/][^\\/]*$/, ""))).catch((e) => toast(errText(e), "bad"));
    } else scanFolder(p);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const done = (g: Game) => {
    putGame(g);
    toast(`Added ${g.title}`, "good");
    onClose();
    go({ name: "game", id: g.id });
  };

  const title = useMemo(() => {
    switch (step.s) {
      case "choose": return "Add a game";
      case "scanning": return "Looking inside the folder…";
      case "review": return step.single ? "Add executable" : "Import game folder";
      case "stores": return "Store libraries";
    }
  }, [step]);

  return (
    <div className="modal-back" onMouseDown={(e) => e.target === e.currentTarget && step.s === "choose" && onClose()}>
      <div className={`modal glass ${step.s === "review" ? "wide" : ""}`} role="dialog" aria-modal="true" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <div className="modal-head">
          <div className="grow">
            <h2>{title}</h2>
            {step.s === "review" && <div className="dim selectable ellipsis" style={{ marginTop: 4, fontSize: 12.5 }}>{step.scan.root}</div>}
          </div>
          <button className="btn ghost icon sm" onClick={onClose} aria-label="Close"><I.IconClose size={16} /></button>
        </div>
        {step.s === "choose" && (
          <div className="modal-body">
            <div className="choice-grid">
              <button className="choice" onClick={chooseFolder}>
                <div className="ic"><I.IconFolder size={20} /></div>
                <div className="t">Import a game folder</div>
                <div className="d">Portable games, extracted ZIPs, old installs. Nova looks inside and suggests the right executable.</div>
              </button>
              <button className="choice" onClick={chooseExe}>
                <div className="ic"><I.IconFile size={20} /></div>
                <div className="t">Add an executable</div>
                <div className="d">Point straight at a game's .exe. It's launched from where it is.</div>
              </button>
              <button className="choice" onClick={scanStores}>
                <div className="ic"><I.IconRefresh size={20} /></div>
                <div className="t">Find store games</div>
                <div className="d">Add everything Steam, Epic Games and GOG have installed on this PC.</div>
              </button>
            </div>
            <div className="callout info" style={{ marginTop: 18 }}>
              <I.IconShield size={16} />
              <span>Importing never runs anything. Nova reads file headers to tell games from installers and tools, and flags anything that looks unsafe. Tip: you can also drop a folder onto the window.</span>
            </div>
          </div>
        )}
        {step.s === "scanning" && (
          <div className="modal-body"><div className="empty" style={{ padding: 60 }}><div className="spinner lg" /><div className="selectable">{step.path}</div></div></div>
        )}
        {step.s === "review" && <Review scan={step.scan} single={step.single} onBack={() => setStep({ s: "choose" })} onDone={done} />}
        {step.s === "stores" && (
          <>
            <div className="modal-body">
              {step.busy ? (
                <div className="empty" style={{ padding: 60 }}><div className="spinner lg" /><div>Reading Steam, Epic and GOG manifests…</div></div>
              ) : (
                <div className="card list">
                  {step.reports.map((r) => (
                    <div key={r.store} className="list-row">
                      <span className="dot" style={{ color: r.detected ? "var(--good)" : "var(--faint)" }} />
                      <div className="grow">
                        <div className="title">{r.store}</div>
                        <div className="sub ellipsis">{r.detected ? `${r.found} installed · ${r.added} new` : r.error}</div>
                      </div>
                      {r.detected && <span className="faint ellipsis" style={{ fontSize: 12, maxWidth: 300 }}>{r.location}</span>}
                    </div>
                  ))}
                </div>
              )}
            </div>
            <div className="modal-foot">
              {!step.busy && <span className="dim grow">{step.added ? `Added ${step.added} game${step.added === 1 ? "" : "s"}.` : "Your library is up to date."}</span>}
              <button className="btn primary" disabled={step.busy} onClick={onClose}>Done</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
