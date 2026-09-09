//! Local HTTP responses for SDK behavior tests. Hyper owns HTTP framing.
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::{convert::Infallible, future::Future, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};

pub(crate) struct HttpFixture {
    pub client: crate::core::NominalClient,
    task: JoinHandle<()>,
}
impl HttpFixture {
    pub async fn new<F, Fut>(respond: F) -> Self
    where
        F: Fn(Request<Bytes>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Value> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let respond = Arc::new(respond);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let respond = respond.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                                let respond = respond.clone();
                                async move {
                                    let (parts, body) = request.into_parts();
                                    let body = body.collect().await.unwrap().to_bytes();
                                    let json = respond(Request::from_parts(parts, body)).await;
                                    Ok::<_, Infallible>(Response::builder()
                                        .header("content-type", "application/json")
                                        .body(Full::new(Bytes::from(json.to_string())))
                                        .unwrap())
                                }
                            });
                            // A timeout may close a connection before the response is written.
                            let _ = http1::Builder::new().serve_connection(TokioIo::new(socket), service).await;
                        });
                    }
                    Some(result) = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap();
                    }
                }
            }
        });
        let client = crate::core::NominalClient::builder("token")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        Self { client, task }
    }

    pub async fn run<T>(&self, operation: impl Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(30), operation)
            .await
            .expect("test operation stalled")
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) fn dataset_file(id: &str, status: &str) -> Value {
    serde_json::json!({"id":id,"datasetRid":"ri.catalog.main.dataset.test","name":"output",
        "handle":{"type":"future","future":{}},"uploadedAt":"2026-01-01T00:00:00Z",
        "ingestStatus":{"type":status,status:{}}})
}
pub(crate) fn ingest_job(rid: &str) -> Value {
    serde_json::json!({"ingestJobRid":rid,"status":"COMPLETED","ingestType":"MULTI",
        "createdBy":"00000000-0000-0000-0000-000000000000",
        "orgUuid":"00000000-0000-0000-0000-000000000000"})
}
