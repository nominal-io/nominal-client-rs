use conjure_object::ResourceIdentifier;
use std::fmt;

#[derive(Debug, Clone)]
pub(crate) struct RidConversionError {
    rid: String,
    reason: String,
}

impl fmt::Display for RidConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid RID '{}': {}", self.rid, self.reason)
    }
}

impl std::error::Error for RidConversionError {}

/// Convert a string RID into a typed Conjure RID.
pub(crate) fn parse_rid<T>(rid: &str) -> Result<T, RidConversionError>
where
    T: From<ResourceIdentifier>,
{
    let resource_id = ResourceIdentifier::new(rid).map_err(|e| RidConversionError {
        rid: rid.to_string(),
        reason: format!("{e:?}"),
    })?;

    Ok(resource_id.into())
}

/// Convert a typed RID into its string representation.
pub(crate) fn rid_to_string<T>(rid: &T) -> String
where
    T: ToString,
{
    rid.to_string()
}
