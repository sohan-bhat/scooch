# Scooch design

Scooch moves your wallpaper so your desktop icons stay off the best part of the picture. It works with any image, uses plain image math with no AI, and draws nothing between changes.

## Rules

1. **Nothing runs at rest.** Scooch has no window, no render loop and no timer. One small process stays idle until the operating system reports a change. An icon moved, a file landed on the desktop, a screen changed or the wallpaper changed. Scooch then writes one image, hands that image to the system as a normal wallpaper and goes idle again. While the glide is on, a second process, the helper, is parked beside it. Parked, it holds a GPU device and its compiled shaders and nothing else, no window, no picture, no timer, so it sleeps in the system until the daemon sends it a line. When a move glides, the helper loads the picture, holds a window and draws the frames, and it lets the picture go 30 s after its last command. Until the daemon has freed its held render data, it checks every 2 s.
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
     + 1.5  * saliency the home frame shows that this crop does not
     + 2.0  * subject newly cut by a crop edge
     + 0.6  * (zoom - 1)
     + 0.15 * distance of the crop from the home frame
     + 0.25 if mirrored
```

**Axis warp.** Columns and rows that hold no subject can stretch or squeeze, so the subject slides without any zoom. Straight horizontal and vertical lines stay straight. For each axis, the bins that hold the subject form a rigid core. Bins outside the core are flexible, less so where the picture is busy. A shift moves the core, stretching flexible bins on one side by up to 45% and squeezing those on the other side by up to 35%. Each shift pays a distortion cost.

**One decision.** The best crop for every zoom level is warped on both axes, and all results are scored with one function. The lowest total wins. On a soft background the subject slides at zoom 1.0. Zoom appears only when a slide cannot clear the icons.

**Home frame.** When the picture and the screen differ in shape, a fill crops one axis. The home frame is the pan along that axis the screen shows with no icons at all. Among the pans that keep within 2% of the most saliency any pan keeps, it is the one centered nearest the subject's centroid. It replaces the centered crop only when the centered crop keeps under 90% of that most or cuts the subject's border by more than 0.15 of its size, and only when the new pan shows at least 95% of the solid subject or 20 points more of it than the centered crop. Icons never change the home frame, and the crop costs above are measured from it.

**Never worse.** The result never hides more of the subject than the home frame. With no icons, or icons that cover nothing important, the output equals the home frame, which is a normal fill wallpaper whenever the centered crop already shows the subject.

**Stability.** The previous placement is kept unless the new one is clearly better, so a small nudge of one icon does not move the picture. The daemon keeps a placement only under the flip, scenery, strength and zoom settings it was solved with, which each screen entry records as `placed`, so a changed setting always gets a fresh solve. An entry saved before that record existed takes the settings of its next unchanged tick.

A `Placement` is separable. `fx` and `fy` are tables of 257 values. `fx[k]` is the source x, from 0 to 1, shown at output x `k / 256`. Mirror and crop are folded into the tables. Both tables are monotone and smooth.

### Render

For each output pixel, the renderer looks up the source position through `fx` and `fy` and samples in linear light with a Catmull-Rom filter. A source much larger than the screen is shrunk first with an area filter. Rows render in parallel.

**Pads.** Under each icon, where the picture is busy, the renderer blends toward a blurred copy so labels stay readable. The mask is a feathered union of the icon rectangles. Pads stay off the subject, and where the picture is already calm they do nothing.

Measured on a six core laptop from 2018, a 2880 by 1800 render takes about 95 ms and a 5120 by 2880 render about 230 ms, decode not included.

### Scenery

With scenery on, the still can also carry clouds or sand islands that gather around the icons and give their labels a calm ground.

**Where it is drawn.** Where scenery belongs is worked out once per picture, on the whole picture at the size the home frame shows it, and then carried through the placement like the picture itself. It changes only with the screen size and the icon size, so a pan, a zoom or an icon move never switches it. Clouds go only where the picture is sky blue at mid brightness or above and at least 0.08 darker than the shaded cloud tone, so a cloud always reads lighter than its sky, in a calm region that reaches the top edge of the picture. From those places the sky grows upward and sideways, never down, through smoothly joined blue down to a lightness of 0.3. A step may change the colour by 0.01 upward and 0.003 sideways, and a grown place may sit at most 0.3 from the colour it grew from, so a sky that darkens toward the top keeps its clouds there while a sea under the horizon or a wall beside a window stays bare. The grown sky must reach the top edge of the picture, and going down from that edge it may get lighter freely but never fall more than 0.08 below the lightest sky already passed, since water under a horizon is darker than the sky just above it. Each place keeps the smallest such peak any path from the top can give it, so the result does not depend on the order places are visited. A dark vignette corner on the top edge starts its own path and keeps its clouds, while one lower in the sky is cut like water. Below that horizon a place gets nothing. Each region joined to the top edge is sky rather than water when more than half of its open top row cells are blue from hue 215 to 295 with a chroma of at least 0.02, or a light grey, so turquoise water that fills the frame stays water, a cyan sky under a blue one keeps its clouds and a top edge part blue, part white and part cream still reads as sky. Islands go only where the picture is teal or blue like water, full from hue 191 to 235 and gone by 181 and 245, from a lightness of 0.25 up to 0.1 under the sand tone the picture gives its islands, in a calm region. Sand fades out over that same lightness, so the beach is always at least 0.1 lighter than its water. The inside may be as dark as light water and is told apart by its colour. Each colour test runs on the picture blurred by 0.2 icon units, is opened by 1.35 units so strips and small patches drop out, and is blurred by 0.45 units so scenery fades out where belonging ends. Everything else gets no scenery, and the pads still work there. The map lives in the session and a later call reuses it while the picture, its analysis, the screen size, the icon size and the scenery setting stay the same. The sky region and the horizon are worked out once per picture and cost 26.5 and 18 ms on a filled 2880 by 1800 screen and 390 and 365 ms on the largest grid, four times the cells of a 7680 by 4320 screen, on a six core laptop from 2018. When nothing belongs anywhere on the picture, no lattice, puffs, field or look are built, a move only updates the pads, and `Session::solve` places the picture as with scenery off, since `Session::belongs` says false. `Bundle::bare` tells such tables apart, and `belonging_stamp` keys the answer without the pixels.

**Subject guard.** Scenery is cleared from every 16 pixel cell that holds solid subject, which is saliency of at least 0.35 in a colour at least 0.1 apart from the blurred backdrop, and from every cell within 24 pixels or 0.2 units of one, whichever is larger. Past that it fades back in over half a unit. The guard travels in the paint table, so the still, the CPU frame and both shaders apply it. A glide reads the guard of each state where that state showed the part of the picture now on screen, found by two Newton steps on the warp maps, so the cleared place travels with the subject.

**Unit and lattice.** The unit u is the median height of the icons on the screen, measured before clipping, with a floor of 16 pixels. A median under 0.5 pixels turns scenery off. Clouds and islands grow from jittered sites spaced a third of a cell apart, where a cell is 1.1 u. A screen gets at most 250,000 sites, and the spacing widens when it would need more.

**Tables.** For each output the daemon writes `<output>.<key>.tables` in the cache, where the key is a hash of the picture and the flip setting. Tables of version 3 start with `SCBT` and hold the screen and canvas sizes, the warp maps, the field, look, pad, paint and ramp grids, the canvas fingerprint and a checksum. For a glide the daemon hands the helper the tables of the picture on screen and of the new one. The helper checks the fingerprint against the canvas it loaded, which it keeps only on the GPU, so no copy of the canvas is made at the glide.

**The canvas file.** One per screen, `<screen>.<key>.canvas` beside the tables. Its 36 byte header is `SCCV`, then version 2, width, height, 4 channels, colour 1 for sRGB and the number of mip levels as 32 bit numbers, then the canvas fingerprint that the tables also hold. Every mip level follows as RGBA. A 3840 by 2160 picture makes a file of 44,236,256 bytes. The daemon writes the file on its own thread after the first set of a picture with those options, and deletes the screen's older canvas files. The helper reads it when it is primed, uploads the stored levels and checks the fingerprint. When the file is missing or does not match, the helper decodes the picture instead.

**The raw still.** One per screen, `<output>.still` beside the tables, the picture on screen as raw RGBA after a 16 byte header, `SCST`, version 1, width and height. A 2880 by 1800 screen makes a file of 20,736,016 bytes. The daemon writes it on its own thread after each set, from the picture it drew when the output is lossless and otherwise from the output decoded the way the system decodes it, so it is the picture on screen. The helper reads it in about 5 ms where decoding the JPEG took 170 to 280 ms on an Intel Mac. When it is missing the helper decodes the output with the image crate. The helper keeps the last two stills it drew, so a move from the picture it just settled on reads nothing.

**Milestones.** Each is signalled by the thread that reaches it:

| Milestone | Signalled by | Reply |
|---|---|---|
| Hello | The main thread, before any other work | `hello` |
| Device made | The warm thread | `device` |
| Parked | The main thread, when the device for the park is made | `ready` for the park |
| Canvas loaded | The warm thread | `canvas` |
| Ready | The main thread | `ready` |
| Window primed | The main thread, after the render thread drew the first frame hidden | `primed` with the window and draw times and how the still was read |
| Layer shown | The main thread, after the first frame is drawn and the layer unhidden | `drawn` with the window, draw and reveal times, how the still was read and the screen's GPU, then `shown` |

The first glide runs at the scale remembered for that GPU and screen size, or at 1.0 on a discrete GPU and 0.6 on any other. Once no layer is open and no window is primed, the helper draws five test frames in a row and sets the scale for the next glide from the middle one, so the GPU's idle clock does not count against it.

**Off switches.** `scooch glide` turns the glide off or on, and it starts on. The glide is also off while reduced motion is on, through Reduce motion on macOS and animation effects on Windows. On a machine with only a software renderer the swap stays plain and no helper stays parked, and after two glides in a row stutter it stays plain until Scooch starts again.

### What the live glide cannot do

- Linux has no live layer, so every swap there is plain.
- Several monitors gliding at once, a display at scale 2 with wide gamut on a real machine, 60 frames a second in a window on real hardware, and a drag start found from a real mouse or trackpad are not proven yet.
- A move in the first moments after Scooch starts, before the helper has parked, swaps plainly. On a machine that just installed Scooch the shaders compile once, which took about 1.4 s on an Intel UHD 630.
- The layer is never open at rest. When the layer is not shown yet, the daemon waits for it until 150 ms after the tables are made or 450 ms after the wake, so that drag's wallpaper can be set up to that much later than with the glide off.
- A move with no drag start seen and no wake before it loads the picture at the move, which can miss the wait on a slow machine.

### Measured

Runner numbers come from GitHub's hosted virtual machines. Their GPUs are virtual, and the Windows runners have only a software renderer, so the probe there forces the glide with `SCOOCH_GLIDE_ANYWAY`.

| What | Result | Where, commit |
|---|---|---|
| Helper launch to ready and to hello, median of ten | Ready 186 ms, hello 25 ms | Owner's Mac, Intel UHD Graphics 630, one minute load 32, a873bd2 |
| An early launch quit unused | 72 ms of helper CPU after 70 ms, 100 ms after 300 ms | Owner's Mac, a873bd2 |
| Ten cold drags, each after 35 s of rest, the helper started at the drag start | 10 of 10 glided, first moving frame median 178 ms after the drop, wallpaper set median 255 ms against 172 ms with the glide off | GitHub's virtual Mac, arm64, a1d617c |
| Ten cold drags, each after 35 s of rest, the helper started at the drag start | 10 of 10 glided, first moving frame median 278 ms after the drop, wallpaper set median 309 ms against 290 ms with the glide off | GitHub's virtual Mac, Intel, a1d617c |
| Ten drags with a helper that sends hello 1 s late | All plain at once, drop to set median 18 ms over the glide off drags on arm64 and 34 ms on Intel | macOS arm64 and Intel runners, 8b2666c |
| Drags where the daemon waited the full 150 ms and then swapped plainly | 0 | Every runner, 8b2666c |
| A drag whose tables on screen come from another version | Plain at once with no layer, and the next drag glides | macOS arm64 and Intel runners, 8b2666c |
| A move while a window covers the whole desktop | Glide skipped as hidden, plain at once | macOS and Windows runners, 8b2666c |
| A quiet minute 35 s after the last glide | 0 ms of CPU and 0 wakeups on macOS, 0 ms of CPU on Windows | Every runner, 8b2666c |
| Thirty wakes with nothing moved, after the helper left | 1 early launch on each macOS runner and 2 on each Windows runner, each quit 1.5 s after launch, nothing left running | Every runner, 8b2666c |
| Change outside the dropped icon between the settled layer and one second after close | 0 of 255 | macOS arm64 and Intel runners, 8b2666c |
| The drag after the helper is killed mid glide | Glides | macOS and Windows runners, 8b2666c |
| The first drag after the helper left | Plain at once, since no cold start is remembered there | Windows Server 2022 and 2025 runners, 8b2666c |

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

## App

The app is one window a person opens, looks at and closes. It is a Tauri shell in `crates/scooch-app` with its page in `crates/scooch-app/ui`. It links the daemon crate and reaches it only through `crates/scooch/src/facade.rs`, which calls the same functions the commands call, so the app never runs `scooch` and never reads printed text. The installers carry the `scooch` program beside the app and the login item points at that program. While the window is open it follows the data folder for changes. When the window closes the app process ends and only the daemon stays, as it does without the app.

A job of the app workflow that holds a signing key runs on a fresh runner and trusts only what cannot have been changed by the code it signs. It runs the checkout, download and upload actions pinned by commit, the system's own tools, the scripts in `.github/app`, and the Tauri command line installed with `npm ci --ignore-scripts` as the bundler, with the NSIS it fetches. It compiles nothing and runs no package script, because a crate's build script or a package's install script could rewrite the bundler or the signing scripts on the shared disk before the key arrives. It takes the compiled programs from the build jobs as plain files it names one by one, and it signs them without running them. `.github/app/guard.rb` stops the word secrets, the workflow token and any write permission anywhere else in the workflows. It runs on every push and keeps a SHA-256 of each job that holds a key or a write token and of each file such a job runs in `.github/app/guard.lock`, so a change to the signing path always needs a second, visible edit of that file. Every action in those workflows is pinned by the commit of a release tag that the guard checks.

## Website

The page picks its post chain by measurement. detect-gpu gives the first guess from the GPU name, and a name it cannot match starts on the mid chain. After that the page moves between chains by the share of a frame the GPU spends on the fast chain in motion and on the sharp chain at rest, at rest only and at most three times a visit.

drei's PerformanceMonitor is not used for this, for two reasons. It counts frames per window, so it cannot see how much GPU time a frame that already holds 60 fps leaves free. It also needs an unbroken run of frames, and the sharp chain at rest only draws in short bursts between an input and the still.

## Code rules

- No comments in code. Names and small functions carry the meaning.
- Commit messages are at most about six words, with no body.
- Every platform claim is tested on that platform, on a real session in CI or on a real machine. Until it is, the claim is listed as unproven in the README.
