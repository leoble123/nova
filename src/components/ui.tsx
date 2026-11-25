import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { artUrl } from "../api";
import type { Game } from "../types";
import { IconClose, IconStar } from "./Icons";

// ------------------------------------------------------------------ formatting

export function fmtPlaytime(secs: number): string {
  if (!secs) return "Never played";
  if (secs < 60) return "< 1 min";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h === 0) return `${m} min`;
  if (h < 10) return m ? `${h} h ${m} min` : `${h} h`;
  return `${h.toLocaleString()} h`;
}

export function fmtAgo(ms: number | null | undefined): string {
  if (!ms) return "Never";
  const d = Date.now() - ms;
  const m = Math.round(d / 60000);
  if (m < 1) return "Just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const days = Math.round(h / 24);
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short", year: days > 300 ? "numeric" : undefined });
}

export function fmtClock(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let i = -1;
  do {
    n /= 1024;
    i++;
  } while (n >= 1024 && i < u.length - 1);
  return `${n < 10 ? n.toFixed(1) : Math.round(n)} ${u[i]}`;
}

export const sourceName = (s: string) => ({ steam: "Steam", epic: "Epic Games", gog: "GOG", manual: "Local" })[s] ?? s;

export function useNow(intervalMs = 1000) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  return now;
}

// ------------------------------------------------------------------ cover art

function hash(s: string) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

/** A typographic cover for games without artwork: muted, title-derived gradient. */
/** The game's own icon (from its executable or store), or nothing. */
export function GameIcon({ game, size = 16 }: { game: Pick<Game, "icon">; size?: number }) {
  const [bad, setBad] = useState(false);
  const src = artUrl(game.icon);
  useEffect(() => setBad(false), [game.icon]);
  if (!src || bad) return null;
  return <img className="game-icon" src={src} width={size} height={size} alt="" draggable={false} onError={() => setBad(true)} />;
}

function Generated({ game }: { game: Pick<Game, "title" | "developer" | "source" | "icon"> }) {
  const [iconOk, setIconOk] = useState(true);
  const icon = iconOk ? artUrl(game.icon) : "";
  useEffect(() => setIconOk(true), [game.icon]);
  const h = hash(game.title);
  const hue = h % 360;
  const hue2 = (hue + 40 + (h >> 8) % 60) % 360;
  const g = `radial-gradient(120% 90% at ${20 + (h % 50)}% ${10 + ((h >> 4) % 30)}%, hsl(${hue} 32% 34%), transparent 60%), radial-gradient(100% 80% at 90% 100%, hsl(${hue2} 30% 22%), transparent 70%), hsl(${hue} 14% 11%)`;
  const len = game.title.length;
  const size = len > 28 ? 17 : len > 16 ? 20 : 25;
  return (
    <div className={`gen ${icon ? "has-icon" : ""}`} style={{ ["--g" as string]: g, ["--gt" as string]: `${size}px` }}>
      {icon && (
        <>
          <img className="ico-bg" src={icon} alt="" draggable={false} />
          <img className="ico" src={icon} alt="" draggable={false} onError={() => setIconOk(false)} />
        </>
      )}
      <div className="gt">{game.title}</div>
      <div className="gs">{game.developer || sourceName(game.source)}</div>
    </div>
  );
}

export function Cover({ game, badges = true }: { game: Game; badges?: boolean }) {
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const src = game.cover && !failed ? artUrl(game.cover) : "";
  useEffect(() => {
    setFailed(false);
    setLoaded(false);
  }, [game.cover]);
  return (
    <div className={`cover ${game.installed ? "" : "missing"}`}>
      {src ? <img src={src} className={loaded ? "" : "loading"} onLoad={() => setLoaded(true)} onError={() => setFailed(true)} draggable={false} alt="" /> : <Generated game={game} />}
      <div className="shade" />
      {badges && (
        <div className="badges">
          {game.running && (
            <span className="pill good">
              <span className="dot live" /> Playing
            </span>
          )}
          {!game.installed && !game.running && <span className="pill">{game.source === "manual" ? "Missing" : "Not installed"}</span>}
          {game.favorite && <IconStar className="fav" size={16} filled />}
        </div>
      )}
    </div>
  );
}

export function heroUrl(g: Game): { url: string; blur: boolean } {
  if (g.hero) return { url: artUrl(g.hero), blur: false };
  if (g.cover) return { url: artUrl(g.cover), blur: true };
  return { url: "", blur: true };
}

// ------------------------------------------------------------------ controls

export function Switch({ on, onChange, disabled, label }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; label?: string }) {
  return <button role="switch" aria-checked={on} aria-label={label} className={`switch ${on ? "on" : ""}`} disabled={disabled} onClick={() => onChange(!on)} />;
}

export function Seg<T extends string | number>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div className="seg" role="radiogroup">
      {options.map(([v, l]) => (
        <button key={String(v)} role="radio" aria-checked={v === value} className={v === value ? "on" : ""} onClick={() => onChange(v)}>
          {l}
        </button>
      ))}
    </div>
  );
}

export function Slider({ value, min = 0, max = 1, step = 0.01, onChange, onCommit, label }: { value: number; min?: number; max?: number; step?: number; onChange: (v: number) => void; onCommit?: (v: number) => void; label?: string }) {
  const fill = ((value - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      className="slider"
      aria-label={label}
      min={min}
      max={max}
      step={step}
      value={value}
      style={{ ["--fill" as string]: `${fill}%` }}
      onChange={(e) => onChange(parseFloat(e.target.value))}
      onPointerUp={(e) => onCommit?.(parseFloat((e.target as HTMLInputElement).value))}
      onKeyUp={(e) => onCommit?.(parseFloat((e.target as HTMLInputElement).value))}
    />
  );
}

// ------------------------------------------------------------------ modal & menu

export function Modal({ title, sub, onClose, children, foot, size = "", closeOnBack = true }: { title: ReactNode; sub?: ReactNode; onClose: () => void; children: ReactNode; foot?: ReactNode; size?: "" | "wide" | "narrow"; closeOnBack?: boolean }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="modal-back" onMouseDown={(e) => closeOnBack && e.target === e.currentTarget && onClose()}>
      <div className={`modal glass ${size}`} role="dialog" aria-modal="true">
        <div className="modal-head">
          <div className="grow">
            <h2>{title}</h2>
            {sub && <div className="dim" style={{ marginTop: 4, fontSize: 13 }}>{sub}</div>}
          </div>
          <button className="btn ghost icon sm" onClick={onClose} aria-label="Close">
            <IconClose size={16} />
          </button>
        </div>
        <div className="modal-body">{children}</div>
        {foot && <div className="modal-foot">{foot}</div>}
      </div>
    </div>
  );
}

export interface MenuItem {
  label?: string;
  icon?: ReactNode;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  sep?: boolean;
  header?: string;
  checked?: boolean;
}

export function Menu({ x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ x: Math.min(x, window.innerWidth - r.width - 8), y: Math.min(y, window.innerHeight - r.height - 8) });
  }, [x, y]);
  useEffect(() => {
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    setTimeout(() => window.addEventListener("mousedown", close), 0);
    window.addEventListener("keydown", key);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);
  return (
    <div ref={ref} className="menu glass" style={{ left: pos.x, top: pos.y }} role="menu">
      {items.map((it, i) =>
        it.sep ? (
          <hr key={i} />
        ) : it.header ? (
          <div className="label" key={i}>{it.header}</div>
        ) : (
          <button
            key={i}
            role="menuitem"
            className={it.danger ? "danger" : ""}
            disabled={it.disabled}
            onClick={() => {
              onClose();
              it.onClick?.();
            }}
          >
            <span style={{ width: 16, display: "grid", placeItems: "center" }}>{it.icon}</span>
            <span className="grow">{it.label}</span>
            {it.checked !== undefined && <span className={`check ${it.checked ? "on" : ""}`} style={{ width: 15, height: 15 }} />}
          </button>
        ),
      )}
    </div>
  );
}

export function useMenu() {
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const el = menu ? <Menu {...menu} onClose={() => setMenu(null)} /> : null;
  return {
    el,
    open: (e: { clientX: number; clientY: number; preventDefault?: () => void }, items: MenuItem[]) => {
      e.preventDefault?.();
      setMenu({ x: e.clientX, y: e.clientY, items });
    },
    openAt: (el: HTMLElement, items: MenuItem[]) => {
      const r = el.getBoundingClientRect();
      setMenu({ x: r.left, y: r.bottom + 6, items });
    },
  };
}

export function Confirm({ title, body, confirm, danger, onYes, onClose }: { title: string; body: ReactNode; confirm: string; danger?: boolean; onYes: () => void; onClose: () => void }) {
  return (
    <Modal
      title={title}
      size="narrow"
      onClose={onClose}
      foot={
        <>
          <button className="btn ghost" onClick={onClose}>Cancel</button>
          <button
            className={`btn ${danger ? "danger" : "primary"}`}
            autoFocus
            onClick={() => {
              onClose();
              onYes();
            }}
          >
            {confirm}
          </button>
        </>
      }
    >
      <div className="dim" style={{ lineHeight: 1.55 }}>{body}</div>
    </Modal>
  );
}
