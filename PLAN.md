# Fate Launcher — a Rust launcher for Minecraft 1.20.1 + Cursed Fate pack

Target: one launcher, one pack, 1.20.1 Forge only. Low-end PCs (2 cores / 4 GB RAM).
Every player picks **one** ability from the mods; everyone else's abilities stay locked for them.

---

## 1. What Horizon actually is (read this before copying anything)

`horizon-rust/horizon` is **Minecraft: Java Edition 26.3 rewritten from scratch in Rust** (MIT).
It is not a launcher that runs other versions. Two separate things live in that repo:

| Part | What it is | Useful to us? |
|---|---|---|
| `crates/` | The game — client, server, world, protocol, its own mod system | **No.** It is a 1:1 Rust reimplementation of 26.3. It will never run a 1.20.1 Forge jar. |
| `launcher/` | A separate Cargo workspace, GPUI desktop app | **Yes** — this is the part worth studying. |

The launcher already contains the exact machinery you need, and it is permissively licensed:

- **Instance model** + `instances.json`, per-instance memory, JVM args, launch hooks, env vars, wrapper commands (`horizon_launcher::launch`).
- **Java runtime management** — reads `javaVersion` from a version JSON, finds Mojang's runtimes in `minecraft/runtime/`, reads `release` files from installed JDKs without launching them, and downloads Mojang's runtime (sha1-checked) when none matches.
- **Version/asset resolution** from piston-meta / piston-data / libraries.minecraft.net / resources.download.minecraft.net — never a mirror. Asset index, rules evaluation, natives.
- **Mod browser** — Modrinth search filtered by version+loader, install only after SHA-512 matches.
- **Server hosting** — copies a prepared runtime into `server-runtime/`, runs `init`/`config`/`start`/`backup`/`stop`, talks to it via a control API.
- **Microsoft account** handling, self-update handoff, CLI + MCP surface.

"Reverse engineering" here is just reading source. Because it is MIT you may reuse code as long as the
copyright notice and licence text travel with it. Do not ship the name "Horizon".

**What does not transfer:** Horizon plays 26.3 in Rust. To play 1.20.1 Forge with these mods you must
launch the **real Java game**. So the launcher is the Rust part; the game is stock Forge. That is
exactly what Prism / Modrinth App do — and it is the only road that ends with your mod list running.

### Do not copy the UI layer

The launcher is built on GPUI (Zed's framework, via a fork). It is GPU-heavy, has a large native
dependency surface per OS, and draws a glass/pixel-art look. On a 2-core / 4 GB machine the launcher
should be the smallest possible process, because every MB it holds is an MB the JVM cannot have.

**Recommendation: Tauri 2 + Rust core** (or `egui` if you want zero webview). A Tauri launcher idles
at ~80–130 MB; GPUI at 250 MB+. Copy Horizon's *crate boundaries and data model*, not its renderer.

---

## 2. Reality check — five constraints that shape the whole design

1. **Rust cannot load Forge mods.** Your 37 jars are JVM bytecode against Minecraft 1.20.1 + Forge 47.x.
   They run in a real JVM. Anyone promising "Rust launcher runs the mods" means "Rust launcher starts a JVM".
2. **"Mods asleep in background" is not a Forge feature.** Forge loads every jar in `mods/` at startup and
   cannot unload a mod at runtime. What is possible: cut what they *do*, shrink the JVM, and never start
   the client when it is not needed. Section 6 is honest about this.
3. **Ability locking is a server-side mod job, not launcher work.** A launcher cannot stop a player from
   using an ability — the player owns the files. Authority must live on the server. Section 5.
4. **A 2-core / 4 GB box cannot host and play this pack at once.** 175 MB of jars (TaCZ alone is 52 MB),
   a 36 MB core mod, Photon shaders. Client wants ~2 GB, server wants ~1.5–2 GB, Windows eats 1.2 GB.
   **One role per machine**: potato players join, the best machine (or a cheap VPS) hosts. The launcher
   should make this explicit with Host / Join buttons instead of pretending both fit.
5. **Pack hygiene is currently a blocker.** `ImmediatelyFast-Forge-1.5.5+1.20.4` is a **1.20.4** build in a
   1.20.1 pack — expect a crash or silent no-op. There are duplicates (`(1)`, `(2)` suffixes), and
   `oculus` + `xenon` (an Embeddium fork) needs a verified pairing. Fix before writing launcher code, or
   you will debug the launcher for mod bugs.

---

## 3. Architecture

One repo, one Cargo workspace, one small Java mod.

```
fate-launcher/
├─ core/                     # no UI, fully testable, no webview
│  ├─ hl-paths      data dirs, instance dirs, media-to-another-drive (copy+verify+switch)
│  ├─ hl-meta       piston-meta/piston-data, version JSON, rules, natives, asset index
│  ├─ hl-fetch      parallel ranged downloader, resume, sha1/sha512 verify, offline cache
│  ├─ hl-java       discover JDKs (release files, registry), install Mojang JRE 17
│  ├─ hl-loader     Forge 1.20.1 install + version JSON merge
│  ├─ hl-auth       MSA device-code flow, refresh in OS keychain, offline profile
│  ├─ hl-launch     argv builder, memory budget, pre/post hooks, process supervision, log tail
│  ├─ hl-server     dedicated Forge server: install, eula, properties, whitelist, backup, start/stop
│  ├─ hl-pack       pack manifest (pinned hashes), apply/repair/update, client-side mod flags
│  └─ hl-ability    ability catalog + server-side state sync + admin reset (talks to the mod)
├─ app/                      # Tauri 2 shell: Home, Instances, Mods, Abilities, Server, Settings
├─ mod/fate-lock/            # tiny Forge 1.20.1 mod (Java 17) — the real enforcement
└─ pack/pack.json            # the single source of truth for the mod set
```

Launch path is a straight line with no magic:

```
pack.json → ensure 1.20.1 client + asset index 5 + Forge 47.4.x → ensure Java 17
        → ensure mods/ matches manifest hashes → build argv → spawn JVM → supervise
```

### Forge install (the one fiddly part)

Cheapest reliable route: run the official installer headlessly.

```bash
# requires launcher_profiles.json to already exist in the target dir, or it refuses
java -jar forge-1.20.1-47.4.20-installer.jar --installClient "<instance dir>"
```

It runs the processors and writes `versions/1.20.1-forge-47.4.20/1.20.1-forge-47.4.20.json`.
We then read that JSON instead of reimplementing `install_profile.json` processing. Keep a manual
fallback (extract `install_profile.json`, resolve libraries + processors yourself) only if you later
need offline installs with no Java present.

### The resulting command line (what `hl-launch` must produce)

```
java -Xms1800m -Xmx1800m -XX:+UseG1GC -XX:MaxGCPauseMillis=50 -XX:G1HeapRegionSize=8M
  -Djava.library.path=<natives> -Dfile.encoding=UTF-8 -Dfml.earlyprogresswindow=false
  -Dforge.logging.console.level=warn
  -p <module path: bootstraplauncher, securejarhandler, asm, ...>
  -cp <libraries + client.jar + forge universal> 
  cpw.mods.bootstraplauncher.BootstrapLauncher
  --username <name> --version 1.20.1-forge-47.4.20 --gameDir <dir> --assetsDir <dir>
  --assetIndex 5 --uuid <hex> --accessToken <token> --userType msa|legacy
  --versionType release --launchTarget forgeclient
  --fml.forgeVersion 47.4.20 --fml.mcVersion 1.20.1 --fml.forgeGroup net.minecraftforge
  --fml.mcpVersion 20230612.114412
  --width <w> --height <h>
```

`--assetIndex 5` is 1.20.1's index. Login for LAN-only play can be offline (`--userType legacy`,
random UUID) which is enough for "Open to LAN" and a LAN server with `online-mode=false`.
Real MSA sign-in is only needed for online servers, skins and UUID identity — phase 4, not phase 1.

---

## 4. Pack manifest — the thing that makes 37 jars survive updates

```jsonc
// pack/pack.json
{
  "pack": "cursed-fate", "mc": "1.20.1", "loader": "forge-47.4.20",
  "java": 17,
  "mods": [
    { "file": "geckolib-forge-1.20.1-4.8.4.jar", "sha512": "…", "source": "modrinth:geckolib" },
    { "file": "cursedfate-1.0.39.jar", "sha512": "…", "source": "local" }
  ],
  "profiles": {
    "potato": { "xmx": 1800, "renderDistance": 6, "shaders": false, "particles": "minimal" },
    "normal": { "xmx": 4096, "renderDistance": 10, "shaders": true }
  }
}
```

- Anything on Modrinth is downloaded and hash-checked.
- Anything not redistributable (many CurseForge packs forbid it) is `"source": "local"` — the launcher
  copies it from the user's folder and verifies the hash instead of shipping it. That keeps you legal
  without breaking the "one click" feel.
- `repair` re-verifies every hash; `update` diffs the manifest and only downloads changed files.

---

## 5. Ability system — the actual feature

**Rule: the server decides. The client only displays.**

A small Forge mod, `fate-lock`, ~1500 lines of Java:

```
PlayerAbility (capability, persisted in player NBT)
  id: "ichigo" | "kashimo" | "angel" | "antigravity" | "meimei" | ...
  claimed_at, locked: bool
ClaimTable (server, world save)
  ability_id -> player_uuid        # one owner per ability, first come
```

Flow:

1. **On first join** the server sends the catalog (only entries whose mod is actually loaded) plus the
   already-claimed list. Client opens a server-driven selection screen.
2. **Pick** → `ClaimAbilityPacket(id)`. Server validates: mod present, id exists, not claimed, player has
   no ability yet. Then writes the claim and syncs.
3. **Grant** — one adapter per mod, exactly one of these mechanics:
   - **Item-gated** (Zanpakuto, Devil Fruit, Stand arrow, TaCZ special): grant the item to the owner only.
     Everyone else simply never receives the trigger. This is the cleanest gate and should be the
     default for every ability mod that supports it.
   - **Capability/effect-gated**: attach the mod's capability/attribute modifiers to the owner only.
   - **Keybind/event-gated**: cancel the activation event (`LivingTickEvent`, `InputEvent`, the mod's own
     packet) when `player.ability != mod`.
   - **Unhookable**: don't grant, and block the mod's item `use`/`attack` events as a hard fallback.
4. **Lock others** — the mod hides the HUD/keys for abilities the player does not own (client config
   flag per ability mod) and refuses grants. The player still *sees* other players transformed; that is
   required for the pack to look right.
5. **Release / transfer** — admin command `fate reset <player>`, or auto-release after N days offline
   (configurable), so a friend who quits does not hold Ichigo forever.

Trust boundary: the claim packet is the only client input, it is rate-limited, and the grant is computed
on the server from the registry — never from a client-supplied list. A modified client can at most
re-request its own ability.

**Cheap alternative** while the mod is not ready: KubeJS (already in the pack) + scoreboard-based claims.
It handles attribute/effect-based abilities fine and lets you prototype the whole flow in a day, but it
cannot cleanly block another mod's keybinds. Prototype in KubeJS, ship `fate-lock`.

---

## 6. "Sleeping mods" — what is actually possible

Forge cannot unload a mod. Three real levers, in order of payoff:

| Lever | Effect | How |
|---|---|---|
| **Don't run what you don't need** | biggest win | Never start the client to browse servers/mods. Keep the dedicated server stopped when nobody is online. One JVM per role, no extra daemons. |
| **Cut per-mod work at runtime** | large | ModernFix dynamic resources (already installed), FerriteCore (memory), EntityCulling, ImmediatelyFast, Dynamic FPS, Log Begone. Ship a `potato` config profile that turns off particles/shaders/animations for abilities the player does not own. |
| **Cut startup cost** | large on 2 cores | Build one **AppCDS archive** at first launch (`-XX:ArchiveClassesAtExit`), then launch with `-XX:SharedArchiveFile`. Class loading is the main CPU spike on a dual-core; this typically removes a big chunk of the "loading mods" wall time. Plus `-Dfml.earlyprogresswindow=false` and a warm jar cache. |
| Not possible | — | Unloading a jar mid-session, or having a client with fewer mods than the server (Forge rejects the handshake unless mods are declared optional/server-only, and content mods cannot be). |

So the honest promise to the user is: **the launcher stays tiny, the game starts faster, and unused
ability features stop costing frames — not "mods are unloaded".** Say it that way and nobody is disappointed.

---

## 7. Performance budget for 2 cores / 4 GB

| Item | Budget |
|---|---|
| Launcher idle | ≤ 130 MB (Tauri), ≤ 60 MB (egui) |
| Client heap | `-Xms1800m -Xmx1800m` (equal, no resizing churn) |
| GC | G1, `MaxGCPauseMillis=50`, `G1HeapRegionSize=8M`, `+ParallelRefProcEnabled` |
| Render distance | 6 (potato) / 10 (normal), simulation 5 |
| Shaders | off on potato (Oculus + Photon is ~15 fps on integrated GPUs) |
| Resolution | start at 1280×720 with `--width/--height` |
| Server | separate machine, 2 GB, `view-distance=6`, `simulation-distance=5` |

Do not use ZGC or Aikar's flags here — they assume more cores and more RAM than you have.

---

## 8. Multiplayer: LAN + host a server

Two different features; build both because players will ask for either.

- **Host** — `hl-server` runs the Forge 1.20.1 dedicated server on the host machine:
  install (`--installServer`), write `eula.txt`, `server.properties` (port, `online-mode`,
  `view-distance=6`), whitelist from the pack's player list, `start`/`stop`/`backup` (zip the `world/`),
  console + log tail in the launcher. Print the LAN IPv4 and a copyable `ip:port`.
- **Join** — the launcher adds that address to `servers.dat` (or launches with `--server ip --port p`),
  so the player lands straight in.
- **LAN discovery** — cheapest useful version: host advertises over mDNS/`_minecraft._udp` and, if you
  keep vanilla behaviour, "Open to LAN" already broadcasts to LAN clients on UDP 4445. A "Friends" tab
  that lists discovered hosts is a nice-to-have, not phase one.
- **Invite over the internet** — Horizon solves this with Tailscale-style pairing. Do not build a relay
  in v1. Ship "use Tailscale/ZeroTier, paste the 100.x address", or a tiny UDP hole-punch later.

`online-mode=true` requires every player to own the game and sign in with MSA (phase 4). For a private
friends server, `online-mode=false` + whitelist is the pragmatic default.

---

## 9. Phases and milestones

| # | Phase | Done when | Rough |
|---|---|---|---|
| 0 | **Pack validation** | One known-good launch exists in Prism with the exact jar set; duplicates and the 1.20.4 ImmediatelyFast removed; Forge version pinned; shader decision made. Save the real `java ...` line as the reference. | 1–2 d |
| 1 | `hl-meta` + `hl-java` + `hl-launch` | Launcher starts **vanilla 1.20.1 offline** from a clean folder, assets + natives included. | ~1 wk |
| 2 | `hl-loader` + `hl-pack` | Same launcher starts **1.20.1 Forge with all 37 mods**, hash-verified, zero manual steps. | 3–5 d |
| 3 | Tauri UI | Home (Play), Instances, Mods (search/repair), Settings (RAM, resolution, potato profile), Console with crash highlighting. | 1–2 wk |
| 4 | `hl-auth` | MSA sign-in, token refresh in OS keychain, real UUID/skin, joins online servers. | ~1 wk |
| 5 | `hl-server` | One-click host, whitelist, backup, console, join-by-address, LAN discovery. | ~1 wk |
| 6 | **`fate-lock` mod** | Claim once, locked for everyone else, survives relog, admin reset, works for at least the 3–4 main mods (item-gated first). | 3–6 wk |
| 7 | Perf pass | AppCDS archive, measured cold-start and RAM on a real 2-core/4 GB box; publish the numbers. | 3–4 d |
| 8 | Updates | Manifest channel + delta downloads; launcher self-update (Tauri updater). | ~1 wk |

Phases 1–2 are the "does this idea even work" checkpoint. Do not start the UI before a JVM boots from
your own argv — every launcher bug in the world looks like a mod bug until you have a console.

Solo, Rust + Java, part-time: **~2.5–3 months** to phase 6. Phases 1–2 alone are ~2 weeks and are worth
doing first because they prove the whole concept.

---

## 10. Risks

- **Ability mods are closed-source jars.** Each adapter in `fate-lock` needs the mod's actual trigger
  mechanism. Item-gated abilities are trivial; keybind-driven ones may need reflection into obfuscated
  internals and can break on every mod update. Budget for 1–2 days per mod you cannot item-gate.
- **TaCZ** ships guns via its own "gunpack" system; its content may not be ability-gated at all —
  treat weapons as gear, not abilities.
- **Client/server mod parity**: all ability mods must be on both sides. Anyone launching a trimmed
  client gets rejected by the handshake. That kills the "two profiles, fewer mods" idea for this pack.
- **Redistribution** of CurseForge jars is usually forbidden — use `"source": "local"` + hash verify.
- **Horizon drift**: its launcher is moving fast (0.2.x weekly). Pin the commit you read.

---

## 11. Start here (next 2 days)

```bash
# 1. clean pack folder
mkdir -p pack/mods && cp <your jars> pack/mods/
# remove: ImmediatelyFast ... 1.20.4, duplicates with (1)/(2), pick one oculus/xenon pairing

# 2. get a known-good prisms instance, launch once, then export the real command line from the log:
#    "ModLauncher running: args [...]"  and the java -cp line above it -> that is your spec

# 3. prove the pieces by hand before writing Rust
java -jar forge-1.20.1-47.4.20-installer.jar --installClient ./minecraft
java -Xmx1800m -cp "<paste from log>" cpw.mods.bootstraplauncher.BootstrapLauncher <args from log>
```

Once that hand-rolled command line works, `hl-launch` is just a typed builder around it, and the rest of
the project is plumbing you can copy the shape of from `horizon/launcher/`.
