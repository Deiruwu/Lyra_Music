use chrono::{DateTime, Datelike, Utc};

pub fn format_duration(seconds: i32) -> String {
    let mins = seconds / 60;
    let secs = seconds % 60;
    format!("{:02}:{:02}", mins, secs)
}

pub fn format_added_at(added_at: Option<DateTime<Utc>>) -> String {
    const MESES: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun",
        "jul", "ago", "sep", "oct", "nov", "dic",
    ];
    match added_at {
        Some(dt) => format!("{} {} {}", dt.day(), MESES[dt.month0() as usize], dt.year()),
        None => "-".to_string(),
    }
}