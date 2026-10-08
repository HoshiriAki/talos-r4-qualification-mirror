use chrono::{DateTime, Utc};

/// Process-level source of UTC instants.
pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone)]
pub struct FixedClock {
    instant: DateTime<Utc>,
}

impl FixedClock {
    pub fn new(instant: DateTime<Utc>) -> Self {
        Self { instant }
    }
}

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        self.instant
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{Clock, FixedClock, SystemClock};

    #[test]
    fn fixed_clock_repeats_the_exact_configured_utc_instant() {
        let instant = Utc.with_ymd_and_hms(2026, 8, 3, 12, 34, 56).unwrap();
        let clock = FixedClock::new(instant);

        assert_eq!(clock.now_utc(), instant);
        assert_eq!(clock.now_utc(), clock.now_utc());
        assert_eq!(clock.now_utc().timezone(), Utc);
    }

    #[test]
    fn system_clock_returns_utc_within_a_bounded_interval() {
        let before = Utc::now();
        let actual = SystemClock.now_utc();
        let after = Utc::now();

        assert!(actual >= before);
        assert!(actual <= after);
        assert_eq!(actual.timezone(), Utc);
    }
}
