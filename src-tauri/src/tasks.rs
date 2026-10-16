
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub server_id: String,
    pub kind: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub message: String,
    pub schedule: String,
    #[serde(default)]
    pub interval_minutes: u32,
    #[serde(default)]
    pub daily_time: String,
    #[serde(default)]
    pub last_run_at: u64,
    /// 0 = Monday … 6 = Sunday, for the weekly schedule.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// Minutes of in-game warning before a restart or stop.
    #[serde(default)]
    pub warn_minutes: u32,
    #[serde(default)]
    pub last_result: String,
    #[serde(default)]
    pub last_ok: bool,
}

fn yes() -> bool {
    true
}

pub fn tidy(mut task: Task) -> Result<Task, String> {
    task.name = task.name.trim().to_string();
    if task.name.is_empty() {
        return Err("Enter a task name.".into());
    }
    match task.kind.as_str() {
        "command" => {
            task.command = task.command.trim().trim_start_matches('/').to_string();
            if task.command.is_empty() {
                return Err("Enter a command to send.".into());
            }
            if task.server_id.trim().is_empty() {
                return Err("Choose a server to send the command to.".into());
            }
        }
        "notify" => {
            task.message = task.message.trim().to_string();
            if task.message.is_empty() {
                return Err("Enter a message.".into());
            }
        }
        "backup" => {
            if task.server_id.trim().is_empty() {
                return Err("Choose a server to back up.".into());
            }
        }
        "restart" | "start" | "stop" => {
            if task.server_id.trim().is_empty() {
                return Err("Pick a server for this task.".into());
            }
            task.message = task.message.trim().to_string();
            task.warn_minutes = if task.kind == "start" { 0 } else { task.warn_minutes.min(30) };
        }
        _ => return Err("Unknown task type.".into()),
    }
    match task.schedule.as_str() {
        "interval" => {
            if task.interval_minutes == 0 {
                return Err("Set how many minutes between runs.".into());
            }
            task.interval_minutes = task.interval_minutes.clamp(1, 60 * 24 * 30);
        }
        "daily" | "weekly" => {
            parse_daily(&task.daily_time)
                .ok_or("Enter the time in 24‑hour format (e.g. 04:30).")?;
            if task.schedule == "weekly" {
                task.weekdays.retain(|day| *day < 7);
                task.weekdays.sort_unstable();
                task.weekdays.dedup();
                if task.weekdays.is_empty() {
                    return Err("Pick at least one day.".into());
                }
            }
        }
        _ => return Err("Unknown repeat mode.".into()),
    }
    Ok(task)
}

pub fn parse_daily(value: &str) -> Option<u32> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour: u32 = hour.trim().parse().ok()?;
    let minute: u32 = minute.trim().parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(hour * 60 + minute)
}

pub fn is_due(task: &Task, now: u64, minutes_today: u32, weekday: u8) -> bool {
    if !task.enabled {
        return false;
    }
    match task.schedule.as_str() {
        "weekly" if !task.weekdays.contains(&weekday) => false,
        "weekly" => is_due(&Task { schedule: "daily".into(), ..task.clone() }, now, minutes_today, weekday),
        "interval" => {
            let gap = u64::from(task.interval_minutes) * 60;
            gap > 0 && now.saturating_sub(task.last_run_at) >= gap
        }
        "daily" => {
            let Some(at) = parse_daily(&task.daily_time) else {
                return false;
            };
            if minutes_today < at {
                return false;
            }
            let today_started = now - u64::from(minutes_today) * 60;
            task.last_run_at < today_started + u64::from(at) * 60
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_due, parse_daily, tidy, Task};

    fn sample(kind: &str, schedule: &str) -> Task {
        Task {
            id: "t1".into(),
            name: "test".into(),
            enabled: true,
            server_id: "s1".into(),
            kind: kind.into(),
            command: "say hi".into(),
            message: "hi".into(),
            schedule: schedule.into(),
            interval_minutes: 30,
            daily_time: "04:30".into(),
            last_run_at: 0,
            weekdays: Vec::new(),
            warn_minutes: 0,
            last_result: String::new(),
            last_ok: false,
        }
    }

    #[test]
    fn runs_a_weekly_task_only_on_its_days() {
        let mut task = sample("restart", "weekly");
        task.weekdays = vec![0, 4];
        let now = 1_700_000_000 + 5 * 3600;
        assert!(is_due(&task, now, 300, 4));
        assert!(!is_due(&task, now, 300, 2));
        task.weekdays.clear();
        assert!(tidy(task).is_err());
    }

    #[test]
    fn a_backup_task_needs_a_server() {
        let mut task = sample("backup", "daily");
        task.daily_time = "04:00".into();
        task.server_id = String::new();
        assert!(tidy(task.clone()).is_err());
        task.server_id = "instance-1".into();
        task.command = String::new();
        task.message = String::new();
        let done = tidy(task).unwrap();
        assert_eq!(done.kind, "backup");
        assert_eq!(done.server_id, "instance-1");
    }

    #[test]
    fn reads_the_clock() {
        assert_eq!(parse_daily("04:30"), Some(270));
        assert_eq!(parse_daily("00:00"), Some(0));
        assert_eq!(parse_daily("23:59"), Some(1439));
        assert_eq!(parse_daily("24:00"), None);
        assert_eq!(parse_daily("morning"), None);
    }

    #[test]
    fn strips_the_leading_slash() {
        let task = tidy(Task {
            command: "/say hi".into(),
            ..sample("command", "interval")
        })
        .expect("should be fixable");
        assert_eq!(task.command, "say hi");
    }

    #[test]
    fn refuses_an_empty_command() {
        let result = tidy(Task {
            command: "   ".into(),
            ..sample("command", "interval")
        });
        assert!(result.is_err());
    }

    #[test]
    fn waits_out_the_interval() {
        let mut task = sample("command", "interval");
        task.last_run_at = 1_000;
        assert!(!is_due(&task, 1_000 + 1_799, 0, 0));
        assert!(is_due(&task, 1_000 + 1_800, 0, 0));
    }

    #[test]
    fn runs_a_daily_task_once() {
        let mut task = sample("notify", "daily");
        let midnight = 1_700_000_000_u64;
        let now = midnight + 5 * 3600;
        task.last_run_at = 0;
        assert!(is_due(&task, now, 300, 0));
        task.last_run_at = now;
        assert!(!is_due(&task, now, 300, 0));
    }

    #[test]
    fn skips_a_daily_task_before_its_time() {
        let mut task = sample("notify", "daily");
        task.last_run_at = 0;
        assert!(!is_due(&task, 1_700_000_000 + 4 * 3600, 240, 0));
    }

    #[test]
    fn ignores_a_task_that_is_turned_off() {
        let mut task = sample("command", "interval");
        task.enabled = false;
        task.last_run_at = 0;
        assert!(!is_due(&task, 999_999, 0, 0));
    }
}
