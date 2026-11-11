use super::*;

mod lifecycle;

fn pool_options() -> WriteStreamPoolOptions {
    WriteStreamPoolOptions {
        stream_count: 2,
        max_pending_messages: 4,
        max_pending_bytes: 1024 * 1024,
        stream_options: WriteStreamOptions {
            max_in_flight_messages: 2,
            ..options()
        },
    }
}

async fn open(h: &mut Harness, options: WriteStreamPoolOptions) -> (WriteStreamPool, Vec<Session>) {
    let client = h.client.clone();
    let opening =
        tokio::spawn(async move { client.write_stream_pool(vec![schema(1)], options).await });
    let mut sessions = Vec::new();
    for id in 1..=options.stream_count {
        let session = h.next_session().await;
        assert_eq!(
            session.first,
            connection::declaration(id as u64, vec![schema(1)])
        );
        assert_eq!(session.auth, "Bearer test-token");
        session.declared(&session.first).await;
        sessions.push(session);
    }
    (opening.await.unwrap().unwrap(), sessions)
}

async fn drain(pool: WriteStreamPool, sessions: &mut [Session]) -> Result<()> {
    let finish = tokio::spawn(pool.finish());
    for s in sessions {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), s.requests.recv())
                .await
                .unwrap()
                .unwrap(),
            None
        );
        drop(s.replies.take());
    }
    finish.await.unwrap()
}

#[tokio::test]
async fn distributes_concurrent_writes_across_fixed_members_without_extra_streams() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(&mut h, pool_options()).await;
    let writes = async {
        futures::try_join!(
            pool.write_chunk(data()),
            pool.write_chunk(data()),
            pool.write_chunk(data()),
            pool.write_chunk(data())
        )
    };
    let server = async {
        let a = sessions[0].next().await;
        let b = sessions[1].next().await;
        let c = sessions[0].next().await;
        let d = sessions[1].next().await;
        assert_eq!(
            [
                request_id(&a),
                request_id(&b),
                request_id(&c),
                request_id(&d)
            ],
            [3, 4, 5, 6]
        );
        for request in [&a, &b, &c, &d] {
            assert!(
                matches!(&request.request, Some(Message::Chunk(chunk)) if chunk.channels==data())
            );
        }
        sessions[1].ack(vec![request_id(&d), request_id(&b)]).await;
        sessions[0].ack(vec![request_id(&c), request_id(&a)]).await;
    };
    let (result, _) = tokio::join!(writes, server);
    result.unwrap();
    assert!(h.sessions.try_recv().is_err());
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn global_count_window_does_not_multiply_with_stream_count() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(
        &mut h,
        WriteStreamPoolOptions {
            max_pending_messages: 2,
            ..pool_options()
        },
    )
    .await;
    let pool = Arc::new(pool);
    let a = tokio::spawn({
        let pool = pool.clone();
        async move { pool.write_chunk(data()).await }
    });
    let first = sessions[0].next().await;
    let b = tokio::spawn({
        let pool = pool.clone();
        async move { pool.write_chunk(data()).await }
    });
    let second = sessions[1].next().await;
    let mut third = Box::pin(pool.write_chunk(data()));
    assert!(futures::poll!(&mut third).is_pending());
    assert!(sessions[0].requests.try_recv().is_err());
    assert!(sessions[1].requests.try_recv().is_err());
    sessions[0].ack(vec![request_id(&first)]).await;
    a.await.unwrap().unwrap();
    let server = async {
        let request = sessions[0].next().await;
        sessions[0].ack(vec![request_id(&request)]).await;
    };
    let (result, _) = tokio::join!(&mut third, server);
    result.unwrap();
    drop(third);
    sessions[1].ack(vec![request_id(&second)]).await;
    b.await.unwrap().unwrap();
    drain(Arc::try_unwrap(pool).ok().unwrap(), &mut sessions)
        .await
        .unwrap();
}

#[tokio::test]
async fn global_byte_window_is_shared_across_workers() {
    let mut h = Harness::start().await;
    let bytes = proto::WriteStreamRequest {
        request: Some(Message::Chunk(proto::WriteChunk {
            message_id: 3,
            channels: data(),
        })),
    }
    .encoded_len();
    let (pool, mut sessions) = open(
        &mut h,
        WriteStreamPoolOptions {
            max_pending_bytes: bytes,
            ..pool_options()
        },
    )
    .await;
    let mut a = Box::pin(pool.write_chunk(data()));
    let first = tokio::select! { request = sessions[0].next() => request, result = &mut a => panic!("no ack yet: {result:?}") };
    let mut b = Box::pin(pool.write_chunk(data()));
    assert!(futures::poll!(&mut b).is_pending());
    assert!(sessions[1].requests.try_recv().is_err());
    sessions[0].ack(vec![request_id(&first)]).await;
    a.await.unwrap();
    let server = async {
        let second = sessions[1].next().await;
        sessions[1].ack(vec![request_id(&second)]).await;
    };
    let (result, _) = tokio::join!(&mut b, server);
    result.unwrap();
    drop(b);
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn full_workers_wait_for_any_worker_capacity_without_blocking_ack_reads() {
    let mut h = Harness::start().await;
    let options = WriteStreamPoolOptions {
        stream_options: WriteStreamOptions {
            max_in_flight_messages: 1,
            ..options()
        },
        ..pool_options()
    };
    let (pool, mut sessions) = open(&mut h, options).await;
    let mut a = Box::pin(pool.write_chunk(data()));
    let first = tokio::select! { request = sessions[0].next() => request, result = &mut a => panic!("no ack yet: {result:?}") };
    let mut b = Box::pin(pool.write_chunk(data()));
    let second = tokio::select! { request = sessions[1].next() => request, result = &mut b => panic!("no ack yet: {result:?}") };
    let mut c = Box::pin(pool.write_chunk(data()));
    assert!(futures::poll!(&mut c).is_pending());
    sessions[1].ack(vec![request_id(&second)]).await;
    b.await.unwrap();
    let server = async {
        let third = sessions[1].next().await;
        sessions[1].ack(vec![request_id(&third)]).await;
    };
    let (result, _) = tokio::join!(&mut c, server);
    result.unwrap();
    drop(c);
    sessions[0].ack(vec![request_id(&first)]).await;
    a.await.unwrap();
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn cancelling_admitted_write_retains_global_capacity_until_acknowledged() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(
        &mut h,
        WriteStreamPoolOptions {
            max_pending_messages: 1,
            ..pool_options()
        },
    )
    .await;
    let mut cancelled = Box::pin(pool.write_chunk(data()));
    let first = tokio::select! { request = sessions[0].next() => request, result = &mut cancelled => panic!("no ack yet: {result:?}") };
    drop(cancelled);
    let mut next = Box::pin(pool.write_chunk(data()));
    assert!(futures::poll!(&mut next).is_pending());
    assert!(sessions[1].requests.try_recv().is_err());
    sessions[0].ack(vec![request_id(&first)]).await;
    let server = async {
        let request = sessions[1].next().await;
        sessions[1].ack(vec![request_id(&request)]).await;
    };
    let (result, _) = tokio::join!(&mut next, server);
    result.unwrap();
    drop(next);
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn one_worker_reconnects_without_replaying_or_blocking_another_workers_writes() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(
        &mut h,
        WriteStreamPoolOptions {
            stream_options: WriteStreamOptions {
                max_in_flight_messages: 1,
                ..options()
            },
            ..pool_options()
        },
    )
    .await;
    let mut a = Box::pin(pool.write_chunk(data()));
    let first = tokio::select! { request = sessions[0].next() => request, result = &mut a => panic!("no ack yet: {result:?}") };
    sessions[0]
        .fail(Status::unavailable("rotate this worker"))
        .await;
    let mut restored = h.next_session().await;
    assert!(
        matches!(&restored.first.request, Some(Message::DeclareSchemas(decl)) if decl.schemas==vec![schema(1)])
    );
    // Withhold restoration ack. A new write must still complete on the healthy worker.
    let healthy = async {
        let write = pool.write_chunk(data());
        let server = async {
            let request = sessions[1].next().await;
            sessions[1].ack(vec![request_id(&request)]).await;
        };
        let (result, _) = tokio::join!(write, server);
        result.unwrap();
    };
    healthy.await;
    restored.declared(&restored.first).await;
    assert_eq!(restored.next().await, first);
    restored.ack(vec![request_id(&first)]).await;
    a.await.unwrap();
    assert!(h.sessions.try_recv().is_err());
    drop(sessions.remove(0));
    sessions.push(restored);
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn new_schemas_are_acknowledged_on_every_worker_before_new_writes() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(&mut h, pool_options()).await;
    let mut declaring = Box::pin(pool.declare_schemas(vec![schema(2)]));
    let a = tokio::select! { request = sessions[0].next() => request, result = &mut declaring => panic!("not declared yet: {result:?}") };
    let b = sessions[1].next().await;
    assert!(
        matches!(&a.request, Some(Message::DeclareSchemas(decl)) if decl.schemas==vec![schema(2)])
    );
    sessions[0].declared(&a).await;
    let mut write = Box::pin(pool.write_chunk(data()));
    assert!(futures::poll!(&mut write).is_pending());
    assert!(sessions[0].requests.try_recv().is_err());
    sessions[1].declared(&b).await;
    declaring.await.unwrap();
    let server = async {
        let chunk = sessions[0].next().await;
        sessions[0].ack(vec![request_id(&chunk)]).await;
    };
    let (result, _) = tokio::join!(&mut write, server);
    result.unwrap();
    drop(write);
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn cancelled_partial_schema_broadcast_closes_admission_but_can_drain() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(&mut h, pool_options()).await;
    let mut declaring = Box::pin(pool.declare_schemas(vec![schema(2)]));
    let a = tokio::select! { request = sessions[0].next() => request, result = &mut declaring => panic!("not declared yet: {result:?}") };
    let b = sessions[1].next().await;
    sessions[0].declared(&a).await;
    drop(declaring);
    assert!(matches!(
        pool.write_chunk(data()).await,
        Err(WriteStreamError::Protocol(_))
    ));
    sessions[1].declared(&b).await;
    assert!(matches!(
        drain(pool, &mut sessions).await,
        Err(WriteStreamError::Protocol(_))
    ));
}

#[tokio::test]
async fn constructor_failure_closes_already_opened_streams() {
    let mut h = Harness::start().await;
    let client = h.client.clone();
    let opening = tokio::spawn(async move {
        client
            .write_stream_pool(vec![schema(1)], pool_options())
            .await
    });
    let mut first = h.next_session().await;
    first.declared(&first.first).await;
    let second = h.next_session().await;
    second
        .fail(Status::permission_denied("cannot open second stream"))
        .await;
    assert!(
        matches!(opening.await.unwrap(), Err(WriteStreamError::Rpc(status)) if status.code()==Code::PermissionDenied)
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), first.requests.recv())
            .await
            .unwrap()
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn invalid_pool_size_fails_without_opening_any_streams() {
    let h = Harness::start().await;
    assert!(matches!(
        h.client
            .write_stream_pool(
                vec![schema(1)],
                WriteStreamPoolOptions {
                    stream_count: 0,
                    ..pool_options()
                }
            )
            .await,
        Err(WriteStreamError::InvalidOptions(_))
    ));
}
