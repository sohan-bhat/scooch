use block2::RcBlock;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillPowerOffNotification};
use objc2_foundation::NSNotification;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

static GOING: AtomicBool = AtomicBool::new(false);

pub fn watch() {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let block = RcBlock::new(|_: NonNull<NSNotification>| {
        GOING.store(true, Ordering::SeqCst);
        eprintln!("scooch-app the Mac logs out, restarts or shuts down");
    });
    std::mem::forget(unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSWorkspaceWillPowerOffNotification), None, None, &block) });
}

pub fn going() -> bool {
    GOING.load(Ordering::SeqCst)
}
