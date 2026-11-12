# Scooch design

Scooch moves your wallpaper so your desktop icons never cover the best part of the picture. It works with any image, uses plain image math with no AI, and draws nothing between changes.

## Rules

1. **Nothing runs at rest.** Scooch has no window, no render loop and no timer. One small process sleeps on operating system events. It wakes when an icon moves, a file lands on the desktop, a screen changes or the wallpaper changes, writes one image, hands the image to the system as a normal wallpaper and goes back to sleep.
2. **Any wallpaper.** The picture is the user's own. Scooch reframes and gently reshapes that picture. It never swaps in its own art.
3. **No AI.** Subject detection is classic image processing. No model files, no network.
4. **One codebase.** macOS, Windows and Linux share the engine and the daemon. Only the desktop adapter differs.
5. **Leave no trace.** `scooch off` restores the original wallpaper and removes the login item.

## Layout

| Path | Holds |
|---|---|
| `crates/scooch-core` | The engine. Pure Rust, no platform code. Analyze, solve, render, encode. |
| `crates/scooch` | The `scooch` command, the daemon loop, state, and `platform/` adapters. |
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

Runs on a copy scaled so the long side is 320 pixels, in a perceptual color space.

- **Saliency.** Frequency-tuned saliency, the distance between each lightly blurred pixel and the mean color, averaged with center-surround contrast at three scales. Smoothed and scaled so the 99th percentile is 1.
- **Busyness.** Gradient magnitude of lightness, blurred, scaled the same way. A busy area hurts label legibility even when it is not the subject.
- **Subject.** The weighted centroid and spread of the saliency map.

The result is cached on disk by a hash of the image bytes.

### Solve

Finds where the picture should sit. All sums use summed-area tables on the 320 pixel maps, so a solve takes a few milliseconds.

**Stage 1, crop.** Search over zoom from 1.0 to `max_zoom`, pan on a grid, and mirror when allowed. The cost is

```text
cost = a * saliency hidden under icons, as a share of visible saliency
     + b * busyness under icons
     + c * share of total saliency cropped out of frame
     + d * (zoom - 1)
     + e * distance of the crop center from the natural center
     + f * distance from the previous crop
     + g * mirrored
```

With no icons, or icons that cover nothing important, the natural centered crop wins and the output equals a normal fill wallpaper.

**Stage 2, axis warp.** Columns and rows that hold nothing important can stretch or squeeze, so the subject slides further than panning alone allows. Straight horizontal and vertical lines stay straight. For each axis, find the band holding most of the importance, then search a shift `d` for that band. The elastic bins on one side stretch and the bins on the other side squeeze, each in proportion to its elasticity, and every local scale stays inside `1 - 0.4 * strength` to `1 + 0.6 * strength`. Pick the `d` with the least importance under icons plus a distortion penalty. Skip the warp when the gain is small.

**Stability.** Keep the previous placement unless the new one is clearly better, so a small nudge of one icon does not move the picture.

A `Placement` is separable. `fx` and `fy` are tables of 257 values. `fx[k]` is the source x, from 0 to 1, shown at output x `k / 256`. Mirror and crop are folded into the tables. Both tables are monotone and smooth.

### Render

For each output pixel, look up the source position through `fx` and `fy` and sample in linear light. Shrink the source with an area filter first when it is much larger than the screen. Rows render in parallel.

**Pads.** Under each icon, where the picture is busy, blend toward a blurred copy so labels stay readable. The mask is a feathered union of the icon rectangles. Where the picture is already calm the pad does nothing.

Targets on a four core laptop from 2018 are under 120 ms for 2880 by 1800 and under 400 ms for 5120 by 2880, decode not included.

## Desktop adapter

```text
snapshot()                    -> screens and icon rectangles, in global logical coordinates
wallpaper(screen)             -> path of the image the system shows now
set_wallpaper(screen, path)   -> show this image, filling the screen
run(wake)                     -> block, calling wake() when anything may have changed
```

`run` must be event driven. Polling on a timer breaks rule 1. Every method is safe to call from any thread.

| System | Icons from | Wallpaper through | Wakes on |
|---|---|---|---|
| macOS | Finder, by Apple Events | NSWorkspace | Desktop folder events, mouse up over the desktop, screen and space changes |
| Windows | The shell's IFolderView | IDesktopWallpaper | Shell change notifications, WinEvents from the desktop list, display changes |
| Linux | The desktop's own files, per desktop | gsettings, xfconf or Plasma | inotify on those files |

## Daemon

On each wake, after 250 ms of quiet, for each screen

1. Read the current wallpaper. If it is not a Scooch output and differs from the recorded source, the user picked a new picture, so adopt it as the source.
2. Skip when the source, screen, icons and options match the last render.
3. Analyze from cache, solve, render, write `out/<screen>-<n>` and set it. Delete older outputs.

State lives in the per-user data folder as `state.json`, with the source per screen, the last placement, and the wallpaper to restore.

## Command

| Command | Does |
|---|---|
| `scooch` | Sets everything up on first run, then shows status |
| `scooch use <image>` | Switches the picture |
| `scooch off` | Stops, removes the login item and restores the wallpaper |
| `scooch status` | Shows whether Scooch is on and what it last did |
| `scooch render` | Renders one image from a picture and a list of rectangles, for tests and the website |
| `scooch watch` | Runs the daemon in the foreground. The login item runs this |
| `scooch doctor` | Checks permissions and the desktop adapter |

## Code rules

- No comments in code. Names and small functions carry the meaning.
- Commit messages are at most about six words, with no body.
- Every platform claim is tested on that platform, on a real session in CI or on a real machine.
