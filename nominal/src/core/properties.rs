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

/// Converts properties for API fields that only accept string values, formatting numbers with
/// [`PropertyValue`]'s `Display` impl.
pub(crate) fn string_properties_to_api(
    properties: HashMap<String, PropertyValue>,
) -> BTreeMap<PropertyName, ApiPropertyValue> {
    properties
        .into_iter()
        .map(|(k, v)| {
            let value = match v {
                PropertyValue::String(s) => s,
                number => number.to_string(),
            };
            (PropertyName(k), ApiPropertyValue(value))
        })
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

    #[test]
    fn string_properties_format_numbers() {
        let api = string_properties_to_api(collect_properties([
            ("s", PropertyValue::from("v")),
            ("n", 1.5.into()),
        ]));

        assert_eq!(
            api.get(&PropertyName("n".into())),
            Some(&ApiPropertyValue("1.5".into()))
        );
        assert_eq!(
            api.get(&PropertyName("s".into())),
            Some(&ApiPropertyValue("v".into()))
        );
    }
}
