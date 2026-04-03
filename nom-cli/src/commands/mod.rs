pub mod asset;
pub mod config;
pub mod user;

use clap::error::ErrorKind;
use nominal_client::{Config, Error as ClientError, NominalClient};

pub(crate) fn clap_error(kind: ErrorKind, message: impl std::fmt::Display) -> clap::Error {
    clap::Error::raw(kind, format!("{message}\n"))
}

pub(crate) fn load_client(profile_name: &str) -> Result<NominalClient, clap::Error> {
    let config = Config::from_file(None)
        .map_err(|e| clap_error(ErrorKind::Io, format!("Failed to load config: {e}")))?;

    let profile = config.get_profile(profile_name).ok_or_else(|| {
        clap_error(
            ErrorKind::InvalidValue,
            format!("Profile '{profile_name}' not found"),
        )
    })?;

    NominalClient::from_profile(profile).map_err(|e| {
        clap_error(
            ErrorKind::InvalidValue,
            format!("Failed to create Nominal client from profile '{profile_name}': {e}"),
        )
    })
}

pub(crate) fn client_error(message: impl std::fmt::Display, error: ClientError) -> clap::Error {
    let kind = match error {
        ClientError::Rid { .. }
        | ClientError::InvalidBearerToken { .. }
        | ClientError::InvalidServiceUrl { .. }
        | ClientError::NotFound { .. } => ErrorKind::InvalidValue,
        _ => ErrorKind::Io,
    };

    clap_error(kind, format!("{message}: {error}"))
}
