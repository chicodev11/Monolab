//! Announcement schedules, read the same way by the Pumpkin add-on and (in Java) the Paper add-on's
//! `StudioFeatures`. A file without `schedule` is `every`: its next message every `everySeconds`.
//!
//! - `every`: `everySeconds` (10 to a day, 300 if missing), to everyone it shows to.
//! - `daily`: at each `times` ("HH:MM", the server's clock), on `days` (`mon`..`sun`; none = every day).
//! - `after-start`: once `delaySeconds` after the studio loads, then every `repeatSeconds` (0 = only once).
//! - `after-join`: `delaySeconds` after each player joins, to that player only.

use crate::fields::{self, Data};

pub const SCHEDULES: [&str; 4] = ["every", "daily", "after-start", "after-join"];
pub const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DAY: i64 = 24 * 60 * 60;

pub fn schedule(data: &Data) -> String {
    fields::choice(data, "schedule", &SCHEDULES, "every")
}

pub fn every_seconds(data: &Data) -> i64 {
    fields::number(data, "everySeconds", 300, 10, DAY)
}

pub fn delay_seconds(data: &Data) -> i64 {
    fields::number(data, "delaySeconds", 60, 0, DAY)
}

/// 0 = once; anything else is at least 10 seconds, like `everySeconds`.
pub fn repeat_seconds(data: &Data) -> i64 {
    match fields::number(data, "repeatSeconds", 0, 0, DAY) {
        0 => 0,
        some => some.max(10),
    }
}

/// "8:05" and "08:05" both read as "08:05"; anything that isn't a clock time is left out.
pub fn clock(text: &str) -> Option<String> {
    let (hour, minute) = text.trim().split_once(':')?;
    if hour.is_empty() || hour.len() > 2 || minute.len() != 2 || !hour.bytes().chain(minute.bytes()).all(|one| one.is_ascii_digit()) {
        return None;
    }
    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
    (hour < 24 && minute < 60).then(|| format!("{hour:02}:{minute:02}"))
}

pub fn times(data: &Data) -> Vec<String> {
    let mut out: Vec<String> = fields::lines(data, "times").iter().filter_map(|one| clock(one)).collect();
    out.sort();
    out.dedup();
    out
}

/// The weekdays picked, in order; empty = every day.
pub fn days(data: &Data) -> Vec<&'static str> {
    let picked: Vec<String> = fields::lines(data, "days").iter().map(|one| one.trim().to_ascii_lowercase()).collect();
    DAYS.into_iter().filter(|day| picked.iter().any(|one| one.starts_with(day))).collect()
}

/// The weekday of a day counted from 1970-01-01 (a Thursday).
pub fn weekday(days_since_epoch: i64) -> &'static str {
    DAYS[(days_since_epoch + 3).rem_euclid(7) as usize]
}

/// A `daily` announcement is due in this minute ("HH:MM") of this weekday (`mon`..).
pub fn daily_due(data: &Data, minute: &str, day: &str) -> bool {
    let days = days(data);
    (days.is_empty() || days.contains(&day)) && times(data).iter().any(|one| one == minute)
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    if data
        .get("schedule")
        .and_then(|one| one.as_str())
        .is_some_and(|given| !SCHEDULES.contains(&given.trim().to_ascii_lowercase().as_str()))
    {
        out.push(format!("\"schedule\" must be one of: {}.", SCHEDULES.join(", ")));
    }
    if schedule(data) == "daily" {
        let given = fields::lines(data, "times");
        if given.iter().any(|one| clock(one).is_none()) {
            out.push("\"times\" must be 24-hour clock times like 08:00.".into());
        }
        if times(data).is_empty() {
            out.push("Add at least one time.".into());
        }
        if fields::lines(data, "days").iter().any(|one| !DAYS.iter().any(|day| one.trim().to_ascii_lowercase().starts_with(day))) {
            out.push(format!("\"days\" must be some of: {}.", DAYS.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(value: serde_json::Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn old_files_repeat() {
        let old = data(json!({ "everySeconds": 60 }));
        assert_eq!(schedule(&old), "every");
        assert_eq!(every_seconds(&old), 60);
        assert_eq!(every_seconds(&data(json!({ "everySeconds": 2 }))), 10);
        assert!(problems(&old).is_empty());
    }

    #[test]
    fn reads_clock_times() {
        assert_eq!(clock("8:05").as_deref(), Some("08:05"));
        assert_eq!(clock(" 20:30 ").as_deref(), Some("20:30"));
        assert_eq!(clock("24:00"), None);
        assert_eq!(clock("8:5"), None);
        assert_eq!(clock("noon"), None);
        let daily = data(json!({ "schedule": "daily", "times": ["20:30", "8:00", "08:00"], "days": ["sat", "Sunday"] }));
        assert_eq!(times(&daily), vec!["08:00", "20:30"]);
        assert_eq!(days(&daily), vec!["sat", "sun"]);
        assert!(daily_due(&daily, "08:00", "sun"));
        assert!(!daily_due(&daily, "08:00", "mon"));
        assert!(!daily_due(&daily, "08:01", "sat"));
        assert!(daily_due(&data(json!({ "schedule": "daily", "times": ["08:00"] })), "08:00", "wed"));
    }

    #[test]
    fn weekdays_from_epoch() {
        assert_eq!(weekday(0), "thu");
        assert_eq!(weekday(4), "mon");
        assert_eq!(weekday(-1), "wed");
    }

    #[test]
    fn delays() {
        let start = data(json!({ "schedule": "after-start", "delaySeconds": 30 }));
        assert_eq!(delay_seconds(&start), 30);
        assert_eq!(repeat_seconds(&start), 0);
        assert_eq!(repeat_seconds(&data(json!({ "repeatSeconds": 3 }))), 10);
        assert_eq!(delay_seconds(&data(json!({}))), 60);
    }

    #[test]
    fn finds_problems() {
        assert_eq!(problems(&data(json!({ "schedule": "hourly" }))).len(), 1);
        assert_eq!(problems(&data(json!({ "schedule": "daily" }))), vec!["Add at least one time."]);
        assert_eq!(problems(&data(json!({ "schedule": "daily", "times": ["25:00"], "days": ["someday"] }))).len(), 3);
    }
}
