use chrono::{DateTime, Utc};
use nominal_api::api::TimeUnit as ApiTimeUnit;
use nominal_api::ingest::api::{
    AbsoluteTimestamp, CustomTimestamp, EpochTimestamp, Iso8601Timestamp, RelativeTimestamp,
    TimestampMetadata, TimestampType,
};

/// The time unit used to interpret numeric timestamps in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    Nanoseconds,
    Microseconds,
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl TimeUnit {
    pub(crate) fn into_conjure(self) -> ApiTimeUnit {
        match self {
            TimeUnit::Nanoseconds => ApiTimeUnit::Nanoseconds,
            TimeUnit::Microseconds => ApiTimeUnit::Microseconds,
            TimeUnit::Milliseconds => ApiTimeUnit::Milliseconds,
            TimeUnit::Seconds => ApiTimeUnit::Seconds,
            TimeUnit::Minutes => ApiTimeUnit::Minutes,
            TimeUnit::Hours => ApiTimeUnit::Hours,
            TimeUnit::Days => ApiTimeUnit::Days,
        }
    }
}

/// How timestamps are encoded in a file, plus which column holds them.
///
/// Use [`Timestamp::iso8601`], [`Timestamp::epoch`], [`Timestamp::custom`], or
/// [`Timestamp::relative`] to construct one.
#[derive(Debug, Clone)]
pub struct Timestamp {
    series_name: String,
    kind: TimestampKind,
}

#[derive(Debug, Clone)]
enum TimestampKind {
    Iso8601,
    Epoch(TimeUnit),
    Custom {
        format: String,
        default_year: Option<i32>,
        default_day_of_year: Option<i32>,
    },
    Relative {
        unit: TimeUnit,
        offset: Option<DateTime<Utc>>,
    },
}

impl Timestamp {
    /// Timestamps are ISO 8601 strings.
    pub fn iso8601(series_name: impl Into<String>) -> Self {
        Self {
            series_name: series_name.into(),
            kind: TimestampKind::Iso8601,
        }
    }

    /// Timestamps are numeric epochs in the given unit (e.g. epoch-seconds).
    pub fn epoch(series_name: impl Into<String>, unit: TimeUnit) -> Self {
        Self {
            series_name: series_name.into(),
            kind: TimestampKind::Epoch(unit),
        }
    }

    /// Timestamps use a custom format string (Java `DateTimeFormatter` syntax).
    pub fn custom(series_name: impl Into<String>, format: impl Into<String>) -> Self {
        Self {
            series_name: series_name.into(),
            kind: TimestampKind::Custom {
                format: format.into(),
                default_year: None,
                default_day_of_year: None,
            },
        }
    }

    /// Timestamps are numeric offsets in the given unit relative to a start
    /// time. Use [`Self::with_offset`] to set the start time (required when
    /// ingesting into an existing dataset).
    pub fn relative(series_name: impl Into<String>, unit: TimeUnit) -> Self {
        Self {
            series_name: series_name.into(),
            kind: TimestampKind::Relative { unit, offset: None },
        }
    }

    /// Set the starting offset for a relative timestamp.
    #[must_use]
    pub fn with_offset(mut self, offset: DateTime<Utc>) -> Self {
        if let TimestampKind::Relative { offset: slot, .. } = &mut self.kind {
            *slot = Some(offset);
        }
        self
    }

    /// Set a default year for custom-format timestamps that lack year
    /// information (e.g. IRIG). No-op for other kinds.
    #[must_use]
    pub fn with_default_year(mut self, year: i32) -> Self {
        if let TimestampKind::Custom { default_year, .. } = &mut self.kind {
            *default_year = Some(year);
        }
        self
    }

    /// Set a default day-of-year for custom-format timestamps that lack date
    /// information. No-op for other kinds.
    #[must_use]
    pub fn with_default_day_of_year(mut self, day: i32) -> Self {
        if let TimestampKind::Custom {
            default_day_of_year,
            ..
        } = &mut self.kind
        {
            *default_day_of_year = Some(day);
        }
        self
    }

    pub(crate) fn into_conjure(self) -> TimestampMetadata {
        let ts_type = match self.kind {
            TimestampKind::Iso8601 => {
                TimestampType::Absolute(Box::new(AbsoluteTimestamp::Iso8601(
                    Iso8601Timestamp::new(),
                )))
            }
            TimestampKind::Epoch(unit) => {
                TimestampType::Absolute(Box::new(AbsoluteTimestamp::EpochOfTimeUnit(
                    EpochTimestamp::new(unit.into_conjure()),
                )))
            }
            TimestampKind::Custom {
                format,
                default_year,
                default_day_of_year,
            } => {
                let mut b = CustomTimestamp::builder().format(format);
                if let Some(y) = default_year {
                    b = b.default_year(y);
                }
                if let Some(d) = default_day_of_year {
                    b = b.default_day_of_year(d);
                }
                TimestampType::Absolute(Box::new(AbsoluteTimestamp::CustomFormat(b.build())))
            }
            TimestampKind::Relative { unit, offset } => {
                let mut b = RelativeTimestamp::builder().time_unit(unit.into_conjure());
                if let Some(o) = offset {
                    b = b.offset(o);
                }
                TimestampType::Relative(b.build())
            }
        };
        TimestampMetadata::new(self.series_name, ts_type)
    }
}
