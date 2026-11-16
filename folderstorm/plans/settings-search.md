# Settings search keywords

FEATURES.md row 1.9. The Preferences search finds the Folderstorm settings
by words that are not in their labels or help: "MCP", "dual monitor",
"workspace", "undo", as Folderstorm (C++) did with its panels' search words
(`doc/help.md`).

## 1. What Aurora has

The search of the Preferences window (`ui/options/cx.rs`) finds a row when
every typed word starts a word of its label, its help or its **keywords**
(`Cx::keywords`, set just before the row). Text is folded (lower case, no
accents); words are runs of letters and digits, so "multi-monitor" is two
words, and a Japanese run without spaces is one word found by its start.
Group titles are not searched. Upstream keywords are mostly untranslated
setting names (`RenderVolumeSAProtection`) or translated choice labels.

## 2. What we add

- `ui/options/pages/folderstorm_search.rs`: `words(cx, &[keys])` adds the
  words of catalogue entries `fs-search-*` to the next row through
  `Cx::keywords`, only while a search is under way (like upstream's costly
  keywords). No fork of the mechanism.
- **Languages.** Each entry has English, French and Japanese words. The
  search reads them in the interface's language **and always in English
  too**: the fork's own names (MCP, workspace, layout) are typed in English
  whatever the language, and its docs are in English. Japanese entries put
  spaces between words so that each is found by its start.
- The Folderstorm pages name their entries at the rows (local assistant,
  world viewport, layouts, other windows of the viewer, image uploads). The
  layouts page's former English-only keywords became entries.
- Upstream hooks, one line each, marked `// Folderstorm:`: the `mod` line in
  `pages.rs`; `pages/shortcuts.rs`, so that the five *Layouts* keyboard
  actions are found as the layouts are; `pages/inventory.rs`, on the outfit
  safety row (already ours, under a Folderstorm comment).

## 3. What "undo" finds

*Previous arrangement* (Preferences › Layout › Layouts), the one Folderstorm
setting that undoes something. Inventory history and its undo (row 2.5) are
not on `main`; the local assistant's receipts are listed under its
*Activity* row (found by "history", "receipts", "activity") but cannot be
undone. When row 2.5 lands, its Preferences rows take "undo" too.

## 4. Not covered

- Rows that are not in Preferences: the Outfit Gallery's thumbnail size
  (Appearance window menu), the bulk review (Inventory gear menu), the
  session host's settings (its own window).
- Rows added later need their entries: each new Folderstorm row in
  Preferences names a `fs-search-*` entry when its label and help miss the
  words users type.

## 5. Tests

`folderstorm_search::tests`: every entry in every catalogue; each word
found in its language (en, fr, ja); the English words found in every
language; and the whole Preferences search (Count pass) finding the rows on
their pages by English words, whatever language the tests run in. Demo:
`AURORA_DEMO_OPTIONS="layout:search=dual monitor"` (`docs/TESTING.md`).
