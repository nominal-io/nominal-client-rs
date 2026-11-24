use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use conjure_object::BearerToken;
use futures::future::poll_fn;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use rand::Rng;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::{Interceptor, interceptor::InterceptedService};
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Code, Status, body::Body, client::GrpcService};
use tower::{Layer, Service};

use crate::ai_agent::{AI_AGENT_HEADER, ai_agent_header_value};
use crate::{Error, Result, TransportError};
use nominal_streaming::client::ProductAgent;

const MAX_RETRIES: u32 = 4;
const INITIAL_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(120);

/// A gRPC-aware Tower layer which retries replayable unary requests before generated clients decode them.
#[derive(Clone, Debug, Default)]
pub(crate) struct RetryLayer;

impl<S: GrpcService<Body>> Layer<S> for RetryLayer {
    type Service = RetryService<S>;

    fn layer(&self, service: S) -> Self::Service {
        RetryService { service }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RetryService<S> {
    service: S,
}

impl<S> Service<Request<Body>> for RetryService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: std::fmt::Display + Send,
{
    type Response = Response<Body>;
    type Error = Status;
    type Future = Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Status>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.service
            .poll_ready(cx)
            .map_err(|error| Status::unknown(error.to_string()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let mut service = self.service.clone();
        std::mem::swap(&mut self.service, &mut service);
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let bytes = body.collect().await?.to_bytes();
            let method = parts.method;
            let uri = parts.uri;
            let version = parts.version;
            let headers = parts.headers;
            for attempt in 0..=MAX_RETRIES {
                let mut builder = Request::builder()
                    .method(method.clone())
                    .uri(uri.clone())
                    .version(version);
                for (name, value) in &headers {
                    builder = builder.header(name, value);
                }
                let request = builder
                    .body(Body::new(Full::new(bytes.clone())))
                    .map_err(|error| Status::internal(error.to_string()))?;
                poll_fn(|cx| service.poll_ready(cx))
                    .await
                    .map_err(|error| Status::unknown(error.to_string()))?;
                match service.call(request).await {
                    Ok(response) if retryable_response(&response) && attempt < MAX_RETRIES => {
                        tokio::time::sleep(jittered_backoff(attempt)).await;
                    }
                    Ok(response) => return Ok(response),
                    Err(error) => return Err(Status::unknown(error.to_string())),
                }
            }
            unreachable!()
        })
    }
}

fn retryable_response<T>(response: &Response<T>) -> bool {
    Status::from_header_map(response.headers())
        .is_some_and(|status| matches!(status.code(), Code::Unavailable | Code::ResourceExhausted))
}

fn jittered_backoff(attempt: u32) -> Duration {
    let ceiling = INITIAL_BACKOFF
        .saturating_mul(1 << attempt)
        .min(MAX_BACKOFF);
    Duration::from_secs_f64(rand::rng().random_range(0.0..ceiling.as_secs_f64()))
}

/// Adds `authorization: Bearer <token>` and the [`AI_AGENT_HEADER`] header to every outgoing gRPC
/// request, keeping either when the request already sets it.
#[derive(Clone)]
pub(crate) struct AuthInterceptor {
    header: MetadataValue<Ascii>,
    ai_agent: MetadataValue<Ascii>,
}

pub(crate) type GrpcTransport = InterceptedService<RetryService<Channel>, AuthInterceptor>;

impl Interceptor for AuthInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        if !request.metadata().contains_key("authorization") {
            request
                .metadata_mut()
                .insert("authorization", self.header.clone());
        }
        if !request.metadata().contains_key(AI_AGENT_HEADER) {
            request
                .metadata_mut()
                .insert(AI_AGENT_HEADER, self.ai_agent.clone());
        }
        Ok(request)
    }
}

/// A lazily-connected gRPC channel to the Nominal API host, shared by all
/// gRPC-backed service clients.
#[derive(Clone)]
pub(crate) struct GrpcConnection {
    channel: RetryService<Channel>,
    auth: AuthInterceptor,
}

impl GrpcConnection {
    /// Build a connection from the API base URL. The channel connects on first use.
    ///
    /// Requests send `<product>` as their user agent, followed by tonic's own, and the
    /// [`AI_AGENT_HEADER`] header.
    pub(crate) fn connect_lazy(
        base_url: &str,
        token: &BearerToken,
        product: &ProductAgent,
    ) -> Result<Self> {
        let url = grpc_root_url(base_url)?;
        let mut endpoint = Channel::from_shared(url.clone())
            .map_err(|e| Error::InvalidServiceUrl {
                url: url.clone(),
                reason: e.to_string(),
            })?
            .user_agent(product.to_string())
            .map_err(|e| Error::InvalidUserAgent {
                reason: e.to_string(),
            })?;
        if url.starts_with("https://") {
            endpoint = endpoint
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(TransportError::new)?;
        }
        let header = format!("Bearer {}", token.as_str())
            .parse::<MetadataValue<Ascii>>()
            .map_err(|e| Error::InvalidBearerToken {
                reason: e.to_string(),
            })?;
        Ok(Self {
            channel: RetryLayer.layer(endpoint.connect_lazy()),
            auth: AuthInterceptor {
                header,
                ai_agent: MetadataValue::try_from(ai_agent_header_value().as_bytes())
                    .expect("the AI agent header value is ASCII"),
            },
        })
    }

    pub(crate) fn channel(&self) -> RetryService<Channel> {
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

    /// A gRPC server for the drives service that records each request's user agent and AI agent
    /// header, failing the first `unavailable` requests with UNAVAILABLE so the client retries.
    #[derive(Clone)]
    struct UserAgentRecorder {
        user_agents: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
        unavailable: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl tonic::server::NamedService for UserAgentRecorder {
        const NAME: &'static str = "nominal.file_store.v1.DrivesService";
    }

    impl Service<Request<Body>> for UserAgentRecorder {
        type Response = Response<Body>;
        type Error = std::convert::Infallible;
        type Future = std::future::Ready<std::result::Result<Response<Body>, Self::Error>>;

        fn poll_ready(
            &mut self,
            _: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: Request<Body>) -> Self::Future {
            let header = |name: &str| {
                request
                    .headers()
                    .get(name)
                    .map(|value| value.to_str().unwrap().to_string())
                    .unwrap_or_default()
            };
            let recorded = (header("user-agent"), header(AI_AGENT_HEADER));
            self.user_agents.lock().unwrap().push(recorded);
            let mut unavailable = self.unavailable.lock().unwrap();
            let retry = *unavailable > 0;
            if retry {
                *unavailable -= 1;
            }
            let status = if retry {
                Code::Unavailable
            } else {
                Code::NotFound
            };
            std::future::ready(Ok(Response::builder()
                .header("content-type", "application/grpc")
                .header("grpc-status", (status as i32).to_string())
                .body(Body::empty())
                .unwrap()))
        }
    }

    /// Serves a [`UserAgentRecorder`] on a local port and returns the API base URL.
    async fn serve(recorder: UserAgentRecorder) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(recorder)
                .serve_with_incoming(tonic::transport::server::TcpIncoming::from(listener)),
        );
        format!("http://{addr}/api")
    }

    #[tokio::test]
    async fn requests_send_the_product_as_user_agent_and_the_ai_agent_header() {
        let recorder = UserAgentRecorder {
            user_agents: Default::default(),
            unavailable: std::sync::Arc::new(std::sync::Mutex::new(1)),
        };
        let base_url = serve(recorder.clone()).await;
        let product = ProductAgent::new("nominal-cli", "1.2.3").unwrap();
        let connection =
            GrpcConnection::connect_lazy(&base_url, &BearerToken::new("token").unwrap(), &product)
                .unwrap();

        let result = crate::core::DrivesClient::new(&connection, None)
            .get("ri.drive.main.drive.1")
            .await;

        assert!(result.is_err(), "the recorder answers NOT_FOUND");
        let user_agents = recorder.user_agents.lock().unwrap().clone();
        assert_eq!(
            user_agents.len(),
            2,
            "one retry after UNAVAILABLE: {user_agents:?}"
        );
        let ai_agent = ai_agent_header_value();
        for (user_agent, recorded_ai_agent) in &user_agents {
            let tokens: Vec<&str> = user_agent.split_whitespace().collect();
            assert_eq!(tokens.len(), 2, "{user_agent:?}");
            assert_eq!(tokens[0], "nominal-cli/1.2.3", "{user_agent:?}");
            assert!(tokens[1].starts_with("tonic/"), "{user_agent:?}");
            assert_eq!(recorded_ai_agent, ai_agent.to_str().unwrap());
        }
    }

    #[test]
    fn auth_interceptor_preserves_an_existing_authorization_header() {
        let mut interceptor = AuthInterceptor {
            header: "Bearer default".parse().unwrap(),
            ai_agent: "none".parse().unwrap(),
        };
        let mut request = tonic::Request::new(());
        request
            .metadata_mut()
            .insert("authorization", "Bearer override".parse().unwrap());

        let request = interceptor.call(request).unwrap();

        assert_eq!(
            request.metadata().get("authorization").unwrap(),
            "Bearer override"
        );
    }
}
