# Local assistant (MCP) guide: phase 1, read-only

This guide is for PR #10. It lets an AI assistant on your own computer read
your Folderstorm2 inventory. It cannot change anything yet.

## 1. What it is

Three pieces work together:

1. **The bridge**, inside the viewer. It listens on this computer only.
2. **The helper `folderstorm-mcp`** (a small program, `folderstorm-mcp.exe` on Windows). Your assistant starts it. It talks MCP over stdio to the assistant and finds the running viewer to ask it questions.
3. **Your assistant** (Claude Code, Cursor or Codex have a "Copy configuration" entry).

The viewer never starts the helper. The assistant does.

### Safety model

- **Off by default.** Nothing listens until you turn it on.
- **This computer only**: the bridge binds `127.0.0.1` on a port the system picks. Requests with an `Origin` header, or a wrong `Host`, are refused.
- **Access key (token)**: new every time the bridge starts, 32 random bytes. It is written only to a small discovery file, `folderstorm2-mcp-<pid>.json`, in the viewer's settings folder, and deleted when access is turned off or the viewer quits. It is never shown in Preferences, never copied by Copy configuration and never printed by the helper. Other programs running under your Windows account can read that file, so turn access off when you do not need it.
- **Permission classes**: Read, Edit, Move, Wear, Trash, each Allow, Ask or Never. Read has no Ask. In phase 1 only Read is used; Ask is refused like Never.
- **Presets**: Read only (default), Ask before changes, Custom.
- **Session only**: "Use these settings for this session only" keeps changes in memory until you quit. The command-line option `--mcp-api` turns access on for one session the same way, and saves nothing.
- **Creators' permissions**: an item's asset id is shown only for items with full permissions. Thumbnails are never returned. If RLVa `@showinv` is active, inventory reads are refused.
- **Activity log**: Preferences shows tool names and outcomes only, never what was asked or answered.
- Limits: 4 connections at once, small request sizes, timeouts.

## 2. Build the helper

The viewer looks for the helper **beside its own executable**, so build both
the same way. `cargo run -p aurora-viewer` alone does not build the helper.

- [ ] From the repository root (branch `claude/test-all`, see TEST-GUIDE.md):

```powershell
cargo build --release --workspace
```

- [ ] The helper is then at `target\release\folderstorm-mcp.exe`, next to `target\release\aurora-viewer.exe`. (With a plain `cargo build --workspace` both are in `target\debug\`.)
- [ ] Check it runs:

```powershell
.\target\release\folderstorm-mcp.exe --version
.\target\release\folderstorm-mcp.exe --help
```

Release packaging does not include the helper yet.

## 3. Turn it on in Preferences

- [ ] Start the viewer, from `target\release` so the helper is beside it:

```powershell
.\target\release\aurora-viewer.exe
```

- [ ] Open Preferences › Privacy. Find the group **Local assistant**.
- [ ] Turn on **Allow local assistant access**. (To keep it only for this run, first turn on **Use these settings for this session only**, or start the viewer with `--mcp-api`.)
- [ ] Under **What the assistant may do**, leave **Read only** selected.
- [ ] **Status** shows "Ready on 127.0.0.1, port ...". The access key is not shown.
- [ ] Log in to the grid yourself. The inventory tools need a logged-in viewer.

One-session alternative (nothing saved):

```powershell
.\target\release\aurora-viewer.exe --mcp-api
```

Under Custom, the only row is **Read inventory and outfits** (Allow or
Never). Never makes every inventory read answer `permission_denied`.
`viewer_status` still works. The page also says: "This version only reads."

## 4. Connect your assistant

In Preferences › Privacy › Local assistant › Setup:

- [ ] **Assistant**: choose Claude Code, Cursor or Codex.
- [ ] Press **Copy configuration**. The message says "Copied. Add it to the assistant's MCP settings, then restart the assistant." If it says the helper is not beside the viewer, build it (section 2).
- [ ] Paste the entry into the assistant's MCP settings. Keep the other servers already there.

The viewer does not offer Claude Desktop in its list. Its JSON entry is the
same `mcpServers` shape, but nothing in the viewer or the plan tests it with
Claude Desktop. If you try it, treat it as untested.

### Claude Code and Cursor (JSON)

Claude Code reads `.mcp.json` (the notes in the viewer's code name this file),
Cursor reads `mcp.json`. The entry looks like this (your paths will differ;
the viewer fills them in; there is no token):

```json
{
  "mcpServers": {
    "folderstorm2": {
      "command": "C:\\path\\to\\target\\release\\folderstorm-mcp.exe",
      "args": [],
      "env": {
        "FOLDERSTORM2_MCP_DISCOVERY": "C:\\Users\\<you>\\AppData\\Roaming\\Folderstorm\\Folderstorm2\\config"
      }
    }
  }
}
```

- `command` is the absolute path of the helper beside your viewer.
- `FOLDERSTORM2_MCP_DISCOVERY` is the viewer's settings folder, where the discovery file is written. Use the value the viewer copies, not the one above: demo mode, for example, uses a `demo` subfolder of it.

### Codex (TOML)

```toml
[mcp_servers.folderstorm2]
command = "C:\\path\\to\\target\\release\\folderstorm-mcp.exe"
args = []
env = { FOLDERSTORM2_MCP_DISCOVERY = "C:\\Users\\<you>\\AppData\\Roaming\\Folderstorm\\Folderstorm2\\config" }
```

- [ ] Restart the assistant so it loads the new server.

## 5. Check the connection

### In the viewer

- [ ] Press **Check connection** (same Setup group). It runs the helper once. "Checking..." then "The helper reached this viewer." (or "... Log in to use the inventory." when not logged in).
- [ ] If it fails: "The check failed at the step "<stage>"", "The helper is missing beside the viewer.", "The helper could not be started." or "The helper did not answer in time." See section 8.
- [ ] Note: the check does not prove your assistant has loaded its settings.

### From PowerShell

```powershell
.\target\release\folderstorm-mcp.exe --diagnose
```

It prints one JSON line (never the token). Exit code 0 means the bridge
answered, 1 that it did not, 2 bad arguments. Fields: `ok`, `stage`,
`viewer_pid`, `port`, `api`, `viewer_version`, `read_allowed`, `session`
(`logged_in`, `logged_out` or `demo`), `inventory_usable`. The `stage` says
how far it got: `discovery`, `connect`, `api`, `health`, `status`, or `done`.

Options: `--viewer-pid <PID>` picks one viewer; `--discovery <folder>` names
the folder with the discovery file (default: `FOLDERSTORM2_MCP_DISCOVERY`,
else the viewer's settings folder). Example:

```powershell
$env:FOLDERSTORM2_MCP_DISCOVERY = "C:\path\to\settings\folder"
.\target\release\folderstorm-mcp.exe --diagnose
Remove-Item Env:FOLDERSTORM2_MCP_DISCOVERY
```

### Without an account: demo mode

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_MCP = "1"
$env:AURORA_DEMO_OPTIONS = "privacy:local-assistant"
.\target\release\aurora-viewer.exe --title "Test mcp"
```

- [ ] Preferences opens on the group: switches, presets, "Ready on 127.0.0.1, port ...", recent activity and Setup.
- [ ] **Check connection** passes, and `folderstorm-mcp --diagnose --discovery <demo settings folder>` reports `"session":"demo"`. The demo settings folder is the `demo` subfolder of the settings folder.
- [ ] The access key appears nowhere on the page or in the copied configuration.
- [ ] Turn access off, or quit the viewer: `folderstorm2-mcp-<pid>.json` disappears from the folder.
- [ ] Under Custom, set Read to Never: the helper's calls answer `permission_denied`.

Clear the variables afterwards:

```powershell
Remove-Item Env:AURORA_DEMO_MCP
Remove-Item Env:AURORA_DEMO_OPTIONS
Remove-Item Env:AURORA_DEMO
```

## 6. The six tools, and prompts to try

Log in first (or use demo mode). Ask your assistant:

| Tool | What it does | Prompt to try |
|---|---|---|
| `viewer_status` | Logged in or not, inventory usable, how far the fetch has gone. Works even when Read is Never. | "Check the viewer status." |
| `inventory_search` | Items and folders by name, optionally in a folder or path, or of one type (results are capped). | "Find my boots." "Search my inventory for 'hair' in the Clothes folder." |
| `inventory_list_folder` | Subfolders and items of one folder, by id or path (My Inventory if none). | "List what is in my Clothes/Boots folder." |
| `inventory_item` | One item's details: name, description, type, folder, creator, date, permissions, worn. Asset id only for full-permission items. | "Show the details of the first boots you found. Who made them?" |
| `outfits_list` | Outfits saved in My Outfits, with the current base marked. | "List my outfits." |
| `worn_items` | What the avatar wears: body parts, clothing, attachments. | "What am I wearing?" |

Check these:

- [ ] "Check the viewer status" and "list my outfits" both answer.
- [ ] "What am I wearing?" matches the Appearance window.
- [ ] Preferences › Recent activity lists each tool name and "done", without content.
- [ ] Ask for the asset id of an item you cannot copy or transfer: the answer says it is hidden.
- [ ] Ask the assistant to rename or move something: it cannot (phase 1 has no such tool).
- [ ] Names are not unique. If a path is ambiguous the answer lists candidate ids.
- [ ] A folder not yet loaded shows `fetched: false` and is not loaded for you. Open that folder in the inventory once, then ask again.

## 7. Several viewers

Each viewer writes its own discovery file, named with its process id. With
several running, the helper uses the newest file whose viewer answers. To pick
one, give the process id:

```powershell
Get-Process aurora-viewer | Select-Object Id, MainWindowTitle
.\target\release\folderstorm-mcp.exe --diagnose --viewer-pid 12345
```

In the assistant's configuration, add the option to `args`:

```json
"args": ["--viewer-pid", "12345"]
```

A process id changes at every start, so this is best for a quick test. The
helper finds the viewer again after a restart, if no `--viewer-pid` is fixed.
Viewers started with `--profile <name>` each have their own settings folder, so
each has its own `FOLDERSTORM2_MCP_DISCOVERY` value to copy.

## 8. Troubleshooting

| Symptom | What to do |
|---|---|
| "The helper is not beside the viewer" / "Reinstall the viewer, or build it with cargo build --workspace" | Run `cargo build --release --workspace` and start the viewer from `target\release`. |
| Check fails at `discovery` | The viewer is not running, or **Allow local assistant access** is off, or the helper looks in another folder. Turn access on; compare `FOLDERSTORM2_MCP_DISCOVERY` in the assistant's configuration with the viewer's settings folder. |
| Check fails at `connect` | A stale discovery file: the viewer quit or crashed. Start it, or turn access off and on. |
| Check fails at `api` | The helper and viewer are from different builds. Rebuild both. |
| Check fails at `health` or `status` | Run `--diagnose` and read the `message` field. |
| Status shows "Could not open the connection (...)" | The bridge could not start. Turn access off and on. |
| `permission_denied` | Under Custom, **Read inventory and outfits** is Never. Set it to Allow, or choose Read only. |
| Answers say the inventory is not available | You are not logged in, still loading, or RLVa `@showinv` is on. |
| The assistant does not list the server | Restart the assistant after pasting. Check the JSON or TOML is valid and the path is absolute. Keep your other servers. |
| Settings reset at the next start | You used "this session only" or `--mcp-api`. Turn the switch off to save them. |
| Anything else | See "If something breaks" in [TEST-GUIDE.md](TEST-GUIDE.md). |

## 9. Not in phase 1

- Phase 1 only reads. **Edit, Move, Wear and Trash** (rename, create folders, move, copy, link, wear, take off, Trash and restore) come in **phase 2**, with an approval dialog (60 s) for Ask. Until then Ask behaves like Never.
- Phase 3: camera presets and snapshots. After that, more tools (navigation, chat, teleport, uploads, world observation).
- Permanent delete and Empty Trash will never be offered.
- The release installer does not ship `folderstorm-mcp` yet.

Design details: [plans/mcp.md](plans/mcp.md).
