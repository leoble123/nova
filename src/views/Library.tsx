import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { fail, go, putGame, refresh, refreshCollections, set, toast, useStore } from "../store";
import type { Game } from "../types";
import { play } from "../launch";
import { Confirm, Cover, fmtAgo, fmtPlaytime, heroUrl, Seg, sourceName, useMenu, type MenuItem } from "../components/ui";
import * as I from "../components/Icons";

type Sort = "recent" | "name" | "playtime" | "added";
const SIZES = [140, 172, 214];

function load<T>(k: string, d: T): T {
  try {
    const v = localStorage.getItem(k);
    return v ? (JSON.parse(v) as T) : d;
  } catch {
    return d;
  }
}
function save(k: string, v: unknown) {
  try {
    localStorage.setItem(k, JSON.stringify(v));
  } catch {
    /* storage unavailable */
  }
}

export function gameMenu(g: Game, collections: { id: number; name: string }[], askRemove: (g: Game) => void): MenuItem[] {
  const items: MenuItem[] = [
    { label: g.running ? "Running" : "Play", icon: <I.IconPlay size={13} />, disabled: g.running || !g.installed, onClick: () => play(g) },
    { label: "View details", icon: <I.IconInfo size={15} />, onClick: () => go({ name: "game", id: g.id }) },
    { sep: true },
    {
      label: g.favorite ? "Remove from favorites" : "Add to favorites",
      icon: <I.IconStar size={15} />,
      onClick: async () => {
        try {
          putGame(await api.updateGame(g.id, { favorite: !g.favorite }));
        } catch (e) {
          fail(e);
        }
      },
    },
  ];
  if (collections.length) {
    items.push({ header: "Collections" });
    for (const c of collections) {
      const member = g.collections.includes(c.id);
      items.push({
        label: c.name,
        icon: <I.IconCollection size={14} />,
        checked: member,
        onClick: async () => {
          try {
            await api.setInCollection(c.id, g.id, !member);
            putGame(await api.getGame(g.id));
            refreshCollections();
          } catch (e) {
            fail(e);
          }
        },
      });
    }
  }
  items.push({ sep: true });
  if (g.installDir) items.push({ label: "Show in folder", icon: <I.IconFolder size={15} />, onClick: () => api.reveal(g.installDir).catch(fail) });
  items.push({
    label: g.hidden ? "Unhide" : "Hide from library",
    icon: g.hidden ? <I.IconEye size={15} /> : <I.IconEyeOff size={15} />,
    onClick: async () => {
      try {
        putGame(await api.updateGame(g.id, { hidden: !g.hidden }));
      } catch (e) {
        fail(e);
      }
    },
  });
  items.push({ label: "Remove from library…", icon: <I.IconTrash size={15} />, danger: true, onClick: () => askRemove(g) });
  return items;
}

export function RemoveConfirm({ game, onClose, onDone }: { game: Game; onClose: () => void; onDone?: () => void }) {
  return (
    <Confirm
      title={`Remove ${game.title}?`}
      body={
        <>
          It's removed from Nova's library along with its playtime history.
          <br />
          <b style={{ color: "var(--text)" }}>No game files are deleted</b>
          {game.installDir ? <> — everything in <span className="selectable">{game.installDir}</span> stays exactly where it is.</> : "."}
        </>
      }
      confirm="Remove from library"
      danger
      onClose={onClose}
      onYes={async () => {
        try {
          await api.removeGame(game.id);
          await refresh();
          toast(`Removed ${game.title} from the library`, "info");
          onDone?.();
        } catch (e) {
          fail(e);
        }
      }}
    />
  );
}

function GameCard({ g, index, onMenu }: { g: Game; index: number; onMenu: (e: React.MouseEvent, g: Game) => void }) {
  return (
    <div
      className="game-card"
      tabIndex={0}
      style={{ animationDelay: `${Math.min(index, 24) * 18}ms` }}
      onClick={() => go({ name: "game", id: g.id })}
      onKeyDown={(e) => {
        if (e.key === "Enter") go({ name: "game", id: g.id });
        if (e.key === " " && g.installed && !g.running) {
          e.preventDefault();
          play(g);
        }
      }}
      onContextMenu={(e) => onMenu(e, g)}
      aria-label={g.title}
    >
      <Cover game={g} />
      {g.installed && !g.running && (
        <div style={{ position: "absolute", inset: 0, aspectRatio: "2/3", pointerEvents: "none" }}>
          <button
            className="play"
            style={{ pointerEvents: "auto", position: "absolute", left: "50%", top: "50%" }}
            aria-label={`Play ${g.title}`}
            onClick={(e) => {
              e.stopPropagation();
              play(g);
            }}
          >
            <I.IconPlay size={20} />
          </button>
        </div>
      )}
      <div className="card-meta">
        <div className="t ellipsis">{g.title}</div>
        <div className="s ellipsis">
          {g.problem && !g.installed ? <span>{g.problem.split(" (")[0]}</span> : g.playtimeSecs ? <span>{fmtPlaytime(g.playtimeSecs)}</span> : <span>{sourceName(g.source)}</span>}
        </div>
      </div>
    </div>
  );
}

function ContinueHero({ g }: { g: Game }) {
  const h = heroUrl(g);
  return (
    <div className="hero-row" onClick={() => go({ name: "game", id: g.id })}>
      {h.url && <div className={`bg ${h.blur ? "blur" : ""}`} style={{ backgroundImage: `url("${h.url}")` }} />}
      {!h.url && <div className="bg" style={{ background: "radial-gradient(80% 120% at 80% 0%, rgba(154,168,255,0.18), transparent), #121419" }} />}
      <div className="content">
        <div className="thumb"><Cover game={g} badges={false} /></div>
        <div className="grow">
          <div className="eyebrow">{g.running ? "Playing now" : "Continue playing"}</div>
          <h2 className="ellipsis">{g.title}</h2>
          <div className="dim">{fmtPlaytime(g.playtimeSecs)} played · last played {fmtAgo(g.lastPlayed).toLowerCase()}</div>
        </div>
        {g.installed && !g.running && (
          <button className="btn primary lg" onClick={(e) => { e.stopPropagation(); play(g); }}>
            <I.IconPlay size={16} /> Play
          </button>
        )}
      </div>
    </div>
  );
}

export function Library({ collection, filter }: { collection?: number; filter?: string }) {
  const games = useStore((s) => s.games);
  const loaded = useStore((s) => s.loaded);
  const collections = useStore((s) => s.collections);
  const settings = useStore((s) => s.settings);
  const [q, setQ] = useState("");
  const [chip, setChip] = useState<string>(() => load("lib.chip", "all"));
  const [sort, setSort] = useState<Sort>(() => load("lib.sort", "recent"));
  const [removing, setRemoving] = useState<Game | null>(null);
  const [scanning, setScanning] = useState(false);
  const menu = useMenu();
  useEffect(() => save("lib.chip", chip), [chip]);
  useEffect(() => save("lib.sort", sort), [sort]);
  const size = settings?.cardSize ?? 1;
  const coll = collections.find((c) => c.id === collection);

  const sources = useMemo(() => [...new Set(games.map((g) => g.source))], [games]);
  const list = useMemo(() => {
    let l = games.filter((g) => settings?.showHidden || !g.hidden || filter === "hidden");
    if (collection) l = l.filter((g) => g.collections.includes(collection));
    if (filter === "favorites") l = l.filter((g) => g.favorite);
    if (filter === "recent") l = l.filter((g) => g.lastPlayed);
    if (chip === "installed") l = l.filter((g) => g.installed);
    else if (chip === "missing") l = l.filter((g) => !g.installed);
    else if (chip !== "all") l = l.filter((g) => g.source === chip);
    const needle = q.trim().toLowerCase();
    if (needle) {
      const words = needle.split(/\s+/);
      l = l.filter((g) => {
        const hay = `${g.title} ${g.developer} ${g.publisher} ${sourceName(g.source)}`.toLowerCase();
        return words.every((w) => hay.includes(w));
      });
    }
    const byName = (a: Game, b: Game) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true });
    const s = filter === "recent" ? "recent" : sort;
    l = [...l].sort((a, b) => {
      if (s === "name") return byName(a, b);
      if (s === "playtime") return b.playtimeSecs - a.playtimeSecs || byName(a, b);
      if (s === "added") return b.addedAt - a.addedAt;
      return (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0) || byName(a, b);
    });
    return l;
  }, [games, collection, filter, chip, q, sort, settings?.showHidden]);

  const hero = !q && !collection && !filter && chip === "all" ? [...games].filter((g) => g.lastPlayed && !g.hidden).sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0))[0] : undefined;
  const title = coll?.name ?? (filter === "favorites" ? "Favorites" : filter === "recent" ? "Recently played" : "Library");

  const scanStores = async () => {
    setScanning(true);
    try {
      const r = await api.scanStores();
      await refresh();
      const found = r.reports.filter((x) => x.detected);
      toast(found.length ? `${r.added ? `Added ${r.added} game${r.added === 1 ? "" : "s"}` : "No new games"} from ${found.map((x) => x.store).join(", ")}` : "No Steam, Epic or GOG installation found on this PC", r.added ? "good" : "info");
    } catch (e) {
      fail(e);
    }
    setScanning(false);
  };

  if (loaded && games.length === 0) {
    return (
      <div className="page">
        <div className="empty">
          <div className="art"><I.NovaMark size={38} /></div>
          <h2>Your library is empty</h2>
          <div style={{ maxWidth: 440 }}>Import a game folder (portable games, extracted ZIPs, anything with an .exe), or pull in what Steam, Epic and GOG already have installed.</div>
          <div className="row" style={{ marginTop: 10 }}>
            <button className="btn primary" onClick={() => set({ importOpen: true })}><I.IconFolder size={15} /> Import a game folder</button>
            <button className="btn" onClick={scanStores} disabled={scanning}>{scanning ? <span className="spinner" /> : <I.IconRefresh size={15} />} Find store games</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="page" style={{ ["--card-w" as string]: `${SIZES[size]}px` }}>
      {hero && <ContinueHero g={hero} />}
      <div className="lib-head">
        <h1>{title}</h1>
        <span className="count mono">{list.length}</span>
      </div>
      <div className="toolbar">
        <div className="search">
          <I.IconSearch size={15} />
          <input id="search" className="input" placeholder="Search games, developers…" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.key === "Escape" && (setQ(""), (e.target as HTMLInputElement).blur())} spellCheck={false} />
          {!q && <kbd>/</kbd>}
        </div>
        <div className="chips">
          {[["all", "All"], ["installed", "Installed"], ...sources.filter(() => sources.length > 1).map((s) => [s, sourceName(s)]), ["missing", "Not installed"]].map(([k, l]) => (
            <button key={k} className={`chip ${chip === k ? "on" : ""}`} onClick={() => setChip(k)}>{l}</button>
          ))}
        </div>
        <div className="grow" />
        {filter !== "recent" && (
          <select className="select" style={{ width: 170, height: 32 }} value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort">
            <option value="recent">Recently played</option>
            <option value="name">Name</option>
            <option value="playtime">Most played</option>
            <option value="added">Recently added</option>
          </select>
        )}
        <Seg value={size} options={[[0, "S"], [1, "M"], [2, "L"]]} onChange={async (v) => set({ settings: await api.setSettings({ cardSize: v }) })} />
      </div>
      {list.length === 0 ? (
        <div className="empty" style={{ paddingTop: 50 }}>
          <h2>{q ? `Nothing matches “${q}”` : coll ? "This collection is empty" : "No games here"}</h2>
          <div>{coll ? "Right-click any game and pick this collection to add it." : "Try a different filter."}</div>
        </div>
      ) : (
        <div className="grid">
          {list.map((g, i) => (
            <GameCard key={g.id} g={g} index={i} onMenu={(e, g) => menu.open(e, gameMenu(g, collections, setRemoving))} />
          ))}
        </div>
      )}
      {menu.el}
      {removing && <RemoveConfirm game={removing} onClose={() => setRemoving(null)} />}
    </div>
  );
}
