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
- **Bootstrap.** The child's **stdin is an anonymous pipe** held only by the
  host and that child. The host writes one bounded line (control pipe name
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
  Group chat and conference coverage is to be checked during checkpoint 3.
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
  slot) and global hotkeys (`RegisterHotKey`: Ctrl+Alt+1–5, next, previous;
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
    for the bootstrap: an anonymous pipe has no name to find or squat, only
    the host and that child hold it, and its end is exactly "the host is
    gone" (unmanaged fallback) on every system, with no Windows-only code.
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
    it, or has ended. A viewer that does not answer keeps the voice waiting
    (it may still be transmitting); the host says whom it waits for.
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

### Checkpoint 4: comfort

- Quick-action buttons and global hotkeys; profile copy (section 13); voice
  follows the active character (section 10).
- **Acceptance:** hotkeys switch even while a viewer has focus; a copied
  profile starts with the main profile's interface but no accounts; only the
  active character is heard (two test accounts, live voice).

### Checkpoint 5: monitors (3D becomes Limited)

- Section 12.
- **Acceptance:** four monitors at 1 fps update and show the frame age; the
  image matches the character's own view (aspect, orientation, restrictions);
  no flicker; closing a monitor returns that viewer to Off; foreground fps
  barely changes (measure the added GPU and CPU cost); a busy card lowers the
  monitor rate instead of resetting the GPU.

### Checkpoint 6: pop-out panels and fitted layout (optional)

- Chat, controls and monitors as separate host windows; an opt-in fitted
  borderless stage (section 7), only if checkpoint 2's arrangement is not
  enough.
- **Acceptance:** dialogs, IME, Alt-Tab, minimise and restore, and DPI
  changes leave no vanished world and no stolen focus.

### Checkpoint 7: polish picked by the human

- See section 13.

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
