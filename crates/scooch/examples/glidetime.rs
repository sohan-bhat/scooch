#[cfg(not(target_os = "macos"))]
fn main() {
    println!("glidetime times the glide's steps offscreen on macOS only.");
}

#[cfg(target_os = "macos")]
fn main() {
    timing::main();
}

#[cfg(target_os = "macos")]
mod timing {
    use scooch::live::{canvas, still};
    use scooch::platform::macos::picture;
    use scooch_core::{Bundle, Options, Rect, Scene, Scenery, Session};
    use scooch_live::{Gpu, Look};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    const SEQUOIA: &str = "/System/Library/Desktop Pictures/.wallpapers/Sequoia Sunrise/Sequoia Sunrise.heic";
    const FRAMES: usize = 27;

    struct Table(Vec<(String, f64)>);

    impl Table {
        fn time<T>(&mut self, name: &str, work: impl FnOnce() -> T) -> T {
            let started = Instant::now();
            let made = work();
            self.0.push((name.to_string(), ms(started.elapsed())));
            made
        }

        fn put(&mut self, name: &str, value: f64) {
            self.0.push((name.to_string(), value));
        }
    }

    fn ms(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1000.0
    }

    fn footprint() -> f64 {
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let read = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V2, (&mut info as *mut libc::rusage_info_v2).cast()) };
        if read == 0 {
            info.ri_phys_footprint as f64 / 1_048_576.0
        } else {
            0.0
        }
    }

    fn median(mut values: Vec<Duration>) -> f64 {
        values.sort();
        values.get(values.len() / 2).copied().map_or(0.0, ms)
    }

    pub fn main() {
        let mut args = std::env::args().skip(1);
        let picture = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(SEQUOIA));
        let high = args.next().as_deref() == Some("high");
        let folder = std::env::temp_dir().join(format!("scooch-glidetime-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let mut table = Table(Vec::new());
        let bare = footprint();
        let parked = table.time("make the device as the helper parks", || Gpu::new_with(false, high).expect("a GPU"));
        table.put("of it the shaders", ms(parked.made[2]));
        table.put("footprint of this process before the device, MB", bare);
        table.put("footprint with the device and shaders, MB", footprint());
        drop(parked);
        let image = table.time("decode the picture", || picture::decode(&picture).expect("the picture"));
        let analysis = table.time("analyze", || scooch_core::analyze(&image));
        let (width, height) = (2880u32, 1800u32);
        let icon = |x: f32, y: f32| Rect { x, y, w: 256.0, h: 224.0 };
        let icons = vec![icon(150.0, 186.0), icon(150.0, 410.0), icon(171.0, 1028.0), icon(232.0, 784.0), icon(2418.0, 16.0), icon(2622.0, 240.0)];
        let mut moved = icons.clone();
        moved[3] = icon(1300.0, 800.0);
        let (one, two) = (Scene { width, height, icons }, Scene { width, height, icons: moved });
        let options = Options { strength: 1.0, max_zoom: 1.3, mirror: true, pads: 0.7, scenery: Scenery::Auto };
        let a = table.time("solve", || scooch_core::solve(&analysis, &one, &options, None));
        let b = scooch_core::solve(&analysis, &two, &options, Some(&a));
        let mut session = Session::new();
        let (bundle, _) = table.time("first still with its tables", || session.still_shared(&image, &analysis, &a, &one, &options));
        let next = table.time("tables of a move", || session.tables_shared(&image, &analysis, &b, &two, &options));
        let (_, drawn) = table.time("still of a move", || session.still_shared(&image, &analysis, &b, &two, &options));
        let profile = picture::profile();
        let jpeg = table.time("encode the still", || {
            let mut out = Vec::new();
            let mut encoder = jpeg_encoder::Encoder::new(&mut out, 95);
            encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
            if let Some(profile) = profile.as_deref() {
                encoder.add_icc_profile(profile).expect("the profile");
            }
            encoder.encode(drawn.as_raw(), drawn.width() as u16, drawn.height() as u16, jpeg_encoder::ColorType::Rgb).expect("the encode");
            out
        });
        let output = folder.join("main-1.jpg");
        std::fs::write(&output, &jpeg).expect("the output");
        let decoded = table.time("decode the output as the system does", || picture::decode(&output).expect("the output"));
        table.time("decode the output with the image crate", || scooch_core::open(&output).expect("the output"));
        let raw = folder.join("main-1.jpg.still");
        table.time("write the raw still", || still::write_rgb(&raw, &decoded).expect("the raw still"));
        let read = table.time("read the raw still", || still::read(&raw).expect("the raw still"));
        let pixels = scooch_core::Canvas { width: bundle.canvas_width, height: bundle.canvas_height, pixels: bundle.canvas.clone() };
        let levels = table.time("canvas mip levels", || scooch_live::levels(&pixels));
        let print = canvas::fingerprint(pixels.width, pixels.height, &pixels.pixels);
        let file = folder.join("main.canvas");
        table.time("write the canvas file", || canvas::write(&file, pixels.width, pixels.height, &levels, print).expect("the canvas"));
        let cached = table.time("read the canvas file", || canvas::read(&file).expect("the canvas"));
        table.time("check the canvas file", || cached.canvas().expect("a matching canvas"));
        let (from, to) = (bundle.tables_to_bytes(), next.tables_to_bytes());
        let sides = table.time("parse both tables without the canvas", || (Bundle::tables_for(&from, pixels.width, pixels.height, print).expect("tables"), Bundle::tables_for(&to, pixels.width, pixels.height, print).expect("tables")));
        let gpu = table.time("make the device again", || Gpu::new_with(false, high).expect("a GPU"));
        let loaded = table.time("upload the canvas levels", || {
            let loaded = gpu.try_picture_levels(cached.width, cached.height, &cached.levels()).expect("the canvas");
            gpu.idle(Duration::from_secs(10)).expect("the GPU");
            loaded
        });
        let shown = table.time("upload the still", || {
            let shown = gpu.still_rgba(read.width, read.height, read.rgba()).expect("the still");
            gpu.idle(Duration::from_secs(10)).expect("the GPU");
            shown
        });
        let glide = table.time("upload both tables", || {
            let made = gpu.glide_between(&loaded, &gpu.side(&sides.0).expect("a side"), &gpu.side(&sides.1).expect("a side")).expect("the glide");
            gpu.idle(Duration::from_secs(10)).expect("the GPU");
            made
        });
        let still_look = [(Look::Still(&shown), 1.0)];
        let (frames, _) = gpu.burst_at(width, height, &still_look, 1.0, FRAMES).expect("frames");
        table.put("still frame, median of a burst", median(frames));
        for scale in [1.0f32, 0.75, 0.6, 0.5, 0.4] {
            let look = [(Look::Glide(&glide, 0.5), 1.0)];
            let rested = gpu.time_at(width, height, &look, scale).expect("a frame");
            let (frames, _) = gpu.burst_at(width, height, &look, scale, FRAMES).expect("frames");
            table.put(&format!("glide frame at scale {scale}, one after rest"), ms(rested));
            table.put(&format!("glide frame at scale {scale}, median of a burst"), median(frames));
        }
        let heavy = scooch_live::heavy(width, height, &loaded);
        let worst = gpu.glide(&loaded, &heavy, &heavy).expect("the worst case");
        let look = [(Look::Glide(&worst, 0.5), 1.0)];
        let _ = gpu.time(width, height, &look);
        table.put("worst case frame the chooser times, one after rest", gpu.time(width, height, &look).map_or(0.0, ms));
        table.put("worst case frame, median of a burst", gpu.burst_at(width, height, &look, 1.0, FRAMES).map_or(0.0, |(frames, _)| median(frames)));
        println!("{} on {} at {width}x{height}, picture {}", gpu.name, gpu.kind, Path::new(&picture).display());
        println!("| Step | ms |");
        println!("|---|---|");
        for (name, value) in table.0 {
            println!("| {name} | {value:.1} |");
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
