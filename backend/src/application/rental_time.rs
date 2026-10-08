//! R3 clock and calendar semantics.
//!
//! An authoritative timestamp is always a UTC instant.  A business date is
//! derived at the tenant's configured IANA timezone, never from the host or a
//! browser.  Rental periods are half-open `[start, end)` instant intervals.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantBusinessTimeZone {
    id: String,
    zone: Tz,
}

impl TenantBusinessTimeZone {
    pub fn parse(value: &str) -> Result<Self, String> {
        let id = value.trim();
        let zone = id
            .parse::<Tz>()
            .map_err(|_| "business timezone must be a valid IANA timezone".to_string())?;
        Ok(Self {
            id: id.to_owned(),
            zone,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn business_date(&self, instant: DateTime<Utc>) -> NaiveDate {
        instant.with_timezone(&self.zone).date_naive()
    }
}

/// An explicit half-open authoritative rental interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalPeriod {
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
}

impl RentalPeriod {
    pub fn new(starts_at: DateTime<Utc>, ends_at: DateTime<Utc>) -> Result<Self, String> {
        if ends_at <= starts_at {
            return Err(
                "rental period must be a non-empty half-open [start, end) instant interval".into(),
            );
        }
        Ok(Self { starts_at, ends_at })
    }

    pub fn contains(&self, instant: DateTime<Utc>) -> bool {
        self.starts_at <= instant && instant < self.ends_at
    }
}

/// Parses only RFC3339 values with an explicit offset.  Naive local strings
/// cannot silently become authoritative instants.
pub fn parse_authoritative_instant(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| {
            "authoritative timestamps must be RFC3339 values with an explicit offset".into()
        })
}

pub fn format_authoritative_instant(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn parse_business_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| "business date must be YYYY-MM-DD".into())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::{
        RentalPeriod, TenantBusinessTimeZone, format_authoritative_instant,
        parse_authoritative_instant,
    };

    #[test]
    fn utc_boundary_and_tenant_midnight_are_explicit() {
        let zone = TenantBusinessTimeZone::parse("Pacific/Auckland").unwrap();
        let instant = chrono::Utc
            .with_ymd_and_hms(2025, 12, 31, 11, 30, 0)
            .unwrap();
        assert_eq!(zone.business_date(instant).to_string(), "2026-01-01");
        assert_eq!(
            zone.business_date(instant - Duration::hours(12))
                .to_string(),
            "2025-12-31"
        );
    }

    #[test]
    fn tenant_zone_is_independent_of_server_timezone() {
        let tokyo = TenantBusinessTimeZone::parse("Asia/Tokyo").unwrap();
        let new_york = TenantBusinessTimeZone::parse("America/New_York").unwrap();
        let instant = chrono::Utc.with_ymd_and_hms(2026, 3, 10, 2, 30, 0).unwrap();
        assert_eq!(tokyo.business_date(instant).to_string(), "2026-03-10");
        assert_eq!(new_york.business_date(instant).to_string(), "2026-03-09");
    }

    #[test]
    fn dst_spring_forward_and_fall_back_are_unambiguous_instants() {
        let zone = TenantBusinessTimeZone::parse("America/New_York").unwrap();
        let before_spring = parse_authoritative_instant("2026-03-08T06:59:00Z").unwrap();
        let after_spring = parse_authoritative_instant("2026-03-08T07:01:00Z").unwrap();
        assert_eq!(
            zone.business_date(before_spring),
            zone.business_date(after_spring)
        );

        let first_0130 = parse_authoritative_instant("2026-11-01T05:30:00Z").unwrap();
        let second_0130 = parse_authoritative_instant("2026-11-01T06:30:00Z").unwrap();
        assert_ne!(first_0130, second_0130);
        assert_eq!(
            zone.business_date(first_0130),
            zone.business_date(second_0130)
        );
        assert_eq!(
            format_authoritative_instant(first_0130),
            "2026-11-01T05:30:00.000Z"
        );
    }

    #[test]
    fn rental_period_is_half_open_across_due_boundary() {
        let start = parse_authoritative_instant("2026-10-31T04:00:00+00:00").unwrap();
        let end = parse_authoritative_instant("2026-11-01T04:00:00+00:00").unwrap();
        let period = RentalPeriod::new(start, end).unwrap();
        assert!(period.contains(start));
        assert!(period.contains(end - Duration::milliseconds(1)));
        assert!(!period.contains(end));
        assert!(RentalPeriod::new(end, start).is_err());
    }

    #[test]
    fn ambiguous_and_naive_inputs_are_rejected() {
        assert!(parse_authoritative_instant("2026-11-01T01:30:00").is_err());
        assert!(parse_authoritative_instant("2026-03-08 02:30:00").is_err());
    }
}
