# Scooch design

Scooch moves your wallpaper so your desktop icons stay off the best part of the picture. It works with any image, uses plain image math with no AI, and draws nothing between changes.

## Rules

1. **Nothing runs at rest.** Scooch has no window, no render loop and no timer. One small process stays idle until the operating system reports a change. An icon moved, a file landed on the desktop, a screen changed or the wallpaper changed. Scooch then writes one image, hands that image to the system as a normal wallpaper and goes idle again.
2. **Any wallpaper.** The picture is the user's own. Scooch reframes and gently reshapes that picture. It never swaps in its own art.
3. **No AI.** Subject detection is classic image processing. No model files, no network.
4. **One codebase.** macOS, Windows and Linux share the engine and the daemon. Only the desktop adapter differs.
5. **Leave no trace.** `scooch off` restores the original wallpaper and every setting Scooch changed, and removes the login item.
6. **Do no harm.** When moving the picture would not help, the picture stays exactly as the system would show it.

## Layout

| Path | Holds |
|---|---|
| `crates/scooch-core` | The engine. Pure Rust, no platform code. Analyze, solve, render, encode. |
| `crates/scooch` | The `scooch` command, the daemon loop, state, and the `platform/` adapters. |
| `install` | The install scripts the website serves. |
| `site` | The website. |

## Engine

The engine is three pure functions.

```text
analyze(image)                         -> Analysis
solve(analysis, scene, options, prev)  -> Placement
render(image, placement, scene, opts)  -> RgbImage
```

A `Scene` is one screen. It holds the screen size in pixels and the icon rectangles in that screen's pixels, with the origin at the top left. An icon rectangle covers the icon and its label.

### Analyze

Analysis runs on a copy scaled so the long side is 320 pixels, in the OKLab color space.

- **Saliency.** Two measures are averaged. The first is how far each pixel's color sits from a smooth fit of the background. The second is center-surround contrast at three scales. The result is smoothed, and a floor is removed so calm areas are exactly zero.
- **Subject fill.** A wide blur of the saliency map marks the inside of the subject and a narrow margin around it, so a softly lit face between two bright areas still counts as subject.
- **Busyness.** Gradient magnitude of lightness, blurred, on an absolute scale. A busy area hurts label legibility even when it is not the subject.
- **Subject box.** The weighted centroid and spread of the saliency map.

The result is cached on disk by a hash of the image bytes.

### Solve

All sums use summed-area tables on the 320 pixel maps, so a solve takes about ten milliseconds.

**Crop.** Search over zoom from 1.0 to `max_zoom`, over pan on a grid, and over mirror when the user allows it. The cost of a crop is

```text
cost = 4.0  * saliency hidden under icons, as a share of all saliency
     + 0.35 * busyness under icons
     + 1.5  * saliency the centered crop shows that this crop does not
     + 2.0  * subject newly cut by a crop edge
     + 0.6  * (zoom - 1)
     + 0.15 * distance of the crop from center
     + 0.25 if mirrored
```

**Axis warp.** Columns and rows that hold no subject can stretch or squeeze, so the subject slides without any zoom. Straight horizontal and vertical lines stay straight. For each axis, the bins that hold the subject form a rigid core. Bins outside the core are flexible, less so where the picture is busy. A shift moves the core, stretching flexible bins on one side by up to 45% and squeezing those on the other side by up to 35%. Each shift pays a distortion cost.

**One decision.** The best crop for every zoom level is warped on both axes, and all results are scored with one function. The lowest total wins. On a soft background the subject slides at zoom 1.0. Zoom appears only when a slide cannot clear the icons.

**Never worse.** The result never hides more of the subject than the plain centered fill. With no icons, or icons that cover nothing important, the output equals a normal fill wallpaper.

**Stability.** The previous placement is kept unless the new one is clearly better, so a small nudge of one icon does not move the picture.

A `Placement` is separable. `fx` and `fy` are tables of 257 values. `fx[k]` is the source x, from 0 to 1, shown at output x `k / 256`. Mirror and crop are folded into the tables. Both tables are monotone and smooth.

### Render

For each output pixel, the renderer looks up the source position through `fx` and `fy` and samples in linear light with a Catmull-Rom filter. A source much larger than the screen is shrunk first with an area filter. Rows render in parallel.

**Pads.** Under each icon, where the picture is busy, the renderer blends toward a blurred copy so labels stay readable. The mask is a feathered union of the icon rectangles. Pads stay off the subject, and where the picture is already calm they do nothing.

Measured on a six core laptop from 2018, a 2880 by 1800 render takes about 95 ms and a 5120 by 2880 render about 230 ms, decode not included.

### What the engine cannot do

A subject that touches the screen edge under the icons cannot slide clear on a screen with the same shape as the picture, because there is no picture beyond that edge to fill in. Flipping the picture clears it. Flipping is off by default, since a flipped photo of a known place or person looks wrong. `scooch flip` turns it on, and the status line says when it would help.

## Desktop adapter

```text
snapshot()                    -> screens and icon rectangles, in global logical coordinates
wallpaper(screen)             -> path of the picture file the system shows now, or none
set_wallpaper(screen, path)   -> show this image, filling the screen
run(wake)                     -> block, calling wake() when anything may have changed
autostart(exe or none)        -> add or remove the login item
```

`run` must be event driven. Polling on a timer breaks rule 1. Every method is safe to call from any thread. `wallpaper` returns none when the system shows something that is not one picture file, such as a slideshow or a video wallpaper, and Scooch then leaves the wallpaper alone.

| System | Icons from | Wallpaper through | Changes arrive from |
|---|---|---|---|
| macOS | Finder, by Apple Events | NSWorkspace | Desktop folder events, mouse up over the desktop, the wallpaper store, screen and space changes |
| Windows | The shell's IFolderView | IDesktopWallpaper | Shell change notifications, WinEvents from the desktop list, setting and display messages |
| GNOME | Desktop Icons NG positions in gvfs metadata | gsettings | The gvfs metadata signal, the dconf signal, the Desktop folder |
| KDE Plasma | Folder view positions in the applet config | Plasma scripting | The config folder, the Desktop folder |
| Xfce | The xfdesktop positions file | xfconf | The positions folder, the xfconf signal, the Desktop folder |

Windows builds two programs from the same code. `scooch.exe` is the console command. `scoochw.exe` is the same program with no console, which is what runs at login. No code is injected into Explorer and no input is hooked.

Linux uses pure Rust libraries only, so one static program runs on every distribution. Cinnamon and MATE are not supported yet.

## Daemon

On each change, after 250 ms of quiet, for each screen

1. Read the current wallpaper. If it is a picture file that Scooch did not write and it differs from the recorded source, the user picked a new picture, so adopt it as the source.
2. Skip when the source, screen, icons and options match the last render.
3. Take the analysis from cache, solve, render at most once, write `out/<screen>-<n>` and set it. Delete older outputs.

State lives in the per-user data folder as `state.json`, with the source per screen, the last placement, and the wallpaper to restore. The file is written to a temporary name and renamed, so a crash never leaves half a file. One daemon runs at a time, held by a lock file.

Measured on macOS at rest, the daemon uses 0.01 s of CPU over two minutes, has no idle wakeups and holds about 5 to 7 MB. A moved icon becomes a new wallpaper in about half a second, of which 250 ms is the quiet period.

## Command

| Command | Does |
|---|---|
| `scooch` | Turns Scooch on, then shows status on later runs |
| `scooch use [image]` | Switches the picture, with a picker when no image is given |
| `scooch flip` | Lets Scooch flip the picture when that clears the icons, or stops it |
| `scooch off` | Stops, removes the login item and restores the wallpaper |
| `scooch status` | Shows whether Scooch is on and what it shows |
| `scooch doctor` | Checks permissions and the desktop adapter |
| `scooch render` | Renders one image from a picture and a list of rectangles, for tests and the website |
| `scooch watch` | Runs the daemon in the foreground. The login item runs this |

## Code rules

- No comments in code. Names and small functions carry the meaning.
- Commit messages are at most about six words, with no body.
- Every platform claim is tested on that platform, on a real session in CI or on a real machine. Until it is, the claim is listed as unproven in the README.
