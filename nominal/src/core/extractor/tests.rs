use super::*;
#[test]
fn response_identity_cannot_be_empty() {
    assert!(ContainerImage::from_proto(proto::ContainerImage::default(), "w".into()).is_err());
    assert!(
        ContainerizedExtractor::from_proto(ingest_proto::ContainerizedExtractor::default())
            .is_err()
    );
}
#[test]
fn extractor_update_preserves_unset_and_false() {
    let request = ExtractorUpdate::default()
        .archived(false)
        .into_request("e", "w");
    assert_eq!(request.name, None);
    assert_eq!(request.description, None);
    assert_eq!(request.is_archived, Some(false));
}
#[test]
fn image_unknown_values_and_absent_defaults_survive() {
    let image = ContainerImage::from_proto(
        proto::ContainerImage {
            rid: "image".into(),
            extractor_rid: "extractor".into(),
            status: 999,
            file_output_format: 888,
            ..Default::default()
        },
        "w".into(),
    )
    .unwrap();
    assert_eq!(image.status(), ContainerImageStatus::Unknown(999));
    assert_eq!(image.file_output_format(), FileOutputFormat::Unknown(888));
    assert!(image.default_timestamp().is_none());
}
#[test]
fn extraction_contract_roundtrips() {
    let input = FileExtractionInput::new("Source", "INPUT")
        .description("data")
        .suffix(".csv")
        .required(false);
    let encoded = input.clone().into_proto();
    assert_eq!(FileExtractionInput::from_proto(encoded).unwrap(), input);
    let parameter = FileExtractionParameter::new("Scale", "SCALE")
        .description("factor")
        .required(true);
    assert_eq!(
        FileExtractionParameter::from_proto(parameter.clone().into_proto()),
        parameter
    );
}

use crate::core::{NominalClient, WaitOptions};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use prost::Message;
use std::{
    collections::VecDeque,
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

type MockReply = (String, Vec<u8>, u16);
type RecordedRequest = (String, Vec<u8>);

#[derive(Clone)]
struct Mock {
    replies: Arc<Mutex<VecDeque<MockReply>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
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
fn image_proto(status: i32) -> proto::ContainerImage {
    proto::ContainerImage {
        rid: "image".into(),
        extractor_rid: "extractor".into(),
        status,
        ..Default::default()
    }
}
fn image(status: i32) -> ContainerImage {
    ContainerImage::from_proto(image_proto(status), "A".into()).unwrap()
}
fn extractor() -> ContainerizedExtractor {
    ContainerizedExtractor::from_proto(ingest_proto::ContainerizedExtractor {
        rid: "extractor".into(),
        workspace_rid: "A".into(),
        ..Default::default()
    })
    .unwrap()
}
fn image_reply(status: i32) -> Vec<u8> {
    proto::GetImageResponse {
        image: Some(image_proto(status)),
    }
    .encode_to_vec()
}
#[tokio::test]
async fn activation_refreshes_pending_then_ready_and_mutates_once_in_snapshot_workspace() {
    let fixture = Fixture::new(vec![
        ("GetImage", image_reply(1), 0),
        ("GetImage", image_reply(2), 0),
        (
            "UpdateContainerizedExtractor",
            ingest_proto::UpdateContainerizedExtractorResponse {
                extractor: Some(ingest_proto::ContainerizedExtractor {
                    rid: "extractor".into(),
                    workspace_rid: "A".into(),
                    ..Default::default()
                }),
            }
            .encode_to_vec(),
            0,
        ),
    ])
    .await;
    fixture
        .client
        .extractors()
        .in_workspace("B")
        .activate(
            &extractor(),
            &image(2),
            Activation::Wait(WaitOptions::default().interval(std::time::Duration::from_millis(1))),
        )
        .await
        .unwrap();
    let requests = fixture.mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        proto::GetImageRequest::decode(requests[0].1.as_slice())
            .unwrap()
            .workspace_rid,
        "A"
    );
    let update =
        ingest_proto::UpdateContainerizedExtractorRequest::decode(requests[2].1.as_slice())
            .unwrap();
    assert_eq!(update.workspace_rid, "A");
    assert_eq!(update.active_container_image_rid.as_deref(), Some("image"));
    assert_eq!(update.name, None);
}
#[tokio::test]
async fn activation_rejects_pending_failed_unknown_without_mutation() {
    for status in [1, 3, 77] {
        let fixture = Fixture::new(vec![("GetImage", image_reply(status), 0)]).await;
        assert!(
            fixture
                .client
                .extractors()
                .activate(&extractor(), &image(2), Activation::RequireReady)
                .await
                .is_err()
        );
        assert_eq!(fixture.mock.requests.lock().unwrap().len(), 1);
    }
}
#[tokio::test]
async fn mutation_unavailable_is_attempted_once() {
    let fixture = Fixture::new(vec![("DeleteImage", vec![], 14)]).await;
    assert!(
        fixture
            .client
            .container_images()
            .delete(&image(2))
            .await
            .is_err()
    );
    assert_eq!(fixture.mock.requests.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn image_search_follows_pages_and_ands_filters() {
    let response = |tag: &str, next: Option<String>| {
        proto::SearchImagesResponse {
            images: vec![proto::ContainerImage {
                tag: tag.into(),
                ..image_proto(2)
            }],
            next_page_token: next,
        }
        .encode_to_vec()
    };
    let fixture = Fixture::new(vec![
        ("SearchImages", response("first", Some("next".into())), 0),
        ("SearchImages", response("second", None), 0),
    ])
    .await;
    let result = fixture
        .client
        .container_images()
        .in_workspace("A")
        .search(
            ContainerImageQuery::default()
                .extractor("extractor")
                .tag("tag")
                .status(ContainerImageStatus::Ready),
        )
        .await
        .unwrap();
    assert_eq!(
        result.iter().map(|i| i.tag()).collect::<Vec<_>>(),
        ["first", "second"]
    );
    let requests = fixture.mock.requests.lock().unwrap();
    let second = proto::SearchImagesRequest::decode(requests[1].1.as_slice()).unwrap();
    assert_eq!(second.next_page_token.as_deref(), Some("next"));
    let Some(proto::search_filter::Filter::And(filter)) = second.filter.unwrap().filter else {
        panic!("expected AND");
    };
    assert_eq!(filter.clauses.len(), 3);
}
#[tokio::test]
async fn preflight_rejects_invalid_contract_and_timestamp_before_file_or_network() {
    for registration in [
        ImageRegistration::new(
            "tag",
            RegisterableOutputFormat::Csv,
            crate::core::Timestamp::iso8601("time"),
        ),
        ImageRegistration::new(
            "tag",
            RegisterableOutputFormat::Csv,
            crate::core::Timestamp::iso8601(""),
        )
        .input(FileExtractionInput::new("file", "INPUT")),
    ] {
        let fixture = Fixture::new(vec![]).await;
        let error = fixture
            .client
            .container_images()
            .register(
                &extractor(),
                std::path::Path::new("/does-not-exist.tar"),
                registration,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            crate::Error::Extractor(ExtractorError::InvalidRegistration(_))
        ));
        assert!(fixture.mock.requests.lock().unwrap().is_empty());
    }
}
#[tokio::test]
async fn mismatch_rejects_before_any_request() {
    let fixture = Fixture::new(vec![]).await;
    let wrong_workspace = ContainerImage::from_proto(image_proto(2), "B".into()).unwrap();
    let wrong_extractor = ContainerImage::from_proto(
        proto::ContainerImage {
            extractor_rid: "other".into(),
            ..image_proto(2)
        },
        "A".into(),
    )
    .unwrap();
    for image in [wrong_workspace, wrong_extractor] {
        assert!(
            fixture
                .client
                .extractors()
                .activate(&extractor(), &image, Activation::RequireReady)
                .await
                .is_err()
        );
    }
    assert!(fixture.mock.requests.lock().unwrap().is_empty());
}
#[tokio::test]
async fn waiting_failed_or_timed_out_never_activates() {
    for status in [3, 1] {
        let fixture = Fixture::new(vec![("GetImage", image_reply(status), 0)]).await;
        let error = fixture
            .client
            .extractors()
            .activate(
                &extractor(),
                &image(2),
                Activation::Wait(
                    WaitOptions::default()
                        .interval(std::time::Duration::from_secs(1))
                        .timeout(std::time::Duration::from_millis(50)),
                ),
            )
            .await
            .unwrap_err();
        if status == 3 {
            assert!(matches!(
                error,
                crate::Error::Extractor(ExtractorError::ImageFailed { .. })
            ));
        } else {
            assert!(matches!(
                error,
                crate::Error::Extractor(ExtractorError::Timeout { .. })
            ));
        }
        assert_eq!(fixture.mock.requests.lock().unwrap().len(), 1);
    }
}
#[tokio::test]
async fn extractor_refresh_uses_snapshot_workspace_and_missing_response_is_error() {
    let fixture = Fixture::new(vec![(
        "GetContainerizedExtractor",
        ingest_proto::GetContainerizedExtractorResponse { extractor: None }.encode_to_vec(),
        0,
    )])
    .await;
    assert!(matches!(
        fixture
            .client
            .extractors()
            .in_workspace("B")
            .refresh(&extractor())
            .await
            .unwrap_err(),
        crate::Error::UnexpectedResponse { field: "extractor" }
    ));
    let requests = fixture.mock.requests.lock().unwrap();
    let request =
        ingest_proto::GetContainerizedExtractorRequest::decode(requests[0].1.as_slice()).unwrap();
    assert_eq!(request.workspace_rid, "A");
}
#[tokio::test]
async fn create_and_update_preserve_full_request_and_service_errors() {
    let fixture = Fixture::new(vec![
        ("CreateContainerizedExtractor", vec![], 6),
        ("UpdateContainerizedExtractor", vec![], 7),
    ])
    .await;
    assert!(
        fixture
            .client
            .extractors()
            .in_workspace("A")
            .create(ExtractorCreate::new("name").description("description"))
            .await
            .is_err()
    );
    assert!(
        fixture
            .client
            .extractors()
            .in_workspace("B")
            .update(
                &extractor(),
                ExtractorUpdate::default()
                    .name("new")
                    .description("")
                    .archived(false)
            )
            .await
            .is_err()
    );
    let requests = fixture.mock.requests.lock().unwrap();
    let create =
        ingest_proto::CreateContainerizedExtractorRequest::decode(requests[0].1.as_slice())
            .unwrap();
    assert_eq!(create.workspace_rid, "A");
    assert_eq!(create.name, "name");
    assert_eq!(create.description.as_deref(), Some("description"));
    let update =
        ingest_proto::UpdateContainerizedExtractorRequest::decode(requests[1].1.as_slice())
            .unwrap();
    assert_eq!(update.workspace_rid, "A");
    assert_eq!(update.name.as_deref(), Some("new"));
    assert_eq!(update.description.as_deref(), Some(""));
    assert_eq!(update.is_archived, Some(false));
}
#[test]
fn registration_encodes_completed_upload_and_complete_contract() {
    let input = FileExtractionInput::new("source", "INPUT")
        .description("source data")
        .suffix(".csv")
        .suffix(".parquet")
        .required(true);
    let parameter = FileExtractionParameter::new("scale", "SCALE")
        .description("scale factor")
        .required(false);
    let timestamp = crate::core::Timestamp::epoch("time", crate::core::TimeUnit::Milliseconds);
    let request = ImageRegistration::new(
        "immutable-tag",
        RegisterableOutputFormat::Manifest,
        timestamp.clone(),
    )
    .input(input.clone())
    .parameter(parameter.clone())
    .into_request(&extractor(), "s3://bucket/completed.tar".into());
    assert_eq!(request.object_path, "s3://bucket/completed.tar");
    assert_eq!(request.workspace_rid, "A");
    assert_eq!(request.extractor_rid, "extractor");
    assert_eq!(request.tag, "immutable-tag");
    assert_eq!(request.file_output_format, 6);
    assert_eq!(
        request.default_timestamp_metadata,
        Some(timestamp.to_registry_proto())
    );
    assert_eq!(request.inputs, vec![input.into_proto()]);
    assert_eq!(request.parameters, vec![parameter.into_proto()]);
    assert!(request.source_image_rid.is_none());
}

#[tokio::test]
async fn waiting_unknown_status_rejects_promptly_without_deadline_or_activation() {
    let fixture = Fixture::new(vec![("GetImage", image_reply(78), 0)]).await;
    let extractors = fixture.client.extractors();
    let extractor = extractor();
    let image = image(2);
    let operation =
        extractors.activate(&extractor, &image, Activation::Wait(WaitOptions::default()));
    let error = tokio::time::timeout(std::time::Duration::from_millis(200), operation)
        .await
        .expect("unknown readiness must fail without waiting")
        .unwrap_err();
    assert!(matches!(
        error,
        crate::Error::Extractor(ExtractorError::NotReady {
            status: ContainerImageStatus::Unknown(78),
            ..
        })
    ));
    assert_eq!(fixture.mock.requests.lock().unwrap().len(), 1);
}
