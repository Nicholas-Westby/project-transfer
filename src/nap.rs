//! Keeping macOS from napping the app while nobody can see its window.
//!
//! macOS puts an app whose window is covered or hidden into App Nap: its
//! threads drop to the lowest priority, and its disk reads and writes wait
//! while other programs use the disk. Other computers call this one at any
//! time, and on a busy Mac a napping copy took 24 s to list an 850-entry
//! folder for another computer's preview. App Nap stays off for as long as
//! the app runs, since a call can come at any moment, and an idle copy uses
//! no more than a napping one. The `cfg` and `unsafe` this takes live in
//! this file.

/// Keeps App Nap off while it lives; an idle Mac may still sleep. Other
/// systems have no App Nap, so there it holds nothing.
#[must_use = "App Nap is allowed again as soon as this is dropped"]
pub struct NoNap {
    #[cfg(target_os = "macos")]
    _activity: mac::Activity,
}

impl NoNap {
    pub fn hold() -> NoNap {
        NoNap {
            #[cfg(target_os = "macos")]
            _activity: mac::Activity::begin(),
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    /// Work the user asked for, which App Nap leaves alone, without the
    /// part that keeps an idle Mac awake.
    pub(super) const OPTIONS: NSActivityOptions =
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep;

    /// An activity macOS knows about, ended when dropped. Only `begin`
    /// makes one.
    pub(super) struct Activity(Retained<ProtocolObject<dyn NSObjectProtocol>>);

    impl Activity {
        pub(super) fn begin() -> Activity {
            let reason = NSString::from_str("Answers other computers and runs transfers");
            let info = NSProcessInfo::processInfo();
            Activity(info.beginActivityWithOptions_reason(OPTIONS, &reason))
        }
    }

    impl Drop for Activity {
        fn drop(&mut self) {
            // SAFETY: `endActivity:` takes what `beginActivityWithOptions:reason:`
            // returned, and only `begin` makes an `Activity`, from exactly that.
            unsafe { NSProcessInfo::processInfo().endActivity(&self.0) }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use objc2_foundation::NSActivityOptions;

    #[test]
    fn an_idle_mac_may_still_sleep() {
        for blocks in [
            NSActivityOptions::IdleSystemSleepDisabled,
            NSActivityOptions::IdleDisplaySleepDisabled,
        ] {
            assert!(!mac::OPTIONS.intersects(blocks), "{blocks:?}");
        }
    }

    #[test]
    fn holding_and_letting_go_works() {
        drop(NoNap::hold());
    }
}
