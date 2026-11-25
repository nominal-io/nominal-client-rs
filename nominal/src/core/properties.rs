use std::collections::{BTreeMap, HashMap};
use std::fmt;

use nominal_api::objects::api::{
    PropertyName, PropertyValue as ApiPropertyValue, TypedPropertyValue,
};

/// The value of a property on a run, asset, or dataset.
///
/// Values convert from strings and numbers, so builders accept either:
///
/// ```
/// use nominal::core::{PropertyValue, RunUpdate};
///
/// let update = RunUpdate::new().properties([
///     ("vehicle", PropertyValue::from("rover-1")),
///     ("mass_kg", PropertyValue::from(12.5)),
/// ]);
/// # let _ = update;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    String(String),
    Number(f64),
}

impl PropertyValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            Self::Number(_) => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::String(_) => None,
            Self::Number(n) => Some(*n),
        }
    }

    fn into_api(self) -> TypedPropertyValue {
        match self {
            Self::String(s) => TypedPropertyValue::StringValue(ApiPropertyValue(s)),
            Self::Number(n) => TypedPropertyValue::NumericValue(n),
        }
    }

    fn from_api(value: &TypedPropertyValue) -> Option<Self> {
        match value {
            TypedPropertyValue::StringValue(s) => Some(Self::String(s.to_string())),
            TypedPropertyValue::NumericValue(n) => Some(Self::Number(*n)),
            TypedPropertyValue::Unknown(_) => None,
        }
    }
}

impl fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::Number(n) => write!(f, "{n}"),
        }
    }
}

impl From<String> for PropertyValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for PropertyValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<&String> for PropertyValue {
    fn from(value: &String) -> Self {
        Self::String(value.clone())
    }
}

impl From<f64> for PropertyValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<f32> for PropertyValue {
    fn from(value: f32) -> Self {
        Self::Number(value.into())
    }
}

impl From<i32> for PropertyValue {
    fn from(value: i32) -> Self {
        Self::Number(value.into())
    }
}

impl From<u32> for PropertyValue {
    fn from(value: u32) -> Self {
        Self::Number(value.into())
    }
}

pub(crate) fn collect_properties<I, K, V>(value: I) -> HashMap<String, PropertyValue>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<PropertyValue>,
{
    value
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

pub(crate) fn typed_properties_to_api(
    properties: HashMap<String, PropertyValue>,
) -> BTreeMap<PropertyName, TypedPropertyValue> {
    properties
        .into_iter()
        .map(|(k, v)| (PropertyName(k), v.into_api()))
        .collect()
}

pub(crate) fn properties_from_api(
    properties: &BTreeMap<PropertyName, TypedPropertyValue>,
) -> HashMap<String, PropertyValue> {
    properties
        .iter()
        .filter_map(|(k, v)| match PropertyValue::from_api(v) {
            Some(value) => Some((k.to_string(), value)),
            None => {
                tracing::warn!(property = %k, "skipping property with unknown value type");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_strings_and_numbers() {
        assert_eq!(PropertyValue::from("a"), PropertyValue::String("a".into()));
        assert_eq!(PropertyValue::from(2), PropertyValue::Number(2.0));
        assert_eq!(PropertyValue::from(1.5), PropertyValue::Number(1.5));
    }

    #[test]
    fn round_trips_through_api() {
        let props = collect_properties([("s", PropertyValue::from("v")), ("n", 3.into())]);
        let api = typed_properties_to_api(props.clone());

        assert_eq!(
            api.get(&PropertyName("s".into())),
            Some(&TypedPropertyValue::StringValue(ApiPropertyValue(
                "v".into()
            )))
        );
        assert_eq!(
            api.get(&PropertyName("n".into())),
            Some(&TypedPropertyValue::NumericValue(3.0))
        );
        assert_eq!(properties_from_api(&api), props);
    }

    #[tokio::test]
    async fn atomic_ingest_rejects_numeric_properties_before_opening_the_file() {
        use crate::core::{AvroStreamIngest, DatasetCreate, NominalClient};
        let client = NominalClient::builder("token")
            .base_url("http://127.0.0.1:1")
            .build()
            .unwrap();
        let target = DatasetCreate::new("Flight").properties([("mass_kg", 12.5)]);
        let err = client
            .ingest()
            .upload_avro_stream("missing-input.avro", target, AvroStreamIngest::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::Error::Ingest { details } if details.contains("mass_kg") && details.contains("create_dataset"))
        );
    }

    #[test]
    fn atomic_ingest_preserves_numeric_looking_strings() {
        let target = crate::core::DatasetCreate::new("Flight").properties([("serial", "0012")]);
        let destination = target.into_new_ingest_destination(None).unwrap();
        assert_eq!(
            destination.properties()[&PropertyName("serial".into())].0,
            "0012"
        );
    }
}
