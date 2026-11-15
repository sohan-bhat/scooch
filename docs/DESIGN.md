# Scooch design

Scooch moves your wallpaper so your desktop icons stay off the best part of the picture. It works with any image, uses plain image math with no AI, and draws nothing between changes.

## Rules

1. **Nothing runs at rest.** Scooch has no window, no render loop and no timer. One small process stays idle until the operating system reports a change. An icon moved, a file landed on the desktop, a screen changed or the wallpaper changed. Scooch then writes one image, hands that image to the system as a normal wallpaper and goes idle again. When a move glides, a helper process holds a window and a GPU device and draws the frames. A wake can start the helper early with only its device. The helper exits 30 s after its last command, or 1.5 s after an early start when nothing moved. Until the helper is gone and the daemon has freed its held render data, the daemon checks every 2 s.
2. **Any wallpaper.** The picture is the user's own. Scooch reframes and gently reshapes that picture. It never swaps in its own art.
3. **No AI.** Subject detection is classic image processing. No model files, no network.
4. **One codebase.** macOS, Windows and Linux share the engine and the daemon. Only the desktop adapter and the layer that shows a glide differ.
5. **Leave no trace.** `scooch off` restores the original wallpaper and every setting Scooch changed, and removes the login item. A desktop or screen that is not showing at that moment is restored the next time it shows, and Scooch says so.
6. **Do no harm.** When moving the picture would not help, the picture stays exactly as the system would show it.

## Layout

| Path | Holds |
|---|---|
| `crates/scooch-core` | The engine. Pure Rust, no platform code. Analyze, solve, render, encode. |
| `crates/scooch` | The `scooch` command, the daemon loop, the live helper, state, and the `platform/` adapters. |
| `crates/scooch-live` | The GPU glide the helper draws with, and the mip levels the daemon stores for it. |
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
| Windows | The shell's IFolderView, read on one single threaded apartment thread | IDesktopWallpaper, as PNG | Shell change notifications, capture end on the desktop thread, create through hide on the icon list, setting and display messages |
| GNOME | Desktop Icons NG positions in gvfs metadata | gsettings | The gvfs metadata signal, the dconf signal, the Desktop folder |
| KDE Plasma | Folder view positions in the applet config | Plasma scripting | The config folder, the Desktop folder |
| Xfce | The xfdesktop positions file | xfconf | The positions folder, the xfconf signal, the Desktop folder |
| Cinnamon | Nemo positions in gvfs metadata, on Nemo's desktop grid | gsettings | The gvfs metadata signal, the dconf signal, Nemo's desktop file, the Desktop folder |
| MATE | Caja positions in gvfs metadata | gsettings | The gvfs metadata signal, the dconf signal, Caja's desktop file, the Desktop folder |

Windows builds two programs from the same code. `scooch.exe` is the console command. `scoochw.exe` is the same program with no console, which is what runs at login. No code is injected into Explorer and no input is hooked. Scooch does not run as administrator while Explorer runs as a normal user, because Windows would then block Explorer's messages to it.

On macOS every picture is decoded with its color profile and matched into sRGB, and outputs carry the sRGB profile. Colors outside sRGB are clipped.

Linux uses pure Rust libraries only, so one static program runs on every distribution. On KDE an icon move can take up to 10 seconds to arrive, because Plasma saves icon positions on a 10 second timer and sends no signal for a move. On GNOME, Cinnamon and MATE one picture covers every monitor, so Scooch arranges it for the main one.

## Daemon

On each change, after 250 ms of quiet, for each screen

1. Read the current wallpaper. If it is a picture file that Scooch did not write and it differs from the recorded source, the user picked a new picture, so adopt it as the source.
2. Skip when the source, screen, icons and options match the last render.
3. Take the analysis from cache, solve, render at most once, write `out/<screen>-<n>` and set it. Delete older outputs.

On macOS a mouse up over the desktop with Finder in front gets a second look after 60 ms, and when that look shows icons moved the tick runs at once.

State lives in the per-user data folder as `state.json`, with the source per screen, the last placement, and the wallpaper to restore. The file is written to a temporary name and renamed, so a crash never leaves half a file. A file that cannot be read is moved aside and never overwritten. One daemon runs at a time, held by a lock file.

A screen entry belongs to one desktop. On macOS that is one display on one Space, because macOS keeps a wallpaper for each and an app can only set the one that is showing. Each entry has its own source, original and output files.

**Off.** `scooch off` restores every entry that is showing and removes it. An entry that is not showing, such as another Space or an unplugged monitor, stays as pending. With pending entries the daemon keeps running in a restoring mode. It never renders, restores each pending entry when its desktop or screen next shows, and removes the login item and itself when none are left. A second `scooch off` gives up on the pending ones, keeps their files and says how many.

A restore ends one of three ways, and the closing line counts each. The original is back. Or the original was a kind no app can put back, so the plain picture is shown. Or no file is left to show, so Scooch's picture stays and its file is kept. A picture file that a desktop may still show is never deleted.

**With the glide on.** A tick does this instead of a plain set:

1. At the wake, before the snapshot, the daemon may launch the helper with only its GPU device. The daemon launches the helper early at most once in 10 s and never while a helper runs. After two such launches in a row that ended with nothing moved, the daemon launches no early helper until a real move. An early helper is quit if no snapshot shows a move within 1.5 s of its launch.
2. When the snapshot shows icons moved on a screen that shows Scooch's picture and has its tables, the daemon has the helper load the picture and open the layer over that screen, then renders and encodes the new picture as for a plain set.
3. When the helper has not sent hello, the set is plain at once. When the helper is ready, the daemon holds the set for up to 150 ms until the layer shows. When the helper is still starting, the daemon holds the set for up to 150 ms only when the prediction below puts the show within that time, and otherwise the set is plain at once.
4. On a glide the daemon writes the new tables, starts the 450 ms glide, sets the new wallpaper under the layer and has the layer settle on the new still picture. After the set it stores the tables under the new output's name and writes the canvas file if none exists.
5. The layer closes once it has settled and at least 300 ms on macOS or 700 ms on Windows after the set. If it has not settled within 2 s, it closes anyway over the new wallpaper.

A shown layer with no command for 5 s closes. A helper that exits uncleanly after a warm is started again once, ready for the next drag.

**Never the wrong picture.** Right before setting an image the daemon reads the wallpaper again. If the user picked a new picture while the render ran, the render is dropped. A stop signal lets the running tick finish and save.

Measured on macOS at rest, the daemon uses 0.01 s of CPU over two minutes, has no idle wakeups and holds about 5 to 7 MB. A moved icon becomes a new wallpaper in about half a second, of which 250 ms is the quiet period, or 60 ms when the move ends with a drop over the desktop.

Measured on Windows 11, a dragged icon becomes a new wallpaper in 317 to 435 ms, and a quiet minute uses 0 ms of CPU with 6 MB of private memory. Windows re-encodes a JPEG wallpaper and keeps a PNG exactly, so outputs are PNG there. No event reports a change of icon size or a move made by another program, so those wait for the next drag or click on the desktop.

Measured on Linux, a quiet minute uses 0 to 10 ms of CPU and about 20 MB resident. Scooch's own share of a move is 0.3 to 0.7 s. The rest of the 2 to 4 s from drop to new wallpaper is the desktop saving its positions.

## Live glide

On macOS and Windows a move can slide the picture into its new place. A layer over the wallpaper and below the icons draws the old picture warping into the new one, and the wallpaper changes underneath it. On Linux the swap stays plain.

**A separate helper.** A window and a GPU device in the daemon would break rule 1, and on Windows a GPU device on the software renderer kept about 200 MB that did not come back when the device was dropped, so the layer lives in a helper. The helper is the same program started with the hidden argument `--live-helper`, which is `scoochw.exe` on Windows. The daemon and the helper exchange JSON lines over the helper's standard input and output. The helper holds one GPU device, up to four loaded pictures and one layer per screen. It starts at a wake or a move and exits 30 s after its last command. If it crashes, the daemon keeps running and the move is a plain swap.

On macOS the layer is a borderless window at the desktop window level, above the wallpaper and below Finder's icons, that takes no mouse input. It opens only when a wallpaper window and Finder's icon window are both on that screen and no window covers the whole desktop. On Windows the layer is a child window of Explorer's desktop below the icon view, under Progman or under the wallpaper WorkerW, depending on how the shell is laid out.

**Tables.** For each output the daemon writes `<output>.<key>.tables` in the cache, where the key is a hash of the picture and the flip setting. Tables of version 1 start with `SCBT` and hold the screen and canvas sizes, the warp maps, the field, look, pad, paint and ramp grids, the canvas fingerprint and a checksum. For a glide the daemon hands the helper the tables of the picture on screen and of the new one.

**The canvas file.** One per screen, `<screen>.<key>.canvas` beside the tables. Its 36 byte header is `SCCV`, then version 2, width, height, 4 channels, colour 1 for sRGB and the number of mip levels as 32 bit numbers, then the canvas fingerprint that the tables also hold. Every mip level follows as RGBA. A 2880 by 1800 canvas makes a file of 27,647,488 bytes. The daemon writes the file after a set when none exists for that picture and those options, and deletes the screen's older canvas files. The helper reads it while it makes the device, uploads the stored levels and checks the fingerprint. When the file is missing or does not match, the helper decodes the picture instead.

**Cold start.** Each milestone is signalled by the thread that reaches it:

| Milestone | Signalled by | Reply |
|---|---|---|
| Hello | The main thread, before any other work | `hello` |
| Device made | The warm thread | `device` |
| Canvas loaded | The warm thread | `canvas` |
| Ready | The main thread | `ready` |
| Layer shown | The main thread, after the render thread draws the first frame | `shown`, then `drawn` with the window, draw and reveal times |

The first glide runs at the scale remembered for that GPU and screen size, or at 1.0 on a discrete GPU and 0.6 on any other. Once no layer is open the helper draws two test frames and sets the scale for the next glide from the time the second one took.

**Prediction.** While the helper is still starting, the daemon predicts when the layer will show. It takes the latest milestone it has received and adds the remembered times of only the steps that remain, the canvas after the device and the window, draw and reveal after the open. Each step is timed by the thread that does it, so no step includes time spent waiting in a queue. The daemon keeps the last three of each step per GPU in `state.json` and uses their median, so a single slow or fast sample does not decide a drag. When no milestone past hello has arrived, it uses the median of the last three cold starts, each measured from the moment the drag's open was sent until the layer showed.

**Off switches.** `scooch glide` turns the glide off or on, and it starts on. The glide is also off while reduced motion is on, through Reduce motion on macOS and animation effects on Windows. On a machine with only a software renderer the swap stays plain, and after two glides in a row stutter it stays plain until Scooch starts again.

### What the live glide cannot do

- Linux has no live layer, so every swap there is plain.
- Several monitors gliding at once, a display at scale 2 with wide gamut on a real machine, and 60 frames a second in a window on real hardware are not proven yet.
- On a machine where the helper starts slower than the daemon ticks, the first drag after a rest swaps plainly and the next one glides.
- The layer is never open at rest. On the first drag after a rest the daemon holds the new wallpaper for up to 150 ms while the helper starts, so it lands later than with the glide off.

### Measured

Runner numbers come from GitHub's hosted virtual machines. Their GPUs are virtual, and the Windows runners have only a software renderer, so the probe there forces the glide with `SCOOCH_GLIDE_ANYWAY`.

| What | Result | Where, commit |
|---|---|---|
| Helper launch to ready and to hello, median of ten | Ready 186 ms, hello 25 ms | Owner's Mac, Intel UHD Graphics 630, one minute load 32, a873bd2 |
| An early launch quit unused | 72 ms of helper CPU after 70 ms, 100 ms after 300 ms | Owner's Mac, a873bd2 |
| Ten cold drags, each after 35 s of rest | 10 of 10 glided, waits 34 to 143 ms, drop to set median 471 ms against 304 ms with the glide off | macOS arm64 runner, 83f485e |
| Ten cold drags, each after 35 s of rest | 10 of 10 glided, no wait, drop to set median 633 ms against 495 ms with the glide off | macOS Intel runner, 83f485e |
| Ten drags with a helper that sends hello 1 s late | All plain at once, drop to set median 7 ms and minus 17 ms from the glide off drags | macOS arm64 and Intel runners, 83f485e |
| Drags where the daemon waited the full 150 ms and then swapped plainly | 0 | Every runner, 83f485e |
| A quiet minute 35 s after the last glide | 0 ms of CPU and 0 wakeups on macOS, 0 ms of CPU on Windows | Every runner, 83f485e |
| Thirty wakes with nothing moved, after the helper left | 1 early launch on each macOS runner and 2 on each Windows runner, each quit 1.5 s after launch, nothing left running | Every runner, 83f485e |
| Change outside the dropped icon between the settled layer and one second after close | 0 of 255 | macOS arm64 and Intel runners, 83f485e |
| The drag after the helper is killed mid glide, and the first drag after the helper left | Both glide | Windows Server 2022 and 2025 runners, 83f485e |

## Command

| Command | Does |
|---|---|
| `scooch` | Turns Scooch on, then shows status on later runs |
| `scooch use [image]` | Switches the picture, with a picker when no image is given |
| `scooch flip` | Lets Scooch flip the picture when that clears the icons, or stops it |
| `scooch glide` | Turns the glide off or on, on macOS and Windows |
| `scooch off` | Stops, removes the login item and restores the wallpaper |
| `scooch status` | Shows whether Scooch is on and what it shows |
| `scooch doctor` | Checks permissions and the desktop adapter |
| `scooch render` | Renders one image from a picture and a list of rectangles, for tests and the website |
| `scooch watch` | Runs the daemon in the foreground. The login item runs this |

## Code rules

- No comments in code. Names and small functions carry the meaning.
- Commit messages are at most about six words, with no body.
- Every platform claim is tested on that platform, on a real session in CI or on a real machine. Until it is, the claim is listed as unproven in the README.
