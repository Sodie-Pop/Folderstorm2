# Test guide for the open PRs (Windows)

This guide covers the seven open PRs, merged together on the branch
`claude/test-all`. Test them here, in one build. The local assistant (PR #10)
has its own guide: [MCP-GUIDE.md](MCP-GUIDE.md).

Nothing here needs a password in a file. When a step says "on the grid", you
log in yourself, in the viewer's own login screen.

## 0. Build and run the combined branch

- [ ] Get the branch:

```powershell
git fetch origin
git checkout claude/test-all
git pull
```

- [ ] Build the whole workspace once (this also builds `folderstorm-mcp.exe`, which PR #10 needs):

```powershell
cargo build --release --workspace
```

- [ ] Run the viewer normally (login screen, then the grid):

```powershell
cargo run --release -p aurora-viewer
```

### Demo mode (offline, no account)

Demo mode simulates a plaza, avatars and an inventory. It keeps its own
settings and cache, so it does not touch your real ones.

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_APPEARANCE = "gallery-size"
cargo run --release -p aurora-viewer -- --title "Test gallery-size"
```

### Clear the variables between runs

Environment variables stay set in the PowerShell window. Remove each one you
set before the next test, or the old scenario starts again:

```powershell
Remove-Item Env:AURORA_DEMO_APPEARANCE
Remove-Item Env:AURORA_DEMO_INVENTORY
Remove-Item Env:AURORA_DEMO_VIEWPORT
Remove-Item Env:AURORA_WINDOW_SIZE
Remove-Item Env:AURORA_DEMO_SESSION_BEHIND
Remove-Item Env:AURORA_DEMO_GPU_LOST
Remove-Item Env:AURORA_DEMO_GPU_STALL
Remove-Item Env:AURORA_DEMO_MCP
Remove-Item Env:AURORA_DEMO_OPTIONS
Remove-Item Env:AURORA_DEMO
```

An error "cannot find ... because it does not exist" just means that one was
not set. Ignore it. The simplest way is to open a fresh PowerShell window for
each PR.

---

## PR #4: Outfit safety in the inventory menus

**What it does.** An item's **Wear** is greyed in the inventory menu unless
you turn it on. Replace Current Outfit and Add to Current Outfit (folder menu)
always ask first, with Cancel as the default.

### Demo steps

- [ ] Start the Legacy-style inventory scenario:

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_INVENTORY = "outfit-ask"
cargo run --release -p aurora-viewer -- --title "Test outfit-ask"
```

- [ ] Also try the Aurora-style inventory: set `$env:AURORA_DEMO_INVENTORY = "aurora-outfit-ask"` and run again.
- [ ] The confirmation for Replace Current Outfit is open on the folder "Right-click tests". Check that the question names the folder.
- [ ] Check that **Cancel** comes first and is focused: Enter cancels, Escape cancels.
- [ ] Right-click an item. **Wear** is visible but greyed.
- [ ] Open Preferences › Inventory and turn on **Enable "Wear" in inventory menus**. Right-click the item again: Wear is now enabled.

### On the grid

- [ ] Right-click a folder (not in the Library) › Replace Current Outfit. The question appears; press Cancel. Your outfit does not change.
- [ ] Repeat and accept the question. The folder is worn.
- [ ] Same for Add to Current Outfit.
- [ ] Wear objects and Remove from Current Outfit still work as before, without a question.
- [ ] Add, Attach to, double-click and the Outfits window keep their old behaviour.

**Pass:** Wear is greyed by default; Cancel is the default in the question; Enter and Escape cancel; accepting wears the folder.

**Known limits (PR body):** No "don't ask again" by design. FEATURES.md is updated after #1 is merged.

---

## PR #5: Thumbnail size for the Outfit Gallery

**What it does.** A switch and a slider in the Appearance window set how big
the outfit photos are. Off by default: the gallery looks as before.

### Demo steps

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_APPEARANCE = "gallery-size"
cargo run --release -p aurora-viewer -- --title "Test gallery-size"
```

- [ ] The gallery opens with 96-pixel thumbnails and as many columns as fit.
- [ ] Open the gear (⚙) menu. Use **Choose the thumbnail size**: a slider (80 to 256) appears.
- [ ] Move the slider. The number of columns changes live.
- [ ] Turn the switch off. The usual layout returns (two columns, or three from a wide list).

### On the grid

- [ ] Appearance › outfit gallery › ⚙ › Choose the thumbnail size. Try small and large values with your own outfits.
- [ ] Quit and start again: the size is still there (it is saved).

**Pass:** columns follow the slider, never fewer than one; switch off restores the old layout.

**Known limits:** none listed.

---

## PR #6: Custom world viewport

**What it does.** Draws the 3D world in one rectangle of a big window (for
example the middle one of three monitors). The world, its HUDs and name tags
stay in the rectangle. Other windows can sit in the margins.

### Demo steps

- [ ] Three monitors in one window, world in the middle third:

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_VIEWPORT = "middle"
$env:AURORA_WINDOW_SIZE = "3840,720"
cargo run --release -p aurora-viewer -- --title "Test viewport"
```

- [ ] The world is in the middle third, the margins are dark, and the bars sit inside the rectangle.
- [ ] Drag Inventory or another window into a margin. It stays there and does not cover the world.
- [ ] Click in a margin: nothing is selected in the world.
- [ ] Name tags and HUDs stay inside the rectangle.
- [ ] Other values of `AURORA_DEMO_VIEWPORT` (change it, run again):
  - `1`: viewport on, left third of the window is a margin.
  - `bars-window`: the bars stay over the whole window.
  - `mouselook-full`: mouselook fills the whole window.
- [ ] Preferences › Layout › Bars and windows › **World viewport** holds the settings: Use a custom world viewport, Left/Right/Top/Bottom margin, Fit to a monitor, Keep the viewport in mouselook, Keep the bars inside the viewport.

### On the grid

- [ ] Stretch the window over your monitors. Preferences › Layout › Bars and windows › World viewport: turn on **Use a custom world viewport**, then press the **Fit to a monitor** button for the monitor you want. The margins are set for you.
- [ ] Enter mouselook: the world stays in the rectangle (unless **Keep the viewport in mouselook** is off).
- [ ] Compare the frame rate with the viewport on and off on the stretched window: only the rectangle is drawn now.
- [ ] Take a photo: it uses the whole window (expected).

**Pass:** world, HUDs and name tags stay in the rectangle; margins are dark; clicks in margins pick nothing; Fit to a monitor sets sensible margins.

**Known limits (PR body):** the login screen still uses the whole window. Wallpaper in the margins, the interactive diagram and saved layout profiles come later. Hidden-interface edge peeking uses the rectangle's edges when the bars are inside it. The PR's author could not see the rendering, so look closely at it.

---

## PR #7: Review bulk changes (rename and move with preview)

**What it does.** Rename or move 1 to 50 inventory items at once. You see
every change (before and after, and skipped items with the reason) before
anything is sent.

### Demo steps

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_INVENTORY = "bulk-review"
cargo run --release -p aurora-viewer -- --title "Test bulk-review"
```

(Also try `aurora-bulk-review`.)

- [ ] **Review bulk changes…** is open on three demo items, with the pattern `Sorted {n} - {name}`, already previewed.
- [ ] Check the before → after rows, the skipped rows with their reason, and the counts (requested, eligible, skipped).
- [ ] Edit the pattern. A new preview is required before Execute works.
- [ ] Press Preview, then **Execute**. The three items are renamed.
- [ ] Switch to Move, search for a folder, choose it, preview, and Execute.

### On the grid

- [ ] In your real inventory, select a few items (1 to 50). Right-click › **Review bulk changes…**.
- [ ] Try a rename with a pattern, preview, execute. Check the new names.
- [ ] Try a Move to a folder you pick by search.
- [ ] Rename one of the items elsewhere between Preview and Execute: nothing is sent and a new preview shows.

**Pass:** nothing changes before Execute; the rows match what happens; stale previews are caught.

**Known limits:** no session history or undo (planned, FEATURES.md 2.5). The "review the assistant's large batches" option comes with the local assistant later.

---

## PR #8: Restore previous look

**What it does.** After you replace your whole outfit, Appearance ⚙ ›
**Restore previous look** puts back what you wore before, in one step.

### Demo steps

There is no dedicated scenario. The PR suggests the gallery scenario:

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_APPEARANCE = "gallery"
cargo run --release -p aurora-viewer -- --title "Test previous-look"
```

- [ ] Wear another outfit (a replacement, not an add).
- [ ] Open the gear (⚙) in Appearance › **Restore previous look**. The first outfit is back, layers included.

### On the grid

- [ ] Wear an outfit, then wear another one. ⚙ › Restore previous look puts the first back.
- [ ] Only the last replacement is remembered. Adding items or Wear objects does not count.
- [ ] With an RLVa-locked attachment: Restore is refused with a message.
- [ ] Put an item of the old look in Trash, then try to restore: refused, with the item named.
- [ ] Log out and in: nothing is remembered.

**Pass:** one restore brings the old look back, once; refusals leave your outfit unchanged and say why.

**Known limits:** only the last replacement is remembered (no history, no redo). It lasts for this login only.

---

## PR #10: Local assistant (MCP) bridge, phase 1

Read-only access to your inventory for an AI assistant on your computer. It is
off by default. Everything is in [MCP-GUIDE.md](MCP-GUIDE.md).

- [ ] Quick demo check: `$env:AURORA_DEMO = "1"`, `$env:AURORA_DEMO_MCP = "1"`, `$env:AURORA_DEMO_OPTIONS = "privacy:local-assistant"`, then `cargo run --release -p aurora-viewer -- --title "Test mcp"`. Preferences opens on Privacy › Local assistant and shows "Ready on 127.0.0.1, port …".
- [ ] Then follow the MCP guide for the full test.

**Known limits (PR body):** release packaging does not ship `folderstorm-mcp` yet, and `cargo run -p aurora-viewer` alone does not build it (use `cargo build --workspace`). Ask is refused like Never until phase 2. Listing a folder that has not loaded does not start loading it.

---

## PR #11: Character profiles and background-safe GPU handling (session host checkpoint 1)

**What it does.** Run several viewers side by side, one per character
(`--profile <name>`), each with its own settings and cache. A viewer behind
another window draws little or nothing, and does not close because it thinks
the graphics card is gone.

### Demo steps

- [ ] Window behind another one, 3D off:

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_SESSION_BEHIND = "1"
cargo run --release -p aurora-viewer -- --title "Test behind" --profile Alt1
```

  The world stops drawing, is blurred and darkened, and an amber notice says "3D paused while another Folderstorm2 window is in front". The log says `3D paused: another Folderstorm2 window is in front`. Click the window: the world returns at once (`3D resumed`).
- [ ] Without `--profile` the value `1` follows the main profile (Normal): nothing pauses. Use `$env:AURORA_DEMO_SESSION_BEHIND = "off"`, `"limited"` or `"normal"` to force a policy for the run.
- [ ] Graphics lost while behind. Run with:

```powershell
$env:AURORA_DEMO = "1"
$env:AURORA_DEMO_SESSION_BEHIND = "off"
$env:AURORA_DEMO_GPU_LOST = "600"
cargo run --release -p aurora-viewer -- --title "Test gpu-lost"
```

  After about 10 seconds the log says the card is lost "while the window is not in front ...: the session goes on". No dialog, no close. Click the window (really focused): it recovers on a new window.
- [ ] Graphics stalled while behind:

```powershell
Remove-Item Env:AURORA_DEMO_GPU_LOST
$env:AURORA_DEMO_GPU_STALL = "600:20"
cargo run --release -p aurora-viewer -- --title "Test gpu-stall"
```

  After 6 seconds the log says "the graphics card does not answer while the window is not in front" and no box shows. Click while the stall lasts: the usual box appears.
- [ ] Preferences › Graphics › Frames per second › group **Other Folderstorm2 windows**: **3D while another Folderstorm2 window is in front** (Off / Limited / Normal), **Frame rate behind** (2 to 15, default 5) and, in a named profile, **Share the main profile's cache** (applies at the next start).

### On the grid (suggested steps from the PR)

- [ ] **Profiles.** Start three viewers: `--profile a`, `--profile b`, and none:

```powershell
cargo run --release -p aurora-viewer -- --profile a
cargo run --release -p aurora-viewer -- --profile b
cargo run --release -p aurora-viewer
```

  Each has its own settings and cache. Start `--profile a` a second time: it brings the first window forward. A bad name such as `../x` is refused with a message.
- [ ] **Off policy.** Put a named profile behind another window. It stops drawing, and picks up again when you bring it back.
- [ ] **Switching stress.** With two viewers logged in, minimise, restore and toggle full screen repeatedly. Neither closes or shows the GPU-lost dialog.
- [ ] **Real loss.** Run `AURORA_DEMO_GPU_HANG=900` in the front viewer (a real GPU hang that resets the driver, so close other GPU programs first) while the others are behind. The ones behind wait and recover when clicked.

**Pass:** profiles do not share settings or cache; a viewer behind pauses and resumes on click; a GPU loss behind never closes it.

**Known limits (PR body):**
- A loss behind waits until the window is clicked, for every profile (a change from the plan's R5, recorded in the plan).
- On Linux, link routing only reaches the main profile.
- The updater is not blocked while other profiles run.
- The crash restart path does not keep `--profile`.
- Moving the main cache deletes the old cache under a profile that shares it.
- Using `--title` and `--profile` together can give clashing log names.
- Pausing can take up to 2 s after the window goes behind.
- Muting sound and voice behind is not done yet.

---

## If something breaks

1. Note the PR the feature belongs to (the section titles above), and the exact steps and variables you used.
2. Look at the log. Each run writes its own file in the viewer's logs folder: on Windows, `logs` under the viewer's local data folder (`%LOCALAPPDATA%\Folderstorm\Folderstorm2\data\logs\`; `docs/TESTING.md` still names Aurora's folder, so check both). The name is `aurora-demo-<title>.log` in demo mode with `--title`, `aurora-<title>.log` otherwise, `aurora-<name>.log` for a profile. The previous run is `….previous.log`.
3. Send the log and the PR number. A fix goes on that PR's branch, not on `claude/test-all`.
4. To check whether a bug comes from the merge and not from one PR, check out that PR's branch (`claude/inventory-outfit-safety`, `claude/outfit-gallery-size`, `claude/world-viewport`, `claude/inventory-bulk-review`, `claude/previous-look`, `claude/mcp-bridge`, `claude/session-checkpoint-1`) and repeat the steps there.
