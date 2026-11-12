use super::sys::{self, Owned, Ref};
use super::Screen;
use anyhow::{anyhow, bail, Context, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageScalingKey};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString, NSURL};
use scooch_core::{Rect, RgbImage};
use std::path::{Path, PathBuf};
use std::ptr::null;

const FILL: usize = 3;

pub fn screens(marker: MainThreadMarker) -> Result<Vec<Screen>> {
    let mut found = Vec::new();
    for screen in NSScreen::screens(marker) {
        let Some(display) = number(&screen) else { continue };
        let bounds = unsafe { sys::CGDisplayBounds(display) };
        let frame = Rect { x: bounds.origin.x as f32, y: bounds.origin.y as f32, w: bounds.size.width as f32, h: bounds.size.height as f32 };
        found.push(Screen { id: identifier(display), frame, scale: screen.backingScaleFactor() as f32 });
    }
    if found.is_empty() {
        bail!("macOS reports no screens, so try again once a display is connected.");
    }
    Ok(found)
}

fn number(screen: &NSScreen) -> Option<u32> {
    let value: Option<Retained<AnyObject>> = unsafe { msg_send![&*screen.deviceDescription(), objectForKey: ns_string!("NSScreenNumber")] };
    value.and_then(|value| value.downcast_ref::<NSNumber>().map(|number| number.unsignedIntValue()))
}

fn identifier(display: u32) -> String {
    let named = Owned::new(unsafe { sys::CGDisplayCreateUUIDFromDisplayID(display) })
        .and_then(|uuid| Owned::new(unsafe { sys::CFUUIDCreateString(null(), uuid.0) }))
        .map(|text| unsafe { &*(text.0 as *const NSString) }.to_string());
    named.unwrap_or_else(|| format!("display-{display}"))
}

fn find(marker: MainThreadMarker, id: &str) -> Result<Retained<NSScreen>> {
    NSScreen::screens(marker)
        .into_iter()
        .find(|screen| number(screen).is_some_and(|display| identifier(display) == id))
        .ok_or_else(|| anyhow!("The screen {id} is not connected, so reconnect it and try again."))
}

pub fn wallpaper(marker: MainThreadMarker, id: &str) -> Result<Option<PathBuf>> {
    Ok(super::store::trusted(reported(marker, id)?, id))
}

pub fn restorable(marker: MainThreadMarker, id: &str) -> Result<Option<PathBuf>> {
    Ok(super::store::restorable(reported(marker, id)?, id))
}

fn reported(marker: MainThreadMarker, id: &str) -> Result<Option<PathBuf>> {
    let screen = find(marker, id)?;
    let url = NSWorkspace::sharedWorkspace().desktopImageURLForScreen(&screen);
    Ok(url.and_then(|url| url.path()).map(|path| PathBuf::from(path.to_string())))
}

pub fn set_wallpaper(marker: MainThreadMarker, id: &str, path: &Path) -> Result<()> {
    let screen = find(marker, id)?;
    let url = NSURL::from_file_path(path).ok_or_else(|| anyhow!("{} is not a usable path, so pick another picture.", path.display()))?;
    let fill: Retained<AnyObject> = NSNumber::new_usize(FILL).into();
    let clip: Retained<AnyObject> = NSNumber::new_bool(true).into();
    let options = NSDictionary::from_slices(&[unsafe { NSWorkspaceDesktopImageScalingKey }, unsafe { NSWorkspaceDesktopImageAllowClippingKey }], &[&*fill, &*clip]);
    unsafe { NSWorkspace::sharedWorkspace().setDesktopImageURL_forScreen_options_error(&url, &screen, &options) }
        .map_err(|error| anyhow!("macOS did not take {} as the wallpaper and said {}", path.display(), error.localizedDescription()))
}

pub fn decode(path: &Path) -> Result<RgbImage> {
    let url = NSURL::from_file_path(path).ok_or_else(|| anyhow!("{} is not a usable path.", path.display()))?;
    let source = Owned::new(unsafe { sys::CGImageSourceCreateWithURL(&*url as *const NSURL as Ref, null()) });
    let upright = source.as_ref().and_then(orientation).is_none_or(|turn| turn == 1);
    if upright {
        if let Ok(image) = scooch_core::open(path) {
            return Ok(image);
        }
    }
    let source = source.with_context(|| format!("{} is not a picture macOS can read.", path.display()))?;
    let (wide, tall) = size(&source).with_context(|| format!("{} has no readable size.", path.display()))?;
    let longest: Retained<AnyObject> = NSNumber::new_usize(wide.max(tall)).into();
    let yes: Retained<AnyObject> = NSNumber::new_bool(true).into();
    let keys = unsafe { [key(sys::kCGImageSourceCreateThumbnailFromImageAlways), key(sys::kCGImageSourceCreateThumbnailWithTransform), key(sys::kCGImageSourceThumbnailMaxPixelSize)] };
    let options = NSDictionary::from_slices(&keys, &[&*yes, &*yes, &*longest]);
    let image = Owned::new(unsafe { sys::CGImageSourceCreateThumbnailAtIndex(source.0, 0, &*options as *const NSDictionary<NSString, AnyObject> as Ref) })
        .with_context(|| format!("{} could not be decoded.", path.display()))?;
    drop(source);
    pixels(&image)
}

fn key(name: Ref) -> &'static NSString {
    unsafe { &*(name as *const NSString) }
}

fn property(source: &Owned, name: Ref) -> Option<usize> {
    let properties = Owned::new(unsafe { sys::CGImageSourceCopyPropertiesAtIndex(source.0, 0, null()) })?;
    let properties = unsafe { &*(properties.0 as *const AnyObject) };
    let value: Option<Retained<AnyObject>> = unsafe { msg_send![properties, objectForKey: key(name)] };
    value?.downcast_ref::<NSNumber>().map(|number| number.as_usize())
}

fn orientation(source: &Owned) -> Option<usize> {
    property(source, unsafe { sys::kCGImagePropertyOrientation })
}

fn size(source: &Owned) -> Option<(usize, usize)> {
    unsafe { Some((property(source, sys::kCGImagePropertyPixelWidth)?, property(source, sys::kCGImagePropertyPixelHeight)?)) }
}

fn pixels(image: &Owned) -> Result<RgbImage> {
    let (wide, tall) = unsafe { (sys::CGImageGetWidth(image.0), sys::CGImageGetHeight(image.0)) };
    if wide == 0 || tall == 0 {
        bail!("The picture is empty, so pick another one.");
    }
    let mut canvas = vec![0u8; wide * tall * 4];
    let space = Owned::new(unsafe { sys::CGColorSpaceCreateWithName(sys::kCGColorSpaceSRGB) }).context("macOS has no sRGB color space.")?;
    let context = Owned::new(unsafe { sys::CGBitmapContextCreate(canvas.as_mut_ptr().cast(), wide, tall, 8, wide * 4, space.0, sys::BITMAP_RGBX) }).context("macOS could not make a drawing surface.")?;
    let whole = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: wide as f64, height: tall as f64 } };
    unsafe {
        sys::CGContextSetInterpolationQuality(context.0, sys::INTERPOLATION_HIGH);
        sys::CGContextDrawImage(context.0, whole, image.0);
    }
    drop(context);
    pack(&mut canvas);
    RgbImage::from_raw(wide as u32, tall as u32, canvas).context("The picture did not fit in memory.")
}

pub fn pack(canvas: &mut Vec<u8>) {
    let count = canvas.len() / 4;
    for pixel in 0..count {
        canvas.copy_within(pixel * 4..pixel * 4 + 3, pixel * 3);
    }
    canvas.truncate(count * 3);
    canvas.shrink_to_fit();
}

#[cfg(test)]
mod tests {
    use super::pack;

    #[test]
    fn packing_drops_the_fourth_byte_in_place() {
        let mut canvas = vec![1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0];
        pack(&mut canvas);
        assert_eq!(canvas, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
