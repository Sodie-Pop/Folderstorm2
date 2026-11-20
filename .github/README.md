<div align="center">

<img src="../folderstorm/branding/folderstorm2-logo.png" alt="The Folderstorm2 logo: a winged folder with eyes" width="160" />

# Folderstorm2

**A Second Life viewer in Rust and Vulkan, with the Folderstorm features.**

</div>

Folderstorm2 is a viewer for Second Life and OpenSim built on
[Aurora Viewer](https://github.com/Aurora-Viewer/Aurora-Viewer) (Rust, Vulkan).
It brings over the features of Folderstorm (C++), a fork of the Legacy Viewer
made for people with large inventories.

## Highlights

- **Custom world viewport**: spread the window over several monitors and keep
  the 3D world in one rectangle, with the rest of the space for your windows.
- **Jump to folder**: go to an Inventory folder by typing its name or path.
- **Bulk review**: preview a rename pattern or a move for up to 50 selected
  items, with exact before and after rows, before anything changes.
- **Outfit safety**: Wear is off by default in the context menus, and
  replacing or adding an outfit always asks first.
- **Restore previous look**: one action brings back what you wore before you
  replaced your outfit with a folder.
- **Outfit Gallery size**: a slider for the size of the photos.
- **Local assistant bridge** (off by default): lets an AI assistant on your own
  computer (Claude Code, Cursor, Codex) search and tidy your inventory and
  outfits, with permission levels and prompts before changes.

Everything else Aurora Viewer offers is still there.

## Status

In development, and a personal project: expect rough edges.

## Installing

Windows 10 or 11, 64-bit. From the
[Releases page](https://github.com/Sodie-Pop/Folderstorm2/releases),
download the setup or the zip:

- **`Folderstorm2-v…-windows-x64-setup.exe`**: installs for your account
  only, with no administrator rights, in `%LOCALAPPDATA%\Programs\Folderstorm2`.
  It adds **Folderstorm2** and **Folderstorm2 Session Host** to the Start
  menu, and optionally desktop shortcuts and `secondlife://` links. Uninstall
  it from Settings › Apps; it asks whether to keep your settings and saved
  passwords (kept by default).
- **`Folderstorm2-v…-windows-x64.zip`**: unzip it in a folder you can write
  to (not `C:\Program Files`) and start `Folderstorm2.exe`.

Either way, the viewer then updates itself from new releases. Builds are not
code-signed yet: Windows may show "Windows protected your PC" (More info ›
Run anyway).

## Report a bug

Use **Report a bug** in the viewer (login screen, top bar or Help menu), or go
to [folderstorm.com/bugs](https://folderstorm.com/bugs/).

## Links

- [Website](https://folderstorm.com)
- [Help](https://folderstorm.com/help/)
- [Privacy policy](https://folderstorm.com/privacy)
- [Report a bug](https://folderstorm.com/bugs/)

## Building

You need [Rust](https://rustup.rs) (rustup installs the version that
`rust-toolchain.toml` names) and a GPU with Vulkan support.

```sh
git clone https://github.com/Sodie-Pop/Folderstorm2
cd Folderstorm2
cargo build --release -p aurora-viewer
```

The program is `target/release/aurora-viewer` (`.exe` on Windows); the
executable keeps that internal name. More on prerequisites and tooling:
[Linux](../docs/LINUX.md#building-from-source),
[macOS](../docs/MACOS.md#building-from-source) and the
[build section of the main README](../README.md#build).

## Credits and license

Folderstorm2 is a modified version of
[Aurora Viewer](https://github.com/Aurora-Viewer/Aurora-Viewer), by the Aurora
Viewer contributors. Much of its code is ported from the Second Life viewer
(Linden Research, Inc.) and the Firestorm viewer (The Phoenix Firestorm
Project, Inc.). It is free software under the
**GNU General Public License, version 3 or later**: see [LICENSE](../LICENSE)
and [NOTICE.md](../NOTICE.md), which lists the copyright notices and third-party
credits. [TRADEMARKS.md](../TRADEMARKS.md) explains why this build uses its own
name and logo.

Folderstorm2 is not provided, endorsed or supported by Linden Research, Inc.,
The Phoenix Firestorm Project, Inc. or the Aurora Viewer team. Second Life is
a trademark of Linden Research, Inc.

## More

- [Features being ported](../folderstorm/FEATURES.md) and their status
- [Branding](../folderstorm/BRANDING.md), [CI](../folderstorm/CI.md)
- [Testing and demo mode](../docs/TESTING.md), [translating](../docs/TRANSLATING.md)
