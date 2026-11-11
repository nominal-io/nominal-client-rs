use std::collections::{BTreeMap, BTreeSet};

use tokio::time::Instant;

use super::*;
use connection::{Connection, retryable, rpc};
use proto::write_stream_request::Request;
use proto::write_stream_response::Response;

pub(super) struct Writer {
    grpc: GrpcConnection,
    connection: Option<Connection>,
    schemas: BTreeMap<u32, proto::Schema>,
    pending: BTreeMap<u64, Pending>,
    shared: Arc<Shared>,
    commands: mpsc::Receiver<Command>,
    finishing: Option<oneshot::Sender<Result<()>>>,
    first_rejection: Option<WriteStreamError>,
}

enum Event {
    Sent(u64),
    Response(std::result::Result<Option<proto::WriteStreamResponse>, tonic::Status>),
    Command(Option<Command>),
    Timeout,
}

impl Writer {
    pub fn new(
        grpc: GrpcConnection,
        connection: Connection,
        schemas: Vec<proto::Schema>,
        shared: Arc<Shared>,
        commands: mpsc::Receiver<Command>,
    ) -> Self {
        Self {
            grpc,
            connection: Some(connection),
            schemas: schemas.into_iter().map(|s| (s.schema_id, s)).collect(),
            pending: BTreeMap::new(),
            shared,
            commands,
            finishing: None,
            first_rejection: None,
        }
    }

    pub async fn run(mut self) {
        let result = self.drive().await;
        if let (Some(pool), Err(error)) = (&self.shared.pool, &result) {
            pool.fail(error.clone());
        }
        self.shared.close();
        let error = result.clone().err().unwrap_or(WriteStreamError::Closed);
        for (_, pending) in std::mem::take(&mut self.pending) {
            let _ = pending.reply.send(Err(error.clone()));
        }
        self.commands.close();
        while let Some(command) = self.commands.recv().await {
            match command {
                Command::Submit(pending) => {
                    let _ = pending.reply.send(Err(error.clone()));
                }
                Command::Finish(reply) => {
                    let _ = reply.send(result.clone());
                }
            }
        }
        if let Some(reply) = self.finishing.take() {
            let _ = reply.send(result);
        }
    }

    async fn drive(&mut self) -> Result<()> {
        loop {
            if self.finishing.is_some() && self.pending.is_empty() {
                if let Some(connection) = self.connection.take() {
                    connection::finish(connection, self.shared.options.response_timeout).await?;
                }
                return self.first_rejection.take().map_or(Ok(()), Err);
            }
            if self.connection.is_none() {
                if self.pending.is_empty() {
                    let command = self.commands.recv().await;
                    self.command(command)?;
                    continue;
                }
                self.recover().await?;
            }
            // Reserve outbound capacity inside select: a full request queue must never
            // stop acknowledgment reads. Deadlines start only when handed to transport.
            let outgoing = self
                .pending
                .iter()
                .find(|(_, p)| p.sent_at.is_none())
                .map(|(id, _)| *id);
            let deadline = self
                .pending
                .values()
                .filter_map(|p| p.sent_at)
                .min()
                .map(|sent| sent + self.shared.options.response_timeout);
            let connection = self
                .connection
                .as_mut()
                .expect("connection established above");
            let event = tokio::select! {
                permit = connection.sender.reserve(), if outgoing.is_some() => {
                    let id = outgoing.expect("outgoing branch is enabled only with a message");
                    match permit {
                        Ok(permit) => { permit.send(self.pending[&id].request.clone()); Event::Sent(id) },
                        Err(_) => Event::Response(Err(tonic::Status::unavailable("request transport closed"))),
                    }
                },
                response = connection.responses.message() => Event::Response(response),
                command = self.commands.recv(), if self.finishing.is_none() => Event::Command(command),
                _ = tokio::time::sleep_until(deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(86400))), if deadline.is_some() => Event::Timeout,
            };
            match event {
                Event::Sent(id) => {
                    self.pending
                        .get_mut(&id)
                        .expect("pending message was reserved")
                        .sent_at = Some(Instant::now())
                }
                Event::Command(command) => self.command(command)?,
                Event::Response(Ok(Some(response))) => self.response(response)?,
                Event::Response(result) => {
                    let error = result.err().map(rpc).unwrap_or_else(|| {
                        rpc(tonic::Status::unavailable("server completed stream"))
                    });
                    if !retryable(&error) {
                        return Err(error);
                    }
                    self.connection.take();
                }
                Event::Timeout => {
                    self.connection.take();
                }
            }
        }
    }

    fn command(&mut self, command: Option<Command>) -> Result<()> {
        match command {
            Some(Command::Submit(pending)) => {
                self.pending.insert(request_id(&pending.request), pending);
            }
            Some(Command::Finish(reply)) => {
                self.finishing = Some(reply);
            }
            None => return Err(WriteStreamError::Closed),
        }
        Ok(())
    }

    fn response(&mut self, response: proto::WriteStreamResponse) -> Result<()> {
        match response.response {
            Some(Response::Accepted(ack)) => {
                // Validate the whole batch before releasing any retained write.
                let mut unique = BTreeSet::new();
                for id in &ack.message_ids {
                    let pending = self.sent(*id)?;
                    if !unique.insert(*id)
                        || !matches!(pending.request.request, Some(Request::Chunk(_)))
                    {
                        return Err(WriteStreamError::Protocol(
                            "invalid or duplicate chunk acknowledgment",
                        ));
                    }
                }
                for id in ack.message_ids {
                    self.complete(id, Ok(()));
                }
            }
            Some(Response::SchemasDeclared(ack)) => {
                let pending = self.sent(ack.message_id)?;
                let Some(Request::DeclareSchemas(declaration)) = &pending.request.request else {
                    return Err(WriteStreamError::Protocol(
                        "schema acknowledgment refers to chunk",
                    ));
                };
                let mut installed = self.schemas.values().cloned().collect::<Vec<_>>();
                installed.extend(declaration.schemas.iter().cloned());
                connection::validate_schemas(&installed).map_err(|_| {
                    WriteStreamError::Protocol("server acknowledged invalid schema registry")
                })?;
                self.schemas = installed.into_iter().map(|s| (s.schema_id, s)).collect();
                self.complete(ack.message_id, Ok(()));
            }
            Some(Response::Rejected(rejection)) => {
                let id = rejection.message_id;
                self.sent(id)?;
                let error = connection::rejected(rejection)?;
                self.first_rejection.get_or_insert_with(|| error.clone());
                self.complete(id, Err(error));
            }
            None => return Err(WriteStreamError::Protocol("response has no variant")),
        }
        Ok(())
    }

    fn sent(&self, id: u64) -> Result<&Pending> {
        self.pending
            .get(&id)
            .filter(|p| p.sent_at.is_some())
            .ok_or(WriteStreamError::Protocol(
                "acknowledgment refers to unknown or unsent message",
            ))
    }

    fn complete(&mut self, id: u64, result: Result<()>) {
        let pending = self
            .pending
            .remove(&id)
            .expect("response IDs were validated");
        let _ = pending.reply.send(result);
        // Dropping the request and its count/byte permits opens the admission window.
    }

    async fn recover(&mut self) -> Result<()> {
        let schemas = self.schemas.values().cloned().collect::<Vec<_>>();
        let mut attempt = 0;
        loop {
            // Per-message counters prevent a slow message from retrying forever while
            // other messages continue to make progress on successive connections.
            if self
                .pending
                .values()
                .any(|p| p.replays >= self.shared.options.max_retries)
            {
                return Err(WriteStreamError::RetryExhausted);
            }
            for pending in self.pending.values_mut() {
                pending.replays += 1;
                pending.sent_at = None;
            }
            connection::backoff(self.shared.options, attempt).await;
            attempt = attempt.saturating_add(1);
            match connection::open(&self.grpc, &schemas, &self.shared.ids, self.shared.options)
                .await
            {
                Ok(connection) => {
                    self.connection = Some(connection);
                    return Ok(());
                }
                Err(error) if retryable(&error) => {}
                Err(error) => return Err(error),
            }
        }
    }
}
