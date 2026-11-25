// End-to-end test of the real Nova binary through WebDriver (tauri-driver + WebKitWebDriver).
// Everything is real (database, scanner, launcher, playtime, lagless engine) except the native
// file picker, which is answered by a stub because WebDriver can't click OS dialogs.
//
//   node e2e/run.mjs <path-to-nova-binary> <fixtures-root>
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync, rmSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { execSync } from "node:child_process";
import { join } from "node:path";

const [, , APP, WORLD] = process.argv;
const SHOTS = new URL("../docs/screenshots/", import.meta.url).pathname;
mkdirSync(SHOTS, { recursive: true });
const DATA = join(WORLD, "..", "data");
setTimeout(() => { console.error("suite timed out"); process.exit(2); }, 240000).unref();
// A local update server publishing "9.9.9". Debug builds accept NOVA_UPDATE_FEED; release builds
// only read the compiled-in feed.
// "New version" = the same app, stripped of debug info (so it's a realistic size and still runs).
execSync(`strip -o ${APP}.update ${APP}`);
const payload = readFileSync(`${APP}.update`);
const sha = createHash("sha256").update(payload).digest("hex");
const server = createServer((req, res) => {
  if (req.url === "/latest.json") {
    const base = `http://127.0.0.1:${server.address().port}`;
    res.end(JSON.stringify({ version: "9.9.9", notes: "Test update", platforms: { "windows-x86_64": { url: `${base}/Nova.exe`, sha256: sha, size: payload.length } } }));
  } else if (req.url === "/Nova.exe") res.end(payload);
  else (res.statusCode = 404), res.end();
}).listen(0, "127.0.0.1");
await new Promise((r) => server.once("listening", r));

const env = {
  ...process.env,
  NOVA_UPDATE_FEED: `http://127.0.0.1:${server.address().port}/latest.json`,
  GSETTINGS_BACKEND: "memory",
  NOVA_DATA_DIR: DATA,
  NOVA_STEAM_PATH: join(WORLD, "Steam"),
  NOVA_EPIC_MANIFESTS: join(WORLD, "Epic/Manifests"),
  NOVA_LAGLESS_DIR: join(WORLD, "lagless"),
  LAGLESS_FAKE_PERF: "1",
  LAGLESS_NULL_OUTPUT: "1",
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const ok = (cond, msg) => {
  console.log(`${cond ? "  ✓" : "  ✗"} ${msg}`);
  if (!cond) failures++;
};

const driver = spawn("tauri-driver", ["--port", "4444"], { env, stdio: ["ignore", "ignore", "inherit"] });
await sleep(1500);

const W = "http://127.0.0.1:4444";
let sid;
async function wd(method, path, body) {
  const r = await fetch(`${W}/session${sid ? "/" + sid : ""}${path}`, { signal: AbortSignal.timeout(30000), method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json();
  if (j.value && j.value.error) throw new Error(`${path}: ${j.value.error} ${j.value.message}`);
  return j.value;
}
async function start() {
  sid = undefined;
  const v = await wd("POST", "", { capabilities: { alwaysMatch: { "tauri:options": { application: APP } } } });
  sid = v.sessionId;
  await waitFor(() => !!document.querySelector(".shell"), "app shell");
}
const js = (fn, ...args) => wd("POST", "/execute/sync", { script: typeof fn === "string" ? fn : `return (${fn})(...arguments)`, args });
async function waitFor(pred, what, timeout = 15000, ...args) {
  const t = Date.now();
  while (Date.now() - t < timeout) {
    try {
      if (await js(pred, ...args)) return true;
    } catch {
      /* page reloading */
    }
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${what}`);
}
async function shot(name) {
  await sleep(450); // let animations settle
  const b64 = await wd("GET", "/screenshot");
  writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(b64, "base64"));
}
// Click the first element matching `sel` whose text contains `text`.
async function click(sel, text = "") {
  await waitFor((s, t) => [...document.querySelectorAll(s)].some((e) => e.textContent.includes(t) && !e.disabled), `${sel} "${text}"`, 15000, sel, text);
  await js((s, t) => [...document.querySelectorAll(s)].find((e) => e.textContent.includes(t) && !e.disabled).click(), sel, text);
  await sleep(250);
}
async function type(sel, text, clear = true) {
  const el = await wd("POST", "/element", { using: "css selector", value: sel });
  const id = Object.values(el)[0];
  if (clear) await wd("POST", `/element/${id}/clear`, {});
  await wd("POST", `/element/${id}/value`, { text });
}
const text = () => js(() => document.body.innerText);
const cards = () => js(() => [...document.querySelectorAll(".game-card .t")].map((e) => e.textContent));

try {
  rmSync(DATA, { recursive: true, force: true });
  console.log("1. First launch: store libraries are found automatically");
  await start();
  await waitFor(() => document.querySelectorAll(".game-card").length >= 3, "store games", 20000);
  const c1 = await cards();
  ok(c1.includes("Hades") && c1.includes("Hollow Knight"), `Steam games listed (${c1.join(", ")})`);
  ok(c1.includes("Alan Wake 2"), "Epic game listed");
  ok(!c1.some((t) => t.includes("Redistributables")), "Steam tools filtered out");
  ok(await js(() => [...document.querySelectorAll(".game-card img")].length >= 2), "Steam's cached cover art is used");
  await shot("01-library");

  console.log("2. Import a portable game folder");
  await click(".sidebar .btn", "Add game");
  await js(`window.__novaTestPick = ${JSON.stringify(join(WORLD, "Games/Hollow Sky v1.4.2 [Portable]"))}`);
  await click(".choice", "Import a game folder");
  await waitFor(() => !!document.querySelector(".cand"), "scan results");
  const first = await js(() => document.querySelector(".cand .n").textContent);
  ok(first === "HollowSky.exe", `best guess is the game, not a helper (${first})`);
  ok((await js(() => document.querySelector(".modal input").value)) === "Hollow Sky", "title cleaned from folder name");
  ok((await text()).includes("Unity"), "engine detected");
  await click(".modal .btn.ghost.sm", "other program");
  const t2 = await text();
  ok(t2.includes("Disguised extension"), "disguised Readme.txt.exe flagged");
  ok(t2.includes("Crash reporter") && t2.includes("Installer") && t2.includes("Redistributable"), "helpers classified");
  await shot("02-import-review");
  await click(".modal-foot .btn.primary", "Add to library");
  await waitFor(() => !!document.querySelector(".details h1"), "details page");
  ok((await js(() => document.querySelector(".details h1").textContent)) === "Hollow Sky", "details page opens");
  ok((await text()).includes("Moonlit Studio"), "developer read from the exe's version info");
  await shot("03-details");

  console.log("2b. Game icons");
  await click(".sidebar .btn", "Add game");
  await js(`window.__novaTestPick = ${JSON.stringify(join(WORLD, "Games/Starfall"))}`);
  await click(".choice", "Import a game folder");
  await waitFor(() => !!document.querySelector(".cand"), "Starfall scan");
  await waitFor(() => !!document.querySelector(".modal .gen .ico"), "icon preview in import", 10000).then(() => ok(true, "import preview shows the exe's icon"), () => ok(false, "import preview shows the exe's icon"));
  await js(() => { const i = document.querySelector(".modal input"); i.focus(); });
  await type(".modal input", "Starfall");
  await click(".modal-foot .btn.primary", "Add to library");
  await waitFor(() => document.querySelector(".details h1")?.textContent === "Starfall", "Starfall details");
  await waitFor(() => { const i = document.querySelector(".details .head .gen .ico"); return i && i.complete && i.naturalWidth >= 128; }, "extracted icon renders", 10000).then(
    () => ok(true, "icon extracted from a real Windows exe and rendered (256px)"),
    () => ok(false, "icon extracted from a real Windows exe and rendered"),
  );
  await shot("03b-icon-details");
  await click(".sidebar .nav", "Library");
  await waitFor(() => !!document.querySelector(".game-card .gen .ico"), "icon on library card");
  ok(true, "generated cover uses the game's icon");
  ok(await js(() => [...document.querySelectorAll(".game-card")].some((c) => c.textContent.includes("Hollow Knight") && c.querySelector(".card-meta img.game-icon"))), "Steam game shows its cached icon");

  console.log("3. A game that really runs: launch, warnings, hand-off, playtime");
  await click(".sidebar .btn", "Add game");
  await js(`window.__novaTestPick = ${JSON.stringify(join(WORLD, "Games/Tiny Quest/play.sh"))}`);
  await click(".choice", "Add an executable");
  await waitFor(() => !!document.querySelector(".cand"), "exe review");
  ok((await js(() => document.querySelector(".modal input").value)) === "Tiny Quest", "title from folder");
  await click(".modal-foot .btn.primary", "Add to library");
  await waitFor(() => document.querySelector(".details h1")?.textContent === "Tiny Quest", "Tiny Quest details");
  await click(".details .btn.primary.lg", "Play");
  await waitFor(() => document.body.innerText.includes("This is a .sh script"), "safety prompt before running a script");
  ok(true, "script warning shown before launch");
  await shot("04-launch-warning");
  await click(".modal-foot .btn", "Start");
  await waitFor(() => !!document.querySelector(".nowplaying"), "now playing bar");
  ok(true, "game running (launcher stub exited; tracked via its folder)");
  await sleep(1500);
  await shot("05-playing");
  await waitFor(() => !document.querySelector(".nowplaying"), "game exit detected", 30000);
  await waitFor(() => document.querySelector(".stats .v")?.textContent !== "Never played", "playtime recorded", 5000);
  const pt = await js(() => document.querySelector(".stats .v").textContent);
  ok(/min|< 1/.test(pt), `playtime recorded (${pt})`);
  await click(".tabs button", "Activity");
  await waitFor(() => document.querySelectorAll(".list-row").length >= 1, "session in history");
  ok(true, "session listed in Activity");

  console.log("4. Working directory matters: a wrong one fails clearly, not silently");
  await click(".tabs button", "Launch");
  await type(".card .input", "/definitely/missing");
  await click(".btn.primary.sm", "Save launch options");
  ok((await text()).includes("isn't an existing folder"), "invalid working folder rejected on save");

  console.log("5. Search, favorites, collections");
  await click(".sidebar .nav", "Library");
  await type("#search", "holl");
  await sleep(300);
  const found = await cards();
  ok(found.length === 2 && found.every((t) => t.toLowerCase().includes("holl")), `search narrows (${found.join(", ")})`);
  await type("#search", "moonlit");
  await sleep(300);
  ok((await cards()).join() === "Hollow Sky", "search matches developer");
  await type("#search", "");
  await click(".nav-label button");
  await waitFor(() => !!document.querySelector(".nav input"), "new collection field");
  await type(".nav input", "Indie gems\uE007", false);
  await waitFor(() => document.body.innerText.includes("Indie gems"), "collection created");
  ok(true, "collection created from sidebar");

  console.log("6. lagless inside Nova");
  await click(".sidebar .nav", "Audio");
  await waitFor(() => document.body.innerText.includes("powered by lagless"), "audio page");
  await waitFor(() => !!document.querySelector(".fps-big"), "live game stats from lagless", 15000);
  ok(true, "lagless game stats live in Nova (synthetic game)");
  await waitFor(() => [...document.querySelectorAll(".set-row .t")].some((e) => e.textContent.includes("lagless-e2e")), "receiver discovered", 20000).then(
    () => ok(true, "embedded sender discovered the lagless receiver"),
    () => ok(false, "embedded sender discovered the lagless receiver"),
  );
  await waitFor(() => document.querySelector(".big-num")?.textContent.includes("ms"), "streaming latency", 15000).then(
    () => ok(true, "streaming with live latency"),
    () => ok(false, "streaming with live latency"),
  );
  await shot("06-audio");
  await click(".sidebar .nav", "Settings");
  await shot("07-settings");

  console.log("7. Remove from library never deletes files");
  await click(".sidebar .nav", "Library");
  await js(() => [...document.querySelectorAll(".game-card")].find((e) => e.textContent.includes("Hollow Sky") && !e.textContent.includes("Knight")).click());
  await waitFor(() => document.querySelector(".details h1")?.textContent === "Hollow Sky", "details");
  await js(() => document.querySelector('[aria-label="More"]').click());
  await click(".menu button", "Remove from library");
  ok((await text()).includes("No game files are deleted"), "remove dialog promises files stay");
  await click(".modal-foot .btn.danger", "Remove from library");
  await waitFor(() => !!document.querySelector(".lib-head"), "back to library");
  ok(!(await cards()).includes("Hollow Sky"), "entry removed");
  ok(existsSync(join(WORLD, "Games/Hollow Sky v1.4.2 [Portable]/HollowSky.exe")) && existsSync(join(WORLD, "Games/Hollow Sky v1.4.2 [Portable]/HollowSky_Data/level0")), "game files still on disk");

  console.log("8. Everything persists across restarts");
  await wd("DELETE", "");
  await sleep(1500);
  await start();
  await waitFor(() => document.querySelectorAll(".game-card").length >= 4, "library after restart", 20000);
  const c2 = await cards();
  ok(c2.includes("Tiny Quest") && c2.includes("Hades") && c2.includes("Starfall") && !c2.includes("Hollow Sky"), `library restored (${c2.join(", ")})`);
  ok(await js(() => !!document.querySelector(".game-card .gen .ico")), "icons persist across restarts");
  ok((await text()).includes("Indie gems"), "collections restored");
  ok((await text()).toLowerCase().includes("continue playing"), "last played game featured");
  await sleep(800);
  await shot("08-after-restart");

  console.log("9. Over-the-air update");
  await click(".sidebar .nav", "Settings");
  if (await js(() => [...document.querySelectorAll(".btn.sm")].some((b) => b.textContent.includes("Check now") && !b.disabled))) await click(".btn.sm", "Check now");
  await waitFor(() => document.body.innerText.includes("9.9.9 is downloaded and verified"), "update ready", 30000);
  ok(true, "update found, downloaded and verified");
  await click(".sidebar .nav", "Library");
  ok(await js(() => !!document.querySelector(".update-card") && document.querySelector(".update-card").innerText.includes("9.9.9")), "sidebar offers Restart to update");
  await shot("09-update-ready");
  const before = statSync(APP).mtimeMs;
  const oldPid = execSync(`pgrep -f -x ${APP} | head -1`).toString().trim();
  await js(() => [...document.querySelectorAll(".update-card button")][0].click());
  await sleep(4000);
  ok(statSync(APP).mtimeMs > before, "running executable replaced in place");
  const pids = execSync(`pgrep -f "${APP} --after-update" || true`).toString().trim();
  ok(pids.length > 0, "new version relaunched");
  ok(execSync(`pgrep -f -x ${APP} || true`).toString().trim().split("\n").every((p) => p !== oldPid), "old instance exited");
  if (pids) execSync(`kill ${pids.split("\n").join(" ")} || true`);
  try { await wd("DELETE", ""); } catch { /* the session's app restarted */ }
  sid = undefined;
} catch (e) {
  console.error("FAILED:", e.message);
  try { console.error("SIDEBAR:", await js(() => document.querySelector(".sidebar")?.innerText + " | active=" + [...document.querySelectorAll(".nav.active")].map((e) => e.textContent).join(","))); } catch {}
  try { console.error("UPDATE:", await js(() => [...document.querySelectorAll(".set-row")].map((r) => r.innerText).filter((t) => t.startsWith("Nova")).join(" | "))); } catch {}
  try { console.error("UI:", await js(() => document.getElementById("crash")?.textContent || "")); } catch {}
  failures++;
  try {
    await shot("zz-failure");
  } catch {
    /* no session */
  }
} finally {
  driver.kill();
  server.close();
}
console.log(failures ? `\n${failures} check(s) failed` : "\nAll checks passed");
process.exit(failures ? 1 : 0);
