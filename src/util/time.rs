use std::time::Duration;

pub fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

#[allow(dead_code)]
pub fn duration_to_clock_time(d: Duration) -> Option<gstreamer::ClockTime> {
    Some(gstreamer::ClockTime::from_nseconds(d.as_nanos() as u64))
}

#[allow(dead_code)]
pub fn clock_time_to_duration(ct: gstreamer::ClockTime) -> Duration {
    Duration::from_nanos(ct.nseconds())
}
