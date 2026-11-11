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
    Ctrl+Alt+1–5, Ctrl+Alt+PageDown (next), Ctrl+Alt+PageUp (previous);
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
