#![allow(non_upper_case_globals, non_snake_case, clippy::too_many_arguments)]

use objc2_core_foundation::CGRect;
use std::ffi::{c_char, c_void};

pub type Ref = *const c_void;

#[repr(C)]
pub struct AeDesc {
    pub kind: u32,
    pub data: *mut c_void,
}

unsafe impl objc2::encode::RefEncode for AeDesc {
    const ENCODING_REF: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct(
        "AEDesc",
        &[objc2::encode::Encoding::UInt, objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("OpaqueAEDataStorageType", &[])))],
    ));
}

pub struct Owned(pub Ref);

impl Owned {
    pub fn new(pointer: Ref) -> Option<Self> {
        (!pointer.is_null()).then(|| Self(pointer))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

#[repr(C)]
pub struct StreamContext {
    pub version: isize,
    pub info: *mut c_void,
    pub retain: Ref,
    pub release: Ref,
    pub describe: Ref,
}

pub type StreamCallback = extern "C" fn(Ref, *mut c_void, usize, *const *const c_char, *const u32, *const u64);
pub type Work = extern "C" fn(*mut c_void);

pub const STREAM_SINCE_NOW: u64 = u64::MAX;
pub const STREAM_NO_DEFER: u32 = 0x2;
pub const STREAM_IGNORE_SELF: u32 = 0x8;
pub const STREAM_FILE_EVENTS: u32 = 0x10;

pub const ITEM_CREATED: u32 = 0x100;
pub const ITEM_REMOVED: u32 = 0x200;
pub const ITEM_RENAMED: u32 = 0x800;

pub const NOTE_DELETE: usize = 0x1;
pub const NOTE_WRITE: usize = 0x2;
pub const NOTE_EXTEND: usize = 0x4;
pub const NOTE_RENAME: usize = 0x20;

pub const BITMAP_RGBX: u32 = 5;
pub const INTERPOLATION_HIGH: i32 = 3;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRelease(object: Ref);
    pub fn CFPreferencesCopyAppValue(key: Ref, application: Ref) -> Ref;
    pub fn CFPreferencesAppSynchronize(application: Ref) -> u8;
    pub fn CFUUIDCreateString(allocator: Ref, uuid: Ref) -> Ref;
    pub fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    pub fn CFDataGetLength(data: Ref) -> isize;
    pub fn CFDataGetBytePtr(data: Ref) -> *const u8;
    pub fn CFPropertyListCreateWithData(allocator: Ref, data: Ref, options: usize, format: *mut isize, error: *mut Ref) -> Ref;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub static kCGColorSpaceSRGB: Ref;
    pub fn CGColorSpaceCopyICCData(space: Ref) -> Ref;
    pub fn CGDisplayBounds(display: u32) -> CGRect;
    pub fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
    pub fn CGBitmapContextCreate(data: *mut c_void, width: usize, height: usize, bits: usize, row: usize, space: Ref, info: u32) -> Ref;
    pub fn CGContextSetInterpolationQuality(context: Ref, quality: i32);
    pub fn CGContextDrawImage(context: Ref, rect: CGRect, image: Ref);
    pub fn CGImageGetWidth(image: Ref) -> usize;
    pub fn CGImageGetHeight(image: Ref) -> usize;
}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    pub static kCGImageSourceCreateThumbnailFromImageAlways: Ref;
    pub static kCGImageSourceCreateThumbnailWithTransform: Ref;
    pub static kCGImageSourceThumbnailMaxPixelSize: Ref;
    pub static kCGImagePropertyPixelWidth: Ref;
    pub static kCGImagePropertyPixelHeight: Ref;
    pub fn CGImageSourceCreateWithURL(url: Ref, options: Ref) -> Ref;
    pub fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    pub fn CGImageSourceCreateThumbnailAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    pub fn CGImageSourceGetCount(source: Ref) -> usize;
    pub fn CGImageSourceGetType(source: Ref) -> Ref;
}

#[cfg(test)]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub static kCGColorSpaceDisplayP3: Ref;
    pub fn CGBitmapContextCreateImage(context: Ref) -> Ref;
}

#[cfg(test)]
#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    pub fn CGImageDestinationCreateWithURL(url: Ref, kind: Ref, count: usize, options: Ref) -> Ref;
    pub fn CGImageDestinationAddImage(destination: Ref, image: Ref, properties: Ref);
    pub fn CGImageDestinationFinalize(destination: Ref) -> u8;
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    pub fn FSEventStreamCreate(allocator: Ref, callback: StreamCallback, context: *const StreamContext, paths: Ref, since: u64, latency: f64, flags: u32) -> Ref;
    pub fn FSEventStreamSetDispatchQueue(stream: Ref, queue: Ref);
    pub fn FSEventStreamStart(stream: Ref) -> u8;
    pub fn AEDeterminePermissionToAutomateTarget(target: Ref, class: u32, id: u32, ask: u8) -> i32;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> Ref;
}

extern "C" {
    pub static _dispatch_main_q: c_void;
    pub static _dispatch_source_type_vnode: c_void;
    pub fn dispatch_sync_f(queue: Ref, context: *mut c_void, work: Work);
    pub fn dispatch_async_f(queue: Ref, context: *mut c_void, work: Work);
    pub fn dispatch_source_create(kind: Ref, handle: usize, mask: usize, queue: Ref) -> Ref;
    pub fn dispatch_set_context(object: Ref, context: *mut c_void);
    pub fn dispatch_source_set_event_handler_f(source: Ref, handler: Work);
    pub fn dispatch_source_set_cancel_handler_f(source: Ref, handler: Work);
    pub fn dispatch_source_get_data(source: Ref) -> usize;
    pub fn dispatch_source_cancel(source: Ref);
    pub fn dispatch_resume(object: Ref);
    pub fn dispatch_release(object: Ref);
}

pub fn main_queue() -> Ref {
    unsafe { &_dispatch_main_q as *const c_void }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_pointer_is_never_released() {
        assert!(Owned::new(std::ptr::null()).is_none());
    }
}
