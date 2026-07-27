// Copyright 2026 Daymon Littrell-Reyes
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Tokio actor for concurrent access to a Chessnut Move board.
//!
//! [`spawn`] owns a [`TokioTransport`] in one task and returns a cloneable
//! [`BoardHandle`]. Commands from multiple handles are serialized, request and
//! response helpers are correlated one at a time, and every decoded
//! [`BoardEvent`] is published to each [`EventStream`] subscriber.
//!
//! Transport failures stop the actor and are retained in [`ActorExit`].
//! Malformed protocol notifications are reported through
//! [`EventStreamError::Decode`] without stopping the actor.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, ready};
use std::collections::VecDeque;
use std::time::Duration;

use thiserror::Error;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::{Stream, StreamExt};

use crate::protocol::{BatteryStatus, BoardEvent, Command, PieceStatus};
#[cfg(doc)]
use crate::transport;
use crate::transport::{
  DecodeNotificationError, DecodedNotification, MAX_NOTIFICATION_LEN, Notification,
  NotificationSource, decode_notification,
};

/// A transport whose operations can run inside a task spawned by Tokio.
///
/// Unlike [`AsyncTransport`][transport::AsyncTransport], every returned future is
/// explicitly `Send`. This keeps the runtime-neutral trait usable by local and
/// embedded executors.
///
/// # Examples
///
/// ```
/// use core::convert::Infallible;
/// use chessnut_move::protocol::Command;
/// use chessnut_move::transport::tokio::TokioTransport;
/// use chessnut_move::transport::{Notification, NotificationSource};
///
/// struct MyTransport;
///
/// impl TokioTransport for MyTransport {
///     type Error = Infallible;
///
///     async fn subscribe(
///         &mut self,
///         _source: NotificationSource,
///     ) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     async fn write_command(
///         &mut self,
///         _command: &Command,
///     ) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     async fn next_notification<'a>(
///         &'a mut self,
///         buffer: &'a mut [u8],
///     ) -> Result<Notification<'a>, Self::Error> {
///         buffer[..34].fill(0);
///         Ok(Notification::new(
///             NotificationSource::Position,
///             &buffer[..34],
///         ))
///     }
/// }
/// ```
pub trait TokioTransport: Send + 'static {
  /// Error returned by Bluetooth or adapter operations.
  type Error: Send + 'static;

  /// Enables notifications for one protocol source.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the adapter cannot enable the corresponding
  /// [GATT characteristic][NotificationSource::characteristic].
  fn subscribe(
    &mut self,
    source: NotificationSource,
  ) -> impl Future<Output = Result<(), Self::Error>> + Send + '_;

  /// Disables notifications for one protocol source.
  ///
  /// The default implementation performs no operation.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the adapter cannot disable the corresponding
  /// [GATT characteristic][NotificationSource::characteristic].
  fn unsubscribe(
    &mut self,
    _source: NotificationSource,
  ) -> impl Future<Output = Result<(), Self::Error>> + Send + '_ {
    async { Ok(()) }
  }

  /// Writes an encoded command using its
  /// [required write kind][Command::write_kind].
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the command cannot be written.
  fn write_command<'a>(
    &'a mut self,
    command: &'a Command,
  ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

  /// Receives the next notification into the supplied buffer.
  ///
  /// The returned [`Notification`] must borrow its bytes from `buffer`.
  /// Implementations must return an error instead of truncating a notification
  /// that exceeds the buffer.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when notification delivery ends, Bluetooth I/O
  /// fails, the source is unknown, or the supplied buffer is too small.
  fn next_notification<'a>(
    &'a mut self,
    buffer: &'a mut [u8],
  ) -> impl Future<Output = Result<Notification<'a>, Self::Error>> + Send + 'a;

  /// Closes transport-owned resources after subscriptions are disabled.
  ///
  /// The default implementation performs no operation.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when transport cleanup fails.
  fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send + '_ {
    async { Ok(()) }
  }
}

/// Queue capacities and timeout behavior for a board actor.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use chessnut_move::transport::tokio::ActorConfig;
///
/// let config = ActorConfig {
///     request_timeout: Duration::from_secs(10),
///     ..ActorConfig::default()
/// };
/// assert_eq!(config.request_timeout, Duration::from_secs(10));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorConfig {
  /// Maximum number of handle messages waiting for the actor.
  pub command_capacity: usize,

  /// Number of events retained for each lag-aware subscriber.
  pub event_capacity: usize,

  /// Maximum number of queries waiting behind the active query.
  pub query_capacity: usize,

  /// Time allowed for a battery or piece-status response.
  pub request_timeout: Duration,
}

impl Default for ActorConfig {
  fn default() -> Self {
    Self {
      command_capacity: 32,
      event_capacity: 64,
      query_capacity: 32,
      request_timeout: Duration::from_secs(5),
    }
  }
}

/// Reports invalid actor configuration supplied to [`spawn`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum SpawnError {
  /// [`ActorConfig::command_capacity`] was zero.
  #[error("command channel capacity must be greater than zero")]
  ZeroCommandCapacity,

  /// [`ActorConfig::event_capacity`] was zero.
  #[error("event channel capacity must be greater than zero")]
  ZeroEventCapacity,
}

/// Current lifecycle phase of the board actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LifecycleState {
  /// The actor is subscribing and enabling realtime updates.
  Starting,

  /// Initialization completed and handle requests are accepted.
  Running,

  /// The actor is rejecting new requests and cleaning up the transport.
  ShuttingDown,

  /// The actor completed without a fatal error.
  Stopped,

  /// Initialization, transport I/O, or cleanup failed.
  Faulted,
}

/// Reports why a [`BoardHandle`] operation could not complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum HandleError {
  /// The actor task or its request channel has ended.
  #[error("the board actor has stopped")]
  ActorStopped,

  /// Graceful shutdown has started.
  #[error("the board actor is shutting down")]
  ShuttingDown,

  /// A transport operation failed.
  ///
  /// The underlying transport error is available from [`ActorExit::result`].
  #[error("the transport operation failed; inspect BoardTask for the underlying error")]
  TransportFailed,

  /// A query did not receive its matching response before the configured timeout.
  #[error("the board request timed out")]
  RequestTimedOut,

  /// The bounded queue for serialized queries is full.
  #[error("the pending query queue is full")]
  QueryQueueFull,
}

/// Reports a recoverable or terminal condition while consuming an [`EventStream`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum EventStreamError {
  /// The subscriber fell behind and older events were discarded.
  #[error("event subscriber lagged and missed {0} events")]
  Lagged(u64),

  /// A malformed notification was skipped while the actor remained running.
  #[error(transparent)]
  Decode(#[from] DecodeNotificationError),

  /// The actor closed the event channel.
  #[error("the board event stream is closed")]
  Closed,
}

/// Fatal error retained when the board actor terminates.
#[derive(Debug, Error)]
pub enum ActorError<E> {
  /// A transport operation failed during initialization, operation, or cleanup.
  #[error("transport error: {0}")]
  Transport(E),
}

/// A cloneable command and request handle for a running board actor.
///
/// Clones share one bounded request channel. Dropping the final handle closes
/// that channel; the actor then performs normal transport cleanup. Keep the
/// corresponding [`BoardTask`] when the final [`ActorExit`] result or transport
/// is needed.
///
/// # Examples
///
/// ```no_run
/// use chessnut_move::protocol::{Command, LedPattern};
/// use chessnut_move::transport::tokio::{BoardHandle, HandleError};
///
/// # async fn turn_off_leds(board: &BoardHandle) -> Result<(), HandleError> {
/// board.send(Command::set_leds(&LedPattern::default())).await
/// # }
/// ```
#[derive(Clone)]
pub struct BoardHandle {
  request_tx: ::tokio::sync::mpsc::Sender<Message>,
  lifecycle_rx: ::tokio::sync::watch::Receiver<LifecycleState>,
}

impl BoardHandle {
  /// Returns the most recently observed actor lifecycle state.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use chessnut_move::transport::tokio::{BoardHandle, LifecycleState};
  ///
  /// # fn inspect(board: &BoardHandle) {
  /// if board.lifecycle() == LifecycleState::Running {
  ///     println!("board actor is ready");
  /// }
  /// # }
  /// ```
  pub fn lifecycle(&self) -> LifecycleState {
    *self.lifecycle_rx.borrow()
  }

  /// Subscribes to lifecycle changes from the actor.
  ///
  /// The returned
  /// [`tokio::sync::watch::Receiver`](https://docs.rs/tokio/1/tokio/sync/watch/struct.Receiver.html)
  /// immediately exposes the current state and retains only the latest state.
  pub fn subscribe_lifecycle(&self) -> ::tokio::sync::watch::Receiver<LifecycleState> {
    self.lifecycle_rx.clone()
  }

  /// Sends a command and waits for the transport write to finish.
  ///
  /// Protocol responses are published separately through [`EventStream`].
  /// Prefer [`BoardHandle::battery_status`] and
  /// [`BoardHandle::piece_status`] when sending the corresponding queries so
  /// the actor can correlate the response.
  ///
  /// # Errors
  ///
  /// Returns [`HandleError::ShuttingDown`] after shutdown begins,
  /// [`HandleError::ActorStopped`] when the actor is no longer reachable, or
  /// [`HandleError::TransportFailed`] when the write fails.
  pub async fn send(&self, command: Command) -> Result<(), HandleError> {
    trace_event!(
      command_len = command.bytes().len(),
      write_kind = ?command.write_kind(),
      "submitting command to board actor"
    );
    let (reply_tx, reply_rx) = ::tokio::sync::oneshot::channel();
    self
      .send_message(Message::Send {
        command,
        reply: reply_tx,
      })
      .await?;
    receive_reply(reply_rx).await?
  }

  /// Queries and returns the board's battery status.
  ///
  /// This helper sends [`Command::read_battery_level`] and correlates the next
  /// matching response.
  ///
  /// Queries are serialized because Move responses do not contain request IDs.
  /// The decoded response is also published as
  /// [`BoardEvent::BatteryStatus`].
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use chessnut_move::transport::tokio::{BoardHandle, HandleError};
  ///
  /// async fn report_battery(board: &BoardHandle) -> Result<(), HandleError> {
  ///     let status = board.battery_status().await?;
  ///     let state = if status.charging { "charging" } else { "on battery" };
  ///     println!("{}% ({state})", status.percentage);
  ///     Ok(())
  /// }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`HandleError::QueryQueueFull`] when the query queue is full,
  /// [`HandleError::RequestTimedOut`] when no matching response arrives,
  /// [`HandleError::TransportFailed`] when the query write fails,
  /// [`HandleError::ShuttingDown`] after shutdown begins, or
  /// [`HandleError::ActorStopped`] when the actor is no longer reachable.
  pub async fn battery_status(&self) -> Result<BatteryStatus, HandleError> {
    debug_event!(query = "battery_status", "submitting board query");
    let (reply_tx, reply_rx) = ::tokio::sync::oneshot::channel();
    self
      .send_message(Message::Query(QueryRequest::Battery(reply_tx)))
      .await?;
    receive_reply(reply_rx).await?
  }

  /// Queries and returns the status of all tracked physical pieces.
  ///
  /// This helper sends [`Command::read_piece_status`] and correlates the next
  /// matching response.
  ///
  /// Queries are serialized because Move responses do not contain request IDs.
  /// The decoded response is also published as
  /// [`BoardEvent::PieceStatus`].
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use chessnut_move::transport::tokio::{BoardHandle, HandleError};
  ///
  /// async fn report_low_pieces(board: &BoardHandle) -> Result<(), HandleError> {
  ///     let status = board.piece_status().await?;
  ///
  ///     for tracked in status
  ///         .pieces
  ///         .iter()
  ///         .filter(|piece| piece.battery_percentage.is_some_and(|level| level < 20))
  ///     {
  ///         println!("{:?} is low", tracked.piece);
  ///     }
  ///
  ///     Ok(())
  /// }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`HandleError::QueryQueueFull`] when the query queue is full,
  /// [`HandleError::RequestTimedOut`] when no matching response arrives,
  /// [`HandleError::TransportFailed`] when the query write fails,
  /// [`HandleError::ShuttingDown`] after shutdown begins, or
  /// [`HandleError::ActorStopped`] when the actor is no longer reachable.
  pub async fn piece_status(&self) -> Result<PieceStatus, HandleError> {
    debug_event!(query = "piece_status", "submitting board query");
    let (reply_tx, reply_rx) = ::tokio::sync::oneshot::channel();
    self
      .send_message(Message::Query(QueryRequest::Pieces(reply_tx)))
      .await?;
    receive_reply(reply_rx).await?
  }

  /// Creates a new lag-aware stream of subsequently published board events.
  ///
  /// Events published before this request is processed are not replayed.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use std::error::Error;
  /// use chessnut_move::protocol::BoardEvent;
  /// use chessnut_move::transport::tokio::BoardHandle;
  ///
  /// async fn watch_moves(board: &BoardHandle) -> Result<(), Box<dyn Error>> {
  ///     let mut events = board.subscribe_events().await?;
  ///
  ///     loop {
  ///         if let BoardEvent::PositionChanged(position) = events.recv().await? {
  ///             println!("position: {position:?}");
  ///         }
  ///     }
  /// }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`HandleError::ShuttingDown`] after shutdown begins or
  /// [`HandleError::ActorStopped`] when the actor is no longer reachable.
  pub async fn subscribe_events(&self) -> Result<EventStream, HandleError> {
    debug_event!("subscribing to board actor events");
    let (reply_tx, reply_rx) = ::tokio::sync::oneshot::channel();
    self
      .send_message(Message::SubscribeEvents { reply: reply_tx })
      .await?;
    receive_reply(reply_rx).await?
  }

  /// Requests graceful actor shutdown and waits for transport cleanup.
  ///
  /// The actor unsubscribes from command responses, unsubscribes from position
  /// notifications, and then calls [`TokioTransport::close`]. Other handle
  /// clones stop accepting requests once shutdown begins.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use chessnut_move::transport::tokio::{BoardHandle, HandleError};
  ///
  /// async fn stop(board: &BoardHandle) -> Result<(), HandleError> {
  ///     board.shutdown().await
  /// }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`HandleError::TransportFailed`] when cleanup fails,
  /// [`HandleError::ShuttingDown`] when another shutdown is already in
  /// progress, or [`HandleError::ActorStopped`] when the actor is no longer
  /// reachable.
  pub async fn shutdown(&self) -> Result<(), HandleError> {
    info_event!("requesting graceful board actor shutdown");
    let (reply_tx, reply_rx) = ::tokio::sync::oneshot::channel();
    self
      .send_message(Message::Shutdown { reply: reply_tx })
      .await?;
    receive_reply(reply_rx).await?
  }

  /// Checks lifecycle state and enqueues one handle message.
  async fn send_message(&self, message: Message) -> Result<(), HandleError> {
    let lifecycle = self.lifecycle();
    match lifecycle {
      LifecycleState::Starting | LifecycleState::Running => {}
      LifecycleState::ShuttingDown => {
        debug_event!(?lifecycle, "board actor rejected handle request");
        return Err(HandleError::ShuttingDown);
      }
      LifecycleState::Stopped | LifecycleState::Faulted => {
        debug_event!(?lifecycle, "board actor rejected handle request");
        return Err(HandleError::ActorStopped);
      }
    }

    self.request_tx.send(message).await.map_err(|_| {
      debug_event!("board actor request channel is closed");
      HandleError::ActorStopped
    })
  }
}

/// A lag-aware stream for events published by the board actor.
///
/// The stream implements
/// [`tokio_stream::Stream`](https://docs.rs/tokio-stream/0.1/tokio_stream/trait.Stream.html)
/// with
/// `Item = Result<BoardEvent, EventStreamError>`. Decode and lag errors are
/// recoverable; consumers can continue polling after receiving either one.
/// The stream ends after the actor closes its event channel.
pub struct EventStream {
  inner: BroadcastStream<Result<BoardEvent, DecodeNotificationError>>,
}

impl EventStream {
  /// Wraps one broadcast subscriber as a public lag-aware stream.
  fn new(
    receiver: ::tokio::sync::broadcast::Receiver<Result<BoardEvent, DecodeNotificationError>>,
  ) -> Self {
    Self {
      inner: BroadcastStream::new(receiver),
    }
  }

  /// Receives the next event or stream condition.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use chessnut_move::protocol::BoardEvent;
  /// use chessnut_move::transport::tokio::{
  ///     EventStream, EventStreamError,
  /// };
  ///
  /// async fn next_position(
  ///     events: &mut EventStream,
  /// ) -> Result<(), EventStreamError> {
  ///     loop {
  ///         let event = events.recv().await?;
  ///         if let BoardEvent::PositionChanged(position) = event {
  ///             println!("position: {position:?}");
  ///             return Ok(());
  ///         }
  ///     }
  /// }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`EventStreamError::Lagged`] when older events were discarded,
  /// [`EventStreamError::Decode`] when a malformed notification was skipped,
  /// or [`EventStreamError::Closed`] after the actor closes the channel.
  pub async fn recv(&mut self) -> Result<BoardEvent, EventStreamError> {
    self.next().await.unwrap_or(Err(EventStreamError::Closed))
  }
}

impl Stream for EventStream {
  type Item = Result<BoardEvent, EventStreamError>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let item = ready!(Pin::new(&mut self.inner).poll_next(cx));
    Poll::Ready(match item {
      Some(Ok(Ok(event))) => Some(Ok(event)),
      Some(Ok(Err(error))) => Some(Err(EventStreamError::Decode(error))),
      Some(Err(BroadcastStreamRecvError::Lagged(count))) => {
        warn_event!(missed_events = count, "board event subscriber lagged");
        Some(Err(EventStreamError::Lagged(count)))
      }
      None => {
        debug_event!("board event stream closed");
        None
      }
    })
  }
}

/// The detailed result of a terminated actor, including its transport.
///
/// An exit always returns ownership of the transport, including after a
/// transport failure. Use [`ActorExit::result`] to inspect the outcome without
/// consuming the exit or [`ActorExit::into_parts`] to recover both values.
pub struct ActorExit<T: TokioTransport> {
  transport: T,
  result: Result<(), ActorError<T::Error>>,
}

impl<T: TokioTransport> ActorExit<T> {
  /// Returns a shared reference to the actor's transport.
  pub const fn transport(&self) -> &T {
    &self.transport
  }

  /// Consumes the exit and returns its transport, discarding the actor result.
  pub fn into_transport(self) -> T {
    self.transport
  }

  /// Consumes the exit and returns the transport and actor result.
  pub fn into_parts(self) -> (T, Result<(), ActorError<T::Error>>) {
    (self.transport, self.result)
  }

  /// Returns the actor result by reference.
  ///
  /// # Errors
  ///
  /// Returns the retained [`ActorError`] when initialization, operation, or
  /// cleanup failed.
  pub const fn result(&self) -> Result<(), &ActorError<T::Error>> {
    match &self.result {
      Ok(()) => Ok(()),
      Err(error) => Err(error),
    }
  }

  /// Consumes the exit and returns the owned actor result.
  ///
  /// # Errors
  ///
  /// Returns the retained [`ActorError`] when initialization, operation, or
  /// cleanup failed.
  pub fn into_result(self) -> Result<(), ActorError<T::Error>> {
    self.result
  }
}

/// A joinable Tokio task running the board actor.
///
/// Awaiting the task produces an [`ActorExit`] containing both the transport
/// and final result. Dropping `BoardTask` detaches the Tokio task; it does not
/// cancel the actor.
pub struct BoardTask<T: TokioTransport> {
  join: ::tokio::task::JoinHandle<ActorExit<T>>,
}

impl<T: TokioTransport> BoardTask<T> {
  /// Requests immediate task cancellation.
  ///
  /// Prefer [`BoardHandle::shutdown`] so the actor can unsubscribe and close
  /// the transport cleanly. Awaiting the task after cancellation returns a
  /// cancelled
  /// [`tokio::task::JoinError`](https://docs.rs/tokio/1/tokio/task/struct.JoinError.html).
  pub fn abort(&self) {
    self.join.abort();
  }

  /// Returns whether the actor task has completed.
  pub fn is_finished(&self) -> bool {
    self.join.is_finished()
  }
}

impl<T: TokioTransport> Future for BoardTask<T> {
  type Output = Result<ActorExit<T>, ::tokio::task::JoinError>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    Pin::new(&mut self.join).poll(cx)
  }
}

/// Spawns a concurrent board actor and returns its handle and joinable task.
///
/// Initialization runs inside the spawned task. Observe
/// [`BoardHandle::subscribe_lifecycle`] or await the [`BoardTask`] to detect an
/// initialization failure.
///
/// # Examples
///
/// ```no_run
/// use chessnut_move::transport::tokio::{
///     ActorConfig, BoardHandle, BoardTask, SpawnError, TokioTransport, spawn,
/// };
///
/// # fn start<T: TokioTransport>(
/// #     transport: T,
/// # ) -> Result<(BoardHandle, BoardTask<T>), SpawnError> {
/// spawn(transport, ActorConfig::default())
/// # }
/// ```
///
/// # Errors
///
/// Returns [`SpawnError::ZeroCommandCapacity`] or
/// [`SpawnError::ZeroEventCapacity`] when the corresponding
/// [`ActorConfig`] capacity is zero.
///
/// # Panics
///
/// Panics when called outside a Tokio runtime.
pub fn spawn<T: TokioTransport>(
  transport: T,
  config: ActorConfig,
) -> Result<(BoardHandle, BoardTask<T>), SpawnError> {
  if config.command_capacity == 0 {
    warn_event!(
      field = "command_capacity",
      "invalid board actor configuration"
    );
    return Err(SpawnError::ZeroCommandCapacity);
  }
  if config.event_capacity == 0 {
    warn_event!(
      field = "event_capacity",
      "invalid board actor configuration"
    );
    return Err(SpawnError::ZeroEventCapacity);
  }

  info_event!(
    command_capacity = config.command_capacity,
    event_capacity = config.event_capacity,
    query_capacity = config.query_capacity,
    request_timeout = ?config.request_timeout,
    "spawning board actor"
  );
  let (request_tx, request_rx) = ::tokio::sync::mpsc::channel(config.command_capacity);
  let (event_tx, _) = ::tokio::sync::broadcast::channel(config.event_capacity);
  let (lifecycle_tx, lifecycle_rx) = ::tokio::sync::watch::channel(LifecycleState::Starting);

  let actor = run_actor(transport, config, request_rx, event_tx, lifecycle_tx);
  #[cfg(feature = "tracing")]
  let actor = {
    use ::tracing::Instrument as _;

    actor.instrument(::tracing::info_span!("board_actor"))
  };
  let join = ::tokio::spawn(actor);

  Ok((
    BoardHandle {
      request_tx,
      lifecycle_rx,
    },
    BoardTask { join },
  ))
}

/// Request sent from a handle to the actor.
enum Message {
  Send {
    command: Command,
    reply: ::tokio::sync::oneshot::Sender<Result<(), HandleError>>,
  },
  Query(QueryRequest),
  SubscribeEvents {
    reply: ::tokio::sync::oneshot::Sender<Result<EventStream, HandleError>>,
  },
  Shutdown {
    reply: ::tokio::sync::oneshot::Sender<Result<(), HandleError>>,
  },
}

/// Query whose response must be correlated by notification type.
enum QueryRequest {
  Battery(::tokio::sync::oneshot::Sender<Result<BatteryStatus, HandleError>>),
  Pieces(::tokio::sync::oneshot::Sender<Result<PieceStatus, HandleError>>),
}

/// Active serialized query and its response deadline.
struct PendingQuery {
  request: QueryRequest,
  deadline: ::tokio::time::Instant,
}

impl QueryRequest {
  /// Returns the stable tracing label for this query type.
  #[cfg(feature = "tracing")]
  const fn kind(&self) -> &'static str {
    match self {
      Self::Battery(_) => "battery_status",
      Self::Pieces(_) => "piece_status",
    }
  }

  /// Builds the command associated with this response type.
  fn command(&self) -> Command {
    match self {
      Self::Battery(_) => Command::read_battery_level(),
      Self::Pieces(_) => Command::read_piece_status(),
    }
  }

  /// Completes this query with a handle-level error.
  fn fail(self, error: HandleError) {
    match self {
      Self::Battery(reply) => {
        let _ = reply.send(Err(error));
      }
      Self::Pieces(reply) => {
        let _ = reply.send(Err(error));
      }
    }
  }
}

impl PendingQuery {
  /// Completes the active query with a handle-level error.
  fn fail(self, error: HandleError) {
    self.request.fail(error);
  }
}

/// Receives an actor reply and maps a dropped sender to actor termination.
async fn receive_reply<T>(
  reply: ::tokio::sync::oneshot::Receiver<Result<T, HandleError>>,
) -> Result<Result<T, HandleError>, HandleError> {
  reply.await.map_err(|_| HandleError::ActorStopped)
}

/// Owns the transport and runs the complete actor lifecycle.
///
/// Only transport errors are fatal. Decode failures are broadcast to event
/// subscribers and the receive loop continues. Query responses are published
/// as events after satisfying the active query.
async fn run_actor<T: TokioTransport>(
  mut transport: T,
  config: ActorConfig,
  mut request_rx: ::tokio::sync::mpsc::Receiver<Message>,
  event_tx: ::tokio::sync::broadcast::Sender<Result<BoardEvent, DecodeNotificationError>>,
  lifecycle_tx: ::tokio::sync::watch::Sender<LifecycleState>,
) -> ActorExit<T> {
  info_event!("board actor is starting");
  let mut notification_buffer = [0; MAX_NOTIFICATION_LEN];
  let mut pending_query: Option<PendingQuery> = None;
  let mut queued_queries: VecDeque<QueryRequest> = VecDeque::new();
  let mut shutdown_reply = None;

  let mut result = initialize_transport(&mut transport)
    .await
    .map_err(ActorError::Transport);

  if result.is_ok() {
    lifecycle_tx.send_replace(LifecycleState::Running);
    info_event!("board actor is running");

    result = loop {
      if pending_query.is_none()
        && let Some(query) = queued_queries.pop_front()
      {
        debug_event!(
          query = query.kind(),
          queued_queries = queued_queries.len(),
          "starting queued board query"
        );
        match start_query(&mut transport, query, config.request_timeout).await {
          Ok(query) => pending_query = Some(query),
          Err(error) => break Err(error),
        }
      }

      let deadline = pending_query
        .as_ref()
        .map(|query| query.deadline)
        .unwrap_or_else(::tokio::time::Instant::now);

      ::tokio::select! {
        message = request_rx.recv() => {
          match message {
            Some(Message::Send { command, reply }) => {
              trace_event!(
                command_len = command.bytes().len(),
                write_kind = ?command.write_kind(),
                "board actor is writing a command"
              );
              match transport.write_command(&command).await {
                Ok(()) => {
                  let _ = reply.send(Ok(()));
                }
                Err(error) => {
                  error_event!(operation = "write_command", "board transport failed");
                  let _ = reply.send(Err(HandleError::TransportFailed));
                  break Err(ActorError::Transport(error));
                }
              }
            }
            Some(Message::Query(query)) => {
              debug_event!(
                query = query.kind(),
                queued_queries = queued_queries.len(),
                has_active_query = pending_query.is_some(),
                "board actor received a query"
              );
              if pending_query.is_none() && queued_queries.is_empty() {
                match start_query(&mut transport, query, config.request_timeout).await {
                  Ok(query) => pending_query = Some(query),
                  Err(error) => break Err(error),
                }
              } else if queued_queries.len() < config.query_capacity {
                queued_queries.push_back(query);
              } else {
                warn_event!(
                  query = query.kind(),
                  query_capacity = config.query_capacity,
                  "board query queue is full"
                );
                query.fail(HandleError::QueryQueueFull);
              }
            }
            Some(Message::SubscribeEvents { reply }) => {
              debug_event!(
                subscribers = event_tx.receiver_count() + 1,
                "creating board event subscription"
              );
              let _ = reply.send(Ok(EventStream::new(event_tx.subscribe())));
            }
            Some(Message::Shutdown { reply }) => {
              info_event!("board actor received shutdown request");
              shutdown_reply = Some(reply);
              break Ok(());
            }
            None => {
              info_event!("all board handles were dropped");
              break Ok(());
            }
          }
        }
        notification = transport.next_notification(&mut notification_buffer) => {
          let notification = match notification {
            Ok(notification) => notification,
            Err(error) => {
              error_event!(operation = "next_notification", "board transport failed");
              break Err(ActorError::Transport(error));
            }
          };
          trace_event!(
            source = ?notification.source(),
            notification_len = notification.bytes().len(),
            "board actor received a notification"
          );
          let event = match decode_notification(notification) {
            Ok(DecodedNotification::Event(event)) => event,
            Ok(DecodedNotification::RealtimeUpdatesAcknowledged) => continue,
            Err(error) => {
              warn_event!(error = ?error, "board actor skipped a malformed notification");
              let _ = event_tx.send(Err(error));
              continue;
            }
          };

          resolve_query(&mut pending_query, event);
          let _ = event_tx.send(Ok(event));
        }
        _ = ::tokio::time::sleep_until(deadline), if pending_query.is_some() => {
          if let Some(query) = pending_query.take() {
            warn_event!(query = query.request.kind(), "board query timed out");
            query.fail(HandleError::RequestTimedOut);
          }
        }
      }
    };
  } else {
    error_event!(
      operation = "initialize",
      "board actor initialization failed"
    );
  }

  lifecycle_tx.send_replace(LifecycleState::ShuttingDown);
  info_event!("board actor is shutting down");
  request_rx.close();

  if let Some(query) = pending_query.take() {
    query.fail(HandleError::ShuttingDown);
  }
  for query in queued_queries {
    query.fail(HandleError::ShuttingDown);
  }
  while let Ok(message) = request_rx.try_recv() {
    fail_message(message, HandleError::ShuttingDown);
  }

  let cleanup_result = shutdown_transport(&mut transport)
    .await
    .map_err(ActorError::Transport);
  if cleanup_result.is_err() {
    error_event!(operation = "shutdown", "board transport cleanup failed");
  }
  if result.is_ok() {
    result = cleanup_result;
  }

  let final_state = if result.is_ok() {
    LifecycleState::Stopped
  } else {
    LifecycleState::Faulted
  };
  lifecycle_tx.send_replace(final_state);
  match final_state {
    LifecycleState::Stopped => info_event!("board actor stopped"),
    LifecycleState::Faulted => error_event!("board actor stopped after a fatal error"),
    _ => {}
  }

  if let Some(reply) = shutdown_reply {
    let reply_result = if result.is_ok() {
      Ok(())
    } else {
      Err(HandleError::TransportFailed)
    };
    let _ = reply.send(reply_result);
  }

  ActorExit { transport, result }
}

/// Performs the protocol-required startup sequence.
///
/// Command responses are subscribed after the realtime-enable write because
/// the board may emit an undocumented `0x23` acknowledgement on that
/// characteristic. The decoder also recognizes delayed acknowledgements.
async fn initialize_transport<T: TokioTransport>(transport: &mut T) -> Result<(), T::Error> {
  trace_event!(
    operation = "subscribe",
    source = ?NotificationSource::Position,
    "initializing board transport"
  );
  transport.subscribe(NotificationSource::Position).await?;
  trace_event!(
    operation = "enable_realtime_updates",
    "initializing board transport"
  );
  transport
    .write_command(&Command::enable_realtime_updates())
    .await?;
  trace_event!(
    operation = "subscribe",
    source = ?NotificationSource::CommandResponse,
    "initializing board transport"
  );
  transport
    .subscribe(NotificationSource::CommandResponse)
    .await
}

/// Performs ordered best-effort-compatible transport cleanup.
///
/// Cleanup returns at the first transport error. The transport is still
/// retained in [`ActorExit`] so callers can perform adapter-specific recovery.
async fn shutdown_transport<T: TokioTransport>(transport: &mut T) -> Result<(), T::Error> {
  trace_event!(
    operation = "unsubscribe",
    source = ?NotificationSource::CommandResponse,
    "shutting down board transport"
  );
  transport
    .unsubscribe(NotificationSource::CommandResponse)
    .await?;
  trace_event!(
    operation = "unsubscribe",
    source = ?NotificationSource::Position,
    "shutting down board transport"
  );
  transport.unsubscribe(NotificationSource::Position).await?;
  trace_event!(operation = "close", "shutting down board transport");
  transport.close().await
}

/// Writes the next serialized query and records its response deadline.
async fn start_query<T: TokioTransport>(
  transport: &mut T,
  query: QueryRequest,
  timeout: Duration,
) -> Result<PendingQuery, ActorError<T::Error>> {
  debug_event!(
    query = query.kind(),
    timeout = ?timeout,
    "writing serialized board query"
  );
  if let Err(error) = transport.write_command(&query.command()).await {
    error_event!(
      operation = "write_query",
      query = query.kind(),
      "board transport failed"
    );
    query.fail(HandleError::TransportFailed);
    return Err(ActorError::Transport(error));
  }

  Ok(PendingQuery {
    request: query,
    deadline: ::tokio::time::Instant::now() + timeout,
  })
}

/// Completes the active query when an event has its expected response type.
///
/// Move query responses do not carry request IDs, so at most one query can be
/// active. Nonmatching events remain public events and leave the query pending.
fn resolve_query(pending: &mut Option<PendingQuery>, event: BoardEvent) {
  let matches = matches!(
    (pending.as_ref().map(|query| &query.request), event),
    (Some(QueryRequest::Battery(_)), BoardEvent::BatteryStatus(_))
      | (Some(QueryRequest::Pieces(_)), BoardEvent::PieceStatus(_))
  );

  if !matches {
    return;
  }

  let query = pending.take().expect("matching pending query exists");
  debug_event!(
    query = query.request.kind(),
    "board query received its response"
  );
  match (query.request, event) {
    (QueryRequest::Battery(reply), BoardEvent::BatteryStatus(status)) => {
      let _ = reply.send(Ok(status));
    }
    (QueryRequest::Pieces(reply), BoardEvent::PieceStatus(status)) => {
      let _ = reply.send(Ok(status));
    }
    _ => unreachable!("query and event variants were checked before taking the query"),
  }
}

/// Rejects a queued handle message during shutdown or actor failure.
fn fail_message(message: Message, error: HandleError) {
  match message {
    Message::Send { reply, .. } | Message::Shutdown { reply } => {
      let _ = reply.send(Err(error));
    }
    Message::Query(query) => query.fail(error),
    Message::SubscribeEvents { reply } => {
      let _ = reply.send(Err(error));
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use super::*;

  #[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
  #[error("mock notification channel closed")]
  struct MockError;

  struct OwnedNotification {
    source: NotificationSource,
    bytes: Vec<u8>,
  }

  #[derive(Debug, PartialEq, Eq)]
  enum Operation {
    Subscribe(NotificationSource),
    Write(Vec<u8>),
  }

  #[derive(Default)]
  struct Record {
    subscriptions: Vec<NotificationSource>,
    unsubscriptions: Vec<NotificationSource>,
    writes: Vec<Vec<u8>>,
    operations: Vec<Operation>,
    closed: bool,
  }

  struct MockTransport {
    record: Arc<Mutex<Record>>,
    notification_rx: ::tokio::sync::mpsc::Receiver<OwnedNotification>,
  }

  impl MockTransport {
    fn new() -> (
      Self,
      Arc<Mutex<Record>>,
      ::tokio::sync::mpsc::Sender<OwnedNotification>,
    ) {
      let record = Arc::new(Mutex::new(Record::default()));
      let (notification_tx, notification_rx) = ::tokio::sync::mpsc::channel(8);

      (
        Self {
          record: Arc::clone(&record),
          notification_rx,
        },
        record,
        notification_tx,
      )
    }
  }

  impl TokioTransport for MockTransport {
    type Error = MockError;

    async fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
      let mut record = self.record.lock().unwrap();
      record.subscriptions.push(source);
      record.operations.push(Operation::Subscribe(source));
      Ok(())
    }

    async fn unsubscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
      self.record.lock().unwrap().unsubscriptions.push(source);
      Ok(())
    }

    async fn write_command(&mut self, command: &Command) -> Result<(), Self::Error> {
      let bytes = command.bytes().to_vec();
      let mut record = self.record.lock().unwrap();
      record.writes.push(bytes.clone());
      record.operations.push(Operation::Write(bytes));
      Ok(())
    }

    async fn next_notification<'a>(
      &'a mut self,
      buffer: &'a mut [u8],
    ) -> Result<Notification<'a>, Self::Error> {
      let notification = self.notification_rx.recv().await.ok_or(MockError)?;
      buffer[..notification.bytes.len()].copy_from_slice(&notification.bytes);
      Ok(Notification::new(
        notification.source,
        &buffer[..notification.bytes.len()],
      ))
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
      self.record.lock().unwrap().closed = true;
      Ok(())
    }
  }

  #[::tokio::test]
  async fn actor_correlates_queries_without_stealing_events() {
    let (transport, record, notification_tx) = MockTransport::new();
    let (handle, task) = spawn(transport, ActorConfig::default()).unwrap();
    let mut events = handle.subscribe_events().await.unwrap();
    wait_until_running(&handle).await;

    assert_eq!(
      record.lock().unwrap().operations[..3],
      [
        Operation::Subscribe(NotificationSource::Position),
        Operation::Write(Command::enable_realtime_updates().bytes().to_vec()),
        Operation::Subscribe(NotificationSource::CommandResponse),
      ]
    );

    notification_tx
      .send(OwnedNotification {
        source: NotificationSource::CommandResponse,
        bytes: vec![0x23, 0x01, 0x00],
      })
      .await
      .unwrap();

    let query_handle = handle.clone();
    let query = ::tokio::spawn(async move { query_handle.battery_status().await });
    wait_for_write(&record, Command::read_battery_level().bytes()).await;

    notification_tx
      .send(OwnedNotification {
        source: NotificationSource::Position,
        bytes: vec![0; 34],
      })
      .await
      .unwrap();

    assert!(matches!(
      events.recv().await,
      Ok(BoardEvent::PositionChanged(_))
    ));
    assert!(!query.is_finished());

    notification_tx
      .send(OwnedNotification {
        source: NotificationSource::CommandResponse,
        bytes: vec![0x41, 0x03, 0x0c, 0x01, 88],
      })
      .await
      .unwrap();

    assert_eq!(
      query.await.unwrap(),
      Ok(BatteryStatus {
        charging: true,
        percentage: 88,
      })
    );
    assert_eq!(
      events.recv().await,
      Ok(BoardEvent::BatteryStatus(BatteryStatus {
        charging: true,
        percentage: 88,
      }))
    );

    let query_handle = handle.clone();
    let query = ::tokio::spawn(async move { query_handle.piece_status().await });
    wait_for_write(&record, Command::read_piece_status().bytes()).await;
    notification_tx
      .send(OwnedNotification {
        source: NotificationSource::CommandResponse,
        bytes: piece_status_response(),
      })
      .await
      .unwrap();

    let piece_status = query.await.unwrap().unwrap();
    assert_eq!(piece_status.pieces[0].battery_percentage, Some(50));
    assert_eq!(piece_status.pieces[33].battery_percentage, Some(83));
    assert!(matches!(
      events.recv().await,
      Ok(BoardEvent::PieceStatus(_))
    ));

    handle.shutdown().await.unwrap();
    let exit = task.await.unwrap();
    assert!(exit.result().is_ok());
    assert_eq!(handle.lifecycle(), LifecycleState::Stopped);
    assert_eq!(events.recv().await, Err(EventStreamError::Closed));

    let record = record.lock().unwrap();
    assert_eq!(
      record.subscriptions,
      [
        NotificationSource::Position,
        NotificationSource::CommandResponse,
      ]
    );
    assert_eq!(
      record.unsubscriptions,
      [
        NotificationSource::CommandResponse,
        NotificationSource::Position,
      ]
    );
    assert!(record.closed);
  }

  #[::tokio::test(start_paused = true)]
  async fn request_timeout_does_not_stop_the_actor() {
    let (transport, record, _notification_tx) = MockTransport::new();
    let config = ActorConfig {
      request_timeout: Duration::from_secs(5),
      ..ActorConfig::default()
    };
    let (handle, task) = spawn(transport, config).unwrap();
    wait_until_running(&handle).await;

    let query_handle = handle.clone();
    let query = ::tokio::spawn(async move { query_handle.battery_status().await });
    wait_for_write(&record, Command::read_battery_level().bytes()).await;

    ::tokio::time::advance(Duration::from_secs(6)).await;
    assert_eq!(query.await.unwrap(), Err(HandleError::RequestTimedOut));
    assert_eq!(handle.lifecycle(), LifecycleState::Running);

    handle.shutdown().await.unwrap();
    assert!(task.await.unwrap().result().is_ok());
  }

  #[::tokio::test]
  async fn malformed_notification_is_reported_without_stopping_actor() {
    let (transport, _record, notification_tx) = MockTransport::new();
    let (handle, task) = spawn(transport, ActorConfig::default()).unwrap();
    let mut events = handle.subscribe_events().await.unwrap();
    wait_until_running(&handle).await;

    notification_tx
      .send(OwnedNotification {
        source: NotificationSource::Position,
        bytes: vec![0; 29],
      })
      .await
      .unwrap();

    assert!(matches!(
      events.recv().await,
      Err(EventStreamError::Decode(DecodeNotificationError::Position(
        crate::protocol::DecodePositionNotificationError::NotificationTooShort(
          crate::protocol::NotificationTooShortError {
            expected: 34,
            actual: 29,
          }
        )
      )))
    ));
    assert_eq!(handle.lifecycle(), LifecycleState::Running);

    handle.shutdown().await.unwrap();
    assert!(task.await.unwrap().result().is_ok());
  }

  #[::tokio::test]
  async fn event_stream_reports_lag() {
    let (sender, receiver) = ::tokio::sync::broadcast::channel(1);
    let mut events = EventStream::new(receiver);

    sender
      .send(Ok(BoardEvent::BatteryStatus(BatteryStatus {
        charging: false,
        percentage: 10,
      })))
      .unwrap();
    sender
      .send(Ok(BoardEvent::BatteryStatus(BatteryStatus {
        charging: false,
        percentage: 11,
      })))
      .unwrap();

    assert_eq!(events.recv().await, Err(EventStreamError::Lagged(1)));
    assert_eq!(
      events.recv().await,
      Ok(BoardEvent::BatteryStatus(BatteryStatus {
        charging: false,
        percentage: 11,
      }))
    );
  }

  async fn wait_until_running(handle: &BoardHandle) {
    let mut lifecycle = handle.subscribe_lifecycle();
    while *lifecycle.borrow() != LifecycleState::Running {
      lifecycle.changed().await.unwrap();
    }
  }

  async fn wait_for_write(record: &Arc<Mutex<Record>>, expected: &[u8]) {
    loop {
      if record
        .lock()
        .unwrap()
        .writes
        .iter()
        .any(|write| write == expected)
      {
        return;
      }
      ::tokio::task::yield_now().await;
    }
  }

  fn piece_status_response() -> Vec<u8> {
    const IDENTITIES: [u8; 34] = [
      1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 9, 9, 10,
      10, 11, 11, 12,
    ];

    let mut response = vec![0; MAX_NOTIFICATION_LEN];
    response[..3].copy_from_slice(&[0x41, 0x89, 0x0b]);
    for (index, identity) in IDENTITIES.iter().copied().enumerate() {
      let offset = 3 + index * 4;
      response[offset] = identity;
      response[offset + 1] = index as u8;
      response[offset + 2] = u8::MAX - index as u8;
      response[offset + 3] = 50 + index as u8;
    }
    response
  }
}
