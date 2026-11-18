# Deep sleep: a third standby that lets go of all video memory

Plan, 2026-11-18. FEATURES 5.1 (session host), follow-up of
[gpu-memory.md](gpu-memory.md) phases 2–4 and "Economy, second pass"
(PR #111). Not built yet: the maintainer decides first (section 8).

Legend: **[code]** read in code, with `file:line` on `main` (M) or on PR #111
`claude/economy-release-more` (B); **[est]** an estimate to measure on
Windows.

## 0. Why, and the idea

Measured on Windows (RTX 3060 12 GB, busy region, 2026-11-18): Economy takes
a background viewer from 4323 MB to 1845 MB taken from the driver (Task
Manager agrees). What stays, besides about 435 MB of texture pages, is mostly
the geometry arena (it only grows, `arena.rs:191-239` M), the per-face
record tables, the staging pool, window-size targets and the driver.

The maintainer asked for a third choice next to Warm and Economy that lets go
of that too, accepting a slower return (like a short teleport).

**Recommendation: put the renderer to sleep instead of shrinking it.** Once
the window is minimised: do the GPU-loss recovery's CPU bookkeeping
(`forget_gpu_scene`), then **drop the whole `Renderer`** (device, arena,
records, texture pages, staging pool, pipelines, swapchain) and keep the
window, egui's state, the world and every CPU cache. On return: a new
`Renderer` on the **same window**, then the recovery's second half
(`adopt_gpu_scene`, `egui_replay`). The world reloads from memory and the
disk cache; nothing is downloaded again.

The "drop everything and replay the scene" path already exists and is used
by GPU-loss recovery (`app/gpu_recovery.rs:161-198`, `:263-317` M). It frees
**all** video memory, needs about four one-line upstream hooks and no change
to aurora-render.

- **Floor [est]:** 0–100 MB per sleeping character (Economy: 1845 MB).
- **Cost [est]:** 0.5–1.5 s to make the renderer, 3–6 s until the world is
  back, about 10 s until textures are sharp. While asleep the viewer runs
  only the first half of `App::frame` (network, chat, IMs, local assistant);
  the world is paused.

The other design (shrink the renderer in place and keep the device) floors at
about 350–550 MB [est] and needs deep aurora-render changes; section 2.5
keeps it as a possible upstream proposal.

## 1. What is freed, compared with Economy

| Item | Where [code] | Economy (#111) | Deep sleep |
|---|---|---|---|
| Geometry arena (vertex, skin, index) | `arena.rs:191-239` M; grows by doubling, never shrinks | kept | freed |
| Mesh GPU residency | `scene/mod.rs:216` M; `forget_gpu` clears `geoms` (`:2457-2490` M) | kept | freed; LODs rebuilt from mesh bytes in memory (`meshes.rs:968` M) |
| Record store (304 + 64 + 32 B per slot) | `arena.rs:591-660` M | kept | freed |
| GPU culling buffers | `folderstorm_cull.rs` B | first size | freed |
| Texture pages | `aurora-render/src/textures.rs` | last levels (435 MB in the test) | freed; reloaded from the J2C in memory or the BC7 disk cache (`scene/textures.rs:1935-2000` M) |
| Impostor atlas | `scene/mod.rs:2478` M | kept | freed |
| Probes, shadow atlas | `economy_render.rs` B | 8 slots, 512 px | freed |
| Terrain and water buffers | `scene/mod.rs:2425-2433` M | kept | freed; `terrain_surface.gpu_reset()` on return |
| Avatar bakes and composites | texture table; `clear`/`forget_gpu` | lowered | freed; rebuilt from the appearance kept on the CPU |
| Staging pool (64 MB here) | `main_thread.rs:645-652`, `upload.rs:102-110` M | kept | freed once no clone is left (2.2 step 6) |
| egui and HUD textures | CPU shadow `egui_shadow` (`gpu_recovery.rs:140-142` M) | kept | freed; replayed through `egui_replay` |
| Swapchain, window-size targets | `Backend` | 3D at 1/16 | freed; the window stays, minimised, with no surface |
| Pipelines, device, driver overhead | `main_thread.rs:343` M | kept | freed |

Residual risks for the floor: a staging pool kept alive by a decode result
still in a channel, and the `VkDevice` until those are dropped (2.2 step 6).
Verify with Task Manager per process.

## 2. How to free and rebuild

### 2.1 What exists [code]

- `App::forget_gpu_scene` (`gpu_recovery.rs:161-166` M): `scene.forget_gpu`
  (CPU bookkeeping only, render origin reset so the next sync rebuilds
  everything, `scene/mod.rs:2449-2490` M), `probes.reset()`,
  `forget_gpu_requests()`.
- `App::adopt_gpu_scene` (`:183-197` M): `scene.adopt_gpu(new)` (textures get
  new slots and are uploaded again from memory or disk, `scene/mod.rs:2496-2525`
  M), particles, terrain `gpu_reset`, media `gpu_reset`, local imports,
  wearable preview, Blender Live.
- `replace_renderer` (`:263-317` M) does both around a **new window**, only
  needed after a real device loss. `open_renderer`, `note_gpus`,
  `egui_state_for` are `pub(super)`; `forget_gpu_scene` and `adopt_gpu_scene`
  are private.
- `App::frame` returns at `let Some(mut gfx) = self.gfx.take() else { return; }`
  (`app.rs:10954` M) after the network events, inventory, transcripts, the
  local assistant and the social section. The AgentUpdate keep-alive comes
  from the network tick (`aurora-net/src/session.rs:1611-1616` M). The host's
  commands run in `folderstorm_managed_pump` from `about_to_wait`
  (`app.rs:25006` M), independent of frames.

### 2.2 Sleep (new fork file `app/folderstorm_characters/sleep.rs`, rules in `folderstorm_session::sleep`)

1. The host's `Minimise` arrives with `memory.sleep == true` (section 5). Run
   Economy's release as today, including #111's wait for the keyboard to
   leave, and minimise.
2. Grace delay `SLEEP_AFTER` (proposed 20 s) minimised and not active: a quick
   flip back costs only an Economy return, and the outgoing teardown does not
   overlap the incoming wake.
3. Stay in Economy (and log why) when: not in the world or less than 30 s in
   it; holding the voice; background sound "All"; VR set up;
   `AURORA_GPU_RECOVERY=0` (egui has no CPU copy); a photo under way.
4. Release CPU holders of the old device: `forget_gpu_scene`;
   `scene.textures.staging = None`; a new
   `TextureStreamer::folderstorm_forget_gpu()` in the existing child module
   `scene/folderstorm_textures.rs` (uploads cleared, last driver reading
   zeroed); `blender_live.gpu_reset(None)`, `media.gpu_reset()`, terrain
   `gpu_reset()`.
5. Take `Gfx` apart, `drop(renderer)` (joins the render thread, as
   `replace_renderer` does), keep `Asleep { window, size, maximized, since }`.
6. **[P3]** Leftover decode results still hold staging chunks and so the old
   device: drain and discard results not from a live pool (small fork child
   module of `scene`).
7. While asleep, the pump sets `ControlFlow::WaitUntil(now + 200 ms)` (the
   loop is `Poll`, `main.rs:341` M) and calls `frame` every 200 ms, so
   network, chat, local assistant and inventory keep running.
8. Log: `session memory: deep sleep after N s minimised, renderer dropped
   (was A MB taken from the driver)`.

### 2.3 Wake

- **Triggers:** an assignment `Active` or `Lowered` (watched), the host's
  `Show`, the user restoring or focusing the window, the host gone and the
  window restored.
- **Order:** restore the window first (`folderstorm_show`, or `restore_behind`
  when watched); build the renderer in the next pump turn once
  `inner_size()` is not zero (a Vulkan surface on a minimised Windows window
  is 0×0; **validate on Windows**). Then `open_renderer`, `apply_settings`,
  `adopt_gpu_scene`, `note_gpus`, `screen_rate.poll`, a new `egui_state_for`,
  `egui_replay = true`, `Gfx { shown: true, .. }`, `ControlFlow::Poll`.
- No "GPU recovered" notice or loss counters; clear `gpu_stall`,
  `loss_waiting`, `stall_held`.
- If `open_renderer` fails: retry every 2 s; after 3 failures while active,
  Aurora's failed-recovery path; while behind, stay asleep.

### 2.4 Hooks in upstream files (all `// Folderstorm:`)

| File | Change |
|---|---|
| `app/gpu_recovery.rs:161`, `:183` | `fn` → `pub(super) fn` on `forget_gpu_scene`, `adopt_gpu_scene` |
| `app.rs:24466` (`window_event`, no gfx) | `self.folderstorm_sleep_event(&event);` (Close wakes then exits; focus or a non-zero resize wakes) |
| `app.rs:11579`, `:12008` (`let covered = …`) | `\|\| self.folderstorm_waking()`: the loading screen's sync budget while waking (P3) |
| aurora-render | none |

Fork-only: a `folderstorm_window()` helper (gfx's window or the sleeping one)
for every window access in `app/folderstorm_characters/*`; `folderstorm_status`
must not report `Starting` while asleep; `folderstorm_memory_pump` must not take
"no gfx" for "between renderers".

### 2.5 Alternative to propose upstream, not to build here

`Renderer::shed_world()` (reset `GeometryArena`, `RecordStore`, cull tables to
first sizes; drop every texture page but the reserved one; shrink the staging
pool) plus `Scene::shed_gpu()`. Keeps device, pipelines and swapchain: faster
return, floor about 350–550 MB [est], but a deep change across `arena.rs`,
`textures.rs`, `upload.rs` and the render thread. Aurora could use it for
"behind and past the driver budget" (`vram.rs:38-40` M). Propose only if Deep
sleep's return proves too slow.

## 3. Risks and rules

- **The world is paused**, not the session: no AO, animations, voice state,
  typing or teleport UI until wake; network and keep-alive continue. A
  teleport (local assistant, RLV, lure) completes its circuit and finishes
  loading on wake; the host shows "Loading". Say so in the help text.
- **Avatars and attachments** come back from the world's data (hidden until
  loaded); animesh, skin bindings and bakes are rebuilt. Verify no appearance
  message is sent on wake (recovery has the same property).
- **HUDs** are rebuilt with the attachments.
- **Selection and build:** selection survives; a background character with a
  build window open stays in Economy (optional rule).
- **Particles, media:** reset on wake; media processes keep running (muted).
- **Window restored by hand or Alt-Tab:** wakes.
- **Monitors:** a character whose own monitor window is on screen is
  `Lowered` (#111) and wakes behind. **Decision:** the cycling monitor should
  not wake sleeping characters (proposal: show their last picture with an
  "asleep" badge).
- **GPU loss:** impossible while asleep; a loss waiting behind becomes a
  sleep plus an uncounted rebuild.
- **Keyboard:** as #111, never minimised or put to sleep while it has the
  keyboard; the grace starts once minimised.
- **Quit while asleep:** the exit path runs before the renderer is needed;
  verify `shutdown` with `gfx = None`.
- **Instances registry and host status:** report 0 MB and `asleep`, so the
  active character's reserve stays right.

## 4. Return, as felt

1. Host switch: `Active`, then `Show`. Window restored, renderer built the
   next turn (0.5–1.5 s [est], window shown but not painted). Log
   `deep sleep: awake, renderer in N ms`.
2. First frame: interface, HUDs, sky, and a notice "Waking up: the world is
   loading again…" that does not catch clicks; it ends when geometry is in
   and texture jobs are few, or after 10 s.
3. Loading budget while waking; meshes 3–6 s, textures about 10 s [est],
   probes recaptured.
4. Log: `session memory: awake after N s asleep: renderer X ms, world synced
   +Y s, textures settled +Z s`.
5. The outgoing character sleeps only after the grace, so the two GPU bursts
   do not overlap.
6. Later [P4]: try building the renderer while still minimised, if Windows
   accepts a surface then.

## 5. Protocol, settings, interface

- **Wire, no protocol bump:** `ipc::Memory` gets `sleep: bool` (the struct is
  `#[serde(default)]`, `ipc.rs:216` B); `Status` gets `asleep: bool`. No new
  `Standby` variant: an older peer would fail on it. An older viewer sees
  Economy; an older host sends no `sleep`.
- **`session-host.json`:** `SlotConfig` gets `deep_sleep: bool` (default false,
  skipped when false) with `standby: Economy` beside it, so an older build
  reads Economy. The host side maps `Waiting { Warm, Economy, DeepSleep }` to
  `(Standby, bool)`.
- **Host rules** (`folderstorm_session::memory`): sleep only when Economy and
  not watched; the fallbacks the host can see (voice holder, background sound
  All) applied there too. `waits()` unchanged.
- **Interface:** three segments in the host's settings (Characters); the
  card's Economy checkbox becomes a three-way choice; the bar shows "asleep";
  GPU diagnostics get a "deep sleep" entry; the local assistant's
  `host_background_mode` accepts `deep_sleep`.
- **Name (decision):** proposed **"Deep sleep" / "Veille profonde" /
  "ディープスリープ"** ("Hibernate" suggests the whole program is suspended,
  but chat keeps working). Alternative: "Hibernation" / "Hibernation" / "休止".
- **Help (en draft):** "Deep sleep: as Economy, then after a short while the
  window also lets go of the 3D world and all of its video memory; chat,
  messages and the assistant keep working. Coming back is like a short
  teleport: the world loads again for a few seconds."
- **Demo:** `AURORA_DEMO_SESSION_HOST_MEMORY=deep,<MB>` and
  `AURORA_DEMO_SLEEP_AFTER=<s>`; rows in `docs/TESTING.md`.
- **Tests:** ipc fields travel and default; host rules (deep, watched, voice
  holder, file round trip, old file reads Economy); the pure sleep/wake state
  machine; the settings window's three segments; the controller's parsing.

## 6. Phases (one PR each, from `main`)

- **P0, quick win, independent: Economy waits for the texture pages.** #111
  counts the release done when released and settled; `settled` can be true
  between two eager compaction passes while spare pages (2 s) and the 1/s
  compaction still free memory. Fixed-size textures (HUDs, sculpts, terrain)
  are never lowered, so "pages ≤ target" may never hold. Rule: done when
  settled **and** the pages have not dropped for 1 s, bounded by a release
  wait raised to 10 s; log the pages again 10 s after the minimise.
  Expected: 150–200 MB more in a busy place [est].
- **P1, protocol, setting and interface;** the viewer treats Deep sleep as
  Economy for now. Fully testable in Linux CI.
- **P2, sleep and wake** through the renderer drop: `sleep.rs`, the hooks of
  2.4, the window helper, status, pacing, fallbacks, logs. Windows tests:
  Task Manager per process; switches from the bar, a hotkey and the host;
  restore by hand, Alt-Tab, taskbar Close; a teleport while asleep; a monitor
  shown and hidden; Quit while asleep.
- **P3, polish:** the waking notice and loading budget; leftover staged
  results; the cycling-monitor rule; the host reads each viewer's dedicated
  memory itself (Windows PDH, next to `gpu_load.rs`), since a sleeping viewer
  has no driver to ask; the Performance monitor shows the last wake.
- **P4, measure and decide:** figures into [gpu-memory.md](gpu-memory.md);
  try the renderer while minimised; if wake is too slow, write the upstream
  `Renderer::shed_world` proposal.

## 7. What was read and what is estimated

- **[code]:** recovery `gpu_recovery.rs:42-317` M, `scene/mod.rs:2398-2525` M;
  arena `arena.rs:191-239`, `:591-660` M; streaming `upload.rs:102-230`,
  `main_thread.rs:343-480`, `:620-660` M; frame and loop `app.rs:10954`,
  `:11579`, `:12008`, `:24466`, `:25006` M, `main.rs:341` M; #111's
  `memory.rs`, `folderstorm_textures.rs`, `ipc.rs:83-232`, `host.rs`,
  host settings and controller (B).
- **[est]:** every MB figure and timing, whether Windows accepts a Vulkan
  surface on a minimised window, and whether `shutdown` and every handler are
  clean with `gfx = None` for a long time (recovery only holds that state for
  an instant; check in P2).

## 8. Decisions for the maintainer

1. The approach: drop the whole renderer (floor near 0, return 4–7 s) rather
   than shrink it in place (floor 350–550 MB, deep upstream change).
2. The name: "Deep sleep" or "Hibernation".
3. The grace before sleeping: 20 s?
4. The cycling monitor does not wake sleeping characters (last picture with
   an "asleep" badge)?
5. Do P0 (Economy waits for the texture pages) now, separately?
