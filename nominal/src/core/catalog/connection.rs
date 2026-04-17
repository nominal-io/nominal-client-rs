use nominal_api::objects::scout::datasource::connection::api::{
    Connection as ApiConnection, UpdateConnectionRequest,
};

use crate::core::rid::rid_to_string;

/// Represents a connection in Nominal.
///
/// Connections are data sources that stream or otherwise provide live data,
/// such as TimescaleDB, InfluxDB, or Nominal streaming connections.
#[derive(Debug, Clone)]
pub struct Connection {
    rid: String,
    name: String,
    description: Option<String>,
}

impl Connection {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub(crate) fn from_conjure(connection: ApiConnection) -> Self {
        Self {
            rid: rid_to_string(connection.rid()),
            name: connection.display_name().to_string(),
            description: connection
                .description()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
        }
    }
}

/// An update to connection metadata. Only fields that are set will be changed.
#[derive(Debug, Default, Clone)]
pub struct ConnectionUpdate {
    name: Option<String>,
    description: Option<String>,
}

impl ConnectionUpdate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn name(mut self, value: impl Into<String>) -> Self {
        self.name = Some(value.into());
        self
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    pub(crate) fn into_request(self) -> UpdateConnectionRequest {
        let ConnectionUpdate { name, description } = self;
        let mut b = UpdateConnectionRequest::builder();
        if let Some(n) = name {
            b = b.name(n);
        }
        if let Some(d) = description {
            b = b.description(d);
        }
        b.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_empty() {
        let req = ConnectionUpdate::new().into_request();
        assert!(req.name().is_none());
        assert!(req.description().is_none());
    }

    #[test]
    fn update_name_and_description() {
        let req = ConnectionUpdate::new()
            .name("My Connection")
            .description("desc")
            .into_request();
        assert_eq!(req.name(), Some("My Connection"));
        assert_eq!(req.description(), Some("desc"));
    }
}
