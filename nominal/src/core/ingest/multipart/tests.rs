use super::*;

#[tokio::test]
async fn recovers_missing_etags_once_and_completes_in_part_order() {
    let receipts = vec![(2, None), (1, Some("first".to_owned()))];
    let parts = completion_parts(receipts, async {
        Ok(vec![Part::new(2, "second"), Part::new(1, "first")])
    })
    .await
    .unwrap();
    assert_eq!(parts, vec![Part::new(1, "first"), Part::new(2, "second")]);
}

#[tokio::test]
async fn skips_listing_when_puts_return_etags() {
    let receipts = vec![(1, Some("first".to_owned()))];
    let parts = completion_parts(receipts, async { panic!("unexpected list_parts") })
        .await
        .unwrap();
    assert_eq!(parts, vec![Part::new(1, "first")]);
}

#[tokio::test]
async fn rejects_incomplete_or_mismatched_listings() {
    for listing in [
        vec![Part::new(1, "one")],
        vec![Part::new(1, "one"), Part::new(1, "duplicate")],
        vec![Part::new(1, "one"), Part::new(3, "unexpected")],
        vec![Part::new(1, "one"), Part::new(2, "")],
    ] {
        assert!(
            completion_parts(vec![(1, None), (2, None)], async { Ok(listing) })
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn keeps_header_etags_outside_the_listed_page() {
    let receipts = (1..=1001)
        .map(|number| (number, (number != 1).then(|| "header".to_owned())))
        .collect();
    let listing = (1..=1000)
        .map(|number| Part::new(number, "listed"))
        .collect();
    let parts = completion_parts(receipts, async { Ok(listing) })
        .await
        .unwrap();
    assert_eq!(parts.len(), 1001);
    assert_eq!(parts[0], Part::new(1, "listed"));
    assert!(parts[1..].iter().all(|part| part.etag() == "header"));
}

#[tokio::test]
async fn successful_put_without_etag_lists_and_completes_without_retrying() {
    use serde_json::json;
    use wiremock::matchers::{body_bytes, body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let upload = "/upload/v1/multipart-upload";
    let session = format!("{upload}/id");
    Mock::given(method("POST"))
        .and(path(upload))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "uploadId": "id", "key": "key", "bucket": "bucket"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(&session))
        .and(query_param("partNumber", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "url": format!("{}/blob", server.uri()), "expiresInSeconds": 300
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/blob"))
        .and(body_bytes(b"abc"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(&session))
        .and(query_param("bucket", "bucket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"partNumber": 1, "etag": "listed", "size": 3}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{session}/complete")))
        .and(body_json(json!([{"partNumber": 1, "etag": "listed"}])))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"location": "s3://bucket/key"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = crate::NominalClient::builder("test-token")
        .base_url(server.uri())
        .build()
        .unwrap()
        .ingest();
    let result = upload_reader_to(
        client.conjure_client,
        &client.runtime,
        client.token,
        None,
        None,
        &b"abc"[..],
        3,
        "example.csv".into(),
        "text/csv".into(),
        UploadOptions::default().max_retries(2),
    )
    .await
    .unwrap();
    assert_eq!(result.s3_path, "s3://bucket/key");
}
