use conjure_object::BearerToken;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::{Error, Result};

/// Adds `authorization: Bearer <token>` to every outgoing gRPC request.
#[derive(Clone)]
pub(crate) struct AuthInterceptor {
    header: MetadataValue<Ascii>,
}

impl Interceptor for AuthInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.header.clone());
        Ok(request)
    }
}

/// A lazily-connected gRPC channel to the Nominal API host, shared by all
/// gRPC-backed service clients.
#[derive(Clone)]
pub(crate) struct GrpcConnection {
    channel: Channel,
    auth: AuthInterceptor,
}

impl GrpcConnection {
    /// Build a connection from the API base URL. The channel connects on first use.
    pub(crate) fn connect_lazy(base_url: &str, token: &BearerToken) -> Result<Self> {
        let url = grpc_root_url(base_url)?;
        let mut endpoint =
            Channel::from_shared(url.clone()).map_err(|e| Error::InvalidServiceUrl {
                url: url.clone(),
                reason: e.to_string(),
            })?;
        if url.starts_with("https://") {
            endpoint = endpoint
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|e| Error::GrpcTransport {
                    details: e.to_string(),
                })?;
        }
        let header = format!("Bearer {}", token.as_str())
            .parse::<MetadataValue<Ascii>>()
            .map_err(|e| Error::InvalidBearerToken {
                reason: e.to_string(),
            })?;
        Ok(Self {
            channel: endpoint.connect_lazy(),
            auth: AuthInterceptor { header },
        })
    }

    pub(crate) fn channel(&self) -> Channel {
        self.channel.clone()
    }

    pub(crate) fn interceptor(&self) -> AuthInterceptor {
        self.auth.clone()
    }
}

/// gRPC services live at the host root, not under the `/api` path prefix.
fn grpc_root_url(base_url: &str) -> Result<String> {
    let invalid = |reason: String| Error::InvalidServiceUrl {
        url: base_url.to_string(),
        reason,
    };
    let url = reqwest::Url::parse(base_url).map_err(|e| invalid(e.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| invalid("URL has no host".to_string()))?;
    Ok(match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_root_url_strips_api_path() {
        assert_eq!(
            grpc_root_url("https://api.gov.nominal.io/api").unwrap(),
            "https://api.gov.nominal.io"
        );
    }

    #[test]
    fn grpc_root_url_keeps_explicit_port() {
        assert_eq!(
            grpc_root_url("http://localhost:8080/api").unwrap(),
            "http://localhost:8080"
        );
    }

    #[test]
    fn grpc_root_url_rejects_invalid() {
        assert!(grpc_root_url("not a url").is_err());
    }
}
