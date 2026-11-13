# Session host: several characters, one desk

Plan for [FEATURES.md](../FEATURES.md) row 5.1. Planning only: nothing here is
implemented yet. Written 2026-11-10 from Folderstorm2 `main` (`6f586de`) and
Folderstorm (C++) `main` (`abbe3cb0ca`).

The goal, as in Folderstorm (C++): up to five characters logged in at once,
one of them "active" (full 3D, keyboard, voice), switching between them without
relogging, one shared chat for all of them, low-rate read-only views
("monitors") of the others, restart and recovery, quick actions, profile copy,
and sound and voice that follow the active character. Off by default; an
ordinary launch never changes.

Sections 3 and 6 rely on code read in this repository; their claims cite the
file. Claims about Windows behaviour that a Linux sandbox cannot check are
marked **(verify on Windows)**. Section 16 sorts what was verified from what
was inferred.

---

## 1. Recommendation in short

- **One viewer process per character.** Aurora is built around one agent, one
  world and one window per process (section 4). It also already cooperates
  with other Aurora windows running at the same time (section 3).
- **Start without a host.** Checkpoint 1 adds `--profile <name>` (separate
  settings per character), a policy for "3D while another Folderstorm2 window
  is in front" (Off / Limited / Normal), and background-safe GPU-stall and
  device-loss handling. Useful on its own: characters side by side, the ones
  behind costing little.
- **Then a session host**, a mode of the same executable
  (`aurora-viewer --session-host`): it launches and arranges the viewers over
  a private local pipe, and owns switching, shared chat, sound and voice,
  monitors and recovery.
- **Separate top-level windows, arranged by the host**, not embedded
  windows. Embedding is where Folderstorm (C++) spent most of its native
  debugging, and it never fully stabilised.
- **Background 3D: Off first, Limited once monitors exist.** "Off" reuses
  Aurora's existing paused-scene state. "Limited" draws a small offscreen
  image at the monitor's rate and size, and only while a monitor shows it.

## 2. What Folderstorm (C++) built, and what it learned

From `doc/plans/multi-character-*.md`, `session-*.md`, `host-*.md`,
`character-quick-actions.md` and `tools/session-host/README.md`:

- A native Win32 controller (`folderstorm-session-host.exe`) ran up to five
  viewer processes with private profiles
  (`%LOCALAPPDATA%\FolderstormSessions\…\CharacterN`). Profiles were reserved
  by an inherited exclusive file handle. Control went over a current-user
  named pipe, with versioned fixed-size commands bound to account, session
  generation and sequence numbers. Credentials never crossed the pipe.
- Warm standby reused the viewer's hidden-window path. Economy released render
  targets. Both kept networking, teleports and chat running. Neither saved a
  graphics change.
- Hosting went from child/owner parenting (a controller exit could destroy a
  viewer) to a borderless unowned top-level window that followed the
  controller. That still showed focus, z-order and visibility problems: "hosted
  world disappears when interacting with host controls or other apps". Native
  acceptance was never completed.
- **Lessons worth keeping:**
  - 12 s and 15 s reply timeouts detached *healthy* viewers whose loop was
    briefly blocked; a "recovering" state that keeps the link fixed it
    (`session-connection-recovery.md`). **Slow is not dead.**
  - Never resend a send whose outcome is unknown; bind drafts to their
    character and conversation; never auto-accept offers, payments or
    teleports.
  - Monitor flicker came from GDI repainting every 100 ms, not the capture.
    Notices evicted real IMs from a shared quota: give them their own bound.
  - Voice was never tested live. The user reported five characters at
    single-viewer-like frame rates: one process per character scales.

## 3. What Aurora already has (verified in code)

Aurora already expects several of its viewers to run on one PC. This removes
much of the work Folderstorm (C++) had to do:

| Already there | Where | What it gives us |
|---|---|---|
| Instance registry: each viewer writes a small file every 2 s (video memory, "in front", card) and reads the others' | `crates/aurora-viewer/src/instances.rs` | Peer discovery; a window behind draws at most `BEHIND_FPS` = 10 fps while another viewer is in front (`throttle`); fair VRAM share (`share_cap`). Off in demo and timed runs. |
| Busy-card pause: a window behind whose GPU frames take ≥150 ms twice stops drawing 3D (empty draw lists, 3D targets at 1/16) until it is in front | `app/busy_pause.rs`, `PAUSED_RESOLUTION_DIVISOR` in `app.rs`, option `ui-options-pause-when-card-busy-tip` | A tested "3D off, everything else on" state, which is our **Off** policy. Draw lists are skipped too (`app.rs`, `let lists = if self.in_world() && !self.scene_paused()`), so CPU is saved as well. |
| Texture release behind: background 60 s or minimised 1 s, then the textures are given back (`texture_budget::Background`) | `scene/texture_budget.rs` | Our **Economy** standby can just be "minimised". |
| Background frame cap (default on, 15 fps) | `settings.rs` `background_fps_cap/limit`, `frame_cap` in `app.rs` | Already low-cost when behind. |
| GPU stall notice and device-loss recovery | `app/gpu_stall.rs`, `app/gpu_recovery.rs`, `aurora-render/src/device_lost.rs`, `main_thread.rs` (`STALL_AFTER`) | Must be made background-safe (section 6.3). |
| Card-wide memory counters (Windows) | `gpu_load.rs` | Another protection against overfilling the card. |
| Multi-writer-safe files: per-process temporary name and rename retry | `atomic_file.rs`, `cache.rs`, `aurora-render/src/pipeline_cache.rs` | The asset cache can be **shared** between characters. |
| Browser-cache purge that leaves another running viewer's profiles alone; media-pack install lock | `media/cef_cache.rs`, `aurora-updater/src/media_pack.rs` | Same. |
| Logs named per PID or `--title`; crash sessions in UUID folders | `logging.rs` (`log_stem`), `crash/mod.rs` | No collisions between processes. |
| Per-instance `secondlife://` pipe, last-focused routing, `--title` scoping; Linux bus name scoped by demo and title | `slurl_handoff.rs`, `slurl_handoff/windows.rs` | A profile can add its own scope the way `--title` does. |
| Saved passwords keyed by `grid\|user` in the credential store | `credentials.rs` | Shared by all profiles without conflict. |
| Session-only mute (MuteWhenMinimized), not saved | `app/audio_mute.rs` | Model for "background sound muted". |
| Voice is in-process WebRTC (no SLVoice daemon or fixed port) | `aurora-voice/src/lib.rs`, `voice.rs` | No port clash. Only microphone ownership matters. |
| `Focused(false)` clears held keys, gesture keys and drags | `app.rs` window events | Switching away cannot leave keys held. |
| Snapshot drawn offscreen at its own size; it overrides the paused scene (`scene_paused = drawing_paused && !capturing`) | `aurora-render` `SnapshotSpec`, `app/snapshot.rs` | Basis for monitors. |
| World viewport (PR #6, branch `claude/world-viewport`, not merged): `Renderer::set_world_rect`, `world_extent` | `aurora-render/src/main_thread.rs`, `renderer.rs` on that branch | Draws the world in a sub-rectangle at that size. Useful for a "Limited" window and for host layouts on wide screens. |

## 4. Architecture

### 4.1 Why one process per character

- `App` (`app.rs`, about 22,000 lines) holds one `World` (one `agent_id`), one
  network session, one camera, one scene and one renderer. Much state is
  process-global: `cli::get()`, the settings and their saver, the cache key,
  the logger, the crash session, the voice session and the audio engine.
- winit allows one event loop per process, and Aurora's whole session advances
  inside `App::frame` on that loop's main thread.
- Making it multi-agent would touch most of the upstream code, so every merge
  would conflict. Separate processes also keep one character's crash, freeze
  or GPU recovery away from the others. The exception is a driver reset (TDR):
  it hits every process on the card, and each one recovers on its own.

### 4.2 Options

| Option | Summary | For | Against |
|---|---|---|---|
| **(a) Separate host app** (the C++ way) | A host process launches N viewers, IPC to each; it owns switching, chat, monitors and recovery | One owner of truth; survives any character's crash; clean lifecycle | The most UI to build; chat UI written again in the host; window hosting is hard on Windows |
| **(b) Viewers plus one "controller" viewer** | The first viewer also shows the other characters' chat in Aurora's own chat windows | Reuses Aurora's chat UI | Deep changes to upstream chat and world code (conversations would need an account key), so high merge cost; management dies with the controller character's logout or crash; controller hand-over is complex |
| **(c) Lighter first** | Profiles, a background policy and peer awareness (the registry exists), with no IPC | Small, all in our own modules plus tiny hooks; useful at once | No shared chat, monitors or managed switching |

**Recommended: (c) as checkpoint 1, then (a), with the host built into the
viewer executable as a mode.**

- **Host as a mode of `aurora-viewer`** (`--session-host`, code in
  `crates/aurora-viewer/src/folderstorm/session_host/`): its own winit window
  drawn with `egui-winit` and `egui-wgpu` (already dependencies, 0.37).
  - For: one binary, so host and viewers never run mismatched protocols (C++
    had to reject mixed builds); no upstream packaging change; reuses the
    Palette, shared widgets, Tabler icons, `tr!`, `folderstorm-brand`,
    logging and crash reports.
  - Against: the host maps the large viewer binary (pages load lazily) and
    needs one early branch in `main.rs`.
- **Fallback:** a separate binary crate `crates/folderstorm-session-host`.
  Cleaner in isolation, but it needs upstream packaging changes and cannot
  reuse the theme unless that moves into a library.
- **Pure logic in a new library crate `crates/folderstorm-session`**
  (protocol, framing, slot registry and switching state machine, chat model,
  liveness, monitor budget, profile-name rules), used by host and viewer and
  unit-tested on Linux CI.

### 4.3 Costs per character (to measure; not promised)

| Resource | What drives it | How we keep it down |
|---|---|---|
| VRAM | Textures, geometry, render targets. Aurora's own notes: two viewers took 6.6 and 5.9 GB of a 12 GB card; a viewer behind at the highest texture pressure still held about 6.1 GB (2.1 GB textures, 1.35 GB geometry, the rest targets and allocator) (`scene/texture_budget.rs`) | Off policy (targets at 1/16); Economy (minimised, textures given back after 1 s); the existing VRAM share. Geometry stays: measure. |
| GPU time | Drawing | Off = interface-only frames at a low rate; busy pause as a safety net |
| CPU | Object and avatar updates for its region, decoding, the main loop | Off skips draw lists; low loop rate behind; shared cache avoids decoding the same assets twice |
| RAM | World, scene, caches: several GB per character in busy regions **(measure)** | Economy; later perhaps a runtime-only smaller draw distance behind (experimental: fewer objects, but a reload on switch) |
| Bandwidth | Each character has its own circuits and asset fetches | Shared asset cache (a second character in the same region hits the disk); optional lower bandwidth setting per profile |

Measure in checkpoints 1 and 2, in the same scene: one viewer; two plain
viewers; two with the Off policy; then active plus Warm, plus Economy and plus
a monitor. Record Task Manager per-process GPU, dedicated GPU memory, CPU, RAM
and network.

## 5. Per-character isolation: `--profile <name>`

The main profile (no argument) keeps today's paths and behaviour, with no
migration. A named profile changes the following:

| Item | Named profile | Why |
|---|---|---|
| Settings folder (`settings::config_dir()`): `settings.json`, `login_accounts.json`, `accounts/<uuid>/` (transcripts, contact sets, HUD sizes), layouts, skins, keybinds, spelling, autoreplace, `cache.key` fallback, `graphics-before-safe.json` | `<config root>/profiles/<name>/` (under the demo subfolder in demo mode) | `settings.json` is last-writer-wins between processes (`Settings::save` → `atomic_file::write`); window geometry, graphics and UI belong to each character. One hook redirects all callers (they all go through `config_dir()`). |
| Profile lease | `profiles/<name>/profile.lock`, held with `File::try_lock` (as `media_pack.rs` does) for the process's life | One process per profile. A second launch hands its link (if any) to the running one, brings it forward and exits. The main profile keeps allowing several instances, as today. |
| Asset cache (`cache_dir()`), pipeline cache, browser cache, media plugins | **Separate per profile by default** (decision 4, section 17); sharing the main cache is an option per profile | Files are multi-writer safe (section 3); a shared cache saves bandwidth, decoding and disk. The `cache.key` fallback file (used without a credential store, `cache_key.rs`) must then be read from the main settings root, or profiles would seal one cache with different keys |
| Logs | Shared folder; log name `aurora-<profile>.log` with the usual `.previous.log` (one hook in `log_stem`) | Easy to find; still one file per process thanks to the lease |
| `instances/` registry | **Shared** (it sits beside the logs folder: `logs_dir().with_file_name("instances")`) | Otherwise the viewers would stop seeing each other. A reason not to move the logs folder per profile. |
| Crash sessions | Shared (UUID folders). The "GPU trouble" marker (`crash::take_gpu_trouble`) is consumed by whichever viewer starts next | Accepted for now; can be scoped per profile later |
| Credential store (saved passwords, MFA, cache key) | Shared (`grid\|user` keys) | A profile's login screen finds the saved password of an account it lists |
| `secondlife://` hand-over | Scope gains the profile, like `--title` (`Scope` in `slurl_handoff/windows.rs`, `name_for` on Linux) | A launch with `--profile X --url …` goes to profile X or starts it. An untitled browser launch still reaches the last-focused viewer, normally the active character. |
| Window title | `Folderstorm2 <version> — <profile>` (hook in `cli::window_title`) | Tell windows apart in Alt-Tab and the taskbar |
| Blender live link (opt-in, default port 17655) | Per-profile setting; a clash is reported by the existing error path | Only one process can listen on a port |
| Local assistant / MCP (FEATURES 4.1, future) | Discovery file per profile; tool calls name their target (4.6 "session routing") | Never route a request to "whichever is active" |
| Updater | Managed viewers do not offer the in-viewer restart-to-update; the host offers "quit all and update" | The installer cannot move the running exe while instances run (`aurora-updater/src/install.rs`: "close other instances") |

- **Names:** 1–32 characters from `[A-Za-z0-9_-]`, compared case-insensitively
  (Windows); `main` is reserved. Anything else is refused with a message, never
  turned into a path.
- **Parsing (verified):** `cli.rs` takes the first bare argument as the
  startup SLURL and stops reading. Without a hook, `--profile Alt1` would make
  "Alt1" the SLURL, so `--profile` (and `--session-host`, `--managed`) must be
  parsed in `cli.rs` (a few lines, like `--fps-limit`).
- **Order (verified):** the very first line of `main()` reads `settings.json`
  (`Settings::allow_overlays_at_start` → `Settings::path()` →
  `config_dir()`), before `cli::get()`. So the `config_dir()` hook must
  resolve the profile itself (through the lazy `cli::get()`, which starts no
  thread), not rely on a later initialisation.
- **Never shared, never copied:** passwords outside the credential store,
  transcripts, per-account files, the inventory cache.
- **Constants** (pipe prefix, profiles folder name, host window name) go in
  `folderstorm-brand`, with the collision test extended.

## 6. Background rendering policy

### 6.1 States

| State | Who | 3D | Loop and presentation | Textures | Sound / voice |
|---|---|---|---|---|---|
| **Active** | The character in front (managed: chosen by the host, not by keyboard focus) | Normal | Normal caps | Normal | User settings |
| **Behind: Off** (stage 1 default for managed characters) | Others | Paused: empty draw lists, 3D targets at 1/16 (the busy-pause state), a small "3D paused" notice | Interface-only frames at a steady low rate (default 5 fps, range 2–15). **Never zero.** | Kept (Warm) or given back (Economy = minimised) | Muted (runtime only), voice off |
| **Behind: Limited** | Others, by choice | Drawn at 1–5 fps, optionally at a lower resolution | As above | As above | As above |
| **Monitor** (stage 2) | A character with a monitor shown in the host | Off between captures; a small offscreen image at the monitor's size and rate | As above | Warm | As above |
| **Behind: Normal** | Opt-out | Today's behaviour (`BEHIND_FPS` 10, background cap) | – | – | – |

- **Stage 1 (checkpoints 1–4): Off by default for managed characters**,
  Limited optional (the unmanaged default is open question 1).
- **Stage 2 (checkpoint 5): monitors make it Limited** for that character, at
  the monitor's size and rate and only while the monitor is visible. A hidden
  or closed monitor returns it to Off.

Unmanaged (checkpoint 1), "behind" means *another Folderstorm2 window is in
front*, read from the instances registry (`Peers::in_front`), exactly the way
`BEHIND_FPS` works today. Switching to a browser does not pause the main
character. Managed (checkpoint 2 on), the host sets each process's role.

### 6.2 The past bug: "an inactive viewer shut itself down, believing the graphics card was inaccessible"

These are the places where Aurora can show the same symptom or a related one:

1. **Stall notice from a window behind** (`app/gpu_stall.rs`). A frame counts
   as "held" while the render thread is inside acquire, encode, submit or
   present; past 2 s it is a stall (`STALL_AFTER`, `aurora-render`
   `main_thread.rs`), and after 6 s (`NOTICE_AFTER`) a `MB_TOPMOST |
   MB_SETFOREGROUND` box asks "keep waiting or close the viewer". Windows
   serves the foreground first (`busy_pause.rs` measured 364–611 ms frames
   behind), and a present blocked on a hidden or minimised swapchain would
   count too **(verify on Windows)**: a box over the active character, one
   click from closing the window behind.
2. **Device loss** (`Renderer::device_lost`, `app/gpu_recovery.rs`, on by
   default): the first loss recovers on a new window and device; the second
   within 5 min (`LOSS_SPAN`) applies the Potato level **and saves it**
   (`safe_graphics_after_losses` → `settings.save()`); the third closes the
   viewer (`LossStep::Close`). A TDR resets the whole card, so *every*
   character takes each loss and counts it, even ones that hardly draw: the
   foreground's load can downgrade and then close the background characters.
   **The most likely Aurora form of the past bug.** With recovery off, the
   first loss closes.
3. **Hidden windows freeze the session.** `about_to_wait` notes that hidden
   windows may get no `RedrawRequested`, and the world and network part of
   `App::frame` (network events drained around `app.rs` line 9089, before the
   renderer is taken at line 9341) only runs from it. A viewer hidden
   (`SW_HIDE`), cloaked or left without a window stops handling chat,
   teleports and event queues, and a naive host then declares it dead.
4. **A host that reads slowness as death**, as Folderstorm (C++) did. Aurora's
   own watchdog only observes (`crash/watchdog.rs`: `IsHungAppWindow` after
   5 s writes `freeze.txt`).
5. **Focus rules hurting the active character.** Typing in the host's chat
   takes keyboard focus from the active viewer: the background cap (15 fps)
   applies, textures go after 60 s (`texture_budget::Background`) and the
   busy pause may stop its 3D (`busy_pause.rs`: `behind = !self.focused`).

### 6.3 Rules

- **R1 – Never stop the loop or presenting.** Behind is a policy inside the
  normal frame: the loop keeps running (chat, teleports, region crossings) and
  presents interface-only frames at a steady low rate, through the paths the
  renderer already uses (no new acquire or present logic).
- **R2 – Only window states Aurora already handles.** Behind another window,
  or minimised. No `SW_HIDE`, cloaking, off-screen or zero-size windows. If a
  hidden state is ever needed, the viewer drives `frame()` itself from
  `about_to_wait`, as it already does for a window not yet shown.
- **R3 – Role over focus.** Effective "in front" = keyboard focus, or the host
  says Active. Used by the frame cap, the `instances` throttle and
  `Own.active`, texture release and the busy pause.
- **R4 – No modal from behind.** A stall behind is logged, noted in the crash
  session and reported to the host as "GPU busy"; the usual box shows only if
  the stall remains when the window comes to the front. A stall never closes
  a viewer.
- **R5 – Losses behind neither downgrade nor close.** Behind, a device loss
  recovers without saving Potato (skipped, or for the session only: 3D is
  off) and without counting towards the close; the host gets the new window
  handle (`WindowChanged`). After repeated losses the character stays logged
  in with drawing suspended and offers "Recover graphics" when brought to the
  front. That needs R2's `about_to_wait` driver to run frames without a
  window (feasibility to verify in checkpoint 1).
- **R6 – No false deaths.** The host watches a heartbeat from a dedicated IPC
  thread (not the main loop) plus a main-loop counter: Healthy, Slow (loop
  stuck > 10 s), Not answering (no heartbeat > 30 s), Exited (process handle
  signalled). It never kills or detaches on its own.
- **R7 – Keep the safety nets.** The busy pause and the VRAM share stay on
  behind (a monitor lowers its rate on a busy card). Only the active
  character loads a busy region at full speed.

### 6.4 Code touch points (all small hooks marked `// Folderstorm:`)

| Rule | Upstream place | Change |
|---|---|---|
| Off / Limited | `app/busy_pause.rs` `drawing_paused()` | `\|\| folderstorm` "behind: Off" flag |
| Loop rate behind | `app.rs` after `instances::throttle(...)` | Apply the behind loop rate |
| R3 | `app.rs` `set_window_state(self.focused, …)`, `Own { active: self.focused && … }`, `frame_cap(..., self.focused, ...)`, `throttle(cap, self.focused, …)`; `busy_pause.rs` `behind = !self.focused` | Use the effective focus |
| R2 | `app.rs` `about_to_wait` | Also drive `frame()` when the session adapter asks |
| R4 | `app/gpu_stall.rs` `notice_due(...)` call | Defer while behind |
| R5 | `app/gpu_stall.rs` loss branch | Behind: recover without counting or saving |
| Notice | Next to the `busy_pause_notice` call | Our "3D paused, click to resume" notice |
| Sound | `app.rs` `sync_minimized_mute` | OR in the session mute (never saved) |
| Voice | Where `VoiceInputs { enabled, … }` is built | `&& voice allowed` |

## 7. Window hosting

| Approach | Windows | Linux | Verdict |
|---|---|---|---|
| **Separate top-level windows, arranged by the host** (it moves and resizes them into one "stage" rectangle; Warm = behind the active one, Economy = minimised) | Low risk. Switching uses `SetForegroundWindow`, allowed while the host is in front (after a click or its own hotkey), or `AllowSetForegroundWindow` as `slurl_handoff` already does. Risks: foreground-lock rules (the taskbar flashes instead), z-order disputes, a stage spanning monitors with different DPI **(verify on Windows)** | X11: raise via `_NET_ACTIVE_WINDOW`, focus-stealing rules vary. Wayland: a process cannot place or raise another's window; only XDG activation tokens (winit can request one) | **Recommended** (checkpoint 2) |
| Fitted borderless top-level following the host (the C++ final form) | Seen failing in C++: the world vanished when using host controls or other apps; dropdown and focus disruption; IME; z-order | Not possible on Wayland | Optional later (checkpoint 6), opt-in only |
| Cross-process child window (`SetParent`) | Joins the input queues of both processes (a frozen viewer freezes the host's UI), DPI-awareness mismatch, focus, IME and accessibility trouble; owner destruction killed viewers in C++ | X11 reparenting is similar; impossible on Wayland | **Rejected** |
| Frame transfer (offscreen, shared texture or readback, input forwarding) | Full-rate readback is costly; input forwarding would re-implement winit's input | Same | Only for monitors, at low rate |

**Not verifiable from a Linux cloud sandbox:** all Win32 window behaviour
(foreground rules, z-order, minimise and restore with a Vulkan swapchain,
DWM), global hotkeys, driver behaviour when a window behind presents, TDR and
recovery, named-pipe security at runtime, shared microphone capture,
multi-monitor DPI.

**Verifiable here:** unit tests of the logic crate, path and profile logic,
`cargo clippy` for the Windows target, and demo scenarios where a display is
available.

## 8. Host ↔ viewer IPC

- **Launch.** The host spawns `current_exe --profile <name> --managed`. It
  never passes `--login` and never sees passwords: each character logs in on
  its own viewer's login screen, which may use its remembered password.
- **Bootstrap.** The child's **stdin is a pipe** held only by the host and
  that child (anonymous on Linux and macOS; on Windows Rust makes a randomly
  named pipe whose ends are connected as it is made). The host writes one bounded line (control pipe name
  and a one-time 128-bit token) and closes it; the viewer reads it in the CLI
  hook and drops stdin. Nothing on the command line or in the environment.
  Std's `Command` passes stdin to GUI-subsystem processes through
  `STARTUPINFO` **(verify on Windows)**.
- **Transport.** Windows: a named pipe per slot,
  `\\.\pipe\folderstorm2-session-<session>-<host pid>-<slot>-<random>`,
  current-user security, remote clients refused, first instance only (reuse
  the `slurl_handoff/windows` security helpers); the host checks
  `GetNamedPipeClientProcessId` against the child it spawned, then the token.
  Linux: a Unix socket in `$XDG_RUNTIME_DIR`, mode 0600, peer-credentials
  check.
- **Framing.** A `u32` length, then serde JSON (`serde_json` is already used),
  64 KiB at most. Monitor frames: their own message kind, binary, 2 MiB at
  most, latest only. The handshake carries protocol number and build id, so
  different builds refuse each other.
- **Threads.** In the viewer, a `folderstorm-session` IPC thread does the
  blocking I/O and the heartbeat, over bounded `crossbeam` channels the main
  thread polls once per frame without waiting. One I/O thread per slot in the
  host.
- **Identity.** Every message carries the slot, a **login generation**
  (changes at every login or logout) and a role sequence number; stale ones
  are dropped. Grid and agent id are reported after login, and the host
  refuses a second slot on the same account and grid.
- **Host → viewer:** `SetRole{Active | Background{policy, standby} |
  Monitor{w, h, fps}}`, `Raise`, `Minimise`, `Quit{logout}`,
  `SendChat{conversation, text, id}`, `MarkRead`, `SetSound{mode}`,
  `SetVoice{allowed}`, `MonitorFrame{request}`.
- **Viewer → host:** `Status` (on change and every 1 s: screen, agent name and
  id, grid, region, connection, unread counts, window handle, role
  acknowledgement, fps, GPU state OK / busy / stalled / recovering / lost,
  main-loop counter); `Heartbeat` (IPC thread, 1 s); `ChatLine`, `ChatGap`,
  `SendResult{id, accepted | refused(reason) | uncertain}`, `Attention`,
  `WindowChanged`, `Frame`, `Exiting`.
- **Backpressure.** Bounded queues on both sides. Status and frames are
  coalesced to the latest. Chat lines are never dropped silently: an overflow
  becomes a `ChatGap` marker. A send that may have reached the grid is
  reported `uncertain` and never resent.
- **Unmanaged fallback.** On EOF or a broken pipe, the viewer leaves managed
  mode at once: focus-based behaviour, sound and voice back to its settings,
  window left visible where it is, still logged in.

## 9. Shared chat

- **Export by observation, not hooks.** About every 250 ms the viewer adapter
  reads the nearby chat (`World::chat`) and the IM sessions
  (`world.social.ims`). Their `LineClock` numbers lines (`first`, `revision`,
  `token`, in `world/transcripts.rs`), so the adapter sends only new lines and
  restarts cleanly when `token` changes. No change to upstream chat code.
  Group chats and conferences are sessions of `world.social.ims` too, so
  they are covered the same way (checkpoint 3, "As built").
- **Restrictions.** Export only what the viewer's own window would show
  (RLVa names and text redaction, receive restrictions), rechecked at export
  time.
- **Send** through the chat bar's own entry points (`App::send_local_chat`
  for nearby chat; the IM window's send path), so gestures, emotes, line
  splitting and RLVa send restrictions apply. A refusal comes back to the same
  conversation, never retried as another character.
- **Host chat model** (in `folderstorm-session`, tested):
  - one tab per character plus "All";
  - the character's name beside every line and on the compose box ("Send as
    Sam");
  - drafts bound to (slot, generation, conversation), so switching characters
    never retargets typed text, and a relogin never sends an old draft;
  - bounded history (for example 500 lines per conversation, 64
    conversations per character) with gap markers; notices in their own
    bound;
  - unread badges per character.
- **Not in the host:** answering offers, payments, teleport requests and
  dialogs. These appear as "Attention" items with "Switch to answer"; the
  viewer's own dialog handles them. Nothing is auto-accepted.
- **No combined archive.** Transcripts stay in each character's viewer.

## 10. Sound and voice

- **Sound.** Background characters are muted by default. This is a
  session-only override like `MinimizedMute`; it never changes saved volumes.
  Choice per character: muted / alerts only / all. Optional host sound for an
  IM to a background character (checkpoint 3).
- **Voice.**
  - Only the active character joins voice and transmits. Background
    characters run with voice disabled for the session; the saved choice is
    untouched.
  - On a switch, the old character's voice is revoked (transmit off,
    push-to-talk released) before the new one is allowed.
  - "Listen in the background" could come later, once live voice has been
    tested.
  - Several processes capturing the same microphone is normally allowed on
    Windows (shared mode) **(verify on Windows)**. Only the active one sends.

## 11. Restart and recovery

- **Viewer exits** (quit, crash): the host sees its process handle signalled,
  so the slot shows Closed or Crashed with **Restart**. Restart opens the same
  profile's login screen; there is no automatic login. Drafts stay, but they
  are bound to the old generation and are not sent to the new login.
- **Viewer slow or not answering** (R6): the slot shows a badge. Nothing is
  killed automatically. "End process" is a confirmed user action.
- **Host closes or crashes:** viewers fall back to unmanaged mode (section 8)
  and stay logged in and visible: no orphaned hidden sessions. A relaunched
  host does not adopt running viewers at first. Re-attaching through the
  profile lease and a fresh token can come later.
- **GPU loss in a viewer:** R5. The viewer reports its new window, and the
  host arranges it again.
- **Quit all:** `Quit{logout}` to each viewer, a bounded wait (about 20 s),
  then any left over are listed with "End" (confirmed).
- **Updates:** see section 5.

## 12. Monitors (stage 2)

- Up to 4 monitors, for background characters. Sizes 320×180, 480×270 or
  640×360; rates 0.5, 1, 2 or 5 fps (default 1); an aggregate budget across
  all monitors.
- **In the viewer:** while a monitor is visible its role is `Monitor`: 3D
  stays Off between captures, and at each tick it draws a snapshot of its own
  camera at monitor size without the interface (Aurora's offscreen snapshot
  path draws at its own size and overrides the pause), read back and sent as
  the latest frame. RLVa vision restrictions must apply to it as on screen
  (to check).
- **To check in code first:** the snapshot path can wait for the GPU up to
  `CAPTURE_WAIT` (10 s) on the main thread; monitor frames need a short wait
  or the existing late-answer path (`snapshot_sent`), and a timeout drops the
  frame. On a busy card (busy pause) the rate drops and the monitor says so.
- **In the host:** frames become egui textures (no GDI repaint flicker),
  labelled with the character, the frame's age and the connection state.
  "Switch to" is the only action: no input reaches the viewer, nothing is
  recorded or saved. A minimised or hidden host pauses its monitors.
- **World viewport (PR #6):** a "Limited" window could draw only a small
  world rectangle (`set_world_rect`). On wide screens, the host can give the
  active character a large window with its 3D in one part and the host panels
  beside it.

## 13. Quick actions, profile copy and later features

- **Quick actions:** a host button per slot (switch when ready, launch when
  stopped, bring forward at its login screen; never two processes for one
  slot) and global hotkeys (`RegisterHotKey`: Ctrl+Shift+F1–F5, next, previous, since
  the follow-ups to checkpoint 7; first Ctrl+Alt+1–5;
  configurable, conflicts reported). Host settings go in `<config
  root>/session-host.json` (slots, stage rectangle, hotkeys, standby and
  monitor choices), never credentials.
- **Profile copy:** "New character profile…" from the main profile, another
  profile or the defaults, only while the target is not running (lease free).
  Copies `settings.json` (without window geometry and remembered user name),
  keybinds, layouts and skins; never `accounts/`, transcripts, credentials or
  caches. Selective groups (graphics, interface, keys) later, as in C++.
- **Later, for the human to pick:** character colours and aliases, pinned
  chat tabs, an attention inbox, per-category notification sounds, a friends
  list and New IM in the host, session sets (launch several profiles
  together, with no auto-login), the bird's-eye switch effect,
  condensed or minimal host modes, MCP session routing (FEATURES 4.6).
  (Checkpoint 7 built the colours and aliases, the attention inbox and
  the condensed mode, and decided against the bird's-eye effect as C++
  had it: see its "As built". Its follow-ups added the inbox's host-only
  Dismiss, a confirmation for Quit all and new default hotkeys.
  Checkpoint 8 put the quick actions in a bar at the top of every
  managed viewer, with up to twelve positions of the user's choice.)
- **Creators' permissions:** none of this shows, copies or exports assets or
  asset ids. Monitors show what that character's viewer shows and are never
  saved. Profile copy moves the user's own settings files only.

## 14. Code layout and upstream-merge cost

New code, all ours (layout as on the PR #6 branch):

- `crates/folderstorm-session/`: the pure, unit-tested logic (section 4.2).
- `crates/aurora-viewer/src/folderstorm/`: `profile.rs` (paths, lease, name
  check), `session/` (viewer adapter: IPC thread, roles, behind policy, chat
  export, commands), `session_host/` (host mode: window, processes, Windows
  arrangement, hotkeys, chat and monitor panels).
- `app/folderstorm_session.rs` (the `impl App` glue, once per frame) and
  `ui/options/pages/folderstorm_session.rs` (the checkpoint 1 option).
- `folderstorm-brand`: pipe prefix, host window name, profiles folder name,
  with test entries.

Upstream hooks (estimate: about 30 marked lines in about 12 files):

| File | Hook |
|---|---|
| `main.rs` | `mod folderstorm;` (shared with PR #6); a host-mode branch before `run_viewer()` |
| `cli.rs` | Parse `--profile`, `--session-host`, `--managed` (fields plus 3 branches); profile suffix in `window_title` |
| `settings.rs` | `config_dir()` redirect; one field for the behind policy (or a separate fork settings file to avoid touching `Settings`) |
| `logging.rs` | Profile in `log_stem` |
| `slurl_handoff/windows.rs`, `slurl_handoff.rs` | Profile in the scope and bus name |
| `app.rs` | Adapter field and init; one per-frame call; effective focus at 4 sites; loop rate after `throttle`; `about_to_wait` driver; `sync_minimized_mute`; voice `enabled` |
| `app/busy_pause.rs` | `drawing_paused` and `behind` |
| `app/gpu_stall.rs` | Notice deferral; loss policy behind |
| UI | One options-page registration; one notice call |
| Docs and locales | Rows in `README.md` (command line) and `docs/TESTING.md`; en/fr/ja entries (append-only) |

(Checkpoint 8 added two lines to `app.rs`: the bar's strip, given to the
interface's pass as egui's safe-area inset, and the room of the viewer's
own bars under it.)

No upstream file is restructured. If a hook grows beyond a line or two (for
example the loss policy), stop and propose an upstream-friendly extension
point instead.

## 15. Checkpoints

Each checkpoint is its own branch and PR, testable by the human on Windows,
and useful even if the next one never comes.

### Checkpoint 1: profiles and a background limit (no host)

May be two small PRs: 1a profiles, 1b behind policy.

- **1a:** `--profile <name>` with everything in section 5 (paths, lease,
  `secondlife://` scope, title, log name). Optional: World › "Open another
  character…" to launch a profile without typing a command.
- **1b:** Preferences option "3D while another Folderstorm2 window is in
  front": Off / Limited (fps) / Normal, with the behind loop rate. R1, R3
  (registry form), R4 and R5 apply while behind.
- **Demo:** `AURORA_DEMO_SESSION_BEHIND=1` (as if another viewer were in
  front: the registry is off in demo). Combine it with the existing
  `AURORA_DEMO_GPU_STALL`, `AURORA_DEMO_GPU_LOST` and `AURORA_DEMO_BUSY_PAUSE`
  to test R4 and R5 offline.
- **Acceptance (human, grid):**
  - Three windows (main, `Alt1`, `Alt2`) keep their own settings, windows
    and logins across restarts; a second `--profile Alt1` launch brings the
    first forward; a browser `secondlife://` link opens in the last-focused
    window.
  - With Off, windows behind show the paused notice and use little GPU in
    Task Manager; their chat, IMs and a teleport accepted from behind still
    work; a click restores full 3D within about a second.
  - Heavy load in front never raises a graphics box from a window behind;
    after a driver reset every window stays open and none switches to Potato.

- **As built (branch `claude/session-checkpoint-1`):** pure rules in
  `crates/folderstorm-session` (`profile`, `behind`, `gpu`); viewer side in
  `crates/aurora-viewer/src/folderstorm_characters*` and
  `app/folderstorm_characters.rs` (not under `folderstorm/`, which PR #6
  creates). Differences from the text above:
  - R5: a loss on a window that is not in front (not focused, minimised or
    covered) does not recover there; it **waits**, with no box, no count, no
    save and no close, while the session goes on, and recovers uncounted
    when the window comes to the front. A recovery behind would open its new
    window off screen when minimised, could take the focus from the active
    character when shown, and closes the viewer if the new device cannot be
    made during the reset. Cost: the image of a window behind stays frozen
    until it is clicked.
  - R4 holds the stall box back behind for every viewer, not only managed
    ones; the same for R5.
  - Limited is the low frame-rate cap with the 3D drawn; monitors come later.
  - Not done: per-profile Blender live-link port, the World menu's "Open
    another character…", runtime sound mute and voice off behind
    (decision 5 applies with the host, checkpoint 2).

### Checkpoint 2: session host, launch and switch

- `aurora-viewer --session-host`: up to 5 slots (profiles), Launch, Switch,
  Close and Restart; per-slot status (screen, name, region, unread, health).
- IPC (section 8), roles, R2, R3 (role form) and R6.
- Stage arrangement; Warm (behind) or Economy (minimised) per slot.
- Background sound muted, voice only for the active character.
- Host gone → viewers unmanaged. Viewer gone → Restart. Quit all.
- **Demo:** `AURORA_DEMO_SESSION_HOST=3` launches three demo viewers with
  profiles `demo-1…3`.
- **Acceptance:** switch among three characters repeatedly, including from
  the host while typing in it: no held keys, the active one stays at full
  rate. Kill the host in Task Manager: viewers stay logged in and visible.
  Close one viewer: the others are unaffected and Restart works. Measure Warm
  against Economy (VRAM, switch time).

- **As built (branch `claude/session-checkpoint-2`):** the rules are pure
  and tested in `crates/folderstorm-session` (`ipc`: messages, framing,
  token and hello checks; `managed`: role over focus, sound and voice in a
  viewer; `host`: the desk, its slots, liveness, voice hand-over, restart,
  Quit all). In the viewer: `folderstorm_characters/host.rs` and `host/`
  (the host's processes and window), `managed.rs` (a viewer's link),
  `transport.rs` (the endpoint), and `app/folderstorm_characters.rs` (the
  glue). Differences from sections 4, 7, 8 and 10 above:
  - **The host's window is drawn on the processor** (softbuffer and the
    crash window's painter, `crash/software.rs`), not with egui-wgpu: the
    host opens no graphics device, so it takes no video memory from the
    characters and no driver reset or stall reaches it. This is the "most
    stable" form of decision 2. Monitors (checkpoint 5) will upload frames
    to that painter or move the host to wgpu then.
  - **IPC: two one-way channels instead of one two-way pipe.** Host to
    viewer is the viewer's standard input for the whole session, not only
    for the bootstrap: only the host and that child hold it, and its end
    is exactly "the host is gone" (unmanaged fallback) on every system,
    with no Windows-only code. On Linux and macOS it is an anonymous pipe.
    On Windows, Rust's piped standard input is a named pipe with a random
    name (`\\.\pipe\__rust_anonymous_pipe1__.<pid>.<random>`, first
    instance only, remote clients rejected) whose other end is opened as
    soon as it is made, so there is no moment when another process could
    join it, but it does have a name.
    Viewer to host is the private endpoint of section 8, inbound only. A
    blocking two-way named pipe would also have needed overlapped I/O (a
    blocked read holds a write on the same synchronous handle). The
    Windows pipe is made by the `secondlife://` hand-over's own tested code
    through one hook (`slurl_handoff::folderstorm_session_pipe`): first
    instance, remote clients refused, this logon only. The host checks the
    client's process id, then the launch's token (244 random bits from two
    version 4 UUIDs, compared in constant time), protocol and build; the
    viewer checks that the pipe is served by the host's process.
  - **Linux and macOS:** a socket named `s` in a folder made for it with
    mode 0700 (`$XDG_RUNTIME_DIR`, or the temporary folder when that path
    is too long for a socket), the client's uid and pid checked with
    `SO_PEERCRED` on Linux; on macOS the folder's permissions and the token
    alone.
  - **One whole assignment instead of `SetRole`, `SetSound` and
    `SetVoice`:** `Assign{seq, role, behind, standby, sound_muted,
    voice_allowed}` is the full wanted state; the viewer keeps the highest
    `seq` and reports it, the host sends it again until it is reported. A
    lost or late message cannot leave a viewer half set. `Show{stage}`
    (restore, place, focus), `Minimise` and `Quit` are the one-shot
    commands; `Quit` always logs out (no `logout` flag). The slot is in the
    bootstrap and the hello, not in every message: each launch has its own
    two channels, and the host tags what it reads with the slot's launch
    number and drops what comes from an earlier launch. The login
    generation is in the status (used from checkpoint 3). `Exiting` is not
    sent: the process handle says how a viewer ended. `SendChat`,
    `MarkRead`, `ChatLine`, monitor frames: later checkpoints.
  - **Voice hand-over is strict:** the new active character is allowed
    voice only when every other viewer has reported an assignment without
    it **and** that its voice is off, or has ended. A viewer turns its voice
    off as soon as its link reads such an assignment (not at its next
    frame, which may be far off while minimised), and reports it given up
    only once its voice module is disconnected. A viewer that does not
    answer keeps the voice waiting (it may still be transmitting); the host
    says whom it waits for.
  - **The viewer moves its own window.** The host never touches another
    process's window: `Show` and `Minimise` are carried out by the viewer
    with winit (the host only calls `AllowSetForegroundWindow` for it
    first, as the link hand-over does). No cross-process `SetWindowPos`,
    and the same code on X11.
  - **The stage is the active window's rectangle** ("Keep the characters'
    windows in the same place", on by default): the window brought forward
    takes the outer position and size (or the maximised state) of the one
    that was active; moving the active window moves the stage. Nothing is
    computed from monitors by the host, so mixed-DPI layouts are whatever
    the user arranged; a stage that is on no screen any more is not taken.
  - **The host follows the user:** a background window that gets the
    keyboard (a click, Alt-Tab, the taskbar) becomes the active character
    (only a change to focused counts, so a switch from the host is never
    undone by a late report).
  - **A viewer the host cannot manage is let go:** when its hello is
    refused, or it never reaches the endpoint (only other clients came),
    the host closes its command pipe, so the viewer sees the host gone and
    goes on by itself; the slot shows "not managed" until that process
    ends. A viewer still starting after a minute is noted and End is
    offered; nothing ends it by itself.
  - **R2:** only "behind" and "minimised" are used. Commands and reports
    run from `about_to_wait`, not from the frame, so a minimised viewer
    still obeys and reports even if its frames are rare; the main-loop
    counter of R6 counts those turns.
  - **R3:** `App::folderstorm_in_front()` (focus, or the host's active
    character) replaces `self.focused` at the five sites of section 6.4.
    The GPU rules of checkpoint 1 (R4, R5) still go by the real focus: a
    stall box or a recovery of the active character waits for a click on
    its own window while the user is in the host's window. The real focus
    is the window's own (`gpu::has_keyboard`: a window that exists and has
    the focus), not the viewer's focus flag, which starts true before any
    focus event; it is also what the status reports.
  - **Background sound** rides Aurora's "mute when minimised" transient
    mute (never saved); **voice** is `voice_enabled && allowed` where the
    voice session is fed, and the microphone is released by the same mute.
  - **A host's Quit while the card holds a frame** is not recorded as a
    close in GPU trouble: the next start keeps its graphics (R4, R5).
  - Not done here: the host does not adopt viewers already running
    (section 11), the per-character sound choice "alerts only / all"
    (section 10), the same-account refusal (the host only warns: the login
    happens in the viewer), the updater's "quit all and update"
    (section 5), global hotkeys and profile copy (checkpoint 4).

### Checkpoint 3: shared chat

- Section 9. Optional host sound for a background IM.
- **Acceptance:** receive and reply as any character without switching
  (nearby chat, IM, group); drafts keep their sender across switches and
  relogins; a send during a broken pipe is reported "uncertain" and never
  doubled; RLVa receive and send restrictions hold; offers show as "Switch to
  answer".

- **As built (branch `claude/session-checkpoint-3`, on top of checkpoint
  2):** the rules are pure and tested in `crates/folderstorm-session`
  (`ipc`: protocol 2 and its new messages; `export`: which lines are new by
  the viewer's `LineClock`, and "said at most once"; `chat`: the host's
  model). In the viewer: `folderstorm_characters/chat_export.rs` (what a
  managed viewer tells), `app/folderstorm_characters/chat.rs` (the `impl
  App` glue: saying a line, the demo), `folderstorm_characters/host/chat_ui.rs`
  (the panel). **No new upstream hook**: the export only reads
  `World::chat`, `World::social.ims`, their clocks and
  `World::notifications`, and runs from checkpoint 2's
  `folderstorm_managed_pump` call in `about_to_wait`.
  - **Coverage.** Nearby chat, instant messages, group chats and
    conferences: Aurora keeps all three kinds of session in
    `World::social.ims`, so they are told and written to the same way. A
    line is told with the sender's name and the text the conversation shows
    (`ui::chat::sender_name`, `line_body`, `resolved_text`). Only lines of
    this session are told (`Origin::Live`): the transcript's history and a
    group's server history stay in the viewer. An instant message that the
    viewer also repeats in its nearby chat is told once, in its
    conversation. The host starts no conversation: it writes to those a
    viewer has open (a closed one answers "closed in the character's
    window"). No typing notices, no chat volume choice (a nearby line is
    said at normal volume; "/5 text" goes to channel 5 as in the chat bar).
  - **IPC, protocol 2** (a host and a viewer of different protocols refuse
    each other, as for builds). Viewer to host: `Chat(ChatBatch{generation,
    conversation, kind, title, restart, missed, unread, lines})`,
    `Attention(AttentionList)` (whole, latest replaces),
    `SendResult{id, Accepted | Refused(reason)}`. Host to viewer:
    `SendChat{id, generation, conversation, text}`,
    `MarkRead{generation, conversation}`. `ChatGap` is the batch's `missed`
    count; "uncertain" is never on the wire: it is what the host concludes
    when no answer comes. Every text is bounded and stripped of control
    characters on arrival, on both sides (line 2048 characters, text to say
    1024, title 80, 32 lines a batch, 16 attention items); a full batch is
    proven under the 64 KiB frame. A message that is not one ends the link
    (viewer) or the reading of that viewer (host), as in checkpoint 2.
  - **Never as another character.** A line to say travels only on the
    standard input of the one viewer it was typed for (the effect names the
    slot and its launch; the host writes to that launch's pipe or not at
    all), carries that viewer's login generation, and names no character:
    there is nothing in a message that could make viewer B speak for A. In
    the host a draft is bound to (slot, launch, login generation,
    conversation). The launch is part of it because a restarted viewer
    counts its logins from 1 again. After a logout, a login or a restart
    the draft is **held**: shown, not sent, until the user presses "Use it
    as <name> now" or discards it. A character removed from the desk leaves
    nothing behind for the next profile in its slot.
  - **Never twice.** The host hands a line to a pipe once and never again.
    Not queued (the viewer does not read, its queue is full): "not sent",
    text back in the box. Queued and no answer (the process ended, or 15 s
    of silence): "uncertain", with the text, **not** put back in the box.
    The viewer says each `id` at most once (ids grow; an id not above the
    last is refused as repeated) and keeps its answers until the link takes
    them. A refusal is noted in its own conversation and the text goes back
    to that draft.
  - **Sending uses the viewer's own paths.** Nearby: `App::send_local_chat`
    (channel prefix, gestures, chat bar commands, then `App::send` with
    RLVa's `outgoing_chat`). Conversations: **deviation**: the
    conversations window's send is not a function but a loop inside
    `App::handle_actions` (`for (to, text) in a.ims`), which runs once per
    frame with the whole frame's actions; the pump runs outside frames
    (a minimised character has few). Its steps are repeated in
    `App::folderstorm_send_im` (":" emote, emoji shortcodes, group or
    conference session, unblock notice, `World::own_im`, `App::send` with
    RLVa's `sendim` rules), with a comment to keep both alike at upstream
    merges. Options if this drifts: (a) ask upstream to make that loop body
    a method (the clean fix, about 20 moved lines); (b) do it in the fork
    (a conflict at each upstream change of those lines).
  - **RLVa.** Receive restrictions are applied by Aurora before a line is
    stored, so a refused line never reaches the export (tested with
    `@recvchat`, `@recvim`). Names and places are replaced by Aurora
    partly when a line is stored and partly when it is drawn; the export
    uses the drawing's own functions at export time, and while
    RLVa is on it checks every 2 s that the last 200 lines told per
    conversation still read as the window shows them: when one does not (a
    restriction came, a hidden resident's name was learnt) the conversation
    is told again (`restart`) and the host **replaces** its copy, in the
    conversation and in "All" (tested with `@shownames`). Send restrictions
    hold because the viewer's own send path applies them; what RLVa makes
    of the line (redirected, replaced, dropped) shows in the viewer's
    conversation and comes back from there. `Refused(Restricted)` exists in
    the protocol but nothing sends it yet: Aurora's paths do not report a
    refusal to their caller.
  - **Host model:** a tab per character and "All"; in a character's tab
    its conversations (nearby chat first); "Send as <name> — <place>" above
    the box; in "All" the box writes to the conversation of the line that was
    clicked (each line starts with "[character › conversation]"), never to
    "the latest";
    500 lines and 50 host notices per conversation, 64 conversations per
    character (the one heard of longest ago leaves; never the nearby chat
    or the one on screen), 1000 entries in "All"; gap markers for lines the
    viewer dropped first, and "older lines are in the character's own
    window" for what the host dropped; unread per conversation and
    character, cleared when on screen in a focused host window, and the
    viewer is told (`MarkRead`) so its own counter and bell follow.
  - **Attention:** the viewer's notifications that need an answer
    (`Notification::interactive`: teleport offers and requests, friendship,
    inventory offers, group invitations, script dialogs, permissions,
    links, calls) are listed per character with **Switch to answer**; the
    cards of offers inside a conversation have the same button. The host
    has no message that answers anything. A payment received is a plain
    line (nothing to answer).
  - **Sound:** the viewers' interface sounds are grid assets and the host
    never logs in, so it cannot play them. "Sound when a background
    character gets a message" (off by default, saved in
    `session-host.json`) plays the system's notification sound
    (`MessageBeep`, Windows only) for an instant message, a group or
    conference line or an offer to a character that is not the active one;
    the host's taskbar button flashes either way. The per-character
    "alerts only / all" of section 10 is still open.
  - **Window:** still drawn on the processor (egui through the crash
    window's software painter, no graphics device); the chat is the same
    egui with the shared widgets, beside the characters (side by side from
    about 940 points of width, two pages below).
  - **Demo:** each demo character hears a neighbour, gets an instant
    message and one teleport offer (`AURORA_DEMO_SESSION_HOST`,
    `docs/TESTING.md`).
  - Not done: starting a conversation from the host, typing notices,
    shout / whisper, a line's links as links (plain text), names coloured
    per character, the chat as its own window (checkpoint 6).

### Checkpoint 4: comfort

- Quick-action buttons and global hotkeys; profile copy (section 13); voice
  follows the active character (section 10).
- **Acceptance:** hotkeys switch even while a viewer has focus; a copied
  profile starts with the main profile's interface but no accounts; only the
  active character is heard (two test accounts, live voice).

- **As built (branch `claude/session-checkpoint-4`, on top of checkpoint
  3):** the rules are pure and tested in `crates/folderstorm-session`
  (`hotkeys`: the hotkeys' texts, what is wrong with a choice, the numbers
  `RegisterHotKey` takes; `copy`: what a new profile takes and never
  takes; `host`: the quick action, next and previous, what is heard in
  the background, who holds the voice; their tests are in
  `host/comfort_tests.rs`). In the viewer:
  `folderstorm_characters/host/hotkeys.rs` (the keys taken from Windows),
  `host/comfort.rs` (the host's side), `host/ui/comfort.rs` (the window's
  side: the strip, the two pages), `profile_copy.rs` (the files). **No new
  upstream hook**: everything is in the host's own files, and the host's
  event loop is built with winit's message hook, as `raw_mouse.rs` does.
  - **Quick actions.** `Desk::quick(slot)` is the decision table: Switch
    (runs behind the active one), Show (is the active one), LogIn (waits at
    its login screen: its window comes forward and it becomes the active
    one), Wait (starting: it comes forward when it reports), Launch,
    Restart, Nothing (closing, a viewer the host let go that still runs,
    "Quit all" under way, empty slot).
    `quick_act` is `switch` behind that table, so a slot never gets a
    second process: a live slot is never launched. In the window: a strip
    of numbered buttons above the rows ("1 · name"), the icon and the
    tooltip saying what the button will do; the rows keep their own
    buttons.
  - **Global hotkeys, Windows only.** `RegisterHotKey` with no window:
    `WM_HOTKEY` comes to the host's main thread whichever program has the
    keyboard, and winit's message hook hands it to the desk
    (`Desk::hotkey`) in the same turn of the loop. A character's hotkey is
    its quick action (it starts a stopped character); next and previous go
    round the characters that run and never start one. Defaults
    Ctrl+Alt+1–5, Ctrl+Alt+PageDown (next), Ctrl+Alt+PageUp (previous)
    (replaced by Ctrl+Shift+F1–F7 in the follow-ups to checkpoint 7);
    "Hotkeys…" changes them as text ("Ctrl+Shift+F5": digits, letters,
    F1–F24, arrows, PageUp, PageDown, Home, End, Insert, Num0–Num9; Ctrl or
    Alt needed, and a second modifier unless the key is a function key;
    Alt+F4 refused), turns them off, or puts the defaults back. Saved as
    texts under `hotkeys` in `session-host.json`. **Deviation: no key
    capture**, the keys are typed as text (the host's window draws egui on
    the processor and a capture field would need its own key handling).
  - **Conflicts are told.** Each hotkey shows its state on the page: in
    use, refused by Windows (another program holds it), not a hotkey (with
    why), the same keys as an earlier action (the first keeps them); the
    characters' page says "n hotkeys do not work" while one is refused.
    **Added to the plan: AltGr.** On keyboards with an AltGr key, AltGr is
    Ctrl+Alt, so the default Ctrl+Alt+digit would take `#`, `{`, `[`…
    from every program on many European layouts (AZERTY among them).
    Before registering a Ctrl+Alt chord the host asks the current layout
    whether it types a character (`ToUnicodeEx`, keyboard state left as it
    is) and refuses it if so, with "On this keyboard these keys type a
    character". On such a keyboard some defaults therefore start refused
    and the user picks others (Ctrl+Shift+digit, function keys).
    **(verify on Windows)**: the check itself, and whether the layout of
    the host's thread is the one the user types with.
  - **The foreground from a hotkey.** The path is checkpoint 2's: the desk
    emits `AllowForeground` (the host calls `AllowSetForegroundWindow` for
    that viewer), then `Show`, and the viewer brings itself forward with
    winit's `focus_window`. The process that receives a hotkey may set the
    foreground, so the grant holds; the key-ups that follow go to the
    viewer that had the keyboard and may end it before the new viewer has
    acted, but winit's `focus_window` on Windows does not depend on it
    (it sends itself an Alt key press, which lifts the foreground lock,
    then calls `SetForegroundWindow`). **(verify on Windows)**: a switch by
    hotkey while a viewer has the keyboard brings the other window forward
    with the keyboard, not a flashing taskbar button; no key stays held in
    the viewer left (checkpoint 2's acceptance, now by hotkey).
  - **Linux and macOS:** no global hotkeys (X11 would need a key grab on
    the root window and an event connection of its own; a Wayland program
    cannot take a key from the others). The page says so, the choices are
    kept. In the demo every hotkey has a "Try" button that does what the
    key would, on every system.
  - **Profile copy.** "New character profile…": a name (checkpoint 1's
    rules), and what it starts from: the main profile, the defaults, or an
    existing profile. Refused while the target runs (its lease is held, or
    this host has it starting or running) and when the target exists:
    **an existing profile is never written into**, so "only while the
    target is not running" only ever bars a folder that is there (stricter
    than section 13, which did not say what becomes of an existing
    profile). Copied, by an allow-list (`copy::part`): `settings.json`
    through `copy::strip_settings`, `ui_layout.ron`, `skins/` (plain files
    only, links never followed, bounded depth, count and size). The key
    bindings are in `settings.json`. Never: `accounts/` (per-account files
    and transcripts), `login_accounts.json`, `rlva-login.json`, the cache
    key's file, caches, locks, the other profiles, the host's file, and
    anything the list does not name. The system's credential store is not
    read. Stripped from the preferences: the login (`username`,
    `remember_username`, `last_login`, `remember_password`), the window
    (`window_*`), `cache_location` (the new profile keeps its own cache
    folder), `folderstorm_characters` (a named profile's defaults differ),
    `local_assistant` (granted per profile by the user), the GIF service
    keys, and **every setting kept per account** (a value that is an
    account id, a list of them, or an object keyed by them: hover
    heights, bridge, movements, parcel radio…), whatever it is called. A
    test in the viewer checks that the names stripped are still the names
    of Aurora's settings. The copy is made in `profiles/.new-<name>-<random>`
    (no profile name starts with a dot) and renamed into place; a failure
    removes it. A source whose `settings.json` does not read is not copied
    as it is: the profile starts with the defaults and the user is told.
    The new profile joins the list when a slot is free. Not done: the
    selective groups (graphics, interface, keys) of Folderstorm (C++),
    spelling dictionaries and autoreplace (not in section 13's list).
  - **Voice follows the active character.** The mechanism is checkpoint
    2's with its review fixes (a viewer reports its voice off only once it
    is off, and the host gives the voice to the new active character only
    then). Added here: `Desk::voice(slot)` (allowed here, waits for that
    other character, giving it up, off) and its tests under hotkey presses
    faster than the viewers answer (never two characters told they may use
    voice); in the window the active row says **Voice** once its viewer
    reports it allowed, the row left says "Releasing voice…", and the line
    "Voice stays off until … has released it" stays. Live voice with two
    accounts is the human's test.
  - **Sound per character:** "Heard in the background" on a row
    (`BackgroundSound::All`, saved per slot): that character's viewer is
    not muted while another is active; its voice stays off. Section 10's
    **"alerts only" is not done**: Aurora's transient mute cuts everything
    at once, and telling alert sounds from the world's would need a change
    in upstream's audio paths. Options when wanted: (a) ask upstream for a
    mute that spares interface sounds; (b) keep the host's own
    notification sound (checkpoint 3) as the alert, which exists today.
  - **Demo:** `AURORA_DEMO_SESSION_HOST` as before, with the strip, the
    hotkeys (real ones on Windows, "Try" everywhere; choices not saved)
    and "New character profile…" working on the demo's settings folder
    (`docs/TESTING.md`).

### Checkpoint 5: monitors (3D becomes Limited)

- Section 12.
- **Acceptance:** four monitors at 1 fps update and show the frame age; the
  image matches the character's own view (aspect, orientation, restrictions);
  no flicker; closing a monitor returns that viewer to Off; foreground fps
  barely changes (measure the added GPU and CPU cost); a busy card lowers the
  monitor rate instead of resetting the GPU.

- **As built (branch `claude/session-checkpoint-5`, on top of checkpoint
  4):** the rules are pure and tested in `crates/folderstorm-session`
  (`monitor`: sizes, rates, the budget, when a viewer takes a picture and
  how it lowers its rate, the picture's size and its header's checks,
  "latest only"; `ipc`: protocol 3; `managed`: the role; `host`: which
  characters are watched, in `host/monitor_tests.rs`). In the viewer:
  `app/folderstorm_characters/monitor.rs` (a managed viewer takes its
  pictures), `folderstorm_characters/managed.rs` (the link sends the
  latest), `host/process.rs` (the host's reader takes them),
  `host/monitors.rs` and `host/ui/monitors.rs` (the host keeps and shows
  the latest).
  - **Upstream hooks (new):** `aurora-render` `main_thread.rs`:
    `Renderer::snapshot_wait` (how long the frame that carries a photo
    waits for the card, instead of `CAPTURE_WAIT`'s 10 s) and
    `Renderer::snapshot_pending()` (a photo's frame is still on the card);
    `app.rs`: one call after `photo_frame` (`folderstorm_monitor_frame`)
    and one before `photo_drawn` (`folderstorm_monitor_drawn`). Nothing
    else of Aurora's photo path changes: a monitor's picture is a
    `Renderer::snapshot_request` like a photo's part, asked only when no
    photo is drawn, and its answer is taken before `photo_drawn` would
    drop it.
  - **Role.** `Role::Monitor(Order{size, interval_ms})` beside `Active`
    and `Background`, in the one `Assign` (so idempotent, acknowledged and
    sent again like the rest). A watched viewer is a background character
    in everything (3D Off, sound, voice): the 3D pause of checkpoint 1
    stays on between two pictures and only the frame that carries a
    picture draws the scene (`App::scene_paused` is false for a frame
    whose `snapshots.capturing` is set, as for a photo). A window the user
    clicks takes no pictures (it draws for itself), and the host then
    makes it the active character as before.
  - **Watched only while seen.** The desk asks pictures of a character
    only while it runs in the background, its monitor is open **and that
    monitor's box is on screen in the host's window**: each drawing of the
    window notes which boxes are visible (`ui.is_rect_visible`), and none
    is while the window is minimised or covered (`WindowEvent::Occluded`,
    `Window::is_minimized`), on the chat's page of a narrow window, or
    scrolled away. Otherwise the viewer gets a plain `Background`
    assignment: closing or hiding a monitor returns that viewer to Off.
    Monitors are saved per character in `session-host.json` (`monitor`:
    size and rate). At most 4 run at once (decision 3).
  - **Budget.** 1 200 000 pixels a second for all monitors together
    (four 640 × 360 monitors at 1 a second, or one at 5 a second, pass).
    Past it every monitor's interval is stretched by the same factor
    (`monitor::grant`), and the monitor says "n pictures a second: the
    monitors share one budget".
  - **A picture has the window's proportions** (`monitor::fit`): the
    largest even size with the proportions of the world's part of the
    viewer's window inside the monitor's 16:9 box, so it is the
    character's own view scaled down (a 4:3 window gives 480 × 360 in the
    large box), drawn with the frame's own camera and field of view and
    shown in the host with bars, never stretched. Rows come from the top,
    as the photo path reads them back.
  - **Frame transport: raw RGB on the viewer's own endpoint, after a
    checked header.** A `ToHost::Frame(FrameHeader{generation, seq,
    width, height, bytes})` message (JSON, within the 64 KiB frame) is
    followed at once by exactly `bytes` raw bytes, written in one write by
    the link's out thread, the endpoint's only writer. The host's reader
    checks the header before it allocates or reads anything: both sides
    between 16 and 640 × 360, and `bytes == width × height × 3`; so at
    most 691 200 bytes are ever read for a picture, and anything else ends
    the reading of that viewer (as any message that is not one does). Why
    this and not the alternatives: **(a)** no codec: JPEG or WebP would fit
    most pictures in 64 KiB but not all (an adaptive quality loop), cost
    more processor time than a 0.7 MB local copy, and put an image decoder
    in front of bytes from another process; **(b)** no chunks: one length,
    one read, nothing to reassemble or to bound twice; **(c)** no shared
    memory: no new system object and no Windows-only unsafe code to
    secure and test; the picture uses the endpoint that already exists,
    with its security as it is (this logon only, first instance, remote
    clients refused, the launch's process id and one-time token, protocol
    and build). The plan's section 8 said "binary, 2 MiB at most, latest
    only": it is binary, 691 200 bytes at most, latest only. At the
    budget's most, the pipes carry about 3.6 MB a second in all.
  - **Latest only, three times** (`monitor::Latest`, a place for one):
    the viewer's main thread leaves the pixels for the out thread and
    never waits (one not written yet is replaced); the host's reader
    leaves the image for the window (one not looked at yet is replaced);
    the window replaces its texture whole. Nothing is queued anywhere, so
    a slow host or a slow pipe costs pictures, never memory. A viewer
    never has two pictures on its card: the schedule asks one at a time.
  - **Never a long wait on the card.** The frame that carries a picture
    waits 60 ms for it (`snapshot_wait`) instead of 10 s; a picture not
    back by then comes through the renderer's existing late answer
    (`snapshot_sent`) with a later frame, and ordinary paused frames go on
    meanwhile; one not back after 1.5 s is given up
    (`Renderer::drop_snapshot`: its answer is not kept) and nothing more
    is asked until the card has answered it.
  - **A busy card lowers the rate** (`monitor::Schedule`), and the
    monitor says so. A picture that took 150 ms or more (the busy
    pause's own measure) doubles the time to the next one; one given up
    quadruples it; up to 32 times the order's interval, 60 s at most; four
    quick pictures in a row bring it back one step ("fewer pictures: the
    graphics card is busy"). While Aurora's busy pause is on: one picture
    every 10 s at most. While the card holds a frame (stall), is lost,
    out of video memory or past the driver's budget behind: none
    ("paused: the graphics card does not answer"). Checkpoint 1's rules
    are untouched: a loss behind still waits for the window to come
    forward, a stall behind shows no box, and a monitor asks nothing of a
    card in either state. Not on a timed run, with a headset, or while a
    photo of the user's is drawn (the photo comes first).
  - **RLVa.** A picture is drawn by the same `FrameParams` as the frame
    on screen, so what the renderer does for RLVa applies as on screen:
    vision spheres (`rlva_spheres`), overlays (`rlva_overlays`), the
    resolution divisor, restricted textures, hidden avatars. Checked in
    code: `App::scene_paused` is false for the picture's frame, so the
    divisor is RLVa's and not the pause's, and the snapshot target is the
    frame's own surface view, where spheres and overlays are drawn. The
    picture carries **no interface**, so nothing the interface shows can
    leak past a restriction (name tags, hover text, the map, the
    location): stricter than the screen. **Not** covered by a pixel test
    here (no graphics device in the sandbox): to check on Windows with
    `@setsphere` and `@setoverlay`.
  - **Minimised windows.** A minimised viewer gave its textures back
    (Economy) and gets few frames: it takes no picture and its monitor
    says so ("Switch to it once…"). A character whose monitor is open is
    therefore **not minimised behind** even with "Minimise in the
    background" (`Slot::waits`: Warm while its monitor is open); the
    choice itself is kept. Restoring a minimised window without giving it
    the keyboard is not done (winit's restore activates the window).
  - **In the host:** in a character's card, a **Monitor** checkbox; while
    it is open, the size (320 × 180, 480 × 270, 640 × 360) and the rate
    (0.5, 1, 2, 5 a second), the picture in its 16:9 box (1:1 when the
    card is wide enough, scaled down otherwise), and under it "name · n s
    ago · how its viewer does". The picture senses no click and no key;
    the card's **Switch** is the way to act. The host keeps the latest
    picture of each monitor in memory only, and drops it when the monitor
    closes, the character comes to the front or leaves the world, or its
    viewer ends; a hidden monitor keeps its last picture, with its age.
    No file, no log line holds a picture. (`AURORA_SESSION_HOST_CAPTURE`,
    demo only, writes the whole host window, monitors included: demo
    viewers have no grid.) The window is still drawn on the processor;
    the texture is replaced whole and the window is painted off screen
    and presented in one piece: no flicker.
  - **Cost (estimated, not measured: no graphics card here).** In a
    watched viewer, a picture is one offscreen frame of the scene at
    640 × 360 at most (a ninth of a 1080p frame's pixels; the shadow and
    reflection passes do not shrink with it), 230 400 pixels read back
    (0.9 MB) and 0.7 MB written to the pipe: at 1 picture a second about
    one small frame a second per monitor, against the active character's
    60 or more full frames. Its main thread waits 60 ms at most per
    picture. In the host: 0.7 MB read and one image made per picture, off
    the window's thread, and the pictures' pixels painted by the
    processor at the window's 5 drawings a second. To measure on Windows:
    the watched viewer's log says `session monitor: off … (n pictures
    sent, x ms a picture)` when a monitor closes; compare the active
    viewer's frame rate with 0 and 4 monitors open.
  - **Fixed on the way (checkpoint 2's code):** `App::frame` takes the
    window out of the app while it draws, so `folderstorm_focused`, asked
    during the frame, saw no window and answered "no keyboard" every
    time: a focused viewer was capped as a background one, could be
    paused by the busy pause, and its GPU loss in front was left to wait.
    The window's answer is now remembered from just before the frame
    (`folderstorm_characters::window_focus`).
  - **Demo:** `AURORA_DEMO_SESSION_HOST` opens every demo character's
    monitor; the demo viewers draw their demo scene (`docs/TESTING.md`).
  - Not done: the world viewport of section 12 (a "Limited" window
    drawing only a small world rectangle, a large active window with the
    host's panels beside it): not small, it needs the host to place
    panels over another process's window; HUD attachments in the picture
    (off, as in a photo by default); a picture for a minimised window.
    (Monitors in a window of their own: checkpoint 6.)

### Checkpoint 6: pop-out panels and fitted layout (optional)

- Chat, controls and monitors as separate host windows; an opt-in fitted
  borderless stage (section 7), only if checkpoint 2's arrangement is not
  enough.
- **Acceptance:** dialogs, IME, Alt-Tab, minimise and restore, and DPI
  changes leave no vanished world and no stolen focus.

- **As built (branch `claude/session-checkpoint-6`, on top of checkpoint
  5):** the rules are pure and tested in `crates/folderstorm-session`
  (`panels`: which panels are out and where, the file's checks, a saved
  place brought back onto a connected screen, "closing a panel docks
  it", the monitors seen window by window; `host`: the desk keeps the
  panels with the rest of the settings, in `host/panel_tests.rs`). In the
  viewer: `host/ui/panels.rs` (a `View` is one window with its own
  interface, painter and pixels; the panels' windows are opened, placed
  and closed there), `host/ui.rs` (which window draws what),
  `host/ui/monitors.rs` (the monitors' own window), `host/monitors.rs`
  (the latest picture stays with the host, for the window that draws it).
  - **Upstream hooks (new): none.** winit's several windows in one event
    loop and the crash window's processor painter were enough as they
    are: no upstream file changes in this checkpoint (locales and
    `docs/TESTING.md` apart, append-only).
  - **Decision: no fitted borderless stage.** The human validated
    checkpoint 2's arrangement (separate top-level windows, the one
    brought forward taking the stage's place) as working well. The fitted
    stage was only ever "if that is not enough", it is the form that
    failed in Folderstorm (C++) (the world vanishing when the host's
    controls or another program were used, focus and IME trouble), and it
    cannot exist on Wayland. It is **not built** and no code prepares it.
    Should it be wanted later, it is a checkpoint of its own.
  - **Three panels:** the chat, the characters (their cards, the choices,
    the hotkeys' and the new profile's pages, "Quit all"), the monitors
    (every open monitor's picture, side by side while there is room, each
    with its label and "Switch"). At the top of the host's window, "In a
    window of its own:" and a box per panel; in a panel's window, "Dock
    back". With the monitors out, a character's card keeps its monitor's
    box, size and rate and says where the picture is. With the chat or
    the characters out, the host's window gives its whole width to the
    other; with both out it lists what is out, each with "Dock back".
  - **Closing a panel's window docks the panel** (`panels::Config::closed`,
    `Desk::panel_closed`): no question, no character closed, never the
    host's end. Only the host's own window closes the host, and its
    question (Quit all, Close only this window, Cancel) is always asked
    in that window, also when the characters' panel is out. A panel's
    window that cannot be made or drawn is docked back, with a line in
    the log: the host does not close for it.
  - **Every window is drawn on the processor.** A panel's window has its
    own egui context (theme, fonts, icons), its own `egui_winit::State`
    (keys, IME, DPI, cursor: per window, as winit gives them) and its own
    softbuffer surface and painter. The host still opens no graphics
    device. (One context per window, and not egui's viewports: a viewport
    of the host's context would need an integration that hands the frames
    of several windows to one context, which the crash window's loop is
    not; separate contexts need nothing new. The cost is one font atlas
    and one set of icons per open panel, made when it opens.)
  - **Remembered** in `session-host.json`, `panels`: for each of `chat`,
    `controls` and `monitors`, `out` and `place` (`position`, the outer
    corner, absent where the system does not tell, as on Wayland; `size`,
    the inside; physical pixels). A place is noted when its window is
    moved or resized, not while it is minimised or maximised, and kept
    while the panel is docked. **Checked when read:** a size or a
    position no window can have is dropped (the panel is still out, at
    its first size); an older file has no `panels` and reads as all
    docked. **Checked when opened** (`panels::on_screen`): a window keeps
    its place when a connected screen shows enough of its top edge to
    catch it (120 × 24 pixels, the title bar not above the screen);
    otherwise it opens whole on the nearest connected screen, no larger
    than it. A minimised window's place as Windows tells it (−32000) is
    never noted and would be corrected anyway.
  - **No stolen focus, by construction.** A panel the user pops out opens
    in front, as a window they asked for. The panels found out when the
    host starts open after the host's own window and without taking the
    keyboard (`with_active(false)`). After that nothing in the host asks
    for the foreground: no `focus_window`, no raise; a message for a
    background character flashes the chat's window in the taskbar
    (`request_user_attention`), as the host's window did. Nothing here
    sends anything to a viewer: popping out, docking, moving or closing a
    panel changes no assignment, no stage and no window of a character
    (the desk's `panel_*` calls only ask for a save), so no world can
    vanish for it. The global hotkeys belong to the thread, not to a
    window, and work whichever window has the keyboard.
  - **Monitors pause per window** (`panels::seen`): each window notes
    which monitors its last drawing showed; a window that is minimised or
    covered whole sees none; a monitor is asked for pictures while any
    window that draws it is in view. So minimising the monitors' window
    returns the watched viewers to 3D Off although the host's window is
    in view, and minimising the host's window does not pause the
    monitors' window. One window draws the pictures at a time (the
    monitors' own, or the one with the cards); the host keeps the latest
    picture (one per monitor, in memory only, as before) so that it moves
    with its monitor at once when a panel is popped out or docked.
  - **Chat read state per window:** what the chat shows is read only
    while the window that draws the chat has the keyboard, and is told
    at once when that window loses it (a minimised window is not drawn
    again).
  - **Demo:** `AURORA_DEMO_SESSION_HOST_PANELS=chat,controls,monitors`
    (or `all`) starts the demo's host with those panels out;
    `AURORA_SESSION_HOST_CAPTURE` also writes each panel's window
    (`docs/TESTING.md`).
  - **Checked here:** the rules' tests; each panel's window drawn by the
    processor's painter without a window (`host/ui/panels.rs`); the three
    windows opened, drawn and captured under X11 (Xvfb).
  - **To verify on Windows** (nothing of Win32 runs here): a panel popped
    out comes in front and takes the keyboard, and no viewer moves, is
    raised or changes role; the panels restored at start do not take the
    keyboard from the host's window; Alt-Tab lists each panel's window
    and switching among the host's windows and the viewers leaves the
    active character's world drawn; a native dialog of a viewer (file
    picker) and the host's windows do not fight over the foreground;
    IME (Japanese) composes in the chat's own window, the candidate box
    beside its text field; minimise and restore each window, the
    monitors pausing with the window that draws them (`session monitor:
    off` / `on` in the viewers' logs); a panel dragged between two
    screens of different scale is redrawn sharp at the new scale, and
    its place is restored on the right screen; a place on an unplugged
    screen comes back onto a connected one; the taskbar flash for a
    background message is on the chat's window and never brings it
    forward; global hotkeys work with a panel's window focused.
  - Not done: the fitted borderless stage (above); the host's own
    window's place is not remembered (it never was; only the panels'
    are); a panel's window has no "always on top" choice; one monitors'
    window for all monitors, not one window per monitor.

### Checkpoint 7: polish picked by the human

- **The human's picks (2026-11-11)** from section 13 and from what
  checkpoint 6 left: an attention inbox, character colours and aliases, a
  condensed host mode, and the bird's-eye switch effect *if it can be done
  without risk for the viewers' graphics*. MCP session routing (FEATURES
  4.6) is not part of it: it waits for the MCP receipts plan's login
  binding.
- **Acceptance:** nothing is answered from the host; an alias and a colour
  never leave the host (not to a viewer, not to a grid); a colour is
  readable on any skin; the condensed layout works with panels popped out;
  no viewer draws more than its policy allows, and checkpoint 1's rules
  (R4, R5) are untouched.

- **As built (branch `claude/session-checkpoint-7`, on top of checkpoint
  6):** the rules are pure and tested in `crates/folderstorm-session`
  (`inbox`: one list across characters, its order, its bounds, one entry
  per question, what leaves it; `identity`: the colours, how one is fitted
  to a theme's contrast, what an alias may be; `host`: colour, alias and
  the condensed choice among the settings, checked when the file is read,
  in `host/polish_tests.rs`; `panels`: the inbox is a fourth panel). In
  the viewer: `host/faces.rs` (how a character is shown: alias, real
  name, colour as this theme draws it), `host/ui/inbox.rs`,
  `host/ui/identity.rs`, `host/ui/condensed.rs`.
  - **Upstream hooks (new): none.** Everything is in the fork's own
    files; the locales and `docs/TESTING.md` get lines, as before. The
    viewer's side of the link is unchanged: no new message, the protocol
    number stays.
  - **Attention inbox.** One list of what waits for an answer in the
    characters' own windows: whatever a viewer lists as attention items
    since checkpoint 3: the notifications of its list that ask for an
    answer (teleport, friendship, inventory, group, scripts' dialogs and
    permission questions, links, calls, and the rest as "Other"). What
    only informs, such as a payment's notice, asks nothing and is not
    listed, there or here. Each row: the character (colour,
    alias or name), the question in a line, its age, **Switch to
    answer**.
    - **Nothing is answered from the host**, as in checkpoint 3: the
      inbox has no accept, decline or dismiss, and its only act is the
      desk's switch (the follow-ups below added a dismiss that is the
      host's own and answers nothing). Nothing about a question is ever written to a
      viewer (tested: drawing and switching leave the chat's effects
      empty).
    - **The viewers' lists are the truth** (`inbox::Inbox::set`, fed
      from what the chat keeps of each viewer's list, which already
      drops another launch's or login's): a question is listed while its
      viewer lists it; it leaves when the viewer's next list does not
      have it (answered or expired there), when the character logs out
      or in again, when its viewer ends or is let go, when the character
      is removed. The host expires nothing by itself: what is shown
      still waits there. A viewer that stops answering keeps its last
      list, with ages that grow, and its card says it does not answer.
    - **Age** counts from when the host first saw the question listed
      (the link carries no time): "just now", minutes, hours; the hover
      says so. **Order:** oldest first (the nearest to expiring in its
      viewer), then by character, then by the viewer's number.
      **One entry per question:** a question is one login's number in
      one viewer; listed twice it is one row, and the same number after
      another login is another question with a new age.
      **Bounded:** 16 per character (the link's own bound), 80 in all;
      what a viewer has beyond that is counted ("3 more wait in …'s own
      window"), not listed. Counts saturate.
    - **A panel of its own, the fourth** (decision). Docked, it is a
      band at the top of the host's window, drawn only while something
      waits, above whichever page the window shows (characters, chat, or
      "these panels are elsewhere"), so a question is in sight whatever
      else is popped out; it scrolls within about four rows. "In a
      window of its own: Inbox" gives it a window (remembered and
      brought back on screen like the others; closing it docks it),
      whose title says how many wait. Why not a section of the chat or
      of the characters: either of those can be out, on another page of
      a narrow window, or (the characters) condensed; a list whose point
      is to be seen must not depend on them. Why not only a window: with
      nothing popped out the host is one window, and stays so.
    - **Count badge:** beside "Inbox" in the host's window's top strip
      ("Inbox (3)", in the warning colour), wherever the inbox is; in the
      inbox's heading; in its own window's title; and per character in
      the condensed lines (a bell and a count).
    - **The chat keeps its own lists** (checkpoint 3): a character's tab
      still shows what waits for it. "All" leaves it to the inbox when
      the inbox is drawn in the same window, and lists it itself, as
      before, when the inbox is in its own window or the chat is.
  - **Character colours and aliases.**
    - **Colour:** none, or one of eight (red, orange, yellow, green,
      teal, blue, violet, pink). What is drawn is the colour **fitted to
      the theme** (`identity::Colour::fitted`): the palette's own when it
      has a contrast of 4.5 : 1 (WCAG AA for text) with the panel, the
      field and the raised backgrounds of the skin in use; otherwise
      moved towards white (dark skins) or black (light ones) just as far
      as needed. Tested on the default skin, a light one, the high
      contrast palette and the worst background there is; the eight stay
      apart from one another after the move. Colour is never the only
      cue: the alias or name is always beside it.
    - **Alias:** at most 24 characters, on one line, nothing invisible
      (control characters, direction overrides, zero-width characters
      are refused, so an alias cannot pass for another name); the spaces
      around it go; empty takes it away. The editor refuses a text that
      is no alias and says why; a file's alias that is none is dropped,
      never cut into another.
    - **Where:** the cards (dot, alias, then the real name and the
      profile in grey), the quick-action buttons (alias, a line in the
      colour, the real name in the hover), the chat's tabs (alias, a
      line in the colour, the real name on hover) and its "All" lines
      (`[alias › conversation]` in the colour), the monitors' labels
      (dot, alias), the inbox's rows, the condensed lines.
      **"Send as" always says the real name too**: `Send as Main (Jane
      Doe) — Nearby chat`. Not on the title of a panel's window: those
      are named after the panel, not after a character (the inbox's
      title has its count). Not on the viewers' own windows or taskbar
      buttons: they are the viewers', and nothing of this is told to
      them.
    - **Kept** in `session-host.json`, per slot: `colour` (a name; one
      this build does not know reads as none, so a later build's file
      still reads) and `alias` (absent when there is none). An older file
      has neither. Removing a character from the desk takes both with
      it.
    - **Never sent anywhere:** `Desk::set_colour` and `set_alias` ask
      for a save and nothing else (tested: no message for a viewer); no
      message of the link carries either; the host has no connection to
      a grid at all.
    - Chosen in a card: the palette button beside the name ("Alias and
      colour") opens a field and the swatches (each drawn as this theme
      draws it, named on hover).
  - **Condensed host mode.** "Condensed" at the top of the characters'
    panel turns the cards into one line per character; "Cards" turns
    back. Remembered (`condensed` in `session-host.json`).
    - **A line:** the colour (a ring without one), the slot's number and
      the alias or name (the active character's in the accent; hover:
      real name, profile, where it is), then on the right its **quick
      action** (checkpoint 4's `Desk::quick`, worded: Switch, Show, Log
      in, Starting, Start, Restart), what waits for an answer (bell and
      count), its unread lines in the shared chat (count), its monitor
      (open), the voice (held, or being given up), what is wrong (one
      warning sign, everything said on hover) and where its viewer is
      (in the world, at its login screen, starting, stopped, crashed,
      not managed).
    - **What the cards alone have** (standby, sound, monitor, alias and
      colour, Close, End process, Remove, adding a character, the
      hotkeys' and the new profile's pages, the two host choices) is one
      click away; the lines only switch. The host's own questions
      (closing it while characters run, "Quit all" under way, the voice
      waiting) are asked in this layout as in the other.
    - **With panels popped out:** it is the characters' panel drawn
      another way, so it is condensed wherever that panel is (the host's
      window, or its own). With the chat out, the host's window is the
      lines alone and can be made small: its least size goes from
      440 × 360 to 340 × 200 points while condensed. Beside the chat,
      the characters' column is 380 points wide instead of 500, and
      the window that shows the lines keeps 8 points around them instead
      of 18.
    - **Monitors:** the pictures are in the cards or in the monitors'
      own window. Condensed with the monitors docked, no window draws
      them, so by checkpoint 5's rule ("asked only while on screen") the
      watched viewers go back to 3D Off; the line's monitor sign says
      so. With the monitors in their own window nothing changes for
      them. (Tested.)
  - **Demo:** `AURORA_DEMO_SESSION_HOST_POLISH=colours,condensed,inbox`
    (or `all`), with `AURORA_DEMO_SESSION_HOST`: the demo's characters
    get colours and the first two an alias; the host starts condensed;
    the inbox lists sample questions for the characters in the world
    (the host's own samples, demo only: no viewer has them).
    `AURORA_DEMO_SESSION_HOST_PANELS` knows `inbox`
    (`docs/TESTING.md`).
  - **Checked here:** the rules' tests; each new view drawn by the
    processor's painter without a window (the inbox docked and in its
    own window, the editor and its swatches, the condensed lines in the
    host's window at its least size and in the characters' own window),
    with the colours looked for in the pixels; the host's window
    (condensed, with colours and aliases) and the inbox's own window
    opened, drawn and captured under X11 (Xvfb). The demo's viewers do
    not start here (no graphics card), so the inbox with questions in it
    was only seen in the tests' drawings, not in a real window.
  - **To verify on Windows** (nothing of Win32 runs here): the inbox
    with real offers (a teleport offer, a friendship request, an
    inventory offer and a script's dialog to two characters): each
    appears within a second, oldest first, with its character; "Switch
    to answer" brings that character's window forward and nothing is
    accepted; answering or letting it expire there takes the row away;
    the inbox's own window's title follows the count in the taskbar.
    Colours on the skin in use and with Preferences › Accessibility's
    high contrast; an alias typed with the IME (Japanese) in the card's
    field; `session-host.json` after a change, and that no viewer's log
    or window shows an alias. The condensed host's window made small
    beside the stage, with the chat in its own window; the global
    hotkeys with it.
  - Not done: no sound or taskbar flash for a new question (the chat's
    alert for a message is unchanged); no "seen" in the inbox (dismiss
    was added by the follow-ups below, at the human's request); a colour is one of
    the eight, not a free one; no automatic colour for a new character;
    the condensed lines have no End process (it is in the cards, where
    its confirmation is); the host's own window's place is still not
    remembered.

- **Follow-ups after the human's test on Windows (branch
  `claude/session-host-followups`, 2026-11-12).** No new upstream hook,
  no new message on the link (the protocol number stays).
  - **Hotkeys need no key released between two switches.** Cause:
    winit's `focus_window` on Windows sends an Alt press *and release*
    before `SetForegroundWindow` (to lift the foreground lock). With the
    user still holding Ctrl+Alt, that release left Windows believing Alt
    was up: the next digit came as Ctrl+digit, which is no hotkey and is
    the Build tools' key (Ctrl+1 to 5) in the viewer that had just come
    forward; only pressing Alt again brought it back. (`MOD_NOREPEAT` was
    not the cause: it only stops the auto-repeat of one hotkey, and is
    kept.) Fix, `folderstorm_characters/foreground.rs`, used by the
    managed viewer's `Show` and a profile's second launch: the window is
    first asked to the front plainly, which the host's
    `AllowSetForegroundWindow` (given by a process that received the
    hotkey or the click) allows, so usually no key is sent at all; only
    if that is refused is the Alt press sent, ending as the user holds
    Alt (held: released then pressed again; not held: winit's press and
    release). No held key comes back: the viewer left drops its held keys
    on losing the focus (`app.rs`), the one in front ignores winit's
    made-up presses and gets the real releases. The pure part (the Alt
    events end as the user holds Alt) is tested; the Windows calls are
    checked against `windows-sys` for the Windows target. **(verify on
    Windows)**: Ctrl+Shift held, F2, F3, F1 switch three times; the same
    with a chord of the user's that has Alt; no flashing taskbar button
    instead of the window; no key stays held.
  - **New default hotkeys: Ctrl+Shift+F1 to F5, Ctrl+Shift+F6
    (previous), Ctrl+Shift+F7 (next).** The clash with Build that the
    human saw was the lost Alt above (Ctrl+Alt+digit itself is no default
    binding of the viewer: the Build tools take Ctrl+digit without Alt),
    but the old defaults had real faults: Ctrl+Alt is AltGr, so on many
    European layouts (AZERTY among them) Ctrl+Alt+2 to 5 type a character
    and were refused; Ctrl+Alt+PageUp and PageDown start the camera's
    Alt+PageUp / Alt+PageDown orbit in the viewer (a held binding accepts
    more modifiers); Ctrl+Shift+digit is taken too (Ctrl+Shift+1
    Performance, 3 and 4 the texture and debug consoles). Folderstorm
    (C++) suggested Alt+Shift+1 to 5 and Alt+Shift+Right / Left, off by
    default; Alt+Shift with an arrow is the camera's orbit here as well,
    and Alt+Shift+digit is one of Windows' own optional language-switch
    keys. Ctrl+Shift with a function key is no key of the viewer's
    default bindings (a test checks every default against the viewer's
    table, and that the old ones are caught), types nothing on any
    layout, and cannot be a gesture's key (a gesture takes Ctrl or
    Shift, not both). On laptops whose top row is media keys by default,
    Fn is needed with it. **Migration:** `hotkeys.defaults` in
    `session-host.json` says which defaults wrote the file (missing: the
    first); a file of the first defaults whose seven hotkeys are all the
    old defaults, unchanged, gets the new ones (on or off kept); a file
    with any hotkey of the user's own keeps every one of them, and the
    old defaults typed again later are kept as a choice (the file then
    says `defaults: 2`).
  - **Quit all is confirmed** under the rows ("Quit all 3 characters
    that are open?", Quit all / Cancel), in the cards and in the
    condensed lines alike. The close button's question (Quit all, Close
    only this window, Cancel) is a question already: its Quit all does
    not ask again. There is no hotkey for Quit all.
  - **Inbox: Dismiss**, per row, and **Dismiss all** / **Dismiss all
    for <character>** under "Dismiss…" beside the heading, in the band
    and in the inbox's own window. Host-only: the entry leaves the inbox,
    "Inbox (n)", the condensed line's count and the chat's "has something
    to answer" box; nothing is said to the viewer, where the question
    still waits unanswered (nothing is answered from the host stays
    true). A dismissed question stays out while its viewer keeps listing
    it in the same login (`Inbox::dismiss`, by the viewer's number, which
    a viewer never gives twice in one login); a new question, or the same
    number after another login, comes in as usual. Dismissing all of a
    character also leaves out how many more it has than are listed,
    until that count grows. Memory only, bounded (64 numbers per
    character; those the viewer's whole list no longer has are
    forgotten).
  - **Clearing in the viewer.** What a viewer clears does reach the
    host for the inbox: a question answered, declined or closed (its
    card's or toast's ×) leaves the viewer's list and the next list,
    within a quarter of a second, takes it out of the host. What does not
    leave is what still waits there: the bell's "Mark all read" marks the
    questions read but keeps them in its "To do", and a toast that fades
    leaves its question under the bell; the host lists them because they
    are unanswered (Dismiss now hides them in the host). What did not
    follow, and is now fixed, is the **chat's unread count**: a
    conversation read in the viewer stayed unread in the host (the host
    told the viewer when it was read in the host, `MarkRead`, but not the
    other way). The viewer already told its unread count with each
    conversation; the host now takes a fall of that count to zero as
    "read there" (`chat::Chat::batch`). Only the fall: a viewer that
    shows a conversation as its lines come (open behind the others) has
    nothing unread there, and the host still counts those lines for the
    user who has not seen them. A "seen" mark for questions (read in the
    viewer, dimmed in the host) would need a field on the attention items
    and a protocol change: not done.

- **Bird's-eye switch effect: not built (decision), design note.**
  - **What Folderstorm (C++) meant** (its `doc/plans/
    multi-character-sessions.md` section 7, `multi-character-
    usability.md` section 5, PR #17): off by default; on a switch the
    outgoing character's camera rises to a bounded bird's-eye view, the
    hand-over happens, and the incoming character's camera comes down
    from its own bird's-eye view to its saved camera, about a second per
    leg (later 250–2000 ms, 16–96 m, and an optional fade instead). A
    renderer-only camera override: the avatar never moves, nothing is
    sent to the simulator, the exact camera is restored on completion,
    Escape, restriction, disconnect or region change; mouselook, scripted
    and follow cameras, camera restrictions and reduced motion fall back
    to an instant switch.
  - **Why it is not built here.**
    1. *It lives in the viewer's renderer, in upstream's files.* The
       effect is a camera override inside the frame: the camera's
       controller, the frustum and what follows it (culling, level of
       detail, interest list, the minimap's and the sound's listener
       position, mouselook, the RLVa camera restrictions) are upstream
       code with no extension point for "draw from another eye for this
       frame only". That is a deep change to upstream code, which the
       fork's rules stop at: propose first.
    2. *It would hold the outgoing viewer in front past the user's
       click.* Today a switch is one assignment: the old character goes
       behind (3D Off, voice given up) at once. The rising leg needs it
       drawn at full rate for a second more, and it then draws *more*
       than usual: from above, the whole draw distance is in the frustum
       while the incoming viewer is being woken up at the same moment
       (textures back from Economy, a full first frame). Two viewers at
       full load on one card is the very moment the busy pause and the
       device-loss rules (section 6.2, R4, R5, R7) exist for; widening
       it for decoration is the wrong trade.
    3. *It would have to reorder the voice hand-over or run beside it*
       ("given up before it is given", section 10): either the effect
       delays the switch the user asked for, or the outgoing character
       is animated while no longer active, which is drawing beyond its
       policy.
    4. *A host-side effect cannot stand in for it.* The host draws its
       own windows on the processor and never draws over a viewer's (no
       overlay, no hidden or layered window: R2). A flourish inside the
       host's window would not be what was asked.
  - **Options for the human, if it is wanted later** (a checkpoint of
    its own, off by default, with "instant" always the fallback):
    - **A. Incoming leg only, viewer-side.** After the role change, the
      new active viewer comes down from above for under a second. No
      viewer draws beyond its role (it is Active by then), nothing
      delays the switch or the voice. Still needs the camera extension
      point in upstream (best sent upstream as a generic "presentation
      camera offset" that honours mouselook, scripted cameras and RLVa),
      and must be skipped when that viewer reports a busy, stalled or
      lost card, in its first seconds after Economy, and with reduced
      motion.
    - **B. A fade instead** (C++'s later alternative): the incoming
      viewer draws a full-window veil in its interface layer that clears
      over a few hundred milliseconds once it is Active. No camera, no
      extra 3D, one small hook where the managed role is applied. The
      cheapest and the safest; it is not a bird's-eye view.
    - **C. Both legs, as in C++.** Needs a hand-over in two steps in the
      desk (announce, then assign), a bound on the outgoing leg, and the
      measurements of section 4.3 on a real card first. Not recommended
      before A has been lived with.

### Checkpoint 8: docked bar and compact controls

- **The human's notes and decision (2026-11-12)**, from screenshots of
  Folderstorm (C++)'s host: the characters are driven from **one row at
  the top of the active viewer**, not from a second window. **Option (a)
  is decided:** each managed viewer draws the bar itself, at the top of
  its own window; the host feeds it over the link that exists and every
  click goes back to the host as a request. (Not chosen: a host window
  laid over or glued to the viewer's, which is rule R2's and section 7's
  "fitted" trouble again.) Popped out, the bar leaves every viewer and
  the host shows it as its own compact window, "Character controls".
- **The bar, one row, left to right:**
  - *profile dropdown*: "n: name" for the active character; its list has
    every character with "Active", "Inactive" or nothing (not logged
    in). Picking one is that character's quick action (section 13):
    switch when it runs, bring it forward at its login screen, start it
    when it has no viewer;
  - *open a login*: the first character that is not logged in (its
    viewer is started, or brought forward at its login screen); a right
    click lists the characters to choose from;
  - *shared chat*: shows or hides the host's chat in its own window;
  - *monitors*: a menu with each character (greyed when it is not in the
    world, or is the active one), "Open all inactive monitors", "Close
    all monitors", "Monitor settings…". (A place is kept for "Cycling
    monitor": checkpoint 9.);
  - *settings*: a menu to the host's pages (the bar's and quick actions'
    page, the hotkeys), "Show the host's window" and "Quit…";
  - *middle, the quick actions*: up to 12 positions the user fills in
    the host's settings, each Unused, Character 1 to 5, Attention (with
    the count; opens the inbox; shown even at zero), Mute background
    sound (a toggle, new) or Voice follows active (a toggle, new). A
    character's button carries its alias and its colour (checkpoint 7);
  - *right*: pop out, collapse (the bar becomes a thin handle that
    opens it again).
  - Label modes, two settings: the quick actions' (icon and text by
    default) and the toolbar buttons' (icons only by default), each
    icons only / text only / icon and text.
  - No voice status line.
- **New rules of the desk** (both saved, both also quick actions):
  - **Mute background sound**: every character that is not the active
    one is cut, whatever its own "heard in the background"; each
    character's own choice is kept and applies again when the toggle is
    off.
  - **Voice follows active** (on by default: today's behaviour). Off:
    the voice stays with the character that holds it when another one
    becomes active, until that character's viewer ends (the active one
    then takes it and keeps it). "Given up before it is given" is
    unchanged: never two voices. The character that holds the voice in
    the background is heard there (its voice is one of its viewer's
    sounds, and a viewer that is cut cannot transmit), unless "Mute
    background sound" is on, which cuts it too.
- **The host's windows.**
  - *Docked* (the default): while a viewer shows the bar, **the host's
    own window is hidden** (setting "Hide the host's window while the
    bar is shown", on by default). It comes back from the bar ("Show
    the host's window", or any entry that opens one of its pages), and
    by itself as soon as no viewer shows a bar (none runs, or all were
    let go): the host is never out of reach. Shown that way, its close
    button hides it again; "Quit…" in the bar's menu asks the usual
    question (Quit all / Close only the host / Cancel) in it. Nothing
    about a hidden host changes for the viewers: the host's loop does
    not depend on its window being drawn.
  - *Popped out*: the compact "Character controls" window, the same row
    in one line, with a **pin** (always on top: a filled pin on the
    accent when pinned, a plain pin when not, said in its tooltip) and
    "Dock back". Its close button docks the bar back: closing it never
    quits anything. Docked or popped, the pin and the window's place
    are remembered (`session-host.json`), and a place is brought back
    onto a connected screen as the panels' are.
  - The chat, the monitors and the inbox are the panels of checkpoint 6:
    the bar opens them in their own windows (the host's window may be
    hidden).
- **One taskbar button (Windows only, setting, on by default).** While
  the bar is docked: the host's window is hidden (or, shown from the
  bar, off the taskbar); a background character's window is taken off
  the taskbar and out of Alt-Tab; the active character's is on both.
  Switching swaps them.
  - Each viewer does it to **its own window**, in its own process, on
    the host's word (`Assignment::off_taskbar`): the taskbar's tab
    through winit's `set_skip_taskbar` (`ITaskbarList::DeleteTab` /
    `AddTab`, which winit also puts back when Explorer restarts), the
    Alt-Tab list through the `WS_EX_TOOLWINDOW` bit of the window's
    extended style. **No window is hidden, cloaked or re-parented**
    (rule R2), and no window is owned across processes (section 7).
  - **Minimised windows (Economy).** Windows parks a minimised window
    off screen only when it is a taskbar window by its style; a
    minimised tool window is left as a small title bar above the
    taskbar. So the tool-window bit is only ever set on a window that
    is not minimised and that the host does not minimise (Warm standby
    or an open monitor), and it is taken away before the window is
    minimised or shown. An Economy window loses its taskbar tab only:
    it may stay in Alt-Tab. **(verify on Windows)**
  - **Always put back:** when the character becomes the active one, when
    its window gets the keyboard, when the host goes away (the link
    ends), when the viewer is let go, when the setting is turned off or
    the bar is popped out. The decision is a pure function of what the
    viewer holds, applied whenever it changes and again on a new window
    (the one a GPU recovery makes).
  - Elsewhere than Windows nothing changes.
- **Link (protocol 4).** `Assignment` gains `off_taskbar`.
  `ToViewer::Bar(State)`: the bar as the host wants it drawn, whole, the
  latest replacing the others: docked or not, collapsed, the characters
  (slot, shown name, real name behind an alias, colour, presence, what
  picking it does, its monitor), the inbox's count, the chat's window
  and unread count, the two toggles, the quick actions and the two
  label modes. `ToHost::Bar(Request)`: one click (pick a character,
  open a login, the chat, a monitor, all or no monitors, a page of the
  host, show the host, quit, the inbox, a toggle, pop out, collapse).
  A request only ever does what the same click does in the host's own
  window: nothing is answered, no line is said, no process is ended
  from the bar. Every text is bounded and cleaned on arrival, both
  ways.
  - **Checkpoint 7's rule, restated:** an alias and a colour now reach
    the viewers the host manages, on their own private pipe, **for the
    bar and nothing else**: the viewer keeps them in the bar's state,
    draws them and forgets them with the link. They are never written
    to a viewer's settings, logs, chat or window title, and nothing of
    them goes to a grid (tested: the bar's state is used by the bar's
    drawing only).
  - A viewer that loses the host drops the bar's state: the strip goes
    and its interface takes the whole window again.
- **Upstream hook (one planned, in `app.rs`; two as built, see below):**
  before the interface's pass,
  `raw.safe_area_insets = self.folderstorm_bar_inset(); // Folderstorm:
  …`. egui's own "safe area" keeps the top strip out of
  `content_rect()`, which every screen of the viewer lays itself out in
  (login, loading, world, both styles of bars, the windows' work area),
  so nothing of upstream's layout is edited. The bar is drawn in that
  strip by the hook that draws the "3D paused" notice already
  (`folderstorm_characters_notice`, ours). None in a viewer that no
  host manages. The bar is hidden with the interface and in mouselook.
- **Settings** (`session-host.json`, absent fields read as the
  defaults, checked when the file is read): `bar` (docked or popped,
  collapsed, pinned, the popped window's place, the 12 quick actions,
  the two label modes, `one_taskbar_button`, `hide_host`),
  `mute_background`, `voice_follows`. A file of before has none of them
  and reads as: docked, the five characters and Attention as quick
  actions, nothing muted more than before, voice follows.
- **Acceptance:**
  - the bar shows in every managed viewer while docked, in none while
    popped out, and in none that lost its host; an unmanaged viewer is
    as upstream's, to the pixel;
  - every click of the bar does what the host's own window does for it,
    and nothing else; a slot never gets a second process;
  - never two voices, with "Voice follows active" on or off;
  - the host is always reachable: its window is hidden only while a bar
    is on screen;
  - closing the popped-out window docks the bar; closing a viewer or
    the host restores every window's taskbar tab and style;
  - no window is hidden, cloaked, re-parented or minimised by this
    checkpoint (R2), and checkpoint 1's rules (R4, R5) are untouched;
  - long names are cut with the full name in the hover; the row fits a
    narrow window by dropping the labels, then scrolling the quick
    actions.
- **Demo:** `AURORA_DEMO_SESSION_HOST_BAR=docked|popped` (with
  `AURORA_DEMO_SESSION_HOST`), listed in `docs/TESTING.md`.

- **As built (branch `claude/session-checkpoint-8`, on top of the
  follow-ups to checkpoint 7):** the rules are pure and tested in
  `crates/folderstorm-session` (`bar`: the settings and their reading,
  the state and the requests, which quick actions show, the label
  modes, when the host's window hides, what a window is on the taskbar;
  `host`: the two desk rules, `bar_state`, `bar_request`,
  `Assignment::off_taskbar`, in `host/bar_tests.rs`; `ipc`: protocol 4).
  In the viewer: `folderstorm_characters/bar.rs` (the row's drawing,
  one code for both homes), `folderstorm_characters/taskbar.rs` (the
  window's own taskbar tab and tool-window bit),
  `app/folderstorm_characters/bar.rs` (a managed viewer's side),
  `host/ui/bar.rs` (the compact window, the host's window hiding, the
  page).
  - **Upstream hooks (new): two lines, both in `app.rs`.** (The plan
    above counted one; the second was found while building.)
    1. After the frame's `take_egui_input`:
       `self.folderstorm_bar_inset(&mut raw); // Folderstorm: the session
       host's bar keeps a strip at the top`. It sets egui's safe-area
       inset for the pass, in a viewer a host started only (never
       otherwise: the input is not touched). `content_rect()` then
       starts under the strip, and what lays itself out in it follows
       without a change: the login and loading screens, the windows and
       their room, the notices.
    2. Where the bars are given their room:
       `let viewport_bars = crate::folderstorm_characters::bar::
       bars_room(&ctx, viewport_bars); // Folderstorm: under the session
       host's bar`, between the two lines the custom world viewport
       (FEATURES 1.1) already has there. Needed because egui's root `Ui`
       spans the whole window whatever the inset, and upstream's bars of
       both styles are laid out in it (`ui.max_rect()`, root panels):
       they are handed the room under the strip through the child `Ui`
       that place already makes for a custom viewport. Without a strip
       the function returns what it was given.
    The bar itself is drawn from `folderstorm_characters_notice`, the
    hook checkpoint 1 put before the screens are drawn: no third line.
    The viewer's world ↔ screen mapping goes by the window's pixels, not
    by `content_rect()` (checked by reading), so picking is unchanged;
    the top 30 points of the 3D view are under the bar, as they are
    under Firestorm's bars. The bar is an area of its own in the
    windows' order, as upstream's islands are, so that the pointer over
    it is over the interface and a click on it is never also a click in
    the world (tested).
  - **Decisions taken while building.**
    - *The characters' list always names who is in front*, whatever the
      toolbar's label mode: it is the one place that says it. A
      character's quick action keeps its number with icons only (five
      identical icons would say nothing). With too little room the
      toolbar's labels go first, then the quick actions', then the
      active character's name (its number stays); what is left of the
      quick actions scrolls with the wheel. A name longer than 16
      characters is cut with an ellipsis, whole in the hover (with the
      real name behind an alias).
    - *The monitors' menu ticks the open ones* and greys what cannot be
      watched (not in the world, or in front). Opening one from the bar
      also gives the monitors their own window (checkpoint 6's), since
      the host's window may be hidden; "Monitor settings…" shows the
      host's window on the characters' cards, where each monitor's size
      and rate are (the condensed layout is left for it). "Cycling
      monitor" is a greyed entry: its place.
    - *"Open a login" never switches*: it only acts on a character that
      is not logged in.
    - *The settings' menu also has the two toggles*, so they are at hand
      without being quick actions.
    - *The host's window hides once the user leaves it*, not under
      their hands: with a bar on screen it stays while it has the
      keyboard, hides when a character's window takes it, and stays
      when it was asked for from the bar, until its close button. The
      choice and "One taskbar button" are on the bar's page. Hidden, its
      monitors ask nothing (as minimised). Where the system cannot hide
      a window (Wayland), it simply stays.
    - *The compact window is one row high* (its height is fixed, its
      width free, 320 points at least, 1100 the first time); its pin is
      winit's window level. A row with no room even for its two ends
      (a very narrow viewer) scrolls as a whole instead of laying one
      end over the other.
    - **Fixed after review: its menus and hovers had no room.** egui
      draws a menu inside the window that opens it and keeps it in that
      window: one row high (38 points), the characters' list, the
      monitors' and the settings' menus and the hovers were cut to a
      strip (confirmed by a drawing: the list's area was taller than the
      window). Now, while a menu or a hover is open, the window grows
      under its row to hold it, and goes back to one row once it
      closes: the first frame grows it to the most (420 points under
      the row, never past the bottom of its screen), the next measures
      what is open and fits it with 6 points to spare. The row stays at
      the top, the width, the place and the pin are not touched, and the
      place it remembers is the row's, never the grown window's. A menu
      closes when the window loses the keyboard, so a window the user
      left is never left grown. Tested: the list opened by a click is
      painted whole under the row in a grown window, and the window is
      one row again once it is closed (Escape). A window at the very
      bottom of a screen has no room under it: its menus are cut there
      (to move it up while a menu is open is a follow-up if asked).
    - *A click is heard at once*: the reader thread of the viewer that
      was clicked wakes the host's loop (an event-loop proxy), instead
      of waiting for the window's next look (200 ms). The viewer that
      was clicked lets the answering window take the foreground
      (`AllowSetForegroundWindow(ASFW_ANY)`): the host is not the
      foreground process when a bar is clicked. **(verify on Windows)**
    - *The page is the characters' panel's third page* ("Bar and quick
      actions…", beside "Hotkeys…"), until checkpoint 10 gives the
      settings their own place.
  - **Not done, by decision:** the panels' windows (chat, monitors,
    inbox) keep their taskbar buttons: "one taskbar button" is the
    characters' and the host's own window. The shared chat's button
    shows or hides its window; it does not raise a window that is
    behind (checkpoint 9's pin is the answer to that). No voice status
    line (decided). No free order of the toolbar's buttons.
  - **Demo:** `AURORA_DEMO_SESSION_HOST_BAR=docked|popped`
    (`docs/TESTING.md`).
  - **Checked here:** the rules' tests; the row drawn by the
    processor's painter without a window, in both homes: the compact
    window (wide, narrow, narrower than its two ends, pinned) and a
    viewer's pass with the safe-area inset and the bars' room (the
    strip, the viewer's own bar starting under it, collapsed, popped
    out), with the texts, the colours and the backgrounds looked for in
    the pixels, and the pictures looked at; the page; the host's window
    hiding and coming back; the requests from a queue of clicks to the
    desk; the pointer over the strip and under it; a guard on the
    sources that the bar's state is read by the bar's drawing only. The
    real host under X11 (Xvfb), popped out and docked: the compact
    window opened, drawn and captured, the host's own window hidden
    while it is there, its page reached; the demo's viewers linked
    (protocol 4) and then ended, as before, for want of a graphics card.
    The Windows-only code (`taskbar.rs`, `foreground.rs`, the host's
    `set_skip_taskbar`) is type-checked and linted for
    `x86_64-pc-windows-gnu` against `windows-sys` and winit, in a
    scratch crate that includes the real files. **Not run here:** a
    viewer drawing the bar in its own window (no graphics card), and
    nothing of Win32.
  - **To verify on Windows:**
    1. the bar at the top of each character's window on the login
       screen, while loading and in the world, with the islands and with
       Firestorm's bars; nothing of the viewer's interface under it, the
       windows' room and the snapping start under it; mouselook and
       "hide interface" take it away and bring it back;
    2. every button of the row, from the active viewer and from a
       background one that has the keyboard: the list, "Open a login"
       and its right click, Chat, the monitors' menu, the settings'
       menu, each quick action, Pop out, Collapse and the handle; the
       window asked for comes to the front instead of flashing;
    3. the host's window hides when a character takes the keyboard and
       comes back from the bar; "Quit…" asks its question; with every
       viewer closed it is back by itself;
    4. popped out: the compact window, its pin above a full-screen-size
       viewer, its place remembered across a restart and brought back
       after a screen is unplugged; closing it docks the bar; each menu
       (the list, "Open a login"'s right click, the monitors, the
       settings) and the hovers open whole under the row, the window
       grows for them and is one row again when they close, also after
       a click in another window;
    5. **one taskbar button**: only the active character on the taskbar
       and in Alt-Tab; a switch (button, bar, hotkey) swaps them at
       once; a Warm background window keeps its usual title bar (no
       small tool-window caption) and comes back whole; an Economy
       (minimised) one leaves **no small title bar above the taskbar**
       and is restored by a switch; Win+D and Win+M and back; Explorer
       restarted (`taskkill /f /im explorer.exe`, then start it);
       the setting off, the bar popped out, the host closed or ended in
       the Task Manager, a viewer "let go": every window has its button
       again. If Alt-Tab still lists the background characters on this
       version of Windows (the list may only follow a style change when
       a window is shown), say so: the fallback is in "Follow-ups";
    6. "Mute background sound" with a character "Heard in the
       background"; "Voice follows active" off with real voice: the
       character left keeps talking and is heard, the new active one
       has no voice; on again, one voice only at every moment.
  - **Follow-up: the bar over the custom world viewport.** With a
    custom world viewport (FEATURES 1.1: the window over several
    monitors, the world in one rectangle, windows in the margins) the
    strip spanned the whole window and took 30 points from the margins'
    windows too. Now, by default, it stays over the world viewport: at
    the world's top, as wide as the world. The bar's settings gain
    "Span the whole window" (`session-host.json`, `bar.span_window`,
    off by default; a checkbox on the bar's page), sent in the bar's
    state (`State::span_window`, read as off when absent: host and
    viewers are one binary, so the protocol stays 4).
    - *Where the strip goes* is a pure rule
      (`folderstorm_characters::bar::strip_place`): over the world when
      the viewer has a custom viewport this frame (the viewport
      module's own `world_rect`, not a copy), its own bars stay inside
      it ("Keep the bars inside the viewport": the bar goes with the
      viewer's bars, so the two never overlap) and the option is off;
      across the window otherwise, exactly as before: no viewport (or
      none this frame: login, loading), the option on, or the bars
      over the whole window. (In mouselook and the full-frame photo
      view there is no bar at all, as before.)
    - *Over the world*, nothing is taken from the interface's room (the
      safe-area inset is zero), so the margins' windows keep the
      window's whole height; the world's own rectangle moves down by
      the strip (`App::world_viewport`, the fork's function the renderer,
      the HUDs, the mouselook centre and the camera's proportions
      already read), so the 3D view and the HUDs start under the bar,
      and the viewer's bars, laid out in the world's rectangle, follow
      without a change; `bars_room` returns them as they are (no inset),
      and `bars_screen` and the floaters' room read the same area.
    - **Upstream hooks: unchanged**, the same two lines; the rest is in
      `app/folderstorm_viewport.rs` and `app/folderstorm_characters/
      bar.rs`.
    - *Tested:* the rule (a left and a right margin, a top margin, the
      scale, collapsed, the option, no viewport, bars over the window,
      a world too low, popped out); a pass over two monitors with the
      world on either: the strip drawn at the world's top and width,
      the interface's room the whole window, the floaters' room as high
      as the window, the world and the viewer's bars under the strip and
      `bars_screen` agreeing, the pointer on the bar over the interface;
      with the option, all as before; the picture painted by the
      processor (nothing of the bar over the margin, the bar in the
      world's strip, the world under it); the setting on the page and
      in the state.
    - *To verify on Windows:* a window over two monitors, the world on
      one (`AURORA_DEMO_VIEWPORT=1` with the bar's demo, or the
      viewport's "Fit to a monitor"): the bar over the world only, the
      3D view, the HUDs and the viewer's bars under it, a window in the
      margin as high as the window; "Span the whole window" on: the bar
      across the whole window, everything under it as before; the
      viewport off: as before; the world on the left monitor too.
  - **Follow-ups, if the Windows test asks for them:** Alt-Tab through
    a short hide and show around the style change (against R2 as
    written: only with the human's word, and only for a window whose 3D
    is off); the panels' windows off the taskbar too; a free order of
    the toolbar.

### Checkpoint 9: monitor windows (placeholder)

Not built. From the human's notes (2026-11-12):

- each monitor in **its own window**, compact: no "Read-only preview"
  text; Switch (was "Exchange": the monitored character becomes the
  active one and the one that was active is monitored in this window),
  Chat and Settings as icon buttons; a pin (always on top, as the
  Character controls' pin); the character's name somewhere;
- a **cycling monitor** that rotates through the background characters
  (the bar's monitor menu keeps a place for it), which pauses while the
  user types in it ("Require pause to chat", on by default), so a line
  is never said as the wrong character;
- options: sizes 320 / 480 / 640, rates 0.5 / 1 / 2 / 5, the shared
  budget's note, "Show chat", "Use as monitor defaults";
- the chat in a monitor: New IM, Friends; conversations as chips like
  the shared chat's (not a dropdown), a close × per chat (not Nearby);
  names cut to a least length with the full name in the hover; at most
  two rows of chips, then a "+N" chip that lists the rest (order:
  Nearby, pinned, unread, recent; a new message promotes its chat); the
  same in the shared chat.

### Checkpoint 10: settings redesign (placeholder)

Not built. From the human's notes (2026-11-12), after Folderstorm
(C++)'s "Host Preferences", without its sparse pages:

- **Layout**: toolbar button labels and quick action labels (built in
  checkpoint 8, to move here);
- **Character settings**, per character: name, background mode (Warm /
  Economy), alias, colour (C++ had a free colour), "Logged in as", card
  position, "Include in Attention"; sounds: alerts while another
  character is active (direct IM, group chat, conference,
  notifications), a notification sound (WAV, built-in tone, preview,
  reset);
- **Chats and notifications**: what appears in the shared and monitor
  chats (Nearby, direct IMs, group chats, conferences, viewer
  notifications); which offers appear in Attention (teleport offers,
  permission requests, inventory offers, notifications: no chats),
  oldest first; reading: "Mark displayed chats read", "Auto-read the
  active character" after a delay (60 s);
- **Audio and voice**: Voice follows active, Mute background sound
  (both built in checkpoint 8), alert volume;
- **Controls**: the quick actions (checkpoint 8) and the hotkeys
  (checkpoint 4 and its follow-ups);
- **Advanced**: open the settings folder, icon credits, recover window
  positions, show diagnostics;
- **Updates and Support**: point to the viewer's own pages for now;
- the MCP controller's page waits for the MCP session routing
  (FEATURES 4.6).

## 16. Verified in code vs inferred

**Verified by reading this repository:** the instance registry,
`BEHIND_FPS` and VRAM share; the busy pause and its paused state; texture
release behind or minimised; background cap defaults; the stall notice path
and timings; the loss steps (Potato saved on the second loss, close on the
third); where `frame()` drains network events, and the `about_to_wait`
hidden-window comment; multi-writer-safe cache, pipeline cache and
browser-cache purge; logs and crash folders per process; `config_dir()` as
the single settings root and `settings.json` last-writer-wins;
`secondlife://` scoping by title and demo; credential keys; in-process WebRTC
voice; `MinimizedMute`; `LineClock`; `send_local_chat`; the snapshot override
of the pause; the `--profile Alt1` positional-SLURL pitfall in `cli.rs`;
egui 0.37, egui-wgpu and egui-winit in the tree; the world viewport API on
the PR #6 branch.

**Inferred, to check:** whether presenting from a window behind or minimised
can hold the render thread past 2 s on Windows drivers; the stdin bootstrap
into a GUI-subsystem child; foreground and hotkey rules in practice; shared
microphone capture; group and conference chat in the same `ImSession` model;
snapshot wait on a busy card and RLVa effects in snapshots; driving `frame()`
without a window (R5); the real RAM, CPU and VRAM cost per character; what
exactly caused the past bug in the earlier port (section 17).

## 17. Decisions (the human, 2026-11-10)

1. **Inactive characters: 3D Off by default**, on only while a monitor of that
   character is being viewed (then Limited, section 12).
2. **The host is a mode of the main program** (`--session-host`), provided
   that is the most stable implementation; revisit only if it proves less
   stable than a separate program.
3. **At most 5 characters.** (Monitors: one per other character, so at most 4.)
4. **Separate asset caches by default**, one per profile; sharing the cache
   is an **option** per profile (it saves downloads and disk, the files being
   multi-writer safe, section 3). This replaces the shared default suggested in
   sections 3–5: a profile's cache folder defaults to its own, and the
   `cache.key` note in section 5 applies only when sharing is chosen.
5. **Background characters: sound muted and voice off** by default.
6. **The past bug:** the inactive viewer closed almost immediately when another
   viewer became active, with a box listing several possible reasons, one of
   them the graphics card being disconnected. That matches Aurora's
   `app-gpu-lost` message ("its driver was reset …, or the card was removed or
   taken by another program … Aurora closes when you press OK"), i.e. the
   device-lost path of section 6.2, reached when focus moves to another
   instance (an `Outdated`/`Lost` surface or a lost device on a window that lost
   focus or was occluded). **Checkpoint 1 acceptance:** with two instances,
   switching the active window back and forth repeatedly (including minimise and
   restore, and full-screen) never shows that box or closes a character, and a
   real loss in one character never downgrades or closes the others.
7. **Windows is the target platform**; Linux and macOS are nice to have
   (launcher, chat and monitors; switching best effort).
