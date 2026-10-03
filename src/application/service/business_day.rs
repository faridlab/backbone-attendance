//! The business day: which calendar day, and which wall-clock time, a punch
//! instant belongs to.
//!
//! Hand-written (user-owned — see `metaphor.codegen.yaml`). Punches are stored as instants
//! (`timestamptz`), but the attendance day and the rollup's clock-in / clock-out columns are
//! LOCAL facts: a 03:51 clock-in in Jakarta is Saturday's attendance, even though it is still
//! Friday in UTC. Every day boundary in this module (the session date, the clock event date,
//! the daily rollup, the overtime "future date" check) goes through this one timezone so they
//! cannot disagree.
//!
//! Where the timezone comes from: the installation's `locale` / `timezone` setting in
//! `platform.sysparams` (the composing platform's one settings store), read on the verb's own
//! transaction, so each tenant database answers with its own setting. A database without that
//! table, without the row, or with a name that is not an IANA zone answers UTC — the behaviour
//! before this setting was honoured.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use sqlx::PgConnection;

/// The settings group and key the business timezone is read from.
pub const TIMEZONE_GROUP: &str = "locale";
pub const TIMEZONE_KEY: &str = "timezone";

/// The timezone this database's business days are counted in (UTC when unset).
pub async fn business_timezone(conn: &mut PgConnection) -> Result<Tz, sqlx::Error> {
    let has_settings: bool =
        sqlx::query_scalar("SELECT to_regclass('platform.sysparams') IS NOT NULL")
            .fetch_one(&mut *conn)
            .await?;
    if !has_settings {
        return Ok(Tz::UTC);
    }
    let name: Option<String> = sqlx::query_scalar(
        r#"SELECT value FROM platform.sysparams
            WHERE group_name = $1 AND key = $2 AND status::text = 'active'
            LIMIT 1"#,
    )
    .bind(TIMEZONE_GROUP)
    .bind(TIMEZONE_KEY)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(parse_timezone(name.as_deref()))
}

/// An IANA zone name, or UTC when absent or unknown (the unknown case is logged: a
/// misconfigured setting should be visible, but must not stop people clocking in).
pub fn parse_timezone(name: Option<&str>) -> Tz {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        None => Tz::UTC,
        Some(n) => n.parse::<Tz>().unwrap_or_else(|_| {
            tracing::warn!(
                target: "attendance",
                timezone = n,
                "locale.timezone is not an IANA zone name; counting business days in UTC"
            );
            Tz::UTC
        }),
    }
}

/// The calendar day `at` falls on in `tz`.
pub fn business_date(at: DateTime<Utc>, tz: Tz) -> NaiveDate {
    at.with_timezone(&tz).date_naive()
}

/// The wall-clock time `at` reads in `tz`.
pub fn wall_time(at: DateTime<Utc>, tz: Tz) -> NaiveTime {
    at.with_timezone(&tz).time()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn an_early_morning_jakarta_punch_is_the_local_day_not_the_utc_day() {
        // 2026-10-02 20:51:52Z is Saturday 3 October, 03:51:52 in Jakarta.
        let at = Utc.with_ymd_and_hms(2026, 10, 2, 20, 51, 52).unwrap();
        let jakarta = parse_timezone(Some("Asia/Jakarta"));
        assert_eq!(business_date(at, jakarta), NaiveDate::from_ymd_opt(2026, 10, 3).unwrap());
        assert_eq!(wall_time(at, jakarta), NaiveTime::from_hms_opt(3, 51, 52).unwrap());
    }

    #[test]
    fn utc_stays_the_utc_day() {
        let at = Utc.with_ymd_and_hms(2026, 10, 2, 20, 51, 52).unwrap();
        assert_eq!(business_date(at, Tz::UTC), NaiveDate::from_ymd_opt(2026, 10, 2).unwrap());
        assert_eq!(wall_time(at, Tz::UTC), NaiveTime::from_hms_opt(20, 51, 52).unwrap());
    }

    #[test]
    fn an_unset_or_unknown_zone_falls_back_to_utc() {
        assert_eq!(parse_timezone(None), Tz::UTC);
        assert_eq!(parse_timezone(Some("  ")), Tz::UTC);
        assert_eq!(parse_timezone(Some("Mars/Olympus")), Tz::UTC);
        assert_eq!(parse_timezone(Some(" Asia/Jakarta ")), Tz::Asia__Jakarta);
    }
}
