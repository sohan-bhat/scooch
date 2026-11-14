# Scooch

Moves the desktop wallpaper so icons do not cover the subject of the picture. macOS, Windows and Linux.

![The same desktop before and after](docs/demo.jpg)

## Install

macOS and Linux

```sh
curl -LsSf https://sohanbhat.me/scooch/install.sh | sh
```

Windows

```powershell
irm https://sohanbhat.me/scooch/install.ps1 | iex
```

Then run `scooch`.

## Commands

| Command | What happens |
|---|---|
| `scooch` | Turns Scooch on. Run it again for status. |
| `scooch use` | Picks a different picture. |
| `scooch flip` | Allows flipping the picture when that clears the icons. |
| `scooch off` | Restores the wallpaper and removes the login item. Desktops and screens that are not showing are restored the next time they show. |
| `scooch doctor` | Checks that the desktop can be read. |

## How it works

1. Find the subject with a saliency map from color and contrast. No model files.
2. Read the icon rectangles from the desktop.
3. Search crop, pan and a per-axis warp that only stretches columns and rows with no subject in them. Zoom is used only when a slide cannot clear the icons.
4. Render one image and set it as the normal wallpaper.

One background process does this. It is event driven with no timer and no window, so it uses no CPU between changes. The engine, the daemon and each desktop adapter are described in [docs/DESIGN.md](docs/DESIGN.md).

## Status

| System | Tested on |
|---|---|
| macOS | A real Mac on macOS 15.5. GitHub runners on macOS 15 for Apple silicon and Intel, with the login item and two Spaces |
| Windows | GitHub runners on Windows 11 build 26100 and on Windows Server 2022, which has the Windows 10 desktop, at 100% and 150% scaling |
| Linux | Real sessions of GNOME 46 with Desktop Icons NG, KDE Plasma 5.27, 6.7 and 6.8, Xfce 4.18 and 4.20, Cinnamon 6.0 and 6.8, MATE 1.26 and 1.28. Two monitors on Plasma and GNOME |

More than one monitor on macOS and Windows is not yet tested. On Plasma 6.8 under Wayland only reading the icons and setting the picture are tested.

## Limits

- A subject that touches the screen edge under the icons cannot slide clear, because there is no picture beyond that edge. `scooch flip` clears it by mirroring.
- The wallpaper must be a picture file. A slideshow, a video wallpaper or a plain color is left alone until a picture is picked with `scooch use`.
- macOS asks once for permission to read icon positions from Finder. Colors outside sRGB lose a little saturation in the rendered picture.
- GNOME, Cinnamon and MATE use one picture for every monitor, so it is arranged for the main one.
- On Linux a moved icon reaches Scooch when the desktop saves its positions, which takes 1 to 3 seconds and up to 10 on KDE.
- On Windows a change of icon size, or icons moved by another program, is not noticed until the next drag or click on the desktop.

## Build

```sh
cargo install --path crates/scooch
```

To remove it, run `scooch off` and delete the program.

MIT license. Test photos and their sources are in [assets/photos/SOURCES.md](assets/photos/SOURCES.md).

Built by [Sohan Bhat](https://sohanbhat.me/).
