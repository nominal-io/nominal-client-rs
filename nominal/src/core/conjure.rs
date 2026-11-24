//! The Conjure HTTP client the SDK's services share.

use std::future::Future;

use conjure_http::client::{AsyncClient, AsyncRequestBody};
use conjure_runtime::Client;
use http::{HeaderName, HeaderValue, Request, Response};

use crate::ai_agent::{AI_AGENT_HEADER, ai_agent_header_value};

/// A Conjure client that adds the [`AI_AGENT_HEADER`] header to every request.
#[derive(Clone)]
pub(crate) struct ConjureClient {
    inner: Client,
    ai_agent: HeaderValue,
}

impl ConjureClient {
    pub(crate) fn new(inner: Client) -> Self {
        Self {
            inner,
            ai_agent: ai_agent_header_value(),
        }
    }
}

impl AsyncClient for ConjureClient {
    type BodyWriter = <Client as AsyncClient>::BodyWriter;
    type ResponseBody = <Client as AsyncClient>::ResponseBody;

    fn send(
        &self,
        mut request: Request<AsyncRequestBody<'_, Self::BodyWriter>>,
    ) -> impl Future<Output = Result<Response<Self::ResponseBody>, conjure_error::Error>> + Send
    {
        request.headers_mut().insert(
            HeaderName::from_static(AI_AGENT_HEADER),
            self.ai_agent.clone(),
        );
        self.inner.send(request)
    }
}
