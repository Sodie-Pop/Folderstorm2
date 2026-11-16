# Handoff (2026-11-16)

State of the work when one long agent session ended, so the next one can pick
up without that session's private notes. Read this after
[AGENT_RULES.md](AGENT_RULES.md). Delete or update it once it is out of date.

## 1. Where things stand

- `main` is at `061cf52` (PR #55). Everything below is in open PRs, waiting for
  the human to test on Windows. **Merge only when the human says so.**
- **Test branch:** `claude/test-all-2026-11-15` merges every open PR, #54 to
  #75, so the human can build one thing. When a PR changes, merge it into this
  branch again. Don't run the full test suite for a test branch; the human
  builds it. A `cargo check` is fine.
- The upstream sync scheduled task is paused. Restart it, or sync by hand, once
  the PR backlog is merged.

## 2. Open PRs and stacks

Each stacked PR contains the one before it. Merge the stacks bottom-up; after
each merge, GitHub retargets the next PR (or change its base to `main`).

| Stack | PRs, bottom first |
|---|---|
| MCP | #56 task scripts → #58 script testing → #61 building → #67 gestures and posing → #70 camera read and uploads (#70 also contains #65) |
| Workspaces | #57 fixes → #62 checkpoint 6 part 1 → #64 part 2 (folder windows) → #66 favorites strip (1.6) → #72 context: graphics, camera, HUDs (1.7) → #73 backgrounds (1.8) |
| Inventory | #74 custom tabs scoped to folders (2.2) → #75 quick access (2.13) |

Single PRs on `main`: #54 (MCP script read-back), #59 (build tools in the
world rect), #60 (monitor chat), #63 (rez override flags), #65 (folder upload,
2.11), #68 (gesture boards, 3.1), #69 (settings search keywords, 1.9), #71
(docs only: FEATURES shelving).

- #54 is 37 commits behind `main`, and #57 is 2 behind: merge `main` into them
  if GitHub reports conflicts.
- Each PR's "Limits and follow-up" section lists its open questions.

## 3. Decisions waiting for the human

- **#72 (1.7) HUDs:** switching workspaces takes off the HUDs the previous
  workspace put on, but no other worn HUDs. Only a workspace with its own HUD
  list removes HUDs, and one global "keep HUDs" setting turns this off.
- **#73 (1.8):** a background per layout plus a default; local pictures only.
- **#74 (2.2):** adding folders to a tab doesn't switch to that tab; the
  confirmation shows in the Inventory's status line.
- **#75 (2.13):**
  - Pin means "always on top".
  - Adding an item to a tab that showed everything narrows the tab to what was
    added.
  - Tab windows don't reopen at login.
  - It gets its own FEATURES row, 2.13.
- **#65, #66, #68, #70:** see each PR's "Limits and follow-up" section.
- #75's description and #72's and #74's still need their new behaviour and test
  steps added (the GitHub tools failed at the end of the session).

## 4. Decisions the human already made (keep to them)

- **MCP:**
  - Touching an object never asks when the class is set to Allow. When it is
    set to Ask, it asks per object, with "Allow once" and "Allow this session".
  - Next-owner permission changes have their own permission class.
  - A new class starts at Ask only if every other change already asks.
  - Anything that costs L$ always asks.
  - Never expose asset ids.
  - **Group 11 (social and events) is shelved**, maybe one day.
  - The 8 shared-pose tools were dropped.
- **FEATURES:**
  - 3.2 inline image previews: not porting (Aurora has them).
  - Shelved (⏸): 3.4 GIF board and 2.10 landmark folders. Upstream has
    something like 2.10 that the human wants to test first.
- **Layouts** keep "Photo mode uses the whole window".
- **Gesture boards (3.1):** sounds and gestures play publicly.
- **Session host** (most already built): see
  [plans/session-host.md](plans/session-host.md).
  - Dropped: session sets, a free colour picker, "quit all and update", and
    selective groups in profile copy.
  - Deferred to the end of the project: the host adopting viewers that are
    already running, and the bird's-eye switch effect.

## 5. What to do next

1. When the human reports test results, fix what they found on the PR's
   branch, then merge into the test branch again.
2. **Follow-ups:**
   - After #63 merges, #61's next-owner patch should also set
     `rez_overrides::flags` and accept object items.
   - #69 adds `fs-search-*` keyword entries. The settings rows added by #66,
     #72, #73 and the 1.7 "keep HUDs" switch still use plain English
     `cx.keywords`: convert them once #69 is merged.
   - Posing follow-ups from #59: the gizmo and look-at.
   - A separate "Surroundings" permission class for the MCP.
   - Outfit gallery thumbnail size slider: lost in upstream's gallery rework;
     find it or restore it.
3. **MCP, not started:**
   - Group 10: viewer preferences and workspaces (11 tools).
   - The remaining session-routing and host tools (7).
   - The full checklist is [plans/mcp-parity.md](plans/mcp-parity.md); its
     counts are as of 2026-11-15.
4. **Features not started:** see the ⬜ rows of [FEATURES.md](FEATURES.md).
   Ask the human which comes next.

## 6. Working notes

- The human builds and tests on Windows. Keep messages short: what is done,
  what is left, what they have to do.
- **Subagents:** the human prefers Opus at high effort, or medium for simple
  work; Sonnet for searches and routine edits (AGENT_RULES § 7). Pass the
  effort level explicitly.
- **Several agents in parallel:** give each its own git worktree, and run every
  `cargo` command under one shared lock (`flock <lockfile> cargo …`) so builds
  don't fight over the target directory.
- **Known test failures in a cloud container:**
  - `world::chat_log::import::tests::a_failure_leaves_the_file_as_it_was`
    fails only when run as root.
  - The `aurora-render` GPU terrain test fails on a software Vulkan driver.
  - Neither happens in CI.
- **CI:** a "tests (Windows, PR)" run whose parts show "cancelled" was replaced
  by a newer push. It is not a real failure.
- If a branch push fails with HTTP 403, the session's GitHub account has no
  write access to the fork. The human reconnects GitHub with the fork owner's
  account.
