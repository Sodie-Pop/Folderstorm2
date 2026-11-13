# Follow-ups

Things to come back to that are not features (those are in
[FEATURES.md](FEATURES.md)).

| Since | What | Why | Next step |
|---|---|---|---|
| 2026-11-11 | **Photo LUT packs** by Czar Fenris and Nanner Pudding (`assets/photo-luts/`, from upstream Aurora) are left out of Folderstorm2's builds (`crates/aurora-viewer/src/snapshot/lut_packs.rs`, `// Folderstorm:`). | Both creators agreed on 2026-11-10 that *Aurora* includes and shares them; it is not clear that covers a fork. | The human asks each creator whether Folderstorm2 may ship the pack, credited by name. If yes, remove the hook for that pack. The files stay in the repository (upstream's), so syncs do not conflict. |
| 2026-11-12 | **The session host's "Updates and support" settings section** points to the viewer's own Preferences ("Language and updates", "Privacy") and Help menu (`crates/aurora-viewer/src/folderstorm_characters/host/ui/settings.rs`, `updates_settings`). | Decided by the human for checkpoint 10: one place for those settings while the fork uses upstream's updater, crash reports and bug reports. | When Folderstorm2 has its own updater, crash reporting or bug tracker, change what this section says (and where it points) to match. |
