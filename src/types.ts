export type Source = "manual" | "steam" | "epic" | "gog";

export interface Executable {
  id: number;
  gameId: number;
  path: string;
  label: string;
  args: string;
  workingDir: string;
}

export interface Game {
  id: number;
  title: string;
  source: Source;
  sourceId: string;
  developer: string;
  publisher: string;
  description: string;
  installDir: string;
  workingDir: string;
  launchArgs: string;
  launchUri: string;
  preferredExeId: number | null;
  cover: string;
  hero: string;
  favorite: boolean;
  hidden: boolean;
  addedAt: number;
  lastPlayed: number | null;
  playtimeSecs: number;
  notes: string;
  /** Icon file in the art cache; "" not looked for yet, "-" none found. */
  icon: string;
  executables: Executable[];
  collections: number[];
  installed: boolean;
  problem: string;
  running: boolean;
}

export interface Collection {
  id: number;
  name: string;
  gameCount: number;
}

export interface Session {
  id: number;
  gameId: number;
  startedAt: number;
  endedAt: number;
  secs: number;
  avgFps: number | null;
  low1: number | null;
}

export type Level = "info" | "warn" | "danger";
export interface Warning {
  level: Level;
  code: string;
  text: string;
}

export interface PeInfo {
  valid: boolean;
  isDll: boolean;
  arch: string;
  gui: boolean;
  console: boolean;
  productName: string;
  companyName: string;
  fileDescription: string;
  requiresAdmin: boolean;
  hasSignature: boolean;
  packed: boolean;
}

export type Role = "game" | "launcher" | "tool" | "installer" | "redist" | "crash" | "anticheat" | "script";

export interface Candidate {
  path: string;
  relPath: string;
  name: string;
  size: number;
  depth: number;
  role: Role;
  score: number;
  pe: PeInfo;
  warnings: Warning[];
}

export interface ScanResult {
  root: string;
  suggestedTitle: string;
  developer: string;
  publisher: string;
  engine: string;
  candidates: Candidate[];
  filesSeen: number;
  totalSize: number;
  inaccessible: number;
  truncated: boolean;
  alreadyInLibrary: [number, string][];
}

export interface NewExecutable {
  path: string;
  label: string;
  args: string;
  workingDir: string;
}

export interface NewGame {
  title: string;
  source?: Source;
  developer: string;
  publisher: string;
  description: string;
  installDir: string;
  workingDir: string;
  launchArgs: string;
  executables: NewExecutable[];
  preferred: number;
  coverPath: string;
}

export interface LaunchFail {
  code: "missing-exe" | "missing-dir" | "needs-confirm" | "needs-admin" | "already-running" | "spawn-failed" | "no-target";
  message: string;
  warnings: Warning[];
}

export interface RunningGame {
  gameId: number;
  startedAt: number;
  title: string;
  elevated: boolean;
}

export interface SessionEvent {
  gameId: number;
  state: "started" | "ended" | "failed";
  startedAt: number;
  secs: number;
  message: string;
}

export interface StoreReport {
  store: string;
  detected: boolean;
  location: string;
  found: number;
  added: number;
  error: string;
}

export interface NovaSettings {
  scanStoresOnStart: boolean;
  onlineArt: boolean;
  laglessEnabled: boolean;
  minimizeOnLaunch: boolean;
  cardSize: number;
  showHidden: boolean;
  reduceMotion: boolean;
}

// ---- lagless (mirrors lagless's engine status and PerfSnapshot)

export interface PerfSnap {
  game: string;
  fps: number;
  low1: number;
  low01: number;
  frameMs: number;
  gpuName: string;
  gpuUtil: number | null;
  gpuTemp: number | null;
  gpuClockMhz: number | null;
  gpuPowerW: number | null;
  gpuPowerLimitW: number | null;
  vramUsedGb: number | null;
  vramTotalGb: number | null;
  gpuLimit: string;
  cpu: number;
  cpuCoreMax: number;
  ramUsedGb: number;
  ramTotalGb: number;
  verdict: string;
  tip: string;
  stutters: number;
  lastStutter: string;
  fpsProblem: string;
}

export interface Perf {
  enabled: boolean;
  live: boolean;
  fpsShown?: number;
  snap?: PerfSnap;
  history?: number[];
  osd?: { on: boolean; corner: number; style: number; size: number; bg: number; text: number; color: boolean };
}

export interface Peer {
  name: string;
  addr: string;
  link: string;
  linkName: string;
  codec: string;
  lossless: boolean;
  rttMs: number;
  lossPct: number;
  latencyMs: number;
  syncMs: number;
  playing: boolean;
  included: boolean;
}

export interface AudioState {
  available: boolean;
  external: boolean;
  enabled: boolean;
  paused: boolean;
  mode: "send" | "receive";
  phase: "off" | "paused" | "starting" | "searching" | "idle" | "streaming" | "error";
  error: string | null;
  latencyMs: number;
  peerName: string;
  link: string;
  linkName: string;
  deviceName?: string;
  codec: string;
  sampleRate: number;
  lossless: boolean;
  rttMs: number;
  lossPct: number;
  bufferMs: number;
  targetMs?: number;
  jitterMs?: number;
  fixed?: number;
  lost?: number;
  underruns?: number;
  fec?: number;
  engineMs?: number;
  engineDefaultMs?: number;
  capturing?: boolean;
  systemVolume?: number | null;
  peers: Peer[];
  syncDevices: boolean;
  volume: number;
  muted: boolean;
  levels: [number, number];
  silent: boolean;
  mic: { enabled?: boolean; active?: boolean; latencyMs?: number; problem?: string | null };
  perf: Perf;
  alert: string | null;
}

export type SourceSel = { Loopback: string | null } | { Input: string | null } | "Tone";

export interface LaglessSettings {
  mode: "Send" | "Receive";
  source: SourceSel;
  to: string;
  quality: "Auto" | "Pcm24" | "Pcm16" | "Opus";
  fec: number;
  frame_ms: number;
  mic_receive: boolean;
  mic_out: string | null;
  multipath: boolean;
  engine_boost: boolean;
  follow_windows_volume: boolean;
  perf_stats: boolean;
  perf_osd: boolean;
  perf_osd_corner: number;
  perf_osd_style: number;
  perf_osd_size: number;
  perf_osd_bg: number;
  perf_osd_text: number;
  perf_osd_color: boolean;
  perf_smoothing: number;
  [k: string]: unknown;
}

export interface AudioSettingsInfo {
  settings: LaglessSettings;
  outputs: string[];
  inputs: string[];
  cable: string | null;
  external: boolean;
  version: string;
  windows: boolean;
}
