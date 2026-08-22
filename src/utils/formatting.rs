use chrono::{DateTime, Datelike, Utc};

pub fn format_duration(seconds: i32) -> String {
    let mins = seconds / 60;
    let secs = seconds % 60;
    format!("{:02}:{:02}", mins, secs)
}

pub fn format_views(views: i64) -> String {
    const UNITS: [(i64, &str); 3] = [(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")];

    for (threshold, suffix) in UNITS {
        if views >= threshold {
            let value = views as f64 / threshold as f64;
            return if value >= 100.0 {
                format!("{:.0}{}", value, suffix)
            } else {
                format!("{:.1}{}", value, suffix)
            };
        }
    }

    views.to_string()
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