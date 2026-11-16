use super::*;

#[tokio::test]
async fn global_capacity_stays_reserved_during_worker_reconnection() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(
        &mut h,
        WriteStreamPoolOptions {
            max_pending_messages: 1,
            ..pool_options()
        },
    )
    .await;
    let mut a = Box::pin(pool.write_chunk(data()));
    let first = tokio::select! { request = sessions[0].next() => request, result = &mut a => panic!("not acknowledged: {result:?}") };
    sessions[0].fail(Status::unavailable("rotate")).await;
    let mut restored = h.next_session().await;
    let mut b = Box::pin(pool.write_chunk(data()));
    assert!(futures::poll!(&mut b).is_pending());
    assert!(sessions[1].requests.try_recv().is_err());
    restored.declared(&restored.first).await;
    assert_eq!(restored.next().await, first);
    restored.ack(vec![request_id(&first)]).await;
    a.await.unwrap();
    let server = async {
        let next = sessions[1].next().await;
        sessions[1].ack(vec![request_id(&next)]).await;
    };
    let (result, _) = tokio::join!(&mut b, server);
    result.unwrap();
    drop(b);
    drop(sessions.remove(0));
    sessions.push(restored);
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn schema_broadcast_uses_the_global_message_window() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(
        &mut h,
        WriteStreamPoolOptions {
            max_pending_messages: 1,
            ..pool_options()
        },
    )
    .await;
    let mut declaring = Box::pin(pool.declare_schemas(vec![schema(2)]));
    let first = tokio::select! { request = sessions[0].next() => request, result = &mut declaring => panic!("not acknowledged: {result:?}") };
    assert!(sessions[1].requests.try_recv().is_err());
    sessions[0].declared(&first).await;
    let second = tokio::select! { request = sessions[1].next() => request, result = &mut declaring => panic!("second declaration not acknowledged: {result:?}") };
    sessions[1].declared(&second).await;
    declaring.await.unwrap();
    drain(pool, &mut sessions).await.unwrap();
}

#[tokio::test]
async fn terminal_worker_failure_stops_new_admission_but_other_workers_can_drain() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(&mut h, pool_options()).await;
    let mut a = Box::pin(pool.write_chunk(data()));
    tokio::select! { _ = sessions[0].next() => {}, result = &mut a => panic!("not acknowledged: {result:?}") };
    let mut b = Box::pin(pool.write_chunk(data()));
    let healthy = tokio::select! { request = sessions[1].next() => request, result = &mut b => panic!("not acknowledged: {result:?}") };
    sessions[0].fail(Status::permission_denied("revoked")).await;
    assert!(
        matches!(a.await, Err(WriteStreamError::Rpc(status)) if status.code()==Code::PermissionDenied)
    );
    assert!(
        matches!(pool.write_chunk(data()).await, Err(WriteStreamError::Rpc(status)) if status.code()==Code::PermissionDenied)
    );
    sessions[1].ack(vec![request_id(&healthy)]).await;
    b.await.unwrap();
    assert!(
        matches!(drain(pool, &mut sessions).await, Err(WriteStreamError::Rpc(status)) if status.code()==Code::PermissionDenied)
    );
}

#[tokio::test]
async fn per_message_rejection_does_not_close_pool_admission() {
    let mut h = Harness::start().await;
    let (pool, mut sessions) = open(&mut h, pool_options()).await;
    let mut a = Box::pin(pool.write_chunk(data()));
    let request = tokio::select! { request = sessions[0].next() => request, result = &mut a => panic!("not acknowledged: {result:?}") };
    sessions[0]
        .reply(Reply::Rejected(proto::Rejected {
            message_id: request_id(&request),
            status: Some(wire::google::rpc::Status {
                code: 3,
                message: "invalid".into(),
                details: vec![],
            }),
        }))
        .await;
    assert!(matches!(a.await, Err(WriteStreamError::Rejected(_))));
    let write = pool.write_chunk(data());
    let server = async {
        let next = sessions[1].next().await;
        sessions[1].ack(vec![request_id(&next)]).await;
    };
    let (result, _) = tokio::join!(write, server);
    result.unwrap();
    assert!(matches!(
        drain(pool, &mut sessions).await,
        Err(WriteStreamError::Rejected(_))
    ));
}

#[tokio::test]
async fn cancelling_pool_construction_closes_all_partially_opened_streams() {
    let mut h = Harness::start().await;
    let client = h.client.clone();
    let opening = tokio::spawn(async move {
        client
            .write_stream_pool(vec![schema(1)], pool_options())
            .await
    });
    let mut first = h.next_session().await;
    first.declared(&first.first).await;
    let mut second = h.next_session().await;
    opening.abort();
    assert!(matches!(opening.await, Err(error) if error.is_cancelled()));
    for session in [&mut first, &mut second] {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), session.requests.recv())
                .await
                .unwrap()
                .unwrap(),
            None
        );
    }
}
