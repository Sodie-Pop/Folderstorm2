# Video memory with several characters: findings and plan

Investigation for a user report on the session host (FEATURES 5.1, plan
[session-host.md](session-host.md)). Written 2026-11-17 from Folderstorm2
`main` (`a4b0bbf8`). Every claim about the code cites `file:line` on that
commit. Claims about Windows, the driver or the user's machine that a Linux
sandbox cannot check are marked **(verify on Windows)**. Section 11 sorts
what was read in code from what was inferred.

## Status

| Phase | State |
|---|---|
| 0 Ask the user | Questions in section 10 |
| 1 (d) diagnostics | **Built** on `claude/gpu-reset-diagnostics`, to be validated on Windows (below) |
| 2–4 | **Built** on `claude/gpu-memory-share` (the part after the investigation, below), to be validated on Windows |
| 5–6 | Not started |

Phase 1 as built (the human, 2026-11-17: "Should the reset notice say why it
happened? — sure, why not"):

- `crates/aurora-viewer/src/folderstorm/gpu_diag/` (pure, tested) and
  `crates/aurora-viewer/src/app/folderstorm_characters/gpu_diag.rs` (the
  frame's side). Every 5 s a sample of the memory picture: this process's
  driver usage, budget and allocation (`vram.rs`), textures in use, pages,
  target and pressure step, the whole card's use by every program (Windows'
  PDH counter, `gpu_load.rs`: a total, not per program), the card's size,
  each other viewer of the instances registry with its use and whether it
  is in front, the window's size and place. The last minute is kept.
- At the loss (first frame that sees it, in front or behind): the entry
  goes to the log (`GPU loss: likely reason …`) and to
  `<log name>.gpu-losses.txt` next to the log (the last 20 losses): time,
  outcome, likely reason, wgpu's reason and message class, hang or fault,
  window and session-host role / standby, the memory picture at the loss
  and the run-up. The recovery fills in the outcome (`recovered in … ms`,
  same or safe level); behind, it says the loss waits. Figures only.
- The recovery notice says the likely reason (memory full, with the number
  of characters or viewer windows and the GB in use; a frame that took too
  long; a driver error), else Aurora's own text, then where the details are.
  Help › Show the log folder opens the logs.
- Upstream hooks: `app.rs` (one call after `gpu_stall_frame`),
  `app/gpu_recovery.rs` (the notice's body), `ui/bars.rs` (Help menu item),
  `aurora-render` `device_lost.rs` / `lib.rs` (`loss_verdict`, the hang or
  fault verdict kept from the loss line).
- Demo: `AURORA_DEMO_GPU_LOST=<frame>` writes an entry; with
  `AURORA_DEMO_GPU_REASON=memory|hang|fault` the notice shows that reason
  (`docs/TESTING.md`).

Not built from (d): the host showing each character's video memory (d.3,
needs (b)'s protocol field) and the periodic summary line (d.4).

---

## 0. The report

- Windows, RTX 3060 12 GB, 16 GB of RAM at about 84 % in use.
- Session host with one or more extra characters (one viewer process each).
- The viewer's own notice `app-gpu-recovered` ("The graphics card was reset
  … reopened its window on the card again: you stayed logged in"): a device
  loss (Windows TDR) that the active viewer recovered from
  (`crates/aurora-viewer/src/app/gpu_recovery.rs:199-272`, text at
  `crates/aurora-i18n/locales/en.ftl:2950-2951`).
- Garbled avatar face textures.
- The human never sees it on the same card model.

## 1. In short

- **Aurora already does most of option (a).** It reads the real per-process
  budget (`VK_EXT_memory_budget`), the card-wide use of every program
  (Windows PDH counters), and the other Aurora viewers' use (a shared
  registry), and lowers its *textures* to stay under them. What it cannot do
  is give back anything else (geometry, render targets, allocator room), and
  a window behind is treated as needing "no room".
- **Economy is only "minimise the window".** The viewer then relies on
  Aurora's minimised-window texture release (after 1 s, highest pressure
  step). It does **not** drop geometry, shrink render targets, free the
  texture cache, or stop anything else. Whether even the texture release
  happens depends on the minimised window still getting frames, which the
  code itself says are "rare" **(verify on Windows)**. Monitors, the cycling
  monitor, and the monitor that follows a switch all turn Economy into Warm.
- **Most likely cause:** the viewers *together* oversubscribe the 12 GB, and
  with 16 GB of RAM nearly full, Windows' demotion of video memory to system
  memory turns into disk paging; a frame then runs past the 2 s TDR limit.
  The likeliest moments are **switches** (the incoming viewer reloads its
  textures and full-size targets while the outgoing one still holds
  everything) and **open monitors** (each picture rebuilds a full-size set of
  render targets twice in that background viewer, section 3.5).
- **Recommended order:** diagnostics first (d), then fix the monitor's
  render-target churn and make Economy really release (c), then host-given
  shares (b). (e) is a targeted audit, not a blind re-upload.

---

## 2. What Aurora has today (verified in code)

### 2.1 Budget: read from the driver, not a fixed number

| What | Where | Detail |
|---|---|---|
| Per-process budget and usage | `crates/aurora-render/src/vram.rs:1-50` | `VK_EXT_memory_budget`, read again as it changes; the budget used is the **lowest of the last minute** (`BUDGET_WINDOW`, `vram.rs:177`): a drop counts at once, a rise after 60 s. Firestorm reads DXGI `QueryVideoMemoryInfo` once at start (`vram.rs:12-15`). |
| Texture setting | `crates/aurora-viewer/src/settings.rs:672-674`, `2479-2495` | Auto by default: 70 % of the card, at least 1.5 GB left, so **8.4 GB per viewer on a 12 GB card**. Manual value or `AURORA_TEXTURE_BUDGET_MB` (`app.rs:1374-1377`, `texture_budget.rs:120-129`). |
| Texture target | `crates/aurora-viewer/src/scene/texture_budget.rs:146-165` | `min(setting, process target − everything else the process holds)`, where the process target is `max(min(budget − 512 MB, 80 % of budget), 768 MB)`. Never under 256 MB of textures (`:88-89`). |
| Rises slowly | `texture_budget.rs:199-227` | The target rises by 256 MB/s at most (a tester lost the device 1 s after the target jumped 1.6 → 4.4 GB). |
| Other Aurora viewers | `crates/aurora-viewer/src/instances.rs:1-31`, `:334-353` | Each viewer writes its usage, "in front", card LUID every 2 s; `share_cap` splits the budget among viewers **that are in front and hold something**. A window behind, paused or at login "needs no room" (`:115-122`, `:340-341`). Off in demo and timed runs (`app.rs:9380-9398`). |
| Every program on the card (Windows) | `crates/aurora-viewer/src/gpu_load.rs:1-17`, `:111-123` | PDH `GPU Adapter Memory` every 2 s; when all programs together hold more than 90 % of the card (`SYSTEM_SHARE`, `:27-30`), each viewer's textures give back their proportional part of the excess (`texture_budget.rs:167-197`, used at `scene/textures.rs:3669-3678`). |
| Pressure steps | `texture_budget.rs:52-67`, `:354-383` | 0–7; up after 1 s over the target (or 3 s unsettled), down after 5 s under 90 %. Step 7 = textures at their on-screen size less 2 levels. Bakes, HUDs, interface, sculpt maps and terrain are never lowered (`:16-18`) except when "paused behind" (below). |
| Lowering and eviction | `scene/textures.rs:3562-3593`, `texture_budget.rs:85-86` | Lowered on the GPU from the levels held (`TextureTable::shrink`), up to 3,000 a second; unused textures evicted after 5 s under pressure. |
| Texture pages give memory back | `crates/aurora-render/src/dedicated.rs:1-26` | One `VkDeviceMemory` per page, freed when the page goes (wgpu's blocks kept free room counted by the driver). |

### 2.2 Window behind or minimised

- `texture_budget::Background` (`texture_budget.rs:243-301`): behind for
  60 s, or minimised for 1 s, → pressure held at step 7
  (`Pressure::hold_max`, `:333-342`) until in front again. Behind **and past
  the driver's budget** ("crowded") → at once, and the 3D drawing pauses;
  then the bakes are lowered too (`scene/textures.rs:2496-2507`) and every
  texture goes to its last level (`PAUSED_DROPPED`, `scene/textures.rs:159-167`).
- Logged as `texture memory: the window was minimised for 1 s, freeing up
  video memory` / `… in the background for 60 s …` / `… with the video memory
  short …; the 3D drawing pauses` (`scene/textures.rs:3686-3704`).
- "In front" is the session host's role when managed (rule R3):
  `folderstorm_in_front()` feeds `set_window_state` (`app.rs:9377-9379`) and
  the registry's `Own.active` (`app.rs:9390-9394`).

### 2.3 Allocation failure

- `crates/aurora-render/src/gpu_alloc.rs:1-13`: growth sites use
  `try_buffer`/`try_texture` with error scopes; on failure they keep what
  they had and back off.
- Render targets refused: the interface alone until they can be made
  (`renderer.rs:5298-5323`, "Video memory full" notice,
  `last_render.video_memory_full`).
- Texture page refused: the texture waits, logged `texture page … not made
  (…): video memory is full` (`crates/aurora-render/src/textures.rs:755-800`).
- **Nothing besides textures is ever given back under pressure** (stated at
  `vram.rs:38-40`: "Past the budget the viewer has only its textures to give
  back"). Geometry (1.35 GB measured in a busy region,
  `texture_budget.rs:292-297`) and render targets stay.

### 2.4 Device loss and recovery

- Callback: `crates/aurora-render/src/main_thread.rs:583-593` logs `GPU
  device lost (<reason>): <message>` and `GPU work at the loss: …` (hang or
  fault, `device_lost.rs:252-326`). The message is classed into
  `device-lost` / `out-of-memory` / `timeout` / `other`
  (`device_lost.rs:130-165`); wgpu's reason is only Unknown/other.
- Ladder (`app/gpu_recovery.rs:96-124`): 1st loss in 5 min → new window and
  device; 2nd → same with the safe graphics level **saved**; 3rd → close.
- Recovery (`gpu_recovery.rs:199-272`): scene GPU state forgotten
  (`scene/mod.rs:2312-2346`), every texture gets a slot on the new device and
  is decoded again from memory or the disk cache
  (`scene/textures.rs:1864-1924`); a decode staged into the old device's pool
  is detected and done again (`scene/textures.rs:3101-3117`).
- Folderstorm R5 (`crates/folderstorm-session/src/gpu.rs:187-193`,
  `app/folderstorm_characters.rs:146-170`): a viewer **not in front** that
  sees the loss does nothing (`Loss::Wait`), keeps running its session, and
  recovers uncounted when it comes to the front. Called before Aurora's
  ladder (`app/gpu_stall.rs:74`).
- **What is recorded about a recovered loss is thin.** The crash session's
  `alive.txt` carries `vram_mb`, `vram_headroom_60s_mb`, `card_used_mb`,
  `card_others_mb`, `other_viewers`, `window_state`, `loss_*`, `stall_ms`
  (`crash/watchdog.rs:100-113`, `:145-160`), but it is **rewritten every
  5 s** (`watchdog.rs:328-344`) and the recovery resets the crash state
  (`gpu_recovery.rs:246-251`). What survives is one line in
  `gpu-recoveries.txt`: `<unix> <ms> same|safe` (`crash/mod.rs:381-396`).
  The log lines above survive in the viewer's log. No per-process VRAM
  figure is written at the moment of the loss, and nothing is sent to the
  host (the host's `Status.gpu` is `Ok/Busy/Stalled/Lost` only,
  `folderstorm-session/src/ipc.rs:747-759`, `:771-797`).

## 3. What the session host does to background characters (verified)

### 3.1 States

- `Standby` (`crates/folderstorm-session/src/ipc.rs:176-183`): **Warm**
  (default) = window behind the active one; **Economy** = "Its window is
  minimised: Aurora then gives its textures back."
- The background 3D policy (`folderstorm-session/src/behind.rs`) is separate:
  **Off** (default for managed characters) = Aurora's busy-pause state: empty
  draw lists, 3D targets at 1/16 (`PAUSED_RESOLUTION_DIVISOR`, `app.rs:340`,
  applied at `app.rs:11406-11410` through `drawing_paused`,
  `app/busy_pause.rs:87-90`), interface frames at 2–15 fps (default 5).

### 3.2 What Economy does, exactly

1. The host sends `ToViewer::Minimise` to the outgoing character on a
   switch (`folderstorm-session/src/host.rs:1855-1860`) and at connection
   (`host.rs:1520-1521`).
2. The viewer calls `window.set_minimized(true)` unless it is in front
   (`app/folderstorm_characters.rs:261-267`). **That is all Economy does in
   the viewer**; `standby` is otherwise read only by the bar
   (`app/folderstorm_characters/bar.rs:111`).
3. From there Aurora's own rule applies: minimised for 1 s → pressure held at
   step 7 (section 2.2).

What Economy does **not** do:

- **Geometry**: kept (never released under pressure, section 2.3).
- **Render targets**: kept at whatever size the last drawn frame had.
  A zero-size resize is ignored (`main_thread.rs:1031-1034`,
  `renderer.rs:5431-5434`). The Assign (Off) and the Minimise arrive in the
  same `about_to_wait` pump, so the window may be minimised before any paused
  frame shrinks its targets to 1/16 **(verify on Windows: look for a `world
  resolution:` log line after the switch, `renderer.rs:5362-5370`)**.
- **Texture cache**: not dropped. Step 7 lowers textures (on-screen size less
  two levels, by the running maximum of their size on screen, which decays
  while nothing is drawn); **avatar bakes stay at full size** unless the
  viewer is also past its own driver budget (`scene/textures.rs:2496-2507`).
  The CPU copies and the disk cache stay, which is why coming back is quick.
- **Frames**: no change of its own. "Frames are rare in a minimised window"
  (`folderstorm-session/src/managed.rs:113-116`,
  `app/folderstorm_characters.rs:242-243`). The texture release runs inside
  the frame (`press`, `scene/textures.rs:3643`): **if a minimised window gets
  no frame, nothing is released at all (verify on Windows: look for the
  `freeing up video memory` line after an Economy switch).**
- **The registry**: a minimised viewer reports `active = false`
  (`app.rs:9392`), so the viewer in front counts it as needing no room
  (`instances.rs:115-122`) although it may still hold gigabytes.

When Economy is silently Warm:

- a character whose monitor is open (`host.rs:403-405`, `Slot::waits`);
- every character while the cycling monitor is open (`host.rs:1826-1828`);
- on a switch, the monitor of the incoming character **moves to the
  outgoing one** (`host.rs:1307-1330`, `monitor_follows`), so the character
  that just left is Warm and starts taking pictures at once.

### 3.3 What Warm keeps

Everything, with the Off policy shrinking the 3D targets to 1/16 and
skipping draw lists. Textures stay as they were for 60 s, then step 7
(bakes still kept). Geometry, shadow atlas (fixed by the shadow setting,
up to 4096, `settings.rs:3430-3437`), probes, allocator room: kept.

### 3.4 Why a switch is instant (even from Economy)

Nothing has to be rebuilt: the window is restored, the role becomes Active,
the pressure is released (`Pressure::release`, `texture_budget.rs:344-352`)
and the lowered textures load again from memory or the disk cache over the
next seconds. The switch *looks* instant; the memory comes back afterwards,
as a burst, at the same time as the outgoing viewer still holds its
textures (Warm: for 60 s; Economy: after ≥ 1 s, if it gets frames) and
possibly gets the monitor (3.2).

### 3.5 Monitors: a full-size render-target rebuild per picture

A monitor picture is a photo frame
(`app/folderstorm_characters/monitor.rs:136-160`): it sets
`snapshots.capturing`, so `scene_paused()` is false
(`app/snapshot.rs:543-545`) and the frame's divisor goes from 16 to 1
(`app.rs:11406-11410`). In the render thread:

1. `render_view`: the divisor changed → `rebuild_targets()` at the **window's
   full size** (`renderer.rs:6542-6545`; `snapshot_window` is still None, so
   `world_extent` is the window, `renderer.rs:5267-5272`, `5348-5361`);
2. `begin_snapshot` → rebuilt at the picture's size (`renderer.rs:6120-6126`);
3. end of the frame, `end_snapshot` → rebuilt at the **window's full size**
   again, divisor still 1 (`renderer.rs:6101`, `6130-6149`);
4. next frame: `capturing` cleared (`app/snapshot.rs:559`), divisor back to
   16 → rebuilt small.

So each picture makes and drops **two full-resolution sets of world targets**
(colour, depth, normals, AO, bloom, AA, …) in a background process, at the
monitor's rate (0.5–5 a second, up to four monitors). Each rebuild also logs
`world resolution: …` (`renderer.rs:5362-5370`). On a card near its limit,
those allocations are exactly what makes Windows evict or demote other
processes' memory. This is a plain defect of the monitor path, independent of
the user's machine; the size of the cost is to measure **(verify on
Windows)**.

## 4. Coordination between viewer processes

There is some, all reactive and textures-only:

- the registry's fair share among viewers **in front** (`instances.rs`);
- the card-wide PDH reading, each viewer giving back its proportional part
  of the excess above 90 % (`gpu_load.rs`);
- the driver's per-process budget, which Windows already lowers as others
  take memory (`vram.rs`).

There is **no** host-assigned limit: the host passes no budget on the
command line or the link (`folderstorm_characters/host/process.rs:195-200`;
`ipc::Assignment` has no memory field, `ipc.rs:186-207`), and viewers do not
report their video memory to the host (`ipc::Status`, `ipc.rs:771-797`). The
host itself opens no graphics device (it paints on the processor,
session-host plan checkpoint 2), so it takes no video memory.

Gaps that matter with three to five characters:

1. Windows behind count as needing nothing, but they keep geometry, targets
   and (bakes, 60 s of) textures.
2. Every reaction runs on 2 s readings plus 1 s pressure steps: slower than a
   switch, when the incoming viewer reloads gigabytes.
3. Each viewer's auto setting is 8.4 GB: the setting itself never limits.
4. RAM is not considered anywhere. With 16 GB at 84 %, video memory that
   Windows demotes to system memory has nowhere to go but the page file
   **(inferred)**.

## 5. Device loss with several viewers

- A TDR resets the whole card: every viewer loses its device. The active one
  recovers at once; each background one **waits** (R5) with its lost renderer
  still alive in `self.gfx` until it comes to the front
  (`app/folderstorm_characters.rs:149-160`). Whether the driver keeps those
  dead allocations counted against the card until the process releases
  them is unknown **(verify on Windows: Task Manager, "Dedicated GPU memory"
  per process, right after a reset)**. If it does, the active viewer reloads
  its world on a card still full of dead memory, which invites a second loss
  (and the saved safe level).
- The active viewer's recovery reloads every texture and all geometry at
  once, a burst on a shared card.

## 6. Garbled faces: what the code suggests

No certain cause from reading; candidates in order of plausibility:

1. **Layer reuse under pressure.** Textures live as layers of shared pages.
   Pressure (rare for one viewer with a whole card, constant for several)
   enables lowering by GPU copy into a smaller layer (`TextureTable::shrink`,
   `crates/aurora-render/src/textures.rs:1640-1664`) and eager compaction
   that moves layers between pages (`textures.rs:1666-1675`, `maintain`
   `:1683-…`). The code warns about an ordering hazard there ("a texture
   written to it through the journal later in this frame would land before
   the copy reads it", `:1641-1645`) and about a layer showing "the previous
   texels" (`:389-392`). A slip in those paths shows *another texture's
   texels* in a face: the classic "garbled" look. Faces are bakes, which only
   get lowered in the "crowded, paused behind" state
   (`scene/textures.rs:2496-2507`), i.e. precisely with several viewers.
2. **Uninitialised pages.** Pages made by `dedicated.rs` are handed to wgpu as
   `TextureUses::UNINITIALIZED` with no zero fill (`dedicated.rs:186-194`),
   relying on "no layer is read before it is written". A path that breaks
   that invariant (a refused or partial upload, a slot reused across a
   recovery) samples whatever the memory held, possibly another process's
   freed texture **(inferred; whether WDDM/NVIDIA zero new allocations is
   driver behaviour)**.
3. **Just before the loss.** While the card is paging, rendering can show
   artefacts that vanish after recovery. Ask when exactly she saw them.
4. Less likely after reading: stale staged decodes across a recovery (handled,
   `scene/textures.rs:3101-3117`); a corrupted disk cache (files are sealed,
   `cache.rs:44-60`, so a damaged file is a cache miss, not garbage).

A screenshot, the time relative to the reset, and whether the faces fix
themselves (on their own, after a relog, after a cache clear) will tell 1–3
apart (section 10).

## 7. Likely cause, ranked

1. **Combined oversubscription with low RAM** (sections 3–4): N viewers ×
   (geometry + targets + bakes + allocator room) on 12 GB, with the active
   one free to grow to its driver budget, and 16 GB of RAM nearly full so
   demotion becomes paging. Fits "never on the human's identical card" if the
   human runs fewer characters, quieter regions, a smaller window or more RAM.
2. **Monitors** (section 3.5): per-picture full-size target churn in every
   watched background viewer, and Economy disabled for them.
3. **Switch bursts** (section 3.4): incoming reload while the outgoing still
   holds, and gets the monitor.
4. **A second wave after the loss** (section 5): dead allocations of waiting
   viewers, if the driver keeps them.

The diagnostics in option (d) and the questions in section 10 confirm or
rule these out.

---

## 8. Options

### (a) Per-process budget from the real budget

**Already in Aurora** (section 2.1). Re-implementing it with DXGI would
duplicate `vram.rs` and `gpu_load.rs` in upstream territory. What is missing
is not the reading but *what can be given back* (only textures) and *who
counts* (only viewers in front). Those are (b) and (c). **Not recommended as
new work**; at most propose upstream that `share_cap` count windows behind at
their actual usage (an upstream issue, not a fork change).

### (b) The host gives each character a share

- **What:** the host knows roles. It sends each viewer a texture cap: e.g.
  active = card × 0.9 − Σ(background viewers' reported usage) − margin;
  background = a small fixed cap (512 MB–1 GB, configurable), Monitor a bit
  more. The viewer applies `min(target, cap)`.
- **Where:** `folderstorm-session/src/ipc.rs` (`Assignment` gains
  `#[serde(default)] texture_cap_mb: Option<u32>`; `Status` gains
  `vram_mb: Option<u32>`), the computation as a pure function in a new
  `folderstorm-session/src/memory.rs` (unit-tested), the host in
  `folderstorm_characters/host/`, the viewer side in
  `app/folderstorm_characters.rs`. Protocol version bump.
- **Upstream hook:** one. `TextureStreamer` needs a cap it reads in `press`
  (`scene/textures.rs:3674-3678`), e.g. a `pub fn set_external_cap(Option<u64>)`
  plus `.min(cap)` folded into the existing `relieve(target, cap)` call, marked
  `// Folderstorm:`. `relieve` already exists for exactly this shape
  (`texture_budget.rs:167-172`), so the change is two or three lines.
- **Risks:** a cap set too low blurs the active character; a host that lags
  behind the real usage. Mitigate: the cap only ever lowers the target
  (Aurora's own rules still apply), the active cap follows the reports every
  second, and the active cap is lifted when the host is gone (link closed).
- **Effort:** medium (1–2 days with tests and a demo scenario
  `AURORA_DEMO_SESSION_HOST_MEMORY`).

### (c) Economy that really releases, and a lighter background

Split into independent parts, cheapest first:

1. **Fix the monitor's target churn** (section 3.5). Options:
   - (c1-i) In our module only: take the picture without changing the
     divisor, i.e. keep `scene_paused` false only for the snapshot targets.
     Not possible without touching `FrameParams` construction (upstream
     `app.rs:11406-11410`).
   - (c1-ii) **Recommended:** a small upstream hook so that a frame that
     carries a snapshot does not rebuild the window's targets for a divisor
     change, and `end_snapshot` rebuilds at the divisor the *next* frame will
     use. That is in the renderer (`renderer.rs:6542-6545`, `6130-6149`):
     **deep in upstream renderer code, so propose it to the human first;
     better sent upstream** as "a photo frame does not rebuild the window's
     targets twice" (it also affects Aurora's own photos taken while the 3D
     is paused behind).
   - (c1-iii) Keep the monitor divisor at 16 for the *window* targets by
     sending the picture with `rlva_resolution_divisor = 16` but a snapshot
     scale that compensates: hacky, interacts with RLVa. Not recommended.
2. **Make the minimise release reliable:** before minimising, the viewer
   runs one paused frame (targets at 1/16), and while minimised it keeps a
   slow tick of `App::frame` from `about_to_wait` (rule R2 already plans this
   driver, session-host plan 6.3) so `press` and the reductions run. Where:
   `app/folderstorm_characters.rs` (the Minimise handler waits for one
   paused frame), hook in `about_to_wait` (`app.rs:22808-22826`, one line,
   already planned as an R2 hook). Risk: frames into a minimised swapchain
   (acquire outdated, `renderer.rs:6681-6687`): the frame is skipped there,
   which is fine for `press` since it runs before the render.
3. **An "Economy" texture state:** while the host says Economy, drop the
   bakes too and lower everything to its last level (the "crowded" path,
   `Background::paused`, without waiting to be crowded). Hook: a flag the
   viewer passes into `Background::update`'s `crowded` argument
   (`scene/textures.rs:3684-3685`): one `|| folderstorm_economy` marked
   `// Folderstorm:`. Cost: textures and faces reload on switch (a second or
   two, from the CPU/disk copies).
4. **Release geometry behind** (the 1.35 GB): not possible without deep
   scene changes (the geometry cache and arena are upstream,
   `scene/mod.rs:2312-2346` shows how entangled it is). **Out of scope;
   flag only.**
5. **Monitors keep Economy's texture state** (3) while not taking a
   picture, instead of forcing Warm (`host.rs:403-405`). Cost: each picture
   draws with lowered textures (acceptable at 320–640 px).

- **Risks:** slower return from Economy (textures and bakes reload); the R2
  frame driver while minimised is new behaviour **(verify on Windows)**.
- **Effort:** c1 small code but needs the human's go-ahead (upstream
  renderer); c2 small–medium; c3 small; c5 small.

### (d) Better diagnostics

1. **At each loss, one line with the memory picture**, in the log and in a
   file that survives recovery: own `usage/allocated/budget`, texture pages
   and target, pressure step, card used by all programs, other viewers'
   usage from the registry, window state, role (managed), process RAM and
   system RAM free. Where: `app/folderstorm_characters.rs` (called where
   `folderstorm_gpu_loss` runs and on the upstream path via the same hook at
   `app/gpu_stall.rs:74`), file `gpu-losses.txt` in the crash session dir
   through a small `pub` helper next to `crash::gpu_recovered`
   (`crash/mod.rs:381-396`: one more `// Folderstorm:` function or our own
   module writing to `crash::session_dir()`). No upstream logic change.
2. **The recovery notice says why**, in plain words when known: "the video
   memory was full (N characters open)", "an image took too long", or the
   current generic text. A Folderstorm notice next to Aurora's, or a second
   line appended in our module; Aurora's text unchanged. New `tr!` keys in
   en/fr/ja.
3. **The host shows each character's video memory** (from (b)'s
   `Status.vram_mb`) and a card total, plus a warning when the total is near
   the card ("several characters use most of the graphics memory: Economy
   or closing one helps").
4. **A periodic summary line** every 60 s in each viewer while managed:
   `session memory: usage/budget, pages/target, step, card used, peers`.
   Cheap and makes the user's logs useful.

- **Upstream hooks:** none beyond existing Folderstorm hook points (and
  possibly one `pub` helper in `crash/mod.rs`).
- **Risks:** log volume (once a minute is fine); no personal data (figures
  only).
- **Effort:** small (half a day to a day).

### (e) Textures validated after recovery (faces)

A blind "re-upload everything" already happens (`scene/textures.rs:1864-1924`).
What is missing is knowing which path corrupts. Proposal:

1. Reproduce in demo: `AURORA_DEMO_GPU_LOST` with `AURORA_TEXTURE_BUDGET_MB`
   low (forces pressure, shrinks, compaction) and the avatar crowd scenario,
   with `AURORA_DEMO_SESSION_BEHIND` (paused behind, bakes lowered) then
   back to front. Compare faces before and after.
2. A debug check (behind an env variable): after a page is made, clear it
   (`clear_texture`) so uninitialised memory shows as black, not garbage.
   If garbled turns into black, candidate 2 of section 6 is confirmed.
3. If the corruption follows shrinks/compaction, the fix is in upstream
   `aurora-render/src/textures.rs`: **report upstream with the reproduction**
   rather than patch in the fork.

- **Where:** demo scenario and env check only in the fork; any real fix
  upstream.
- **Risks:** clearing pages costs GPU time (debug only).
- **Effort:** 1 day of investigation; the fix is unknown until reproduced.

### (f) Losses behind (from section 5)

If Windows shows that a waiting background viewer keeps its dead memory,
release it while waiting: drop the scene's GPU state and the lost renderer at
the loss, keep the window, recover when in front. Today the app expects
`self.gfx` to hold a renderer; doing without one is a deep change in `app.rs`.
**Measure first; propose options to the human only if confirmed.**

## 9. Phased order

| Phase | Work | Upstream touch | Why first |
|---|---|---|---|
| 0 | Ask the user for logs and facts (section 10) | – | Confirms the cause before code |
| 1 | (d) diagnostics: loss line + file, periodic line, notice reason | none / one helper | Cheap; every later step measurable |
| 2 | (c2) reliable minimise release, (c3) Economy texture state, (c5) monitors keep it | one line in `about_to_wait` (already an R2 hook), one `\|\|` in `press` | Makes Economy mean what the UI says |
| 3 | (c1) monitor target churn | renderer, **human's go-ahead, preferably upstream** | Large, avoidable transient allocations |
| 4 | (b) host shares + VRAM in the host | 2–3 lines in `press` | Proactive limit, visible to the user |
| 5 | (e) reproduce faces in demo, report upstream | none in fork | Depends on phase 1 data |
| 6 | (f) only if measured | deep, propose first | – |

Meanwhile, advice the human can pass on (no code): use Economy, close
monitors (and the cycling monitor), lower the window size / render scale or
the texture memory to manual (e.g. 3–4 GB per viewer), close other GPU
programs, and run fewer characters until 16 GB of RAM has room.

## 10. What to ask the affected user

1. **Logs** from `%LOCALAPPDATA%\Folderstorm\Folderstorm2\data\logs\`
   (path from `folderstorm_brand::project_dirs` + `logging.rs:41-52`):
   every `aurora-<profile>.log` and `aurora-<profile>.previous.log` of that
   session, and the host's log, **before relaunching** (the `.previous` file
   rotates at each start). Lines to look for: `GPU device lost`, `GPU work at
   the loss`, `texture memory: pressure`, `freeing up video memory`, `texture
   page … not made`, `render targets: out of video memory`, `render paused
   behind`, `GPU load:`, `world resolution:`, `session monitor:`, `the
   graphics card is lost while the window is not in front`.
2. The crash folder `…\logs\crashes\sessions\` (any folder from that day,
   `gpu-recoveries.txt`, `alive.txt`).
3. How many characters, which were **Warm or Economy**, **monitors open**
   (how many, size, rate; cycling monitor?), window size / screen
   resolution, graphics preset, texture memory setting (auto?), regions
   (busy?).
4. When it happened: right after a **switch**, while a monitor was open,
   on arrival in a region, or randomly?
5. Task Manager, Performance › GPU: "Dedicated GPU memory" and "Shared GPU
   memory" totals, and Details tab with the "Dedicated GPU memory" column per
   viewer process; Memory: RAM in use and committed. Ideally before and
   right after a reset.
6. Other programs on the card (browser with video, Discord, OBS, games).
7. The garbled faces: a screenshot; whose faces (own, other avatars, in a
   monitor picture?); before or after the reset notice; did they fix
   themselves (time, relog, cache clear)?
8. Windows Event Viewer › Windows Logs › System, source `nvlddmkm` or
   "Display" around the time (event 4101 = TDR), and the NVIDIA driver
   version.
9. Did a second reset follow, and did a character switch to Potato graphics?

## 11. Verified in code vs inferred

**Read in code:** sections 2, 3.1–3.5, 4 (except RAM), 5's `Loss::Wait`
keeping the lost renderer, 6's code pointers, the absence of any memory field
in the host link.

**Inferred, to verify on Windows:** that minimised windows get few or no
frames (stated in code comments, not measured here); that targets stay full
size after an Economy minimise; the size of the monitor's per-picture
allocations and their effect on the other processes; that WDDM keeps a lost
device's allocations counted; that RAM pressure turns demotion into paging;
which of section 6's candidates corrupts faces.

## 12. Questions for the human

1. Do you run the same setup as the user (number of characters, Warm or
   Economy, monitors, window size, busy regions)? How much RAM do you have?
2. Phase 3 changes Aurora's renderer (a photo frame rebuilding the window's
   targets twice). Send it upstream, patch it in the fork with a
   `// Folderstorm:` hook, or work around it on our side?
3. For (c3): is a slower return from Economy (textures and faces reload for
   a second or two) acceptable in exchange for real memory release?
4. For (b): what default share for background characters (e.g. 512 MB, 1 GB)
   and should the user see and change it in the host's settings?
5. Should monitors stop forcing Warm (c5), and should the monitor still
   follow the outgoing character on a switch?
6. Is a "why" in the recovery notice wanted, or should it stay Aurora's text
   with the detail in the log only?

---

# Video memory with several characters: phases 2 to 4 as built

FEATURES 5.1, session host ([session-host.md](session-host.md)). Built on
`claude/gpu-memory-share` (2026-11-17), to be validated on Windows.

## The human's decisions (2026-11-17)

- Phases in the plan's order; this branch does phases 2 to 4: (c) Economy
  really gives the memory back, (c1) the monitor's render-target rebuilds,
  (b) a share per character given by the host.
- The monitor fix stays in the fork, in our own code rather than in
  Aurora's renderer.
- A slower return from Economy is fine (textures and faces load again for a
  second or two).
- Users can change the background characters' share in the host's settings.
- A watched Economy character draws its monitor's pictures with blurrier
  textures instead of turning Warm; the monitor still moves to the
  character you just left.

## Phase 2: Economy that really gives the memory back

What Economy does now (`folderstorm_session::memory`,
`app/folderstorm_characters/memory.rs`, `scene/folderstorm_textures.rs`):

1. The host's assignment carries `Assignment::memory`
   (`ipc::Memory { share_mb, reserved_mb, release }`): Economy behind is
   `Release::Full`. The viewer hands it to the texture streamer at once,
   from the event loop's pump (`folderstorm_memory_assign`), not from a
   frame.
2. With `Release::Full` the textures give back as a window behind past the
   driver's budget does (`texture_budget::Background`, "crowded"): every
   texture to its last level (`PAUSED_DROPPED`), **the avatars' bakes too**,
   the 3D drawing paused, so the next frame shrinks the 3D targets to a
   sixteenth (`PAUSED_RESOLUTION_DIVISOR`). Hook: `|| economy()` on the
   `crowded` line of `press` (`scene/textures.rs:3692`).
3. The host's `Minimise` no longer minimises at once
   (`folderstorm_memory_minimise`): the release begins
   (`memory::Releasing`), the window stays behind (where frames come), and
   the pump minimises it once the textures say they are done
   (`TextureStreamer::folderstorm_released`: paused, held at the highest
   step, no texture left to lower, two passes and two frames since it
   began) or after `RELEASE_WAIT` (5 s). While it waits, the pump draws a
   frame itself (`App::frame`) when the window got none for `DRAW_AFTER`
   (500 ms): the release never depends on a minimised window's rare frames.
   The `about_to_wait` hook passes the event loop to the pump
   (`app.rs:22817`, one word).
4. Coming back, Aurora's own rule ends it: the window is in front, the
   pressure is released, textures and bakes load again from memory and the
   disk cache.
5. A release under way is called off when the character comes to the
   front or leaves Economy.

Logged: `session memory: Economy, giving the video memory back before the
window is minimised`, then `session memory: released after N ms (… MB of
video memory, texture target … MB), the window is minimised` (its second
pass form is in "Economy, second pass" below).

Not given back (out of scope, as the plan said): geometry (the mesh cache
and arena are entangled with the scene, `scene/mod.rs`), the shadow atlas
(fixed by the shadow setting), probes, wgpu's free block room (the atlas,
the probes and the culling buffers are given back since the second pass,
below). Bakes go
through the same GPU lowering as the "crowded" path Aurora already uses
(#1808); if the garbled faces turn out to come from that lowering (plan
section 6, candidate 1), Economy makes it happen more often: watch for it.

### A watched Economy character

A character in Economy whose monitor is open (or while the cycling monitor
is open) — since the second pass: whose monitor's window is **on screen** —
stays behind, as before, for its pictures, but no longer as Warm:
`Release::Lowered`, a share of at most `WATCHED_ECONOMY_MB` (512 MB), and
the highest pressure step at once (hook: `minimized || held()`,
`scene/textures.rs:3693`). Its pictures are drawn with lowered textures
(blurrier); its bakes are not lowered (no new trigger for the bake
lowering while its 3D draws), so faces stay sharp. The monitor swap is
unchanged: on a switch the monitor of the incoming character moves to the
one that leaves, which becomes a watched character at once. A change of
release while behind starts the textures' background state over
(`set_folderstorm_memory`), so the pause Economy latched does not keep a
newly watched character from drawing its pictures.

## Phase 3: no full-size render targets per monitor picture

Before: each picture rebuilt the window's world targets four times, twice
at the window's full size (plan section 3.5). Fixed **without touching the
renderer**: a watched background window draws its world in a rectangle of
the picture's size in the middle of its world part (`world_viewport`,
`app/folderstorm_viewport.rs`, fork code; the rectangle is
`memory::monitor_viewport`). The renderer's world size is then the
picture's, so:

1. the frame that carries the picture rebuilds at the picture's size (the
   divisor's change), not the window's;
2. `begin_snapshot` at the picture's size, as before;
3. `end_snapshot` at the picture's size, not the window's;
4. the next paused frame at a sixteenth of the picture's size.

The four rebuilds remain but none is larger than the picture (640 × 360 at
most, a ninth of full HD, a thirty-sixth of 4K). Removing them altogether
needs the renderer to skip the divisor's rebuild for a photo frame and to
rebuild at the next frame's divisor after it (plan option c1-ii): an
upstream change, not made. While watched, the window behind shows its
paused world in that small rectangle; clicked, it is the full window again
(one rebuild).

## Phase 4: a share per character

- **The setting.** Host settings › Characters › "Background characters'
  video memory": 256 MB to 4 GB in steps of 128 MB, **1 GB by default**
  (`DEFAULT_BACKGROUND_MB`), saved in `session-host.json`
  (`background_memory_mb`), applied at once to every viewer. Why 1 GB: on
  the reported 12 GB card four characters behind leave the active one about
  8 GB; on an 8 GB card two leave it about 6 GB; a character behind in a
  busy region already holds more than that in geometry, so its textures go
  to their floor (256 MB) and the share only stops them from growing back.
- **Background characters** (`Release::Keep`, Warm) hold their share in
  all: the textures' target becomes `share − (what the rest of the process
  holds)`, never under 256 MB (`Limits::cap`, folded into Aurora's
  `relieve` at `scene/textures.rs:3683`). The pressure steps follow as for
  any target; a Warm character no longer keeps all its textures for 60 s.
- **The active character** keeps the rest: it leaves the others the room up
  to their share (`reserved_mb` = Σ max(0, share − held), in steps of
  256 MB so that small changes send nothing), through Aurora's own target
  on its driver budget less that room (`texture_budget::texture_target`).
  A background character counts as needing its share, not nothing (Aurora's
  registry counts a window behind as needing nothing,
  `instances::share_cap`). Economy's full release counts as what it holds:
  it does not grow while minimised. What each viewer holds comes in its
  status (`Status::vram_mb`, the driver's figure); not said yet, the whole
  share is left.
- No new protocol number: both fields are `#[serde(default)]`, an older
  peer reads none (as `off_taskbar` did).
- The host's diagnostics (Advanced) list the share and each viewer's video
  memory.

## Economy, second pass (2026-11-18)

Built on `claude/economy-release-more`, stacked on `claude/gpu-memory-share`,
after the first Windows test (RTX 3060 12 GB, background characters in
Economy, share 256 MB): the background viewers still held 1.5–2 GB, and
their logs never had the `giving the video memory back` line on a switch.

### Why the minimise never came (root cause)

The host did send it: the viewer's log shows the assignment as
`Background, Economy standby` with Economy's textures release, so the
host's rule (`Desk::waits`) said Economy. The viewer dropped it:
`folderstorm_memory_minimise` returned at once when
`folderstorm_in_front()`, which is true while the window **has the
keyboard**. With the docked bar on, a switch is a click in the bar **of the
active viewer's own window** (or a global hotkey while it has the
keyboard): the host's assignment and `Minimise` reach that viewer before
the character brought forward has taken the keyboard, so the old active
window still had it and ignored the minimise, silently. A switch made from
the host's own window worked (the keyboard was the host's), which is why
the first build looked right. Only the release at startup (a character
started behind, never with the keyboard) was logged.

Fixed: only the **active character** ignores the minimise now
(`managed.role() == Active`). A window that still has the keyboard starts
the release and waits, however long, for the keyboard to leave before it
is minimised (`Releasing::poll`'s `keyboard`); the release and its 5 s wait
start over from the moment it leaves (until then the window counts as in
front for its textures, so nothing would be released anyway). A window the
user is in is never minimised under them.

### A monitor keeps a character behind only while it is on screen

`Desk::waits` kept an Economy character Warm (behind, never minimised) as
soon as its monitor was **open** (`Slot::monitor`, saved in
`session-host.json`) or the cycling monitor was, whether or not their
windows were on screen. Now (`Desk::watching`): its own monitor's window is
on screen (`monitors_seen`), or the cycling monitor's window is on screen
(`cycling_seen`, any character shown: it goes round all of them). A
monitor minimised, covered or hidden watches nobody: the character waits as
it chose, and Economy minimises it. When that changes while it is behind
(`monitors_seen`, `cycling_seen`, `set_monitor`, `edit_monitors`), the
characters no monitor keeps any more get their `Minimise` at once
(`minimise_unwatched`); a standby chosen while behind still waits for the
next switch, as before. The monitor swap carries whether the window was on
screen instead of assuming it.

Shown again: the host's assignment becomes `Release::Lowered` with
`Role::Monitor`, and the viewer, minimised at the host's word and not shown
since (`MemoryState::minimised_by_host`, cleared once the user goes to the
window), comes back behind **without the keyboard**
(`foreground::restore_behind`: `ShowWindow(SW_SHOWNOACTIVATE)` on Windows,
winit's restore elsewhere). A window the user minimised is never brought
back.

### More given back while the 3D is paused

Once Economy's release has paused the 3D (`drawing_paused`), once per
release:

- **GPU culling buffers** (`aurora-render/src/folderstorm_cull.rs`, a child
  of `gpu_cull.rs`): the indirect arguments (39 regions × 20 bytes per slot)
  and the masks follow the largest scene seen in powers of two and never
  shrank (51 MB at 65k slots, 204 MB at 262k, 409 MB at 524k). The main
  thread asks (`gpu_cull::folderstorm_release`), the render thread puts the
  first sizes back at the start of its next frame, before `prepare`. A
  paused frame has empty lists (no `prepare`, no `encode`); the first frame
  that draws the world grows them again through `prepare`'s own growth. The
  bind groups are made at each `encode` and the draws read `args()`/`cap()`
  of their frame: nothing holds the old buffers past the frames in flight.
- **Reflection probes and shadow atlas**
  (`folderstorm_characters::economy_render`): the renderer is handed 8 probe
  slots instead of up to 256 (six 128² RGBA16F faces each, about 275 MB at
  256) and 512 px shadow maps with no projector shadows (about 84 MB at
  quality 3 before). One hook at the end of `Settings::render_settings`:
  the user's settings are not touched or saved, Preferences shows them as
  they are, and every place that applies the settings (settings changed,
  resize, GPU recovery, demo) hands the lowered ones while Economy holds, so
  nothing fights it frame by frame. The renderer rebuilds the probes and the
  atlas as for any change of those settings. Out of Economy (in front,
  another standby, watched again, no host) the settings are applied again
  and the probe manager starts over (`ProbeManager::reset`): a few seconds
  of probe captures.

### The textures' release waits for the pages

The release was declared done on the textures' side only (no reduction
left, two passes): the render texture table empties pages at most one per
50 ms and destroys the emptied ones a few per frame, so most of the pages'
memory was still held when the window was minimised, and a minimised
window draws few frames (the test's log: 593 MB in pages for 472 MB of
textures, 226 MB two minutes later). Now the release also waits for
`TextureTable::folderstorm_settled` (no reduction or compaction under way,
no page waiting for its bind group or its destruction), still bounded by
`RELEASE_WAIT` (5 s), and the window draws at `RELEASE_FPS` (20) at least
during the release (`folderstorm_release_cap`, in the fork's frame cap; a
paused frame is the interface and a sky at a sixteenth of the size).

### Logged

One line per release, when the window is minimised:

```
session memory: released after <N> ms, the window is minimised: <A> MB taken from the driver (was <B>), texture pages <C> MB (was <D>), texture target <T> MB, GPU culling <G> MB back, probes and shadows at their smallest (8 probes, shadow maps of 512 px)
```

(`not all released` when the 5 s ran out), with, before it, `session
memory: the 3D is paused, the GPU culling buffers, the reflection probes …
go to their smallest` and the render thread's `GPU culling: N MB of buffers
given back …`; coming back, `session memory: the reflection probes and the
shadows as set again`.

### Expected

From the test's figures, per Economy viewer: the textures' pages down to
what its live textures need before the minimise rather than two minutes
later (hundreds of MB), the culling buffers (tens to hundreds of MB in a
busy region), about 270 MB of probes and up to about 80 MB of shadow atlas.
Geometry (the mesh arena and records) stays: it is entangled with the scene
(`arena.rs`) and is the bulk of what remains. Not measured: no GPU or
Windows here.

## Upstream hooks (all marked `// Folderstorm:`)

| File | Line | What |
|---|---|---|
| `crates/aurora-viewer/src/scene/textures.rs` | 1118 | The streamer's `folderstorm` field (our `Limits`) |
| same | 1230–1232 | `#[path] mod folderstorm_memory` (our `scene/folderstorm_textures.rs`, a child so it reads the streamer's state) |
| same | 1541 | The field's default in `new` |
| same | 3683 | The share folded into Aurora's relief |
| same | 3692 | Economy counts as crowded |
| same | 3693 | A watched Economy character held at the highest step |
| `crates/aurora-viewer/src/app.rs` | 22817 | The pump gets the event loop (existing hook) |
| `crates/aurora-render/src/gpu_cull.rs` | 1424–1427 | `#[path] mod folderstorm_cull` (our `folderstorm_cull.rs`) and its two functions re-exported (second pass) |
| `crates/aurora-render/src/renderer.rs` | 6516 | The render thread does an asked culling release at the start of a frame (second pass) |
| `crates/aurora-render/src/textures.rs` | 1683–1689 | `TextureTable::folderstorm_settled` (second pass) |
| `crates/aurora-viewer/src/settings.rs` | 3472–3473, 3522 | `render_settings` handed through `economy_render::apply` (second pass) |

Fork files only: `folderstorm-session/src/memory.rs`, `ipc.rs`, `host.rs`,
`managed.rs`, `settings.rs`; `app/folderstorm_characters.rs` and
`memory.rs`, `app/folderstorm_viewport.rs`, `folderstorm_characters/host`
and its settings page; second pass: `aurora-render/src/folderstorm_cull.rs`,
`folderstorm_characters/economy_render.rs`, `foreground.rs`.

## Tests and demo

- `folderstorm_session::memory::tests`: the setting's choices, the shares
  and the room left, the release before the minimise (done, waited, frames
  drawn by the viewer), the monitor's rectangle (the picture taken in it
  has the same size).
- `host::memory_tests`: each assignment's memory, Economy minimised after
  its assignment, a watched Economy character behind with a small share,
  the monitor swap kept, the setting saved, clamped and said at once.
- `ipc` and `managed` tests: the fields travel, an older peer reads none,
  the share goes with the host.
- `scene::textures::folderstorm_memory::tests`: the textures' cap, Economy's
  release and its end while behind.
- The host's settings window drawn with the new row; the diagnostics show
  the share and the viewers' video memory.
- Second pass: `host::tests` (Economy minimised with no monitor on screen,
  kept behind by one on screen and minimised once it goes, the cycling
  monitor open but not on screen keeps nobody), the updated
  `memory_tests` and `monitor_window_tests`;
  `memory::tests::a_window_with_the_keyboard_is_minimised_once_it_lost_it`;
  `economy_render::tests`; `folderstorm_cull::tests` (ignored, needs a GPU:
  the release, then the growth again, no validation error).
- Demo: `AURORA_DEMO_SESSION_HOST=3 AURORA_DEMO_SESSION_HOST_MEMORY=economy,512`
  (`docs/TESTING.md`); with `AURORA_DEMO_SESSION_HOST_BAR=docked` for a
  switch from the bar of the active viewer (the keyboard case).

## Decided

**Warm stays the default standby for new characters** (the human's choice,
2026-11-17). Economy now frees what the UI says and costs a second or two of
loading on return; it remains a per-character choice.

## Left

- Measure on Windows (Task Manager, dedicated GPU memory per process) Warm
  vs Economy and with a monitor open; see the test steps in
  `docs/TESTING.md`. Second pass: check that a bar or hotkey switch now
  logs the release, that a monitor window brought back on screen brings
  its Economy character back behind without taking the keyboard
  (`SW_SHOWNOACTIVATE` is untested), and the figures of the release line.
- Geometry behind (deep upstream changes, propose first). The shadow atlas
  and the probes are lowered in Economy since the second pass.
- If the bar's switches keep the old window's keyboard for long (the new
  one failing to come forward), the release waits with it: say so if seen.
- c1-ii upstream (a photo frame that does not rebuild the window's targets).
- The registry (`instances.rs`) still counts a window behind as needing
  nothing for viewers the host does not manage: an upstream issue at most.
