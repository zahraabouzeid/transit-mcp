use std::sync::LazyLock;

use chrono::{
    DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use regex::Regex;

use crate::error::{Result, TransitError};

static CLOCK_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2}):(\d{2})$").expect("valid pattern"));

const LOCAL_FORMATS: [&str; 4] = [
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
];
const OFFSET_FORMATS: [&str; 2] = ["%Y-%m-%dT%H:%M%:z", "%Y-%m-%d %H:%M%:z"];

pub fn parse_user_time(
    value: Option<&str>,
    zone: Tz,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<FixedOffset>>> {
    let Some(text) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let unreadable = || {
        TransitError::new(format!(
            "Could not read the time {text:?}. Use HH:MM for today or an ISO date and time \
             such as 2026-10-28T17:30."
        ))
    };
    if let Some(clock) = CLOCK_TIME.captures(text) {
        let time = NaiveTime::from_hms_opt(
            clock[1].parse().unwrap_or(99),
            clock[2].parse().unwrap_or(99),
            0,
        )
        .ok_or_else(unreadable)?;
        let today = now.with_timezone(&zone).date_naive();
        return in_zone(today.and_time(time), zone)
            .map(Some)
            .ok_or_else(unreadable);
    }
    let text_with_offset = match text.strip_suffix('Z') {
        Some(rest) => format!("{rest}+00:00"),
        None => text.to_owned(),
    };
    if let Ok(moment) = DateTime::parse_from_rfc3339(&text_with_offset) {
        return Ok(Some(moment));
    }
    if let Some(moment) = OFFSET_FORMATS
        .iter()
        .find_map(|format| DateTime::parse_from_str(&text_with_offset, format).ok())
    {
        return Ok(Some(moment));
    }
    let naive = LOCAL_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(text, format).ok())
        .or_else(|| {
            NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .map(|date| date.and_time(NaiveTime::MIN))
        })
        .ok_or_else(unreadable)?;
    in_zone(naive, zone).map(Some).ok_or_else(unreadable)
}

pub fn now_in(zone: Tz) -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&zone).fixed_offset()
}

pub fn to_api_time(moment: DateTime<FixedOffset>) -> String {
    moment
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

pub fn parse_api_time(value: Option<&str>) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value?).ok()
}

pub fn local_minutes(value: Option<&str>, zone: Tz) -> Option<String> {
    parse_api_time(value).map(|moment| format_minutes(moment.with_timezone(&zone).fixed_offset()))
}

pub fn format_minutes(moment: DateTime<FixedOffset>) -> String {
    moment.format("%Y-%m-%dT%H:%M%:z").to_string()
}

pub fn delay_minutes(actual: Option<&str>, scheduled: Option<&str>) -> Option<i64> {
    let difference = parse_api_time(actual)? - parse_api_time(scheduled)?;
    Some(minutes(difference))
}

pub fn minutes(duration: Duration) -> i64 {
    (duration.num_seconds() as f64 / 60.0).round() as i64
}

pub fn parse_db_time(value: Option<&str>, zone: Tz) -> Option<DateTime<FixedOffset>> {
    let naive = NaiveDateTime::parse_from_str(value?, "%y%m%d%H%M").ok()?;
    in_zone(naive, zone)
}

pub fn db_slot(moment: DateTime<FixedOffset>, zone: Tz) -> (String, String) {
    let local = moment.with_timezone(&zone);
    (
        local.format("%y%m%d").to_string(),
        local.format("%H").to_string(),
    )
}

pub fn hour_slots(start: DateTime<FixedOffset>, hours: u8) -> Vec<DateTime<FixedOffset>> {
    let first = start
        .with_minute(0)
        .and_then(|moment| moment.with_second(0))
        .and_then(|moment| moment.with_nanosecond(0))
        .unwrap_or(start);
    (0..hours)
        .map(|offset| first + Duration::hours(i64::from(offset)))
        .collect()
}

fn in_zone(naive: NaiveDateTime, zone: Tz) -> Option<DateTime<FixedOffset>> {
    zone.from_local_datetime(&naive)
        .earliest()
        .map(|moment| moment.fixed_offset())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BERLIN: Tz = chrono_tz::Europe::Berlin;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 28, 12, 0, 0)
            .single()
            .expect("valid date")
    }

    fn parse(text: &str) -> Result<Option<DateTime<FixedOffset>>> {
        parse_user_time(Some(text), BERLIN, now())
    }

    #[test]
    fn clock_time_is_today_in_the_local_zone() {
        let moment = parse("17:30").expect("readable").expect("a time");
        assert_eq!(to_api_time(moment), "2026-10-28T16:30:00Z");
    }

    #[test]
    fn iso_time_without_offset_is_local() {
        let moment = parse("2026-10-28T08:15")
            .expect("readable")
            .expect("a time");
        assert_eq!(to_api_time(moment), "2026-10-28T07:15:00Z");
    }

    #[test]
    fn iso_time_keeps_its_offset() {
        for text in [
            "2026-10-28T08:15:00Z",
            "2026-10-28T08:15Z",
            "2026-10-28T09:15+01:00",
        ] {
            let moment = parse(text).expect("readable").expect("a time");
            assert_eq!(to_api_time(moment), "2026-10-28T08:15:00Z", "{text}");
        }
    }

    #[test]
    fn missing_time_means_now() {
        assert!(
            parse_user_time(None, BERLIN, now())
                .expect("readable")
                .is_none()
        );
        assert!(parse("  ").expect("readable").is_none());
    }

    #[test]
    fn unreadable_time_explains_the_format() {
        for text in ["soon", "25:00", "2026-13-01"] {
            let error = parse(text).expect_err("unreadable").to_string();
            assert!(error.contains("HH:MM"), "{error}");
        }
    }

    #[test]
    fn api_times_are_shown_in_local_minutes() {
        assert_eq!(
            local_minutes(Some("2026-10-28T16:02:00Z"), BERLIN).as_deref(),
            Some("2026-10-28T17:02+01:00")
        );
        assert_eq!(local_minutes(None, BERLIN), None);
    }

    #[test]
    fn delay_is_rounded_to_whole_minutes() {
        assert_eq!(
            delay_minutes(Some("2026-10-28T16:02:40Z"), Some("2026-10-28T16:00:00Z")),
            Some(3)
        );
        assert_eq!(
            delay_minutes(Some("2026-10-28T16:00:00Z"), Some("2026-10-28T16:00:00Z")),
            Some(0)
        );
        assert_eq!(delay_minutes(None, Some("2026-10-28T16:00:00Z")), None);
    }

    #[test]
    fn db_time_window_uses_local_hours() {
        let moment = parse("2026-10-28T18:30")
            .expect("readable")
            .expect("a time");
        assert_eq!(db_slot(moment, BERLIN), ("261028".into(), "18".into()));
        let slots: Vec<_> = hour_slots(moment, 2)
            .into_iter()
            .map(|slot| db_slot(slot, BERLIN).1)
            .collect();
        assert_eq!(slots, ["18", "19"]);
    }

    #[test]
    fn db_times_are_local() {
        let moment = parse_db_time(Some("2610281805"), BERLIN).expect("a time");
        assert_eq!(format_minutes(moment), "2026-10-28T18:05+01:00");
    }
}
