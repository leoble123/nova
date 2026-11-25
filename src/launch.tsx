import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import { get, putGame, set, toast } from "./store";
import type { Game, LaunchFail } from "./types";
import { Modal } from "./components/ui";
import { IconAlert, IconInfo, IconShield } from "./components/Icons";

export async function play(game: Game, opts: { exeId?: number | null; confirmed?: boolean; admin?: boolean } = {}) {
  try {
    const r = await api.launch(game.id, opts);
    set((s) => ({ running: [...s.running.filter((x) => x.gameId !== r.gameId), r] }));
    putGame({ ...game, running: true, lastPlayed: r.startedAt });
    toast(`Starting ${game.title}`, "good");
    if (get().settings?.minimizeOnLaunch) getCurrentWindow().minimize();
  } catch (e) {
    const f = e as LaunchFail;
    if (f && typeof f === "object" && "code" in f) {
      if (f.code === "needs-confirm" || f.code === "needs-admin") {
        set({ prompt: { game, fail: f, exeId: opts.exeId } });
        return;
      }
      toast(f.message, "bad");
    } else {
      toast(String(e), "bad");
    }
  }
}

/** Shown before running something that looks risky, or that wants administrator rights. */
export function LaunchPromptModal() {
  const p = get().prompt;
  if (!p) return null;
  const close = () => set({ prompt: null });
  const admin = p.fail.code === "needs-admin";
  const danger = p.fail.warnings.some((w) => w.level === "danger");
  return (
    <Modal
      size="narrow"
      onClose={close}
      title={admin ? "Run as administrator?" : `Start ${p.game.title}?`}
      sub={admin ? p.game.title : "Nova checked this program before starting it."}
      foot={
        <>
          <button className="btn ghost" onClick={close}>Cancel</button>
          {admin ? (
            <button className="btn primary" onClick={() => { close(); play(p.game, { exeId: p.exeId, confirmed: true, admin: true }); }}>
              <IconShield size={15} /> Run as administrator
            </button>
          ) : (
            <button className={`btn ${danger ? "danger" : "primary"}`} onClick={() => { close(); play(p.game, { exeId: p.exeId, confirmed: true }); }}>
              {danger ? "Start anyway" : "Start"}
            </button>
          )}
        </>
      }
    >
      {admin ? (
        <div className="col" style={{ gap: 14 }}>
          <div className="dim" style={{ lineHeight: 1.55 }}>{p.fail.message}</div>
          <div className="callout warn"><IconAlert size={16} /> Administrator programs can change anything on this PC. Only continue if you trust where this game came from. Windows will ask you to confirm.</div>
        </div>
      ) : (
        <div className="warns" style={{ marginTop: 0, gap: 10 }}>
          {p.fail.warnings.map((w, i) => (
            <div key={i} className={`warn-line ${w.level}`}>
              {w.level === "info" ? <IconInfo size={15} /> : <IconAlert size={15} />}
              <span>{w.text}</span>
            </div>
          ))}
          <div className="faint" style={{ fontSize: 12, marginTop: 6 }}>Windows Defender and SmartScreen still check the program as usual. You won't be asked again unless the file changes.</div>
        </div>
      )}
    </Modal>
  );
}
