use crate::core::rid::rid_to_string;

#[derive(Debug, Clone)]
pub struct User {
    rid: String,
    org_rid: String,
    email: String,
    display_name: String,
}

impl User {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn org_rid(&self) -> &str {
        &self.org_rid
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) fn from_conjure(user: nominal_api::authentication::api::UserV2) -> Self {
        Self {
            rid: rid_to_string(user.rid()),
            org_rid: rid_to_string(user.org_rid()),
            email: user.email().to_string(),
            display_name: user.display_name().to_string(),
        }
    }
}
