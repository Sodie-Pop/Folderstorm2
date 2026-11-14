# Local assistant (MCP): gestures and own posing

Group 9 of the MCP parity checklist (FEATURES.md 4.6): the assistant lists
and inspects the user's gestures, plays one on the user's avatar and stops
it, and poses the user's **own** avatar locally with joint rotations,
presets and a saved pose file. The eight shared-pose tools were dropped by
the human (2026-11-10, FEATURES.md "Not porting"): not built.

Sources, read as information: Folderstorm (C++)'s
`doc/plans/mcp-gestures-and-posing.md` and
`tools/fs-mcp/internal/mcptools/gestures.go`, `pose.go` (names, arguments,
bounds, rules). The viewer's own paths reused: `gestures::Manager` (the
Gestures window's playback: `play`, `stop`, `tick_ready`), `app/gestures.rs`
(`gesture_request` loads a gesture without activating it,
`gesture_preview` plays a definition under a key of its own, as the
gesture editor's Preview does, `gesture_effects` sends its animations,
sounds and chat), `ik_edit::poser` (`Frame::subject`, `SubjectPose::capture`,
the scene's poser layer `Scene::poser`, the pose file `Document` and its
`apply` in rotations mode, the hand presets `presets.json`, the pose
library folder `<account>/poses`), `cmdline::parse` (what the chat bar
would run as a viewer command), `Rlva::outgoing_chat` (what RLVa does to a
line).

## Tools (capabilities `gestures: 1`, `posing: 1`)

| Tool | Class | Asks | What it does |
|---|---|---|---|
| `gesture_list` | Read | – | The active gestures (the Gestures window's active list): item id, name, trigger, loaded, playing. `limit` 1–100 (50). |
| `gesture_status` | Read | – | One gesture of the user's inventory: loads it if needed (never activates it), its steps (animation and sound names, chat lines, waits), whether it can be played and why not, a `revision` (salted hash), and the assistant's playback of it. Never an asset id. |
| `gesture_play` | Gestures (+ Read) | level; **always** when it says something | Plays that gesture at its `revision` on the user's avatar, once, without activating it. Required `client_request_id`. Answers `playback_id`. |
| `gesture_stop` | Gestures (+ Read) | level | Stops the assistant's playback `playback_id`. |
| `pose_status` | Read | – | The assistant's pose session (`pose_session`, `revision`, joints changed, undo depth, state) and whether the viewer's poser poses the user now. |
| `pose_joints` | Read | – | The user's avatar's joint rotations (the session's, else the avatar's pose now), all joints or up to 64 named. |
| `pose_presets` | Read | – | The built-in hand presets and the pose library's files, with an opaque `preset_id`, a `preset_revision` and whether each can be loaded (rotations only). |
| `pose_start` | Pose (+ Read) | level | Starts a session: the avatar's pose now is captured and held on this screen only. |
| `pose_update` | Pose (+ Read) | level | Sets up to 64 joints' rotations at the session's `revision`. |
| `pose_undo` | Pose (+ Read) | level | Undoes the session's last change. |
| `pose_reset` | Pose (+ Read) | level | Back to the pose captured at the start (the undo history stays). |
| `pose_stop` | Pose (+ Read) | level | Ends the session: the avatar moves as before. |
| `pose_load` | Pose (+ Read) | level | Applies a listed preset's rotations at the session's `revision`. |
| `pose_save` | Pose (+ Read) | **always** | Saves the session's pose as a new pose file in the library; never overwrites. Required `client_request_id`. |

Same names as Folderstorm (C++); no `viewer_session` (the sidecar pins the
login) and no `target`: these tools only ever act on the user's own avatar.

## Permission classes and asking (decision)

- **New class Gestures** (`gestures`, "Play gestures on my avatar"):
  play and stop gestures. What a gesture does is public: its animations and
  sounds reach everyone nearby, its chat lines are said in the user's name.
- **New class Pose** (`pose`, "Pose my avatar"): the pose session and its
  file. Posing shows on this screen only (the scene's poser layer, as the
  viewer's poser and the outfit portrait); nothing is sent.
- Both Ask in "Ask before changes", Never in "Read only". Settings saved
  before them: Gestures follows "Ask before changes" when every other
  change asks (Build included), else Never; Pose the same, Gestures
  included.
- The reads use **Read**: they read the user's own inventory and avatar,
  nothing of anyone else. RLVa `@showinv` refuses the gesture reads, as
  every inventory read.
- **A gesture that says something always asks**, even on Allow, and the
  dialog lists each line it will say (channel and text): it speaks in the
  user's name. No separate chat class exists yet; when the social group
  brings one, chat-bearing gestures will need it too (to review then).
- **`pose_save` always asks**: it writes a file on the user's disk.
- `client_request_id` is required for `gesture_play` (an animation, a sound
  or a line said cannot be recalled) and `pose_save` (a file created); the
  pose changes are guarded by the session's revision and send nothing.

## Gestures

- **Which gestures**: the original of a link, a gesture (asset type 21) of
  the user's own inventory, not in the Library or the Trash, with **Copy**
  for the user (`actions::held`): stricter than the gesture editor, which
  shows the steps of any gesture (as Folderstorm (C++)). Never an asset id:
  animations and sounds are named by their step name and kind (built-in or
  not).
- **Loading** goes through `gesture_request(id, open: false, play: false,
  active: None)`: the gesture's asset is fetched and decoded as for the
  window, never activated. A load that fails answers `failed`.
- **Playing** reuses the gesture editor's Preview path: the definition is
  played under a key of its own (the `playback_id`), never under the item's
  id, so a manual play of the same gesture is neither restarted nor
  stopped by the assistant. No activation, no key, no wait for a key
  release.
- **Bounds**: 64 steps; one assistant playback per login (`busy` while one
  plays); at most 120 s, then stopped.
- **Stop** releases only what the assistant's playback started: an
  animation another playback (a manual gesture) still holds is not stopped
  (`Manager::stop_owned`). A sound or a line already sent cannot be
  recalled.
- **Revocation**: Gestures set to Never, access turned off, a login change
  or a teleport stops the assistant's playback.
- **Refused** (`not_allowed`): gestures turned off in the Gestures window,
  RLVa `@sendgesture`; a chat line the chat bar would run as a viewer
  command (`cmdline::parse`: an assistant never runs viewer commands); a
  line RLVa would block or change (`outgoing_chat`: `@sendchat`,
  `@redirchat`, `@rediremote`, `@sendchannel`). Just before the play, all
  of it is checked again; at the send, the viewer's own RLVa filter applies
  to each line as it does to every line.
- **Outcome**: `done` once the playback started, with `playback_id`;
  `gesture_status` gives it as `playing`, `finished`, `stopped`, `expired`
  or `revoked`. Started is not delivered: nothing confirms that anyone saw
  or heard it.

## Own posing

- **One session per login**, on the user's own avatar only, shown on this
  screen only: each frame the app puts the session's pose in the scene's
  poser layer for the avatar, as the viewer's poser does.
- **The viewer's poser wins**: while it poses the user, `pose_start` is
  refused; if it starts posing the user during a session, the session ends
  (`superseded`) and the poser's pose stays.
- A teleport, a login change, access turned off, Pose set to Never, or a
  skeleton that changed (another shape) ends the session.
- **Rotations only**: joints of the Bento skeleton named `m…`, the pelvis
  excluded; a rotation is a unit quaternion `[x, y, z, w]` (within 0.002 of
  unit length, normalised) relative to the joint's rest orientation in its
  parent's frame (identity is the skeleton's rest pose). Never a position,
  a scale or a world pin.
- **Revisions**: an integer, 0 at the start, one more at each change; a
  change at another revision is `stale` (read `pose_status`). Undo keeps 64
  steps.

## Presets and the pose file

- **Listed**: the five built-in hand presets (the poser's Hands presets)
  and the newest 64 pose files of the library (`<account>/poses`,
  `.aurora-pose.json`, 64 KiB each, `Document::valid`), read on a worker
  thread. A preset is named by an opaque `preset_id` and a `preset_revision`
  (salted hashes): never a path.
- **Loadable** only when it holds rotations only: no joint moved, scaled or
  pinned, the pelvis left as it is. Others are listed with why not.
- **Load** applies the listing's frozen content (`Document::apply` in
  rotations mode for a file, the hand preset's angles as the poser sets
  them); a preset gone from the last listing, or of another revision, is
  `stale`.
- **Save**: a name of 1–64 letters, digits, `_` or `-`; the file
  `<name>.aurora-pose.json` in the library, created new (never an
  overwrite: an existing name is `conflict`), written on a worker thread,
  in the poser's own format (`Document::capture`).

## Creators' permissions

`AGENTS.md` § 2: a gesture's steps (its animations', sounds' and lines'
content) need Copy on the gesture item for the user, stricter than the
editor; playing it is what the Gestures window does with the same item. No
asset id leaves (revisions are salted hashes, built-in animations are said
to be built-in). Pose files hold joint rotations of the user's own avatar,
no asset.

## RLVa (the viewer decides)

`@sendgesture` refuses a play; `@sendchat`, `@redirchat`, `@rediremote`,
`@sendchannel` refuse a gesture whose line they would block or change, and
filter each line at the send as for any line; `@showinv` refuses the
gesture reads. The viewer's poser follows no RLVa restriction (it is local):
neither does the assistant's session.

## Where the code goes

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/gestures.rs`, `protocol/posing.rs` (new) | the catalogues, bounds, required ids, `always_asks` (save) |
| `policy.rs`, `binding.rs`, `protocol.rs`, `protocol/inventory.rs` | `Class::Gestures`, `Class::Pose` and their migration; the capabilities; `pub mod`, `all()`, required ids, always asks; the catalogue count |
| sidecar `jsonrpc.rs` | the instructions' paragraph |
| viewer `mcp_bridge/gestures.rs`, `mcp_bridge/posing.rs` (new) | checks, revisions, plans, playbacks, sessions, presets, the demo |
| `mcp_bridge/desk.rs`, `changes.rs`, `tools.rs`, `mcp_bridge.rs` | hook lines: reads, `Work::Gesture`, `Work::Pose`, ask rule, tick, clear, close, the app's entry points, the demo |
| `app/local_assistant.rs` | the play, the stop, the pose layer, the file jobs |
| `app/gestures/folderstorm.rs` (new, hook `mod folderstorm;` in `app/gestures.rs`) | load, play, stop through the window's own private paths |
| `gestures/folderstorm.rs` (new, hook `mod folderstorm;` in `gestures.rs`) | `Manager::stop_owned` |
| `ik_edit/poser/document/folderstorm.rs` (new, hooks in `document.rs` and `poser.rs`) | the hand presets, a file's rotations, a new file's bytes |
| `app.rs` | one call after the poser's layers |
| Preferences page, catalogues | two rows, the Ask sentences, en/fr/ja |

## Demo

`AURORA_DEMO_MCP=gestures` with `AURORA_DEMO_GESTURES=mcp` (which seeds the
demo gesture): Ask before changes; the demo lists the gestures, inspects
the demo gesture, plays it (always asked: it says two lines), then starts a
pose session (asked), turns the head (asked), lists the presets, loads the
"Pointing" hand preset (asked), saves the pose (always asked) and logs
`pose_status`.

## Tests

Catalogue (classes, required ids, always asks, bounds); policy (presets and
migration of both classes); gestures (own, Library, Trash, no copy, link,
not a gesture, load and failure, revision, stale, 64 steps, busy, disabled,
`@sendgesture`, a viewer command line, `@sendchat` and `@redirchat`, always
asks with a line, own key, stop only owned animations, 120 s, revocation);
posing (start captured, native poser refuses and supersedes, joints and
bounds, pelvis, unit length, stale revision, undo, reset, stop, teleport,
login change); presets (hand presets, a rotations-only file, a file that
moves a joint, too large, ids without paths, stale); save (name rules, never
an overwrite, always asks); no asset id in any answer; end to end over a
real socket.

Not testable here: real animation blending and timing on a grid, the grid's
relay of gesture sounds and chat, the dialog's look.

## As built

Branch `claude/mcp-gestures`, on `claude/mcp-building`.

| Where | What |
|---|---|
| `folderstorm-mcp-bridge/src/protocol/gestures.rs`, `protocol/posing.rs` (new) | the 14 tools, bounds, `valid_name`, required ids (play, save), `always_asks` (save) |
| `policy.rs`, `binding.rs` | `Class::Gestures` (`gestures`), `Class::Pose` (`pose`), presets and migration; capabilities `gestures: 1`, `posing: 1` |
| `protocol.rs`, `protocol/inventory.rs` | `pub mod`, `all()`, `ToolSpec::needs_client_request_id`, `always_asks`; the catalogue count |
| sidecar `jsonrpc.rs` | `GESTURES_HINT` |
| viewer `mcp_bridge/gestures.rs`, `posing.rs` (new) | checks, revisions, plans, playback, session, presets and save workers, the demo |
| `mcp_bridge/desk.rs`, `changes.rs`, `tools.rs`, `mcp_bridge.rs` | `Work::Gesture`, `Work::Pose`, `Ctx::avatar`; the reads, plans, ask rule, `tick_avatar`, clear, close; `gesture_loads`, `gesture_stops`, `gesture_started`, `gesture_stopped`, `pose_layer`, `pose_run`; replan; the demo |
| `app/local_assistant.rs` | the View's avatar part, the play and stop, `local_assistant_pose` |
| `app/gestures/folderstorm.rs` (new) | `assistant_gesture_load`, `_failures`, `_play`, `_stop` |
| `gestures/folderstorm.rs` (new) | `Manager::stop_owned` |
| `ik_edit/poser/document/folderstorm.rs` (new) | `hand_preset`, `inspect`, `apply_file`, `file_bytes` |
| Preferences page, catalogues | two rows, nine Ask sentences and four lines, en/fr/ja |

Upstream files touched, each by one `// Folderstorm:` line: `app.rs` (the
pose hook after the poser's layers), `app/gestures.rs` and `gestures.rs`
(`mod folderstorm;`), `ik_edit/poser.rs` (`pub(crate) use
document::folderstorm as assistant;`), `ik_edit/poser/document.rs`
(`pub(crate) mod folderstorm;`).

Differences from the design above: none of substance. The hand presets'
angles are read from the poser's own `presets.json` and set as its Hands
control sets them (a few lines in the new module, since that control needs
the poser window's state).
