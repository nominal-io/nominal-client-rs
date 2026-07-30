use nominal::{Error, NominalClient, User};

pub const AUTH_DOCS_LINK: &str = "https://docs.nominal.io/core/sdk/python-client/authentication";

/// Validate that a profile's credentials can authenticate and resolve a workspace.
pub async fn validate_profile(
    base_url: &str,
    token: &str,
    workspace_rid: Option<&str>,
) -> Result<User, ValidationError> {
    let client = NominalClient::builder(token)
        .base_url(base_url)
        .workspace_rid(workspace_rid.map(ToString::to_string))
        .build()
        .map_err(|e| ValidationError::Unexpected(e.to_string()))?;

    let user = match client.users().who_am_i().await {
        Ok(user) => user,
        Err(err) => return Err(map_auth_error(err)),
    };

    match client.workspaces().resolve_workspace(workspace_rid).await {
        Ok(()) => Ok(user),
        Err(Error::NoDefaultWorkspace) => Err(ValidationError::NoDefaultWorkspace),
        Err(err) => Err(map_workspace_error(err)),
    }
}

fn map_auth_error(err: Error) -> ValidationError {
    match err.http_status() {
        Some(status) => auth_error_for_status(status),
        None => ValidationError::ApiUnreachable(err.to_string()),
    }
}

fn map_workspace_error(err: Error) -> ValidationError {
    match err.http_status() {
        Some(status) => workspace_error_for_status(status),
        None => ValidationError::ApiUnreachable(err.to_string()),
    }
}

/// Whether the failure means the API could not be reached (transport/network
/// error), as opposed to a definitive auth or workspace misconfiguration. Used
/// to decide whether to offer saving a profile without validation.
pub fn is_api_unreachable(err: &ValidationError) -> bool {
    matches!(err, ValidationError::ApiUnreachable(_))
}

fn auth_error_for_status(status: u16) -> ValidationError {
    match status {
        401 => ValidationError::InvalidToken,
        404 => ValidationError::IncorrectBaseUrl,
        _ => ValidationError::AuthMisconfiguration { status },
    }
}

fn workspace_error_for_status(status: u16) -> ValidationError {
    match status {
        404 => ValidationError::IncorrectBaseUrl,
        _ => ValidationError::WorkspaceMisconfiguration { status },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error(
        "The authorization token may be invalid. Read the docs on how to get a new token: {AUTH_DOCS_LINK}"
    )]
    InvalidToken,

    #[error(
        "The base_url may be incorrect. Ensure the url subdomain begins with 'api' (not 'app')."
    )]
    IncorrectBaseUrl,

    #[error(
        "There is likely a misconfiguration between the base_url and token. Ensure the url subdomain begins with 'api' (not 'app'), and create a new token: {AUTH_DOCS_LINK} ({status})"
    )]
    AuthMisconfiguration { status: u16 },

    #[error("Workspace not provided, but there is no default workspace for the user.")]
    NoDefaultWorkspace,

    #[error(
        "Could not reach the API. Check your network connection and that the base_url is correct. ({0})"
    )]
    ApiUnreachable(String),

    #[error(
        "There is likely a misconfiguration; received status={status}. Contact support for help."
    )]
    WorkspaceMisconfiguration { status: u16 },

    #[error("Unexpected error: {0}")]
    Unexpected(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_mapping() {
        assert_eq!(auth_error_for_status(401), ValidationError::InvalidToken);
        assert_eq!(
            auth_error_for_status(404),
            ValidationError::IncorrectBaseUrl
        );
        assert_eq!(
            auth_error_for_status(500),
            ValidationError::AuthMisconfiguration { status: 500 }
        );
    }

    #[test]
    fn workspace_error_mapping() {
        assert_eq!(
            workspace_error_for_status(404),
            ValidationError::IncorrectBaseUrl
        );
        assert_eq!(
            workspace_error_for_status(403),
            ValidationError::WorkspaceMisconfiguration { status: 403 }
        );
    }

    #[test]
    fn only_unreachable_errors_offer_skip() {
        assert!(is_api_unreachable(&ValidationError::ApiUnreachable(
            "connection refused".to_string()
        )));
        assert!(!is_api_unreachable(&ValidationError::InvalidToken));
        assert!(!is_api_unreachable(&ValidationError::IncorrectBaseUrl));
        assert!(!is_api_unreachable(
            &ValidationError::AuthMisconfiguration { status: 500 }
        ));
    }
}
