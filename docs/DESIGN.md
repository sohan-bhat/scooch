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

**Stability.** The previous placement is kept unless the new one is clearly better, so a small nudge of one icon does not move the picture.

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

**Tables.** Each still hands its scenery to the frame as tables, version 3, which start with the bytes `SCBT` and the version as a 32 bit little endian number. `scooch_core::TABLES_VERSION` holds the version a build writes.

| Table | Grid | Holds |
|---|---|---|
| field | 4 pixels | Distance to the scenery edge in u, negative inside, held between minus 1.7 and 1.15 |
| look | 4 pixels | Kind, cloud or land. The pile mark on cloud, a lit arc inside each front puff and a tucked shade outside it, both fading to nothing over 1.5 cells at the puff's edge so the 4 pixel grid never shows as steps, or foam on land. Broad light on cloud or the label hold on land, mixed only in the one cell band where kind is between the two, where nothing is drawn. Facing |
| paints | 16 pixels, 8 layers | Shallows gain, foam, beach sand, inside sand, cloud, lit cloud, deep cloud, and the subject guard in alpha |
| ramps | 2 rows of 512 | Shallows, foam line, sand cover, beach to inside, cloud cover, lit band and under shade, over the field from minus 0.6 to 1.15 |

Once a release has shipped tables, any change in what a table holds must raise the version.

**Island tones.** An island has what a cloud has, a pale rim and a calmer inside. The beach along the coast is the sand tone. From 0.12 u to 0.37 u inside the coast it fades to the inside tone, the sand tone less 0.12 and at most a lightness of 0.70, a little warmer, which is already the ground a label needs. An island too small for an inside, one icon on its own puff, is a beach ring with the inside tone only in its middle. Shallows move the water's own colour halfway toward the sand at the coast and fade out with the shallows. They are never more than 0.12 lighter than the water beside them and never more colourful.

**Label hold.** Under the bottom third of each icon, fading out over 0.7 cell, cloud drops its broad light, lit arcs and lit rim, and sand is drawn dark enough that its luminance stays at or under 0.373, a lightness of 0.72. Inside an island that only clips the specks where mottle and grain lift the inside above the cap, so the hold shows on the beach alone.

**Grain.** Sand grain comes from one integer hash written in Rust, WGSL and GLSL. A test reads both shader files and checks their numbers against the Rust constants, together with the glide numbers below.

**Glide.** Each pixel picks its glide from the kind its two states carry within reach. Sky cross dissolves. Land shrinks and swells. With s running from 0 to 1, the land distance is the least of the old distance plus 1.8 s, the new distance plus 1.8 times 1 minus s, and the larger of the two distances raised by up to 0.3 u times the square of 4 s times 1 minus s. That raise grows with how much the two fields differ, up to a difference of 0.2, at the pixel or at four points 0.3 u away on the diagonals. Those four reads are skipped where the distance lies outside minus 0.4 to 0.06, since the ramps are flat there and the pixel comes out the same. The CPU frame and both shaders compute the same terms.

### What the engine cannot do

A subject that touches the screen edge under the icons cannot slide clear on a screen with the same shape as the picture, because there is no picture beyond that edge to fill in. Flipping the picture clears it. Flipping is off by default, since a flipped photo of a known place or person looks wrong. `scooch flip` turns it on, and the status line says when it would help.

### What scenery cannot do

- Clouds need a sky blue region that reaches the top of the picture and is darker than the cloud. Overcast, pale, foggy, night, dusk and sunset skies get none, and so does sky blue that sits lower in the frame.
- Islands need teal or blue water at least 0.1 darker than the sand. Green water, ocean blue past hue 245, a lake paler than that and a dark lake get none, and so does water under a smooth haze that joins a sky. Water that fills the frame to the top edge from hue 215 to 235 reads as sky.
- A cyan sky that fills the top of the frame below hue 215 reads as water and gets islands. So does a sky that does not reach the top edge, under a canopy, through an arch or below a letterbox bar, where it is as teal and as dark as water. The scenery setting can force clouds there.
- Water within 0.08 of the lightness of the sky above it, with no darker band between, reads as sky as far down as that holds. On a hazy sea under a city skyline that is about a fifth of the water, and on a beach under a pale horizon about a tenth. A blue wall that reaches the top edge reads as sky too.
- When a sun just above the frame lights the whole top edge, sky more than 0.08 darker further down gets no clouds.
- White label text reaches about 1.8 to 1 contrast on cloud and 2.2 to 1 on sand at worst.
- Icons smaller than 16 pixels get scenery sized for 16 pixel icons.
- Past 250,000 sites the puffs spread apart, which starts near a 7680 by 4320 screen with 16 pixel icons.
- Mid glide, land near a change shrinks by up to 0.3 u before it grows back.

### Scenery measured

| What | Result | Where, commit |
|---|---|---|
| One land glide frame, four gate reads against none, fastest of 160 frames | 0.12 ms more at s 0.25, 0.49 ms more at s 0.5 | Owner's Mac, Intel UHD Graphics 630, 2880 by 1800, two-swans, 7bc00dc |
| First still and one move on fox, which draws nothing, instructions on one thread at 2880 by 1800 | Mac column 3,660M and 30M, scattered 3,875M and 29M. On main at 8b2666c, where fox drew scenery, 4,796M and 646M, and 5,501M and 575M | Owner's Mac, 0a5f25b |
| 300 icons at 5120 by 2880 on balloon-sky | First still 26,681M, move 630M, a move that zooms the picture 22,469M | Owner's Mac, fb73c4a |
| A 1 pixel icon at 1920 by 1080, and 120 icons of 16 pixels at 7680 by 4320 | 63,794 sites in 0.45 s, and 233,324 sites in 1.82 s | Owner's Mac, 7bc00dc |
| Solid subject pixels under scenery, 18 photos on two layouts at 960 by 600, with clouds forced, islands forced and automatic | 0 of 23,010,632 in the stills and in glide frames at s 0.25, 0.5 and 0.75 where the picture puts the subject, with 11,362,468 scenery pixels within 1 u of it | Owner's Mac, 0a5f25b |
| Label ground lightness under white labels, 18 photos on four layouts at 960 by 600, with clouds forced, islands forced and automatic | Cloud at most 0.815, which is 1.8 to 1, and sand at most 0.754, which is 2.2 to 1 | Owner's Mac, 0a5f25b |
| Pictures that get scenery, 18 photos and 30 made pictures on two layouts at 2880 by 1800 | balloon-sky, moon-sky and lone-canoe, a sky blue gradient, a deep to mid blue sky, a flat teal, a turquoise lagoon, a deep blue sea under haze, a pool between decks, and the sky over a sea, over a lake and in a window. Every other picture 0.00% of the screen, two-swans among them | Owner's Mac, 0a5f25b |
| Web package, which leaves the still out since the page only reads tables | 171,863 bytes gzipped | Owner's Mac, 0a5f25b |
| First still, one move and a move that zooms the picture by 1.15 on balloon-sky, instructions on one thread, at 2880 by 1800 | Mac column 6,079M, 680M and 4,417M, scattered 8,266M, 1,138M and 6,419M | Owner's Mac, fb73c4a |

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
2. When the snapshot shows icons moved on a screen that shows Scooch's picture and has its tables, the daemon has the helper load the picture and open the layer over that screen, then renders and encodes the new picture as for a plain set. When those tables come from another version, no layer opens and the set is plain at once.
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

**Tables.** For each output the daemon writes `<output>.<key>.tables` in the cache, where the key is a hash of the picture and the flip setting. Tables of version 3 start with `SCBT` and hold the screen and canvas sizes, the warp maps, the field, look, pad, paint and ramp grids, the canvas fingerprint and a checksum. For a glide the daemon hands the helper the tables of the picture on screen and of the new one.

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

**Prediction.** While the helper is still starting, the daemon predicts when the layer will show. It takes the latest milestone it has received and adds the remembered times of only the steps that remain, the canvas after the later of the device and the open, and the window, draw and reveal after the open. Each step is timed by the thread that does it, so no step includes time spent waiting in a queue. The daemon keeps the last three of each step per GPU in `state.json` and uses their median, so a single slow or fast sample does not decide a drag. When no milestone past hello has arrived, it uses the median of the last three cold starts, each measured from the moment the drag's open was sent until the layer showed, and learned only from a helper whose device arrived after that open.

**Off switches.** `scooch glide` turns the glide off or on, and it starts on. The glide is also off while reduced motion is on, through Reduce motion on macOS and animation effects on Windows. On a machine with only a software renderer the swap stays plain, and after two glides in a row stutter it stays plain until Scooch starts again.

### What the live glide cannot do

- Linux has no live layer, so every swap there is plain.
- Several monitors gliding at once, a display at scale 2 with wide gamut on a real machine, and 60 frames a second in a window on real hardware are not proven yet.
- On a machine where the helper starts slower than the daemon ticks, the first drag after a rest swaps plainly and the next one glides.
- The layer is never open at rest. On the first drag after a rest the daemon holds the new wallpaper for up to 150 ms while the helper starts, so that drag lands up to 150 ms later than with the glide off.
- A cold drag on a machine with no cold start remembered swaps plainly unless the helper is already ready. Only a helper that was still making its device when the drag opened teaches a cold start, so where every helper starts early at a wake, as on the Windows runners, the first drag after the helper left keeps swapping plainly.

### Measured

Runner numbers come from GitHub's hosted virtual machines. Their GPUs are virtual, and the Windows runners have only a software renderer, so the probe there forces the glide with `SCOOCH_GLIDE_ANYWAY`.

| What | Result | Where, commit |
|---|---|---|
| Helper launch to ready and to hello, median of ten | Ready 186 ms, hello 25 ms | Owner's Mac, Intel UHD Graphics 630, one minute load 32, a873bd2 |
| An early launch quit unused | 72 ms of helper CPU after 70 ms, 100 ms after 300 ms | Owner's Mac, a873bd2 |
| Ten cold drags, each after 35 s of rest | 10 of 10 glided, waits 52 to 142 ms, drop to set median 471 ms against 281 ms with the glide off | macOS arm64 runner, 8b2666c |
| Ten cold drags, each after 35 s of rest | 10 of 10 glided, waits 0 to 11 ms, drop to set median 488 ms against 422 ms with the glide off | macOS Intel runner, 8b2666c |
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

## Code rules

- No comments in code. Names and small functions carry the meaning.
- Commit messages are at most about six words, with no body.
- Every platform claim is tested on that platform, on a real session in CI or on a real machine. Until it is, the claim is listed as unproven in the README.
