use super::sys::{self, Owned, Ref};
use crate::state::{same, Store};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::msg_send;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

pub const INDEX: &str = "Library/Application Support/com.apple.wallpaper/Store/Index.plist";
pub const DEFAULT_DESKTOP: &str = "/System/Library/CoreServices/DefaultDesktop.heic";
const IMAGE: &str = "com.apple.wallpaper.choice.image";
const DEFAULT: &str = "default";
const VIDEO: [&str; 2] = ["mov", "mp4"];

#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    Dict(Vec<(String, Plist)>),
    Array(Vec<Plist>),
    Text(String),
    Number(f64),
    Other,
}

impl Plist {
    pub fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(entries) => entries.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Plist::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Plist::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn items(&self) -> Option<&[Plist]> {
        match self {
            Plist::Array(items) => Some(items),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Picture(PathBuf),
    Default,
    Other(String),
}

pub fn parse(bytes: &[u8]) -> Option<Plist> {
    let text = bytes.trim_ascii_start();
    if !bytes.starts_with(b"bplist") && !text.starts_with(b"<") {
        return None;
    }
    let data = Owned::new(unsafe { sys::CFDataCreate(null(), bytes.as_ptr(), bytes.len() as isize) })?;
    let made = unsafe { sys::CFPropertyListCreateWithData(null(), data.0, 0, null_mut(), null_mut()) };
    let list = Owned::new(made)?;
    Some(convert(unsafe { &*(list.0 as *const AnyObject) }))
}

pub fn convert(object: &AnyObject) -> Plist {
    if let Some(text) = object.downcast_ref::<NSString>() {
        return Plist::Text(text.to_string());
    }
    if let Some(array) = object.downcast_ref::<NSArray>() {
        return Plist::Array(elements(array).iter().map(|item| convert(item)).collect());
    }
    if let Some(number) = object.downcast_ref::<NSNumber>() {
        return Plist::Number(number.doubleValue());
    }
    if let Some(dictionary) = object.downcast_ref::<NSDictionary>() {
        let keys: Retained<NSArray> = unsafe { msg_send![dictionary, allKeys] };
        let entries = elements(&keys)
            .iter()
            .filter_map(|key| {
                let name = key.downcast_ref::<NSString>()?.to_string();
                let value: Option<Retained<AnyObject>> = unsafe { msg_send![dictionary, objectForKey: &**key] };
                Some((name, convert(&*value?)))
            })
            .collect();
        return Plist::Dict(entries);
    }
    Plist::Other
}

fn elements(array: &NSArray) -> Vec<Retained<AnyObject>> {
    let count: usize = unsafe { msg_send![array, count] };
    (0..count).map(|index| unsafe { msg_send![array, objectAtIndex: index] }).collect()
}

pub fn kind(store: &Plist, display: &str, space: Option<&str>) -> Option<Kind> {
    let in_space = space.and_then(|space| store.get("Spaces")?.get(space));
    let chain = [
        in_space.and_then(|entry| entry.get("Displays")?.get(display)),
        in_space.and_then(|entry| entry.get("Default")),
        store.get("Displays").and_then(|displays| displays.get(display)),
        store.get("AllSpacesAndDisplays"),
        store.get("SystemDefault"),
    ];
    chain.into_iter().flatten().find_map(classify)
}

fn classify(entry: &Plist) -> Option<Kind> {
    let choices = entry.get("Desktop")?.get("Content")?.get("Choices")?.items()?;
    let first = choices.first()?;
    let provider = first.get("Provider")?.text()?;
    let files = first.get("Files").and_then(Plist::items).unwrap_or(&[]);
    Some(match provider {
        IMAGE if choices.len() == 1 && files.len() == 1 => match files[0].get("relative").and_then(Plist::text).and_then(file_path) {
            Some(path) => Kind::Picture(path),
            None => Kind::Other(provider.into()),
        },
        DEFAULT => Kind::Default,
        other => Kind::Other(other.into()),
    })
}

pub fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(pair, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    while decoded.len() > 1 && decoded.last() == Some(&b'/') {
        decoded.pop();
    }
    Some(PathBuf::from(OsStr::from_bytes(&decoded)))
}

pub fn decide(kind: Kind, reported: Option<PathBuf>, still: impl Fn(&Path) -> bool, ours: impl Fn(&Path) -> bool, default_still: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    match kind {
        Kind::Picture(file) => {
            let path = match reported {
                Some(reported) if same(&reported, &file) => reported,
                _ => file,
            };
            (ours(&path) || still(&path)).then_some(path)
        }
        Kind::Default => {
            let path = PathBuf::from(DEFAULT_DESKTOP);
            default_still(&path).then_some(path)
        }
        Kind::Other(_) => None,
    }
}

pub fn trusted(reported: Option<PathBuf>, id: &str) -> Option<PathBuf> {
    let Some(store) = read() else { return reported };
    let (display, space) = place(id);
    match kind(&store, display, space.as_deref()) {
        Some(kind) => decide(kind, reported, still, |path| Store::open().owns(path), default_still),
        None => reported,
    }
}

pub fn restorable(reported: Option<PathBuf>, id: &str) -> Option<PathBuf> {
    let Some(store) = read() else { return reported };
    let (display, space) = place(id);
    match kind(&store, display, space.as_deref()) {
        Some(kind) => keep(kind, reported),
        None => reported,
    }
}

pub fn keep(kind: Kind, reported: Option<PathBuf>) -> Option<PathBuf> {
    match kind {
        Kind::Picture(file) => Some(file).filter(|file| file.exists()),
        Kind::Default => reported,
        Kind::Other(_) => None,
    }
}

pub fn describe(id: &str) -> Option<Kind> {
    let store = read()?;
    let (display, space) = place(id);
    kind(&store, display, space.as_deref())
}

fn place(id: &str) -> (&str, Option<String>) {
    match super::spaces::split(id) {
        (display, Some(space)) => (display, Some(space.to_string())),
        (display, None) => (display, super::spaces::current(display)),
    }
}

fn read() -> Option<Plist> {
    let home = super::real_home()?;
    parse(&std::fs::read(home.join(INDEX)).ok()?)
}

pub fn still(path: &Path) -> bool {
    let Some(url) = NSURL::from_file_path(path) else { return false };
    let Some(source) = Owned::new(unsafe { sys::CGImageSourceCreateWithURL(&*url as *const NSURL as Ref, null()) }) else { return false };
    let known = !unsafe { sys::CGImageSourceGetType(source.0) }.is_null();
    known && unsafe { sys::CGImageSourceGetCount(source.0) } == 1
}

fn default_still(path: &Path) -> bool {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    still(&real) && !moving(&real)
}

pub fn moving(picture: &Path) -> bool {
    let (Some(folder), Some(stem)) = (picture.parent(), picture.file_stem()) else { return false };
    VIDEO.iter().any(|extension| folder.join(stem).with_extension(extension).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: &str = "DFF9CFD0-DCA7-0132-7928-AE23555A573B";
    const SPACE: &str = "A4BDE45F-8181-401F-B4C4-09D9D069AB3F";

    fn choice(provider: &str, files: &[&str], shuffle: &str) -> String {
        let files: String = files.iter().map(|file| format!("<dict><key>relative</key><string>{file}</string></dict>")).collect();
        format!("<dict><key>Content</key><dict><key>Choices</key><array><dict><key>Configuration</key><data></data><key>Files</key><array>{files}</array><key>Provider</key><string>{provider}</string></dict></array><key>Shuffle</key>{shuffle}</dict><key>LastSet</key><date>2026-11-12T03:36:11Z</date></dict>")
    }

    fn entry(desktop: &str) -> String {
        format!("<dict><key>Desktop</key>{desktop}<key>Type</key><string>individual</string></dict>")
    }

    fn store(space_display: &str, space_default: &str, display: &str, system: &str) -> Plist {
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
             <key>AllSpacesAndDisplays</key><dict><key>Type</key><string>idle</string></dict>\
             <key>Displays</key><dict><key>{DISPLAY}</key>{}</dict>\
             <key>Spaces</key><dict><key>{SPACE}</key><dict><key>Default</key>{}<key>Displays</key><dict><key>{DISPLAY}</key>{}</dict></dict></dict>\
             <key>SystemDefault</key>{}</dict></plist>",
            entry(display),
            entry(space_default),
            entry(space_display),
            entry(system)
        );
        parse(xml.as_bytes()).expect("the fixture parses")
    }

    fn everywhere(desktop: &str) -> Plist {
        store(desktop, desktop, desktop, desktop)
    }

    const NULL: &str = "<string>$null</string>";
    const GARBAGE_SHUFFLE: &str = "<dict><key>Duration</key><array><integer>2341</integer><integer>16172123445939666944</integer></array><key>Type</key><string>afterDuration</string></dict>";

    #[test]
    fn the_owners_default_reads_as_the_default_even_with_a_stray_shuffle() {
        let found = kind(&everywhere(&choice(DEFAULT, &[], GARBAGE_SHUFFLE)), DISPLAY, Some(SPACE));
        assert_eq!(found, Some(Kind::Default));
    }

    #[test]
    fn a_plain_picture_reads_as_its_file() {
        let found = kind(&everywhere(&choice(IMAGE, &["file:///System/Library/Desktop%20Pictures/iMac%20Blue.heic"], NULL)), DISPLAY, Some(SPACE));
        assert_eq!(found, Some(Kind::Picture(PathBuf::from("/System/Library/Desktop Pictures/iMac Blue.heic"))));
    }

    #[test]
    fn a_dynamic_system_set_is_not_a_picture() {
        let found = kind(&everywhere(&choice("com.apple.wallpaper.choice.sonoma", &[], NULL)), DISPLAY, Some(SPACE));
        assert_eq!(found, Some(Kind::Other("com.apple.wallpaper.choice.sonoma".into())));
    }

    #[test]
    fn a_folder_shuffle_is_not_a_picture() {
        let folder = choice("com.apple.wallpaper.choice.image-folder", &["file:///Users/someone/Pictures/Walls/"], "<dict><key>Type</key><string>afterDuration</string></dict>");
        assert!(matches!(kind(&everywhere(&folder), DISPLAY, Some(SPACE)), Some(Kind::Other(_))));
    }

    #[test]
    fn several_pictures_in_one_choice_are_a_set() {
        let several = choice(IMAGE, &["file:///p/a.jpg", "file:///p/b.jpg"], NULL);
        assert_eq!(kind(&everywhere(&several), DISPLAY, Some(SPACE)), Some(Kind::Other(IMAGE.into())));
    }

    #[test]
    fn aerials_and_colors_are_not_pictures_unverified_on_this_mac() {
        for provider in ["com.apple.wallpaper.choice.aerials", "com.apple.wallpaper.choice.color", "com.apple.NeptuneOneExtension"] {
            assert_eq!(kind(&everywhere(&choice(provider, &[], NULL)), DISPLAY, Some(SPACE)), Some(Kind::Other(provider.into())));
        }
    }

    #[test]
    fn the_current_space_wins_over_the_display_and_the_system() {
        let picture = choice(IMAGE, &["file:///p/space.jpg"], NULL);
        let sonoma = choice("com.apple.wallpaper.choice.sonoma", &[], NULL);
        let mixed = store(&picture, &sonoma, &sonoma, &sonoma);
        assert_eq!(kind(&mixed, DISPLAY, Some(SPACE)), Some(Kind::Picture("/p/space.jpg".into())));
        assert_eq!(kind(&mixed, DISPLAY, Some("ANOTHER-SPACE")), Some(Kind::Other("com.apple.wallpaper.choice.sonoma".into())));
        assert_eq!(kind(&mixed, DISPLAY, None), Some(Kind::Other("com.apple.wallpaper.choice.sonoma".into())));
        let fallback = store(&picture, &sonoma, &sonoma, &choice(DEFAULT, &[], NULL));
        assert_eq!(kind(&fallback, "OTHER-DISPLAY", None), Some(Kind::Default));
    }

    #[test]
    fn an_unknown_layout_is_not_understood() {
        let odd = parse(b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Something</key><string>new</string></dict></plist>").unwrap();
        assert_eq!(kind(&odd, DISPLAY, Some(SPACE)), None);
        let garbage = parse(b"not a plist");
        assert_eq!(parse(b"bplist00 broken"), None);
        assert_eq!(parse(b"<plist><dict><key>open"), None);
        assert!(garbage.is_none_or(|value| kind(&value, DISPLAY, Some(SPACE)).is_none()));
    }

    #[test]
    fn file_urls_decode_to_paths() {
        assert_eq!(file_path("file:///System/Library/Desktop%20Pictures/iMac%20Blue.heic"), Some("/System/Library/Desktop Pictures/iMac Blue.heic".into()));
        assert_eq!(file_path("file://localhost/p/caf%C3%A9.jpg"), Some(PathBuf::from("/p/café.jpg")));
        assert_eq!(file_path("file:///Users/a/Walls/"), Some("/Users/a/Walls".into()));
        assert_eq!(file_path("https://example.com/a.jpg"), None);
        assert_eq!(file_path("file:///bad%2"), None);
    }

    #[test]
    fn decide_trusts_only_plain_pictures_and_a_still_default() {
        let yes = |_: &Path| true;
        let no = |_: &Path| false;
        let picture = PathBuf::from("/p/a.jpg");
        assert_eq!(decide(Kind::Picture(picture.clone()), Some(picture.clone()), yes, no, no), Some(picture.clone()));
        assert_eq!(decide(Kind::Picture(picture.clone()), Some("/p/old.jpg".into()), yes, no, no), Some(picture.clone()));
        assert_eq!(decide(Kind::Picture("/p/look.madesktop".into()), Some("/p/look.madesktop".into()), no, no, no), None);
        assert_eq!(decide(Kind::Picture("/d/out/x-1.heic".into()), None, no, yes, no), Some(PathBuf::from("/d/out/x-1.heic")));
        assert_eq!(decide(Kind::Default, Some(DEFAULT_DESKTOP.into()), yes, no, no), None);
        assert_eq!(decide(Kind::Default, Some(DEFAULT_DESKTOP.into()), no, no, yes), Some(PathBuf::from(DEFAULT_DESKTOP)));
        assert_eq!(decide(Kind::Other("com.apple.wallpaper.choice.sonoma".into()), Some(DEFAULT_DESKTOP.into()), yes, yes, yes), None);
    }

    #[test]
    fn the_default_and_plain_files_can_be_put_back_but_nothing_else() {
        let default = PathBuf::from(DEFAULT_DESKTOP);
        assert_eq!(keep(Kind::Default, Some(default.clone())), Some(default.clone()));
        assert_eq!(keep(Kind::Default, None), None);
        assert_eq!(keep(Kind::Picture(default.clone()), Some("/elsewhere.jpg".into())), Some(default));
        assert_eq!(keep(Kind::Picture("/nowhere/gone.jpg".into()), None), None);
        assert_eq!(keep(Kind::Other("com.apple.wallpaper.choice.sonoma".into()), Some(DEFAULT_DESKTOP.into())), None);
    }

    #[test]
    fn a_picture_with_a_video_beside_it_is_moving() {
        let folder = crate::state::scratch("moving");
        let picture = folder.join("Sunrise.heic");
        std::fs::write(&picture, b"x").unwrap();
        assert!(!moving(&picture));
        std::fs::write(folder.join("Sunrise.mov"), b"x").unwrap();
        assert!(moving(&picture));
    }

    #[test]
    fn live_the_store_on_this_mac_is_understood() {
        if std::env::var_os("SCOOCH_LIVE").is_none_or(|value| value != "1") {
            return;
        }
        let screen = super::super::picture::screens(unsafe { objc2::MainThreadMarker::new_unchecked() }).unwrap().remove(0);
        assert!(screen.id.contains('@'));
        assert!(describe(&screen.id).is_some());
    }
}
