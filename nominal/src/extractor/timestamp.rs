use crate::core::{TimeUnit as Unit, Timestamp, TimestampKind};
use crate::extractor::{Error, Result};
use nominal_api::objects::{
    api::TimeUnit,
    ingest::{
        api::{AbsoluteTimestamp, TimestampMetadata, TimestampType},
        manifest::{ManifestEpochTimeUnit, ManifestTimestampMetadata},
    },
};

pub(crate) fn output_timestamp(timestamp: Timestamp) -> Result<ManifestTimestampMetadata> {
    let (unit, offset) = match timestamp.encoding() {
        TimestampKind::Epoch(unit) => (unit, None),
        TimestampKind::Relative { unit, offset } => {
            let offset = offset.ok_or_else(|| {
                Error::InvalidOutput("relative timestamps require Timestamp::with_offset".into())
            })?;
            (unit, Some(offset))
        }
        _ => {
            return Err(Error::InvalidOutput(
                "manifest overrides require numeric timestamps".into(),
            ));
        }
    };
    let unit = match unit {
        Unit::Seconds => ManifestEpochTimeUnit::Seconds,
        Unit::Milliseconds => ManifestEpochTimeUnit::Milliseconds,
        Unit::Microseconds => ManifestEpochTimeUnit::Microseconds,
        Unit::Nanoseconds => ManifestEpochTimeUnit::Nanoseconds,
        _ => {
            return Err(Error::InvalidOutput(
                "manifest overrides support seconds, milliseconds, microseconds or nanoseconds"
                    .into(),
            ));
        }
    };
    Ok(ManifestTimestampMetadata::builder()
        .series_name(timestamp.series_name())
        .epoch_time_unit(unit)
        .relative_offset(offset)
        .build())
}

pub(crate) fn job_timestamp(metadata: TimestampMetadata) -> Result<Timestamp> {
    let name = metadata.series_name();
    match metadata.timestamp_type() {
        TimestampType::Absolute(kind) => match kind.as_ref() {
            AbsoluteTimestamp::EpochOfTimeUnit(epoch) => {
                Ok(Timestamp::epoch(name, job_unit(epoch.time_unit())?))
            }
            AbsoluteTimestamp::Iso8601(_) => Ok(Timestamp::iso8601(name)),
            AbsoluteTimestamp::CustomFormat(custom) => {
                let mut timestamp = Timestamp::custom(name, custom.format());
                if let Some(year) = custom.default_year() {
                    timestamp = timestamp.with_default_year(year);
                }
                if let Some(day) = custom.default_day_of_year() {
                    timestamp = timestamp.with_default_day_of_year(day);
                }
                Ok(timestamp)
            }
            _ => Err(Error::UnsupportedJobTimestamp(
                "unknown absolute encoding".into(),
            )),
        },
        TimestampType::Relative(relative) => {
            let timestamp = Timestamp::relative(name, job_unit(relative.time_unit())?);
            Ok(match relative.offset() {
                Some(offset) => timestamp.with_offset(offset),
                None => timestamp,
            })
        }
        _ => Err(Error::UnsupportedJobTimestamp("unknown encoding".into())),
    }
}

fn job_unit(unit: &TimeUnit) -> Result<crate::core::TimeUnit> {
    match unit {
        TimeUnit::Seconds => Ok(Unit::Seconds),
        TimeUnit::Milliseconds => Ok(Unit::Milliseconds),
        TimeUnit::Microseconds => Ok(Unit::Microseconds),
        TimeUnit::Nanoseconds => Ok(Unit::Nanoseconds),
        TimeUnit::Minutes => Ok(Unit::Minutes),
        TimeUnit::Hours => Ok(Unit::Hours),
        TimeUnit::Days => Ok(Unit::Days),
        _ => Err(Error::UnsupportedJobTimestamp(format!(
            "unknown time unit {unit}"
        ))),
    }
}
