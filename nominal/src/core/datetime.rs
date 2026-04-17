use chrono::{DateTime, TimeZone, Utc};
use nominal_api::objects::scout::run::api::UtcTimestamp;
use std::fmt;

/// Local conversion wrapper for Nominal `UtcTimestamp` values.
///
/// This exists because we cannot directly implement `From` for two external types
/// (`chrono::DateTime<Utc>` and `nominal_api::...::UtcTimestamp`) due Rust orphan rules.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NominalDateTime(pub UtcTimestamp);

#[derive(Debug, Clone)]
pub(crate) enum NominalDateTimeError {
    SecondsOutOfRange(i64),
    NanosOutOfRange(i64),
    InvalidTimestamp { seconds: i64, nanos: i64 },
}

impl fmt::Display for NominalDateTimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecondsOutOfRange(v) => write!(f, "seconds_since_epoch out of range: {v}"),
            Self::NanosOutOfRange(v) => write!(f, "offset_nanoseconds out of range: {v}"),
            Self::InvalidTimestamp { seconds, nanos } => {
                write!(f, "invalid timestamp: seconds={seconds}, nanos={nanos}")
            }
        }
    }
}

impl std::error::Error for NominalDateTimeError {}

impl TryFrom<DateTime<Utc>> for NominalDateTime {
    type Error = NominalDateTimeError;

    fn try_from(value: DateTime<Utc>) -> Result<Self, Self::Error> {
        let seconds = value.timestamp();
        let nanos = i64::from(value.timestamp_subsec_nanos());

        let seconds_safe = conjure_object::SafeLong::try_from(seconds)
            .map_err(|_| NominalDateTimeError::SecondsOutOfRange(seconds))?;
        let nanos_safe = conjure_object::SafeLong::try_from(nanos)
            .map_err(|_| NominalDateTimeError::NanosOutOfRange(nanos))?;

        let ts = UtcTimestamp::builder()
            .seconds_since_epoch(seconds_safe)
            .offset_nanoseconds(Some(nanos_safe))
            .build();

        Ok(Self(ts))
    }
}

impl TryFrom<NominalDateTime> for DateTime<Utc> {
    type Error = NominalDateTimeError;

    fn try_from(value: NominalDateTime) -> Result<Self, Self::Error> {
        let seconds = *value.0.seconds_since_epoch();
        let nanos_i64 = value.0.offset_nanoseconds().map(|n| *n).unwrap_or(0);

        if !(0..1_000_000_000).contains(&nanos_i64) {
            return Err(NominalDateTimeError::NanosOutOfRange(nanos_i64));
        }
        let nanos = nanos_i64 as u32;

        Utc.timestamp_opt(seconds, nanos)
            .single()
            .ok_or(NominalDateTimeError::InvalidTimestamp {
                seconds,
                nanos: nanos_i64,
            })
    }
}

impl From<NominalDateTime> for UtcTimestamp {
    fn from(value: NominalDateTime) -> Self {
        value.0
    }
}

/// Convert an API `UtcTimestamp` to chrono `DateTime<Utc>`.
///
/// Returns an error if the timestamp values are out of range or invalid.
pub(crate) fn api_timestamp_to_utc(
    ts: &UtcTimestamp,
) -> Result<DateTime<Utc>, NominalDateTimeError> {
    DateTime::<Utc>::try_from(NominalDateTime(*ts))
}

/// Convert an API `UtcTimestamp` to chrono `DateTime<Utc>`, panicking on invalid input.
///
/// Use this only when invalid API timestamps represent a hard contract violation.
pub(crate) fn api_timestamp_to_utc_or_panic(ts: &UtcTimestamp) -> DateTime<Utc> {
    api_timestamp_to_utc(ts).unwrap_or_else(|e| panic!("API returned invalid timestamp: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nominal_datetime_round_trip() {
        let dt = Utc
            .timestamp_opt(1_720_000_000, 123_456_789)
            .single()
            .expect("valid timestamp");

        let ts = NominalDateTime::try_from(dt)
            .expect("convert to nominal")
            .into();
        let got = DateTime::<Utc>::try_from(NominalDateTime(ts)).expect("convert to chrono");

        assert_eq!(got, dt);
    }
}
