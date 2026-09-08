use chrono::{DateTime, Utc};
use nominal_api::objects::api::TimeUnit as ApiTimeUnit;
use nominal_api::objects::ingest::api::{
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

#[cfg(test)]
mod codec_tests {
    use super::*;

    #[test]
    fn registry_timestamp_preserves_custom_defaults() {
        let timestamp = Timestamp::custom("clock", "yyyy-DDD")
            .with_default_year(2026)
            .with_default_day_of_year(4);
        let encoded = timestamp.to_registry_proto();
        let decoded = Timestamp::from_registry_proto(encoded).unwrap();
        assert_eq!(decoded.into_conjure(), timestamp.into_conjure());
    }

    #[test]
    fn relative_timestamp_round_trips_nanoseconds() {
        let timestamp = Timestamp::relative("elapsed", TimeUnit::Nanoseconds)
            .with_offset(DateTime::from_timestamp(-1, 999_999_999).unwrap());
        let encoded = timestamp.to_registry_proto();
        assert_eq!(
            Timestamp::from_registry_proto(encoded)
                .unwrap()
                .into_conjure(),
            timestamp.into_conjure()
        );
    }

    #[test]
    fn numeric_check_excludes_string_encodings() {
        assert!(Timestamp::epoch("ts", TimeUnit::Seconds).is_numeric());
        assert!(!Timestamp::iso8601("ts").is_numeric());
        assert!(!Timestamp::custom("ts", "yyyy").is_numeric());
    }
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
pub enum TimestampKind {
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
    /// The encoding and its format-specific options.
    pub fn encoding(&self) -> &TimestampKind {
        &self.kind
    }
    pub fn series_name(&self) -> &str {
        &self.series_name
    }

    pub(crate) fn is_numeric(&self) -> bool {
        matches!(
            self.kind,
            TimestampKind::Epoch(_) | TimestampKind::Relative { .. }
        )
    }

    pub(crate) fn to_proto_type(&self) -> nominal_api::tonic::nominal::types::time::TimestampType {
        use nominal_api::tonic::nominal::types::time as p;
        let option = match &self.kind {
            TimestampKind::Relative { unit, offset } => {
                p::timestamp_type::Option::Relative(p::RelativeTimestamp {
                    time_unit: unit.into_conjure().to_string(),
                    offset: offset.map(|value| nominal_api::tonic::google::protobuf::Timestamp {
                        seconds: value.timestamp(),
                        nanos: value.timestamp_subsec_nanos() as i32,
                    }),
                })
            }
            kind => {
                let absolute = match kind {
                    TimestampKind::Iso8601 => {
                        p::absolute_timestamp::Option::Iso8601(p::Iso8601Timestamp {})
                    }
                    TimestampKind::Epoch(unit) => {
                        p::absolute_timestamp::Option::EpochOfTimeUnit(p::EpochTimestamp {
                            time_unit: unit.into_conjure().to_string(),
                        })
                    }
                    TimestampKind::Custom {
                        format,
                        default_year,
                        default_day_of_year,
                    } => p::absolute_timestamp::Option::CustomFormat(p::CustomTimestamp {
                        format: format.clone(),
                        default_year: *default_year,
                        default_day_of_year: *default_day_of_year,
                    }),
                    TimestampKind::Relative { .. } => unreachable!(),
                };
                p::timestamp_type::Option::Absolute(p::AbsoluteTimestamp {
                    option: Some(absolute),
                })
            }
        };
        p::TimestampType {
            option: Some(option),
        }
    }

    pub(crate) fn to_registry_proto(
        &self,
    ) -> nominal_api::tonic::nominal::registry::v2::TimestampMetadata {
        nominal_api::tonic::nominal::registry::v2::TimestampMetadata {
            series_name: self.series_name.clone(),
            timestamp_type: Some(self.to_proto_type()),
        }
    }

    pub(crate) fn to_ingest_proto(
        &self,
    ) -> nominal_api::tonic::nominal::ingest::v2::TimestampMetadata {
        nominal_api::tonic::nominal::ingest::v2::TimestampMetadata {
            column: self.series_name.clone(),
            r#type: Some(self.to_proto_type()),
        }
    }

    pub(crate) fn from_registry_proto(
        value: nominal_api::tonic::nominal::registry::v2::TimestampMetadata,
    ) -> crate::Result<Self> {
        use nominal_api::tonic::nominal::types::time as p;
        let missing = || crate::Error::UnexpectedResponse {
            field: "timestamp_type",
        };
        let kind = match value
            .timestamp_type
            .and_then(|v| v.option)
            .ok_or_else(missing)?
        {
            p::timestamp_type::Option::Relative(v) => TimestampKind::Relative {
                unit: parse_proto_unit(&v.time_unit)?,
                offset: v
                    .offset
                    .map(|t| {
                        if !(0..1_000_000_000).contains(&t.nanos) {
                            return Err(crate::Error::InvalidTimestamp {
                                seconds: t.seconds,
                                nanos: t.nanos as i64,
                            });
                        }
                        DateTime::from_timestamp(t.seconds, t.nanos as u32).ok_or(
                            crate::Error::InvalidTimestamp {
                                seconds: t.seconds,
                                nanos: t.nanos as i64,
                            },
                        )
                    })
                    .transpose()?,
            },
            p::timestamp_type::Option::Absolute(v) => match v.option.ok_or_else(missing)? {
                p::absolute_timestamp::Option::Iso8601(_) => TimestampKind::Iso8601,
                p::absolute_timestamp::Option::EpochOfTimeUnit(v) => {
                    TimestampKind::Epoch(parse_proto_unit(&v.time_unit)?)
                }
                p::absolute_timestamp::Option::CustomFormat(v) => TimestampKind::Custom {
                    format: v.format,
                    default_year: v.default_year,
                    default_day_of_year: v.default_day_of_year,
                },
            },
        };
        Ok(Self {
            series_name: value.series_name,
            kind,
        })
    }
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
            TimestampKind::Iso8601 => TimestampType::Absolute(Box::new(
                AbsoluteTimestamp::Iso8601(Iso8601Timestamp::new()),
            )),
            TimestampKind::Epoch(unit) => TimestampType::Absolute(Box::new(
                AbsoluteTimestamp::EpochOfTimeUnit(EpochTimestamp::new(unit.into_conjure())),
            )),
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

fn parse_proto_unit(value: &str) -> crate::Result<TimeUnit> {
    match value.to_ascii_lowercase().as_str() {
        "nanoseconds" => Ok(TimeUnit::Nanoseconds),
        "microseconds" => Ok(TimeUnit::Microseconds),
        "milliseconds" => Ok(TimeUnit::Milliseconds),
        "seconds" => Ok(TimeUnit::Seconds),
        "minutes" => Ok(TimeUnit::Minutes),
        "hours" => Ok(TimeUnit::Hours),
        "days" => Ok(TimeUnit::Days),
        _ => Err(crate::Error::Ingest {
            details: format!("unknown timestamp time unit: {value}"),
        }),
    }
}
