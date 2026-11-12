# Scooch

Scooch moves your wallpaper so your desktop icons stay off the best part of the picture.

![The same desktop before and after Scooch](docs/demo.jpg)

It works with the picture you already have and uses plain image math with no AI. Between changes it draws nothing and uses no CPU.

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

## Use

| Command | What happens |
|---|---|
| `scooch` | Turns Scooch on. Run it again to see how it is doing. |
| `scooch use` | Picks a different picture. |
| `scooch flip` | Lets Scooch flip the picture when that clears your icons. |
| `scooch off` | Puts your wallpaper back and removes the login item. |
| `scooch doctor` | Checks that Scooch can read your desktop. |

Move an icon and the wallpaper follows about half a second later.

## How it works

1. Scooch finds the subject of the picture from color and contrast.
2. It reads where your icons sit.
3. It slides the picture, and stretches plain background a little, until the subject is clear. It zooms only when a slide is not enough.
4. It saves one image and sets that image as your normal wallpaper.

One small background program does this. It runs only when an icon moves, a file lands on the desktop, a screen changes or you pick a new wallpaper. The full design is in [docs/DESIGN.md](docs/DESIGN.md).

## Where it runs

| System | Status |
|---|---|
| macOS 14 and 15 | Tested on a real Mac |
| Windows 10 and 11 | Written, not yet run on Windows |
| Linux with GNOME, KDE Plasma or Xfce | Written, not yet run on Linux |

## Limits

- A subject that touches the screen edge under your icons cannot slide clear, because there is no picture beyond that edge. `scooch flip` lets Scooch flip the picture, which clears it.
- Scooch needs a picture file. With a slideshow, a video wallpaper or a plain color it leaves your wallpaper alone until you pick a picture with `scooch use`.
- On macOS the first run asks once for permission to read icon positions from Finder.
- On GNOME one picture covers every monitor, so Scooch arranges it for the main one.
- Cinnamon and MATE are not supported yet.

## Remove

Run `scooch off`, then delete the `scooch` program. `command -v scooch` shows where it is on macOS and Linux. On Windows it is in `%LOCALAPPDATA%\Programs\Scooch`.

## Build from source

```sh
cargo install --path crates/scooch
```

## License

MIT. The test photos and their sources are listed in [assets/photos/SOURCES.md](assets/photos/SOURCES.md).
