# Follow-ups

Things to come back to that are not features (those are in
[FEATURES.md](FEATURES.md)).

| Since | What | Why | Next step |
|---|---|---|---|
| 2026-11-11 | **Photo LUT packs** by Czar Fenris and Nanner Pudding (`assets/photo-luts/`, from upstream Aurora) are left out of Folderstorm2's builds (`crates/aurora-viewer/src/snapshot/lut_packs.rs`, `// Folderstorm:`). | Both creators agreed on 2026-11-10 that *Aurora* includes and shares them; it is not clear that covers a fork. | The human asks each creator whether Folderstorm2 may ship the pack, credited by name. If yes, remove the hook for that pack. The files stay in the repository (upstream's), so syncs do not conflict. |
