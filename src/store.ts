import { useSyncExternalStore } from "react";
import { api, errText } from "./api";
import type { AudioState, Collection, Game, LaunchFail, NovaSettings, RunningGame, UpdateInfo } from "./types";

export type View =
  | { name: "library"; collection?: number; filter?: string }
  | { name: "game"; id: number }
  | { name: "audio" }
  | { name: "settings" };

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "good" | "bad" | "warn";
  action?: { label: string; run: () => void };
}

export interface LaunchPrompt {
  game: Game;
  fail: LaunchFail;
  exeId?: number | null;
}

interface State {
  prompt: LaunchPrompt | null;
  update: UpdateInfo | null;
  games: Game[];
  collections: Collection[];
  running: RunningGame[];
  settings: NovaSettings | null;
  audio: AudioState | null;
  view: View;
  history: View[];
  toasts: Toast[];
  importOpen: boolean;
  importPath: string | null;
  loaded: boolean;
}

let state: State = {
  prompt: null,
  update: null,
  games: [],
  collections: [],
  running: [],
  settings: null,
  audio: null,
  view: { name: "library" },
  history: [],
  toasts: [],
  importOpen: false,
  importPath: null,
  loaded: false,
};
const subs = new Set<() => void>();

export function set(patch: Partial<State> | ((s: State) => Partial<State>)) {
  const p = typeof patch === "function" ? patch(state) : patch;
  state = { ...state, ...p };
  subs.forEach((f) => f());
}

export function get() {
  return state;
}

export function useStore<T>(sel: (s: State) => T): T {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => sel(state),
  );
}

export function go(view: View) {
  set((s) => ({ view, history: [...s.history.slice(-20), s.view] }));
}

export function back() {
  set((s) => {
    const h = [...s.history];
    const prev = h.pop() ?? { name: "library" as const };
    return { view: prev, history: h };
  });
}

let toastId = 1;
export function toast(text: string, kind: Toast["kind"] = "info", action?: Toast["action"]) {
  const id = toastId++;
  set((s) => ({ toasts: [...s.toasts.slice(-3), { id, text, kind, action }] }));
  setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), action ? 9000 : 5200);
}

export function fail(e: unknown) {
  toast(errText(e), "bad");
}

export async function refresh() {
  try {
    const [games, collections, running] = await Promise.all([api.listGames(), api.collections(), api.running()]);
    set({ games, collections, running, loaded: true });
  } catch (e) {
    fail(e);
    set({ loaded: true });
  }
}

/** Replace one game in the list after an edit (no full reload). */
export function putGame(g: Game) {
  set((s) => ({ games: s.games.some((x) => x.id === g.id) ? s.games.map((x) => (x.id === g.id ? g : x)) : [...s.games, g] }));
}

export async function refreshCollections() {
  set({ collections: await api.collections() });
}
