//! The Dock menu's AppKit side, on the main thread as AppKit needs. A
//! stand-in for winit's app delegate gets the menu, and picking an item
//! reaches the window's side.

#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::{AnyObject, ClassBuilder, NSObject};
    use objc2::{ClassType, MainThreadMarker, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSMenu};
    use project_transfer::dock::{Dock, DockItem};
    use project_transfer::model::TransferKind;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let items = |labels: &[&str]| -> Vec<DockItem> {
        let item = |label: &&str| DockItem {
            label: label.to_string(),
            kind: TransferKind::Push,
            project: uuid::Uuid::from_u128(10),
            peer: uuid::Uuid::from_u128(2),
        };
        labels.iter().map(item).collect()
    };

    let mtm = MainThreadMarker::new().expect("a test without the harness runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // winit's delegate class answers neither the menu nor its action.
    let class = ClassBuilder::new(c"PretendWinitDelegate", NSObject::class())
        .expect("a new class name")
        .register();
    let delegate: Retained<AnyObject> = unsafe { msg_send![class, new] };
    let _: () = unsafe { msg_send![&*app, setDelegate: &*delegate] };
    let answers = |delegate: &AnyObject| -> bool {
        unsafe { msg_send![delegate, respondsToSelector: sel!(applicationDockMenu:)] }
    };
    assert!(!answers(&delegate));

    let dock = Dock::install(&egui::Context::default());
    assert!(answers(&delegate), "the delegate answers for the Dock menu");
    // winit finds its state through the delegate, so it must stay the same
    // object.
    let now: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
    assert!(now.is_some_and(|d| std::ptr::eq(&*d, &*delegate)));
    autoreleasepool(|_| {
        let menu: Option<Retained<NSMenu>> =
            unsafe { msg_send![&*delegate, applicationDockMenu: &*app] };
        assert!(menu.is_none(), "nothing to offer, no items");

        dock.set_items(items(&[
            "Push garden-planner to Desk",
            "Pull garden-planner from Desk",
        ]));
        let menu: Option<Retained<NSMenu>> =
            unsafe { msg_send![&*delegate, applicationDockMenu: &*app] };
        let menu = menu.expect("a menu");
        assert_eq!(menu.numberOfItems(), 2);
        let pull = menu.itemAtIndex(1).expect("a second item");
        assert_eq!(pull.title().to_string(), "Pull garden-planner from Desk");
        let target = pull.target().expect("a target");
        assert!(std::ptr::eq(&*target, &*delegate));
        let _: () = unsafe { msg_send![&*delegate, projectTransferDockItem: &*pull] };
        assert_eq!(
            dock.take_click().map(|i| i.label).as_deref(),
            Some("Pull garden-planner from Desk")
        );
        assert_eq!(dock.take_click(), None);

        // No sender, or one that is no menu item, is ignored. The action
        // catches its own panics, so a sender it trips over would pass for
        // one it ignores; count them.
        static PANICS: AtomicUsize = AtomicUsize::new(0);
        let report = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::SeqCst);
            report(info);
        }));
        let none = std::ptr::null::<AnyObject>();
        let _: () = unsafe { msg_send![&*delegate, projectTransferDockItem: none] };
        assert_eq!(dock.take_click(), None);
        let _: () = unsafe { msg_send![&*delegate, projectTransferDockItem: &*app] };
        assert_eq!(dock.take_click(), None);
        assert_eq!(PANICS.load(Ordering::SeqCst), 0);

        // The items change while the menu is open: a click still means
        // what was on screen.
        dock.set_items(items(&["Sync garden-planner with Desk"]));
        let push = menu.itemAtIndex(0).expect("a first item");
        let _: () = unsafe { msg_send![&*delegate, projectTransferDockItem: &*push] };
        assert_eq!(
            dock.take_click().map(|i| i.label).as_deref(),
            Some("Push garden-planner to Desk")
        );
    });
    // A second install shares the first one's menu.
    let again = Dock::install(&egui::Context::default());
    again.click("Sync garden-planner with Desk");
    assert_eq!(
        dock.take_click().map(|i| i.label).as_deref(),
        Some("Sync garden-planner with Desk")
    );
    println!("dock menu: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
