use super::*;

#[tokio::test]
async fn restoring_more_than_100_schemas_waits_for_every_batch_before_replay() {
    let mut h = Harness::start().await;
    let schemas = (1..=101).map(schema).collect::<Vec<_>>();
    let initial = schemas.clone();
    let client = h.client.clone();
    let opening =
        tokio::spawn(async move { client.write_stream_with_options(initial, options()).await });
    let mut first = h.next_session().await;
    assert_eq!(
        first.first,
        connection::declaration(1, schemas[..100].to_vec())
    );
    first.declared(&first.first).await;
    let second_batch = first.next().await;
    assert_eq!(
        second_batch,
        connection::declaration(2, schemas[100..].to_vec())
    );
    first.declared(&second_batch).await;
    let stream = opening.await.unwrap().unwrap();
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    let unanswered = first.next().await;
    first.fail(Status::unavailable("rotation")).await;
    let mut second = h.next_session().await;
    assert_eq!(
        second.first,
        connection::declaration(4, schemas[..100].to_vec())
    );
    assert!(second.requests.try_recv().is_err());
    second.declared(&second.first).await;
    let restored_batch = second.next().await;
    assert_eq!(
        restored_batch,
        connection::declaration(5, schemas[100..].to_vec())
    );
    assert!(second.requests.try_recv().is_err());
    second.declared(&restored_batch).await;
    assert_eq!(second.next().await, unanswered);
    second.ack(vec![receipt.message_id()]).await;
    receipt.acknowledged().await.unwrap();
}

#[tokio::test]
async fn unrelated_acknowledgments_do_not_reset_an_old_messages_retry_budget() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h
        .open(WriteStreamOptions {
            max_retries: 1,
            ..options()
        })
        .await;
    let old = stream.enqueue_chunk(data()).await.unwrap();
    let unanswered = first.next().await;
    first.fail(Status::unavailable("first failure")).await;
    let mut second = h.next_session().await;
    second.declared(&second.first).await;
    assert_eq!(second.next().await, unanswered);
    let fresh = stream.enqueue_chunk(data()).await.unwrap();
    second.next().await;
    second.ack(vec![fresh.message_id()]).await;
    fresh.acknowledged().await.unwrap();
    second.fail(Status::unavailable("second failure")).await;
    assert!(matches!(
        old.acknowledged().await,
        Err(WriteStreamError::RetryExhausted)
    ));
    assert!(h.sessions.try_recv().is_err());
}

#[tokio::test]
async fn failed_schema_restoration_is_terminal_and_does_not_replay_data() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    first.next().await;
    first.fail(Status::unavailable("reconnect")).await;
    let mut second = h.next_session().await;
    second
        .reply(Reply::Rejected(proto::Rejected {
            message_id: request_id(&second.first),
            status: Some(wire::google::rpc::Status {
                code: Code::PermissionDenied as i32,
                message: "revoked".into(),
                details: vec![],
            }),
        }))
        .await;
    assert!(
        matches!(receipt.acknowledged().await, Err(WriteStreamError::Rejected(status)) if status.code()==Code::PermissionDenied)
    );
    assert!(second.requests.try_recv().ok().flatten().is_none());
    assert!(matches!(
        stream.enqueue_chunk(data()).await,
        Err(WriteStreamError::Closed)
    ));
}

#[tokio::test]
async fn initial_transport_failure_retries_schema_installation() {
    let mut h = Harness::start().await;
    let client = h.client.clone();
    let opening = tokio::spawn(async move {
        client
            .write_stream_with_options(vec![schema(1)], options())
            .await
    });
    let first = h.next_session().await;
    assert_eq!(first.first, connection::declaration(1, vec![schema(1)]));
    first.fail(Status::unavailable("retry opening")).await;
    let second = h.next_session().await;
    assert_eq!(second.first, connection::declaration(2, vec![schema(1)]));
    second.declared(&second.first).await;
    let stream = opening.await.unwrap().unwrap();
    drop(stream);
}

#[tokio::test]
async fn initial_schema_rejection_is_not_retried() {
    let mut h = Harness::start().await;
    let client = h.client.clone();
    let opening = tokio::spawn(async move {
        client
            .write_stream_with_options(vec![schema(1)], options())
            .await
    });
    let s = h.next_session().await;
    s.reply(Reply::Rejected(proto::Rejected {
        message_id: 1,
        status: Some(wire::google::rpc::Status {
            code: 3,
            message: "invalid".into(),
            details: vec![],
        }),
    }))
    .await;
    assert!(
        matches!(opening.await.unwrap(), Err(WriteStreamError::Rejected(status)) if status.code()==Code::InvalidArgument)
    );
    assert!(h.sessions.try_recv().is_err());
}

#[tokio::test]
async fn cancellation_while_waiting_for_admission_does_not_submit_a_message() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h
        .open(WriteStreamOptions {
            max_in_flight_messages: 1,
            ..options()
        })
        .await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    {
        let mut waiting = pin!(stream.enqueue_chunk(data()));
        assert!(futures::poll!(&mut waiting).is_pending());
    }
    s.ack(vec![a.message_id()]).await;
    a.acknowledged().await.unwrap();
    assert_eq!(stream.shared.messages.available_permits(), 1);
    assert!(s.requests.try_recv().is_err());
    let b = stream.enqueue_chunk(data()).await.unwrap();
    assert_eq!(b.message_id(), 4); // Cancelled admission consumed ID 3, but never sent it.
    assert_eq!(request_id(&s.next().await), 4);
    s.ack(vec![4]).await;
    b.acknowledged().await.unwrap();
}

#[tokio::test]
async fn cancelling_public_write_after_admission_still_allows_finish_to_drain_it() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let mut write = Box::pin(stream.write_chunk(data()));
    let request = tokio::select! {
        request = s.next() => request,
        result = &mut write => panic!("write completed without acknowledgment: {result:?}"),
    };
    drop(write);
    assert_eq!(
        stream.shared.messages.available_permits(),
        options().max_in_flight_messages - 1
    );
    let finish = tokio::spawn(stream.finish());
    s.ack(vec![request_id(&request)]).await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), s.requests.recv())
            .await
            .unwrap()
            .unwrap(),
        None
    );
    drop(s.replies.take());
    finish.await.unwrap().unwrap();
}

#[tokio::test]
async fn individual_message_size_limit_is_checked_before_enqueueing() {
    let mut h = Harness::start().await;
    let (stream, _) = h.open(options()).await;
    let oversized = vec![proto::ChannelData {
        schema_id: 1,
        timestamps: vec![Timestamp {
            seconds: 1,
            nanos: 0,
        }],
        data: Some(proto::channel_data::Data::Values(proto::Values {
            values: Some(proto::values::Values::Strings(proto::Strings {
                values: vec!["x".repeat(MAX_MESSAGE_BYTES)],
            })),
        })),
    }];
    assert!(matches!(
        stream.enqueue_chunk(oversized).await,
        Err(WriteStreamError::MessageTooLarge)
    ));
    assert_eq!(
        stream.shared.messages.available_permits(),
        options().max_in_flight_messages
    );
    assert_eq!(
        stream.shared.bytes.available_permits(),
        options().max_in_flight_bytes
    );
}
