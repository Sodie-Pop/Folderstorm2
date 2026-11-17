# Video memory with several characters: phases 2 to 4 as built

FEATURES 5.1, session host ([session-host.md](session-host.md)). Built on
`claude/gpu-memory-share` (2026-11-17), to be validated on Windows.

> **Merging note.** The investigation (sections 0–12: the report, what
> Aurora has, the options, the phased order, the questions) and phase 1
> (diagnostics) are added to this same file on
> `claude/gpu-reset-diagnostics`. Both branches create the file: when the
> second one merges, keep both, this part after the investigation, and
> set phases 2–4 to "built" in its Status table.

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
video memory, texture target … MB), the window is minimised`.

Not given back (out of scope, as the plan said): geometry (the mesh cache
and arena are entangled with the scene, `scene/mod.rs`), the shadow atlas
(fixed by the shadow setting), probes, wgpu's free block room. Bakes go
through the same GPU lowering as the "crowded" path Aurora already uses
(#1808); if the garbled faces turn out to come from that lowering (plan
section 6, candidate 1), Economy makes it happen more often: watch for it.

### A watched Economy character

A character in Economy whose monitor is open (or while the cycling monitor
is open) stays behind, as before, for its pictures, but no longer as Warm:
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

Fork files only: `folderstorm-session/src/memory.rs`, `ipc.rs`, `host.rs`,
`managed.rs`, `settings.rs`; `app/folderstorm_characters.rs` and
`memory.rs`, `app/folderstorm_viewport.rs`, `folderstorm_characters/host`
and its settings page.

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
- Demo: `AURORA_DEMO_SESSION_HOST=3 AURORA_DEMO_SESSION_HOST_MEMORY=economy,512`
  (`docs/TESTING.md`).

## Proposed, not changed

**Economy as the default standby for new characters.** It now frees what
the UI says, and coming back costs a second or two of loading. Warm stays
the default until the human decides.

## Left

- Measure on Windows (Task Manager, dedicated GPU memory per process) Warm
  vs Economy and with a monitor open; see the test steps in
  `docs/TESTING.md`.
- Geometry and the shadow atlas behind (deep upstream changes, propose
  first).
- c1-ii upstream (a photo frame that does not rebuild the window's targets).
- The registry (`instances.rs`) still counts a window behind as needing
  nothing for viewers the host does not manage: an upstream issue at most.
