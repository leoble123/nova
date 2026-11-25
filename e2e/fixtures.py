"""Builds a realistic test world: a Steam install, an Epic manifest, game folders with real
(tiny) PE executables, and lagless settings. Used by e2e/run.mjs. Usage: fixtures.py <root>"""
import json, os, struct, sys, shutil
from PIL import Image, ImageDraw

root = sys.argv[1]
shutil.rmtree(root, ignore_errors=True)

def pe(product, company, gui=True, size=0, admin=False):
    def entry(k, v):
        kw = (k + "\0").encode("utf-16-le"); vw = (v + "\0").encode("utf-16-le")
        e = struct.pack("<HHH", 0, len(vw) // 2, 1) + kw
        e += b"\0" * (-len(e) % 4) + vw
        return e + b"\0" * (-len(e) % 4)
    rsrc = entry("CompanyName", company) + entry("ProductName", product)
    if admin: rsrc += b'<requestedExecutionLevel level="requireAdministrator"/>'
    rsrc += b"\0" * (-len(rsrc) % 512)
    f = bytearray(0x80); f[0:2] = b"MZ"; f[0x3c:0x40] = struct.pack("<I", 0x80)
    f += b"PE\0\0" + struct.pack("<HHIIIHH", 0x8664, 1, 0, 0, 0, 240, 0x22)
    opt = bytearray(240); opt[0:2] = struct.pack("<H", 0x20b); opt[68:70] = struct.pack("<H", 2 if gui else 3)
    opt[108:112] = struct.pack("<I", 16); opt[112 + 16:112 + 24] = struct.pack("<II", 0x1000, len(rsrc))
    f += opt + b".rsrc\0\0\0" + struct.pack("<IIII", len(rsrc), 0x1000, len(rsrc), 0x200) + b"\0" * 16
    f += b"\0" * (0x200 - len(f)) + rsrc
    if size > len(f): f += b"\0" * (size - len(f))
    return bytes(f)

def put(rel, data):
    p = os.path.join(root, rel); os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "wb").write(data if isinstance(data, bytes) else data.encode())
    return p

def art(rel, w, h, c1, c2, label):
    p = os.path.join(root, rel); os.makedirs(os.path.dirname(p), exist_ok=True)
    im = Image.new("RGB", (w, h)); d = ImageDraw.Draw(im)
    for y in range(h):
        t = y / h; d.line([(0, y), (w, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(c1, c2)))
    for i in range(6):
        d.ellipse([w * (0.1 + 0.13 * i), h * 0.25, w * (0.35 + 0.13 * i), h * 0.25 + w * 0.25], outline=(255, 255, 255, 60), width=2)
    d.text((24, h - 60), label, fill=(255, 255, 255))
    im.save(p, quality=88)

# --- Steam
steam = os.path.join(root, "Steam")
put("Steam/steamapps/libraryfolders.vdf", '"libraryfolders"\n{\n "0" { "path" "%s" }\n}\n' % steam)
for appid, name, d in [("1145360", "Hades", "Hades"), ("367520", "Hollow Knight", "Hollow Knight"), ("228980", "Steamworks Common Redistributables", "x")]:
    put(f"Steam/steamapps/appmanifest_{appid}.acf", f'"AppState"\n{{\n "appid" "{appid}"\n "name" "{name}"\n "installdir" "{d}"\n}}\n')
    os.makedirs(os.path.join(steam, "steamapps/common", d), exist_ok=True)
art("Steam/appcache/librarycache/1145360_library_600x900.jpg", 600, 900, (120, 30, 20), (20, 8, 10), "HADES")
art("Steam/appcache/librarycache/1145360_library_hero.jpg", 1920, 620, (90, 30, 25), (15, 10, 12), "")
art("Steam/appcache/librarycache/367520/abc123/library_600x900.jpg", 600, 900, (40, 55, 90), (8, 10, 20), "HOLLOW KNIGHT")

# --- Epic
put("Epic/Manifests/A1.item", json.dumps({"DisplayName": "Alan Wake 2", "InstallLocation": os.path.join(root, "EpicGames/AlanWake2"), "LaunchExecutable": "AlanWake2.exe",
    "AppName": "Dill", "MainGameAppName": "Dill", "CatalogNamespace": "ns", "CatalogItemId": "item", "bIsIncompleteInstall": False, "AppCategories": ["public", "games"]}))
os.makedirs(os.path.join(root, "EpicGames/AlanWake2"), exist_ok=True)

# --- A portable Unity game (extracted ZIP style)
put("Games/Hollow Sky v1.4.2 [Portable]/HollowSky.exe", pe("Hollow Sky", "Moonlit Studio", size=4_000_000))
put("Games/Hollow Sky v1.4.2 [Portable]/HollowSky_Data/level0", "x")
put("Games/Hollow Sky v1.4.2 [Portable]/UnityPlayer.dll", "x")
put("Games/Hollow Sky v1.4.2 [Portable]/UnityCrashHandler64.exe", pe("Unity Crash Handler", "Unity Technologies"))
put("Games/Hollow Sky v1.4.2 [Portable]/unins000.exe", pe("", ""))
put("Games/Hollow Sky v1.4.2 [Portable]/_CommonRedist/vcredist/vc_redist.x64.exe", pe("Microsoft Visual C++", "Microsoft"))
put("Games/Hollow Sky v1.4.2 [Portable]/Readme.txt.exe", "not a program at all")
put("Games/Hollow Sky v1.4.2 [Portable]/Config Tool.exe", pe("Config", "Moonlit", gui=False))
for i in range(300):
    put(f"Games/Hollow Sky v1.4.2 [Portable]/HollowSky_Data/StreamingAssets/bundle_{i:03}.assets", "a" * 200)

# --- A game that actually runs on this (Linux) test machine: a launcher stub that hands off to
# the real binary in a sub-folder and exits, and depends on its working directory.
g = os.path.join(root, "Games/Tiny Quest")
os.makedirs(os.path.join(g, "bin"), exist_ok=True)
shutil.copy("/bin/sleep", os.path.join(g, "bin/tinyquest"))
put("Games/Tiny Quest/data/ok", "x")
put("Games/Tiny Quest/play.sh", '#!/bin/sh\n# needs the game root as working dir\n[ -f data/ok ] || exit 3\n./bin/tinyquest 6 &\nexit 0\n')
os.chmod(os.path.join(g, "play.sh"), 0o755)

# --- A real Windows program with a real icon (Nova's own Windows build), for icon extraction.
real = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../src-tauri/target/x86_64-pc-windows-gnu/release/nova.exe")
if os.path.exists(real):
    os.makedirs(os.path.join(root, "Games/Starfall"), exist_ok=True)
    shutil.copy(real, os.path.join(root, "Games/Starfall/Starfall.exe"))
# Steam's cached icon (newer clients: a 40-hex-named file in the app's folder).
art("Steam/appcache/librarycache/367520/0123456789abcdef0123456789abcdef01234567.jpg", 64, 64, (200, 210, 230), (90, 100, 140), "")

# --- lagless: send a test tone (no sound card here) with synthetic game stats.
put("lagless/settings.json", json.dumps({"mode": "Send", "source": "Tone", "perf_stats": True, "volume": 0.8}))
print(root)
