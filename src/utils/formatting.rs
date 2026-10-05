use chrono::{DateTime, Datelike, Local, NaiveDateTime, Timelike, Utc};

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

const MESES: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun",
    "jul", "ago", "sep", "oct", "nov", "dic",
];

pub fn format_added_at(added_at: Option<DateTime<Utc>>) -> String {
    match added_at {
        Some(dt) => format!("{} {} {}", dt.day(), MESES[dt.month0() as usize], dt.year()),
        None => "-".to_string(),
    }
}

/// Fecha y hora local de una marca UTC de SQLite, p. ej. "3 oct 2026 14:05".
pub fn format_last_played(last_played: Option<NaiveDateTime>) -> String {
    match last_played {
        Some(naive) => {
            let local = naive.and_utc().with_timezone(&Local);
            format!("{} {} {} {:02}:{:02}", local.day(), MESES[local.month0() as usize], local.year(), local.hour(), local.minute())
        }
        None => "-".to_string(),
    }
}