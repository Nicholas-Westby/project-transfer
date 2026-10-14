//! The Dock icon's menu on macOS. Everything that talks to AppKit, and
//! every `cfg` and `unsafe` that takes, lives in this file; elsewhere the
//! menu keeps its items and clicks and shows nothing.

use crate::model::{InstanceId, ProjectId, TransferKind};
use std::sync::{Arc, Mutex, MutexGuard};

/// One item of the Dock menu.
#[derive(Clone, Debug, PartialEq)]
pub struct DockItem {
    pub label: String,
    pub kind: TransferKind,
    pub project: ProjectId,
    pub peer: InstanceId,
}

#[derive(Default)]
struct Shared {
    /// What the menu offers, top first.
    items: Vec<DockItem>,
    /// The items of the menu last shown, which an item's tag points into,
    /// so a click means what was on screen even if the items changed since.
    #[cfg(target_os = "macos")]
    shown: Vec<DockItem>,
    /// The item picked, until the window takes it.
    clicked: Option<DockItem>,
    /// Asked to draw when an item is picked, so the window notices.
    ctx: Option<egui::Context>,
}

/// The Dock menu. Cheap to clone; clones share the same menu.
#[derive(Clone, Default)]
pub struct Dock {
    shared: Arc<Mutex<Shared>>,
}

impl Dock {
    /// Adds the menu to the Dock icon. Call on the main thread once the
    /// window is open. Off macOS the menu only keeps its items.
    pub fn install(ctx: &egui::Context) -> Dock {
        #[cfg(target_os = "macos")]
        let dock = Dock {
            shared: mac::install(),
        };
        #[cfg(not(target_os = "macos"))]
        let dock = Dock::default();
        dock.lock().ctx = Some(ctx.clone());
        dock
    }

    /// The items from now on, top first; none hides them.
    pub fn set_items(&self, items: Vec<DockItem>) {
        let mut s = self.lock();
        if s.items != items {
            s.items = items;
        }
    }

    /// The labels the menu offers now, top first.
    pub fn items(&self) -> Vec<String> {
        self.lock().items.iter().map(|i| i.label.clone()).collect()
    }

    /// The item picked since the last call, if any.
    pub fn take_click(&self) -> Option<DockItem> {
        self.lock().clicked.take()
    }

    /// Picks the item with this label, as a click on it in the menu does.
    pub fn click(&self, label: &str) {
        let item = self.lock().items.iter().find(|i| i.label == label).cloned();
        if let Some(item) = item {
            pick(&self.shared, item);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }
}

/// A panic elsewhere must not take the menu down with it.
fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn pick(shared: &Mutex<Shared>, item: DockItem) {
    let ctx = {
        let mut s = lock(shared);
        s.clicked = Some(item);
        s.ctx.clone()
    };
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{Shared, lock, pick};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{MainThreadMarker, ffi, sel};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::NSString;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use tracing::{error, warn};

    /// `applicationDockMenu:` and the items' action, as AppKit calls them.
    type MenuFn = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> *mut AnyObject;
    type ActionFn = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

    /// AppKit's calls carry nothing of ours, so they find the menu here.
    static SHARED: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();

    /// Set once the delegate answers for the menu; until then, each call
    /// to `install` tries again.
    static ADDED: AtomicBool = AtomicBool::new(false);

    /// Teaches the app's delegate, which winit set up, to answer for the
    /// Dock menu, once. Every call gets the same menu.
    pub(super) fn install() -> Arc<Mutex<Shared>> {
        let shared = SHARED.get_or_init(Arc::default).clone();
        if !ADDED.load(Ordering::Relaxed) && add_methods() {
            ADDED.store(true, Ordering::Relaxed);
        }
        shared
    }

    /// True once the delegate answers for the menu.
    fn add_methods() -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            warn!("the Dock menu was not set up: it must be set up on the main thread");
            return false;
        };
        let app = NSApplication::sharedApplication(mtm);
        let Some(delegate) = app.delegate() else {
            warn!("the Dock menu was not set up: the app has no delegate yet");
            return false;
        };
        let object: &AnyObject = (*delegate).as_ref();
        let class = object.class() as *const AnyClass as *mut AnyClass;
        // SAFETY: each function has the signature its type encoding gives:
        // an object returned for the app passed in ("@@:@"), and nothing
        // returned for the menu item that sends the action ("v@:@"). Adding
        // a method only fails when the class has one of that name already,
        // and then the class is left as it was.
        let (menu, action) = unsafe {
            (
                ffi::class_addMethod(
                    class,
                    sel!(applicationDockMenu:),
                    std::mem::transmute::<MenuFn, Imp>(dock_menu),
                    c"@@:@".as_ptr(),
                ),
                ffi::class_addMethod(
                    class,
                    sel!(projectTransferDockItem:),
                    std::mem::transmute::<ActionFn, Imp>(picked),
                    c"v@:@".as_ptr(),
                ),
            )
        };
        if !(menu.as_bool() && action.as_bool()) {
            // A later winit could give its delegate a Dock menu of its own.
            warn!("the Dock menu was not set up: the app's delegate already has one");
            return false;
        }
        // AppKit looks at what its delegate answers to when it is set.
        app.setDelegate(Some(&delegate));
        true
    }

    /// `applicationDockMenu:`: the items, or none. A panic must not unwind
    /// into AppKit's menu code, so it is caught and logged.
    extern "C-unwind" fn dock_menu(
        this: *mut AnyObject,
        _: Sel,
        _: *mut AnyObject,
    ) -> *mut AnyObject {
        catch_unwind(AssertUnwindSafe(|| menu(this))).unwrap_or_else(|_| {
            error!("the Dock menu failed while it was opening");
            std::ptr::null_mut()
        })
    }

    fn menu(this: *mut AnyObject) -> *mut AnyObject {
        let (Some(mtm), Some(shared)) = (MainThreadMarker::new(), SHARED.get()) else {
            return std::ptr::null_mut();
        };
        let labels = {
            let mut s = lock(shared);
            s.shown = s.items.clone();
            s.shown.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
        };
        if labels.is_empty() {
            return std::ptr::null_mut();
        }
        let menu = NSMenu::new(mtm);
        for (i, label) in labels.iter().enumerate() {
            // SAFETY: the action names the method `add_methods` gave the
            // delegate's class, and the target is that delegate, which
            // winit keeps for as long as the app runs.
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(label),
                    Some(sel!(projectTransferDockItem:)),
                    &NSString::new(),
                );
                item.setTarget(this.as_ref());
                item
            };
            item.setTag(i as isize);
            menu.addItem(&item);
        }
        Retained::autorelease_return(menu).cast()
    }

    /// The items' action: brings the window forward and tells it which.
    extern "C-unwind" fn picked(_: *mut AnyObject, _: Sel, sender: *mut AnyObject) {
        if catch_unwind(AssertUnwindSafe(|| pick_item(sender))).is_err() {
            error!("the Dock menu failed while an item was being picked");
        }
    }

    fn pick_item(sender: *mut AnyObject) {
        let (Some(mtm), Some(shared)) = (MainThreadMarker::new(), SHARED.get()) else {
            return;
        };
        // SAFETY: AppKit passes the object that sent the action, or nil.
        let sender = unsafe { sender.as_ref() };
        let Some(item) = sender.and_then(|o| o.downcast_ref::<NSMenuItem>()) else {
            return;
        };
        let chosen = {
            let s = lock(shared);
            usize::try_from(item.tag())
                .ok()
                .and_then(|i| s.shown.get(i).cloned())
        };
        let Some(chosen) = chosen else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        // Only a running app has a window to bring forward; a test has none.
        if app.isRunning() {
            app.unhide(None);
            #[allow(deprecated)] // `activate` needs macOS 14.
            app.activateIgnoringOtherApps(true);
            let windows = app.windows();
            for i in 0..windows.count() {
                let window = windows.objectAtIndex(i);
                if window.isMiniaturized() {
                    window.deminiaturize(None);
                }
                window.makeKeyAndOrderFront(None);
            }
        }
        pick(shared, chosen);
    }
}
