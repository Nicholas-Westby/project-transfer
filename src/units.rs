//! Sizes, speeds and durations as people read them.

use std::time::Duration;

pub fn size(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1000 {
        return format!("{n} bytes");
    }
    let mut v = n as f64 / 1000.0;
    let mut unit = 0;
    // Judged by the rounded value, or 999.96 KB would print as "1000.0 KB".
    while (v * 10.0).round() >= 10_000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

/// How long something took: tenths only while they still matter.
pub fn took(d: Duration) -> String {
    // Not up to 10 s: 9.96 s would print as "10.0 s".
    if d < Duration::from_millis(9_950) {
        return format!("{:.1} s", d.as_secs_f64());
    }
    clock(Duration::from_secs_f64(d.as_secs_f64().round()))
}

/// A running clock in whole seconds.
pub fn clock(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..60 => format!("{secs} s"),
        60..3600 => format!("{} min {} s", secs / 60, secs % 60),
        _ => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
    }
}

/// An estimate, rounded so it does not twitch from one second to the next.
pub fn about(d: Duration) -> String {
    let secs = d.as_secs_f64();
    // Seconds round up to a multiple of five, so past 55 they would read "about 60 s".
    if secs <= 55.0 {
        let five = ((secs / 5.0).ceil() as u64).max(1) * 5;
        return format!("about {five} s");
    }
    let mins = ((secs / 60.0).round() as u64).max(1);
    if mins < 60 {
        return format!("about {mins} min");
    }
    let mins = (mins + 2) / 5 * 5;
    match mins % 60 {
        0 => format!("about {} h", mins / 60),
        m => format!("about {} h {m} min", mins / 60),
    }
}

/// Average speed, once enough moved for it to mean something.
pub fn speed(bytes: u64, took: Duration) -> Option<String> {
    let secs = took.as_secs_f64();
    (bytes >= 1_000_000 && secs >= 1.0).then(|| format!("{}/s", size((bytes as f64 / secs) as u64)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn sizes_read_simply() {
        assert_eq!(size(12), "12 bytes");
        assert_eq!(size(1_500), "1.5 KB");
        assert_eq!(size(2_300_000), "2.3 MB");
    }

    #[test]
    fn a_size_that_rounds_up_to_the_next_unit_is_written_in_it() {
        assert_eq!(size(999_949), "999.9 KB");
        assert_eq!(size(999_999), "1.0 MB");
        assert_eq!(size(999_999_999), "1.0 GB");
    }

    #[test]
    fn short_times_keep_a_tenth_and_long_ones_read_in_minutes() {
        assert_eq!(took(Duration::from_millis(3_100)), "3.1 s");
        assert_eq!(took(Duration::from_millis(42_400)), "42 s");
        assert_eq!(took(Duration::from_millis(2_409_300)), "40 min 9 s");
        assert_eq!(took(s(3_900)), "1 h 5 min");
    }

    #[test]
    fn a_time_that_rounds_up_to_ten_seconds_has_no_tenth() {
        assert_eq!(took(Duration::from_millis(9_940)), "9.9 s");
        assert_eq!(took(Duration::from_millis(9_960)), "10 s");
    }

    #[test]
    fn the_clock_counts_whole_seconds() {
        assert_eq!(clock(Duration::from_millis(7_900)), "7 s");
        assert_eq!(clock(s(754)), "12 min 34 s");
        assert_eq!(clock(s(3_600)), "1 h 0 min");
    }

    #[test]
    fn estimates_round_coarsely() {
        assert_eq!(about(s(0)), "about 5 s");
        assert_eq!(about(s(41)), "about 45 s");
        assert_eq!(about(s(59)), "about 1 min");
        assert_eq!(about(s(90)), "about 2 min");
        assert_eq!(about(s(1_500)), "about 25 min");
        assert_eq!(about(s(3_570)), "about 1 h");
        assert_eq!(about(s(3_900)), "about 1 h 5 min");
        assert_eq!(about(s(7_300)), "about 2 h");
    }

    #[test]
    fn an_estimate_just_under_a_minute_reads_as_a_minute_not_sixty_seconds() {
        assert_eq!(about(s(55)), "about 55 s");
        assert_eq!(about(s(56)), "about 1 min");
    }

    #[test]
    fn speed_needs_enough_to_go_on() {
        assert_eq!(speed(9_973_000_000, s(2_409)).as_deref(), Some("4.1 MB/s"));
        assert_eq!(speed(14, s(3)), None);
        assert_eq!(speed(5_000_000, Duration::from_millis(400)), None);
    }
}
