//! Following a running transfer: bytes so far, the file in hand, and how
//! long the rest should take.

use super::state::{StateHandle, TransferState};
use crate::transfer::Progress;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

/// How far back the speed is measured: long enough to ride over a run of
/// small files, short enough to follow a change of pace.
const WINDOW: Duration = Duration::from_secs(30);
/// Before this, the speed says more about starting up than about the transfer.
const SETTLE: Duration = Duration::from_secs(5);
/// Samples closer together than this replace each other.
const STEP: Duration = Duration::from_secs(1);

/// Estimates the time left from the bytes moved lately.
pub struct Pace {
    started: Instant,
    /// (when, bytes done so far), oldest first.
    samples: VecDeque<(Instant, u64)>,
}

impl Pace {
    pub fn new(started: Instant) -> Pace {
        Pace {
            started,
            samples: VecDeque::new(),
        }
    }

    pub fn record(&mut self, at: Instant, done: u64) {
        // The newest sample keeps moving up until the one before it is a step
        // old, so the list holds about one a second however often progress
        // comes, and the latest bytes are always in it.
        let n = self.samples.len();
        if n >= 2 && at.duration_since(self.samples[n - 2].0) < STEP {
            self.samples[n - 1] = (at, done);
        } else {
            self.samples.push_back((at, done));
        }
        // The oldest stays the newest one a full window back, so once there
        // is one the speed covers the whole window.
        while self.samples.len() > 2 && at.duration_since(self.samples[1].0) >= WINDOW {
            self.samples.pop_front();
        }
    }

    pub fn left(&self, at: Instant, total: u64) -> Option<Duration> {
        if at.duration_since(self.started) < SETTLE {
            return None;
        }
        let (t0, b0) = *self.samples.front()?;
        let (t1, b1) = *self.samples.back()?;
        let secs = t1.duration_since(t0).as_secs_f64();
        if b1 <= b0 || secs <= 0.0 {
            return None;
        }
        let rate = (b1 - b0) as f64 / secs;
        // A crawl against a huge remainder overflows a Duration; that is no
        // reason to stop following the transfer, so it just has no estimate.
        Duration::try_from_secs_f64(total.saturating_sub(b1) as f64 / rate).ok()
    }
}

/// Keeps the running state up to date until the transfer stops sending.
pub(super) fn follow(ui: StateHandle, mut rx: UnboundedReceiver<Progress>, started: Instant) {
    tokio::spawn(async move {
        let mut pace = Pace::new(started);
        while let Some(p) = rx.recv().await {
            let Progress::File { rel, bytes_done } = p else {
                continue;
            };
            let now = Instant::now();
            pace.record(now, bytes_done);
            // Only while running: the final state is set by the caller.
            ui.update(|s| {
                if let TransferState::Running {
                    done,
                    total,
                    current,
                    left,
                    ..
                } = &mut s.transfer
                {
                    *done = bytes_done;
                    *current = rel;
                    *left = pace.left(now, *total);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{Pace, follow};
    use crate::core::state::{StateHandle, TransferState, UiState};
    use crate::transfer::Progress;
    use std::time::{Duration, Instant};

    fn at(start: Instant, secs: f64) -> Instant {
        start + Duration::from_secs_f64(secs)
    }

    #[test]
    fn no_estimate_until_the_transfer_settles() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        p.record(at(t0, 1.0), 1_000_000);
        p.record(at(t0, 2.0), 2_000_000);
        assert_eq!(p.left(at(t0, 2.0), 10_000_000), None);
    }

    #[test]
    fn steady_speed_gives_the_plain_estimate() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        for i in 0..=100 {
            p.record(at(t0, i as f64 * 0.1), i * 100_000); // 1 MB/s
        }
        let left = p.left(at(t0, 10.0), 30_000_000).unwrap();
        assert!((left.as_secs_f64() - 20.0).abs() < 0.5, "{left:?}");
    }

    #[test]
    fn the_estimate_follows_the_last_half_minute() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        // 60 s at 1 MB/s, then 60 s at 4 MB/s.
        for i in 0..=600u64 {
            p.record(at(t0, i as f64 * 0.1), i * 100_000);
        }
        for i in 1..=600u64 {
            p.record(at(t0, 60.0 + i as f64 * 0.1), 60_000_000 + i * 400_000);
        }
        let left = p.left(at(t0, 120.0), 300_000_000 + 400_000_000).unwrap();
        // 400 MB left at 4 MB/s.
        assert!((left.as_secs_f64() - 100.0).abs() < 5.0, "{left:?}");
    }

    #[test]
    fn nothing_to_send_or_nothing_moving_gives_no_estimate() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        p.record(at(t0, 6.0), 0);
        p.record(at(t0, 8.0), 0);
        assert_eq!(p.left(at(t0, 8.0), 0), None);
        assert_eq!(p.left(at(t0, 8.0), 5_000_000), None);
    }

    #[test]
    fn samples_stay_few_however_often_progress_comes() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        for i in 0..100_000u64 {
            p.record(at(t0, i as f64 * 0.001), i);
        }
        assert!(p.samples.len() <= 40, "{}", p.samples.len());
    }

    /// Lets the listener task, which runs on this same thread, take what was sent.
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn progress_updates_the_running_state_until_the_caller_ends_it() {
        let me = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::nil(),
            "name": "Desk",
            "projects_folder": "/",
        }))
        .unwrap();
        let ui = StateHandle::new(UiState::new(me, 0), None);
        // Past the settling time, so a second sample is all an estimate needs.
        // A Windows clock counts from boot, so it may not reach back that far.
        let started = Instant::now()
            .checked_sub(Duration::from_secs(10))
            .unwrap_or_else(Instant::now);
        ui.update(|s| {
            s.transfer = TransferState::Running {
                done: 0,
                total: 10_000,
                files: 3,
                current: String::new(),
                started,
                left: None,
            }
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        follow(ui.clone(), rx, started);
        let file = |rel: &str, bytes_done| Progress::File {
            rel: rel.into(),
            bytes_done,
        };
        let shown = || match ui.lock().transfer.clone() {
            TransferState::Running {
                done,
                current,
                left,
                ..
            } => (done, current, left),
            other => panic!("no longer running: {other:?}"),
        };

        tx.send(file("a.txt", 1_000)).unwrap();
        settle().await;
        assert_eq!(
            shown(),
            (1_000, "a.txt".into(), None),
            "one sample, no speed"
        );

        // A speed needs time between the samples, and this is the real clock.
        tokio::time::sleep(Duration::from_millis(20)).await;
        tx.send(file("b.txt", 2_000)).unwrap();
        settle().await;
        let (done, current, left) = shown();
        assert_eq!((done, current.as_str()), (2_000, "b.txt"));
        assert!(left.is_some());

        // The caller sets the final state; a late event must not undo it.
        let end = TransferState::Failed("stopped".into());
        ui.update(|s| s.transfer = end.clone());
        tx.send(file("c.txt", 3_000)).unwrap();
        settle().await;
        assert_eq!(ui.lock().transfer, end);
    }
}
