//! Local time for reports, so a reader sees the time on their own clock.

use crate::util::fmt_time_with;

/// The computer's current offset from UTC, in seconds.
pub fn local_offset_secs() -> i64 {
    chrono::Local::now().offset().local_minus_utc() as i64
}

/// A time formatted in the computer's local time zone.
pub fn fmt_local(ts: f64) -> String {
    fmt_time_with(ts, local_offset_secs())
}
