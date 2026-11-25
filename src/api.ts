import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type * as T from "./types";

let artDir = "";
export async function initArt() {
  artDir = await invoke<string>("art_dir");
}
export function artUrl(name: string): string {
  if (!name || name === "-") return "";
  if (name.startsWith("data:")) return name; // previews before import
  const sep = artDir.includes("\\") ? "\\" : "/";
  return convertFileSrc(artDir + sep + name);
}

export function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

export const api = {
  listGames: () => invoke<T.Game[]>("list_games"),
  getGame: (id: number) => invoke<T.Game>("get_game", { id }),
  addGame: (game: T.NewGame) => invoke<T.Game>("add_game", { game }),
  updateGame: (id: number, update: Partial<T.Game>) => invoke<T.Game>("update_game", { id, update }),
  removeGame: (id: number) => invoke<void>("remove_game", { id }),
  relocate: (id: number, dir: string) => invoke<T.Game>("relocate_game", { id, dir }),
  scanFolder: (path: string) => invoke<T.ScanResult>("scan_folder", { path }),
  inspectExe: (path: string) => invoke<T.Candidate>("inspect_exe", { path }),
  addExecutable: (gameId: number, exe: T.NewExecutable) => invoke<T.Game>("add_executable", { gameId, exe }),
  updateExecutable: (exe: T.Executable) => invoke<T.Game>("update_executable", { exe }),
  removeExecutable: (gameId: number, exeId: number) => invoke<T.Game>("remove_executable", { gameId, exeId }),
  setArt: (gameId: number, kind: "cover" | "hero" | "icon", path: string) => invoke<T.Game>("set_art", { gameId, kind, path }),
  clearArt: (gameId: number, kind: "cover" | "hero" | "icon") => invoke<T.Game>("clear_art", { gameId, kind }),
  exeIconPreview: (path: string) => invoke<string>("exe_icon_preview", { path }),
  imagePreview: (path: string) => invoke<string>("image_preview", { path }),
  fetchOnlineArt: (gameId: number) => invoke<T.Game>("fetch_online_art", { gameId }),
  launch: (id: number, opts: { exeId?: number | null; confirmed?: boolean; admin?: boolean } = {}) =>
    invoke<T.RunningGame>("launch_game", { id, exeId: opts.exeId ?? null, confirmed: !!opts.confirmed, admin: !!opts.admin }),
  stop: (id: number) => invoke<boolean>("stop_game", { id }),
  running: () => invoke<T.RunningGame[]>("running_games"),
  sessions: (gameId: number) => invoke<T.Session[]>("sessions", { gameId }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  collections: () => invoke<T.Collection[]>("list_collections"),
  createCollection: (name: string) => invoke<number>("create_collection", { name }),
  renameCollection: (id: number, name: string) => invoke<void>("rename_collection", { id, name }),
  deleteCollection: (id: number) => invoke<void>("delete_collection", { id }),
  setInCollection: (collectionId: number, gameId: number, member: boolean) => invoke<void>("set_in_collection", { collectionId, gameId, member }),
  scanStores: () => invoke<{ reports: T.StoreReport[]; added: number }>("scan_stores"),
  getSettings: () => invoke<T.NovaSettings>("get_settings"),
  setSettings: (settings: Partial<T.NovaSettings>) => invoke<T.NovaSettings>("set_settings", { settings }),
  appInfo: () => invoke<{ version: string; dataDir: string; laglessVersion: string; windows: boolean }>("app_info"),
  // lagless
  audioState: () => invoke<T.AudioState>("audio_state"),
  audioVolume: (volume: number) => invoke<void>("audio_set_volume", { volume }),
  audioMuted: (muted: boolean) => invoke<void>("audio_set_muted", { muted }),
  audioPaused: (paused: boolean) => invoke<void>("audio_set_paused", { paused }),
  audioMic: (on: boolean) => invoke<void>("audio_set_mic", { on }),
  audioDevice: (name: string, included: boolean) => invoke<void>("audio_set_device", { name, included }),
  audioSync: (on: boolean) => invoke<void>("audio_set_sync", { on }),
  audioSettings: () => invoke<T.AudioSettingsInfo>("audio_settings"),
  audioApply: (settings: T.LaglessSettings) => invoke<void>("audio_apply_settings", { settings }),
  allowFps: () => invoke<void>("audio_allow_fps"),
  openFirewall: () => invoke<void>("audio_open_firewall"),
};

export function on<P>(event: string, cb: (p: P) => void): () => void {
  let un: UnlistenFn | null = null;
  let dead = false;
  listen<P>(event, (e) => cb(e.payload)).then((u) => (dead ? u() : (un = u)));
  return () => {
    dead = true;
    un?.();
  };
}

// End-to-end tests answer file pickers through this (WebDriver can't click native dialogs).
// Only script already running inside Nova's own webview can set it.
function testPick(): string | null | undefined {
  return (window as unknown as { __novaTestPick?: string | null }).__novaTestPick;
}

export async function pickFolder(title: string, defaultPath?: string): Promise<string | null> {
  if (testPick() !== undefined) return testPick() ?? null;
  const r = await open({ directory: true, multiple: false, title, defaultPath });
  return typeof r === "string" ? r : null;
}

export async function pickExe(title: string, defaultPath?: string): Promise<string | null> {
  if (testPick() !== undefined) return testPick() ?? null;
  const r = await open({
    multiple: false,
    title,
    defaultPath,
    filters: [{ name: "Programs", extensions: ["exe", "bat", "cmd"] }, { name: "All files", extensions: ["*"] }],
  });
  return typeof r === "string" ? r : null;
}

export async function pickImage(title: string): Promise<string | null> {
  if (testPick() !== undefined) return testPick() ?? null;
  const r = await open({ multiple: false, title, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
  return typeof r === "string" ? r : null;
}
