use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use nominal::core::NominalClient;
use nominal_api::tonic::nominal::{ingest::v2 as p, registry::v2 as r};
use prost::Message;
use std::{
    collections::VecDeque,
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

#[derive(Clone)]
struct Mock {
    replies: Arc<Mutex<VecDeque<(String, Vec<u8>, u16)>>>,
    requests: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}
impl tonic::server::NamedService for Mock {
    const NAME: &'static str = "nominal.registry.v2.RegistryService";
}
#[derive(Clone)]
struct ExtractorMock(Mock);
impl tonic::server::NamedService for ExtractorMock {
    const NAME: &'static str = "nominal.ingest.v2.ContainerizedExtractorService";
}
type MockFuture =
    Pin<Box<dyn Future<Output = Result<http::Response<tonic::body::Body>, Infallible>> + Send>>;
impl tower::Service<http::Request<tonic::body::Body>> for ExtractorMock {
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = MockFuture;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        self.0.call(request)
    }
}
impl tower::Service<http::Request<tonic::body::Body>> for Mock {
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = MockFuture;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        let state = self.clone();
        Box::pin(async move {
            let method = request.uri().path().rsplit('/').next().unwrap().to_owned();
            let body = request.into_body().collect().await.unwrap().to_bytes();
            state
                .requests
                .lock()
                .unwrap()
                .push((method.clone(), body[5..].to_vec()));
            let (expected, payload, status) = state
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected RPC");
            assert_eq!(method, expected);
            let mut bytes = vec![0];
            bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            bytes.extend(payload);
            Ok(http::Response::builder()
                .header("content-type", "application/grpc")
                .header("grpc-status", status.to_string())
                .body(tonic::body::Body::new(Full::new(Bytes::from(bytes))))
                .unwrap())
        })
    }
}
struct Fixture {
    client: NominalClient,
    mock: Mock,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(replies: Vec<(&str, Vec<u8>, u16)>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let incoming = futures::stream::unfold(listener, |listener| async {
            Some((listener.accept().await.map(|(socket, _)| socket), listener))
        });
        let mock = Mock {
            replies: Arc::new(Mutex::new(
                replies
                    .into_iter()
                    .map(|(method, bytes, status)| (method.into(), bytes, status))
                    .collect(),
            )),
            requests: Default::default(),
        };
        let server = mock.clone();
        let task = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ExtractorMock(server.clone()))
                .add_service(server)
                .serve_with_incoming(incoming)
                .await
                .unwrap();
        });
        let client = NominalClient::builder("test")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        Self { client, mock, task }
    }
}
fn resource() -> p::ContainerizedExtractor {
    p::ContainerizedExtractor {
        rid: "extractor".into(),
        workspace_rid: "workspace".into(),
        name: "name".into(),
        ..Default::default()
    }
}
fn get() -> Vec<u8> {
    p::GetContainerizedExtractorResponse {
        extractor: Some(resource()),
    }
    .encode_to_vec()
}
fn update() -> Vec<u8> {
    p::UpdateContainerizedExtractorResponse {
        extractor: Some(resource()),
    }
    .encode_to_vec()
}
fn scope() -> ScopeArgs {
    ScopeArgs {
        workspace: Some("workspace".into()),
        output: OutputArgs { json: true },
    }
}
fn rid() -> RidArgs {
    RidArgs {
        rid: "extractor".into(),
        scope: scope(),
    }
}
#[tokio::test]
async fn extractor_handlers_preserve_workspace_and_update_fields() {
    let f = Fixture::new(vec![
        (
            "CreateContainerizedExtractor",
            p::CreateContainerizedExtractorResponse {
                extractor: Some(resource()),
            }
            .encode_to_vec(),
            0,
        ),
        ("GetContainerizedExtractor", get(), 0),
        ("UpdateContainerizedExtractor", update(), 0),
        ("GetContainerizedExtractor", get(), 0),
        ("UpdateContainerizedExtractor", update(), 0),
        ("GetContainerizedExtractor", get(), 0),
        ("UpdateContainerizedExtractor", update(), 0),
    ])
    .await;
    handle(
        ExtractorCommands::Create {
            name: "name".into(),
            description: Some("description".into()),
            scope: scope(),
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    handle(
        ExtractorCommands::Update {
            rid: "extractor".into(),
            name: Some("new".into()),
            description: Some("changed".into()),
            scope: scope(),
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    handle(ExtractorCommands::Archive(rid()), f.client.clone())
        .await
        .unwrap();
    handle(ExtractorCommands::Unarchive(rid()), f.client.clone())
        .await
        .unwrap();
    let calls = f.mock.requests.lock().unwrap();
    let create = p::CreateContainerizedExtractorRequest::decode(calls[0].1.as_slice()).unwrap();
    assert_eq!(create.workspace_rid, "workspace");
    assert_eq!(create.description.as_deref(), Some("description"));
    let update = p::UpdateContainerizedExtractorRequest::decode(calls[2].1.as_slice()).unwrap();
    assert_eq!(update.workspace_rid, "workspace");
    assert_eq!(update.name.as_deref(), Some("new"));
    assert_eq!(update.description.as_deref(), Some("changed"));
    assert_eq!(
        p::UpdateContainerizedExtractorRequest::decode(calls[4].1.as_slice())
            .unwrap()
            .is_archived,
        Some(true)
    );
    assert_eq!(
        p::UpdateContainerizedExtractorRequest::decode(calls[6].1.as_slice())
            .unwrap()
            .is_archived,
        Some(false)
    );
    assert!(f.mock.replies.lock().unwrap().is_empty());
}
#[tokio::test]
async fn extractor_search_maps_filters() {
    let f = Fixture::new(vec![(
        "SearchContainerizedExtractors",
        p::SearchContainerizedExtractorsResponse {
            extractors: vec![resource()],
            next_page_token: None,
        }
        .encode_to_vec(),
        0,
    )])
    .await;
    handle(
        ExtractorCommands::Search {
            include_archived: true,
            file_extension: Some("flight".into()),
            scope: scope(),
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    let calls = f.mock.requests.lock().unwrap();
    let request = p::SearchContainerizedExtractorsRequest::decode(calls[0].1.as_slice()).unwrap();
    assert!(request.include_archived);
    assert_eq!(request.file_extension.as_deref(), Some("flight"));
    assert_eq!(request.workspace_rid, "workspace");
}
fn image_reply() -> Vec<u8> {
    r::GetImageResponse {
        image: Some(r::ContainerImage {
            rid: "image".into(),
            extractor_rid: "extractor".into(),
            status: 2,
            ..Default::default()
        }),
    }
    .encode_to_vec()
}
#[tokio::test]
async fn extractor_image_delete_and_activation_delegate() {
    let f = Fixture::new(vec![
        ("GetContainerizedExtractor", get(), 0),
        ("GetImage", image_reply(), 0),
        ("GetImage", image_reply(), 0),
        ("UpdateContainerizedExtractor", update(), 0),
        ("GetImage", image_reply(), 0),
        ("DeleteImage", r::DeleteImageResponse {}.encode_to_vec(), 0),
    ])
    .await;
    handle(
        ExtractorCommands::Activate {
            rid: "extractor".into(),
            image_rid: "image".into(),
            wait: WaitArgs {
                no_wait: true,
                timeout: None,
            },
            scope: scope(),
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    handle(
        ExtractorCommands::Image {
            command: ImageCommands::Delete(RidArgs {
                rid: "image".into(),
                scope: scope(),
            }),
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    let calls = f.mock.requests.lock().unwrap();
    let update = p::UpdateContainerizedExtractorRequest::decode(calls[3].1.as_slice()).unwrap();
    assert_eq!(update.active_container_image_rid.as_deref(), Some("image"));
    let delete = r::DeleteImageRequest::decode(calls[5].1.as_slice()).unwrap();
    assert_eq!(delete.workspace_rid, "workspace");
    assert_eq!(delete.rid, "image");
}

#[tokio::test]
async fn extractor_get_image_search_and_wait_map_arguments() {
    let f = Fixture::new(vec![
        ("GetContainerizedExtractor", get(), 0),
        (
            "SearchImages",
            r::SearchImagesResponse {
                images: vec![],
                next_page_token: None,
            }
            .encode_to_vec(),
            0,
        ),
        ("GetImage", image_reply(), 0),
        ("GetImage", image_reply(), 0),
    ])
    .await;
    handle(ExtractorCommands::Get(rid()), f.client.clone())
        .await
        .unwrap();
    handle(
        ExtractorCommands::Image {
            command: ImageCommands::Search {
                extractor: Some("extractor".into()),
                tag: Some("v1".into()),
                status: Some(ImageStatus::Ready),
                scope: scope(),
            },
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    handle(
        ExtractorCommands::Image {
            command: ImageCommands::Wait {
                rid: "image".into(),
                timeout: Some(1),
                scope: scope(),
            },
        },
        f.client.clone(),
    )
    .await
    .unwrap();
    let calls = f.mock.requests.lock().unwrap();
    let search = r::SearchImagesRequest::decode(calls[1].1.as_slice()).unwrap();
    assert_eq!(search.workspace_rid, "workspace");
    let r::search_filter::Filter::And(and) = search.filter.unwrap().filter.unwrap() else {
        panic!("expected combined filters")
    };
    assert_eq!(and.clauses.len(), 3);
    assert!(matches!(
        and.clauses[2].filter,
        Some(r::search_filter::Filter::Status(r::StatusFilter {
            status: 2
        }))
    ));
}
