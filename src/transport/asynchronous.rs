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

//! Allocation-free asynchronous session built on a runtime-neutral transport.

use crate::protocol::{BoardEvent, Command};
#[cfg(doc)]
use crate::transport;
use crate::transport::{
  BoardError, BoardState, DecodedNotification, Notification, NotificationSource,
  decode_notification,
};

/// An asynchronous I/O adapter for a connected Chessnut Move board.
///
/// Implementations are responsible only for mapping commands and notification
/// sources to their BLE library. Protocol encoding and decoding stay in this
/// crate.
///
/// The returned futures are not required to implement [`Send`], allowing the
/// transport to run on local and embedded executors.
#[cfg_attr(
  feature = "tokio",
  doc = "Implement [`TokioTransport`][transport::TokioTransport] instead when the transport must run in a spawned Tokio task."
)]
///
/// # Examples
///
/// ```
/// use core::convert::Infallible;
/// use chessnut_move::protocol::Command;
/// use chessnut_move::transport::{
///     AsyncTransport, Notification, NotificationSource,
/// };
///
/// struct MyTransport;
///
/// impl AsyncTransport for MyTransport {
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
#[allow(async_fn_in_trait)]
pub trait AsyncTransport {
  /// Error returned by Bluetooth or adapter operations.
  type Error;

  /// Enables notifications for one protocol source.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the adapter cannot enable the corresponding
  /// [GATT characteristic][NotificationSource::characteristic].
  async fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error>;

  /// Disables notifications for one protocol source.
  ///
  /// The default implementation performs no operation.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the adapter cannot disable the corresponding
  /// [GATT characteristic][NotificationSource::characteristic].
  async fn unsubscribe(&mut self, _source: NotificationSource) -> Result<(), Self::Error> {
    Ok(())
  }

  /// Writes an encoded command using its
  /// [required write kind][Command::write_kind].
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the command cannot be written.
  async fn write_command(&mut self, command: &Command) -> Result<(), Self::Error>;

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
  async fn next_notification<'a>(
    &'a mut self,
    buffer: &'a mut [u8],
  ) -> Result<Notification<'a>, Self::Error>;

  /// Closes transport-owned resources after subscriptions are disabled.
  ///
  /// The default implementation performs no operation.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when transport cleanup fails.
  async fn close(&mut self) -> Result<(), Self::Error> {
    Ok(())
  }
}

/// An owning, allocation-free asynchronous session with a Chessnut Move board.
///
/// The session reuses a fixed notification buffer and requires exclusive
/// mutable access for commands and events. Dropping a session does not call
/// [`AsyncTransport::unsubscribe`] or [`AsyncTransport::close`]; call
/// [`AsyncBoard::shutdown`] when cleanup is required.
///
/// # Examples
///
/// The transport must already represent a connected board:
///
/// ```no_run
/// use chessnut_move::protocol::{BoardEvent, Command, LedPattern};
/// use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};
///
/// async fn run<T: AsyncTransport>(
///     transport: T,
/// ) -> Result<(), BoardError<T::Error>> {
///     let mut board = AsyncBoard::new(transport);
///     board.initialize().await?;
///
///     board.send(&Command::set_leds(&LedPattern::default())).await?;
///     match board.next_event().await? {
///         BoardEvent::PositionChanged(position) => {
///             println!("position: {position:?}");
///         }
///         BoardEvent::BatteryStatus(_) | BoardEvent::PieceStatus(_) => {}
///     }
///
///     board.shutdown().await
/// }
/// ```
pub struct AsyncBoard<T> {
  state: BoardState<T>,
}

impl<T> AsyncBoard<T> {
  /// Creates a board session around a connected transport.
  ///
  /// This constructor performs no I/O. Call [`AsyncBoard::initialize`] before
  /// receiving events.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::transport::{AsyncBoard, AsyncTransport};
  ///
  /// fn open_session<T: AsyncTransport>(transport: T) -> AsyncBoard<T> {
  ///     AsyncBoard::new(transport)
  /// }
  /// ```
  pub const fn new(transport: T) -> Self {
    Self {
      state: BoardState::new(transport),
    }
  }

  /// Returns a shared reference to the underlying transport.
  pub const fn transport(&self) -> &T {
    &self.state.transport
  }

  /// Returns a mutable reference to the underlying transport.
  ///
  /// Direct operations can change subscription or connection state expected by
  /// the session.
  pub fn transport_mut(&mut self) -> &mut T {
    &mut self.state.transport
  }

  /// Consumes the session and returns its transport without shutting it down.
  pub fn into_transport(self) -> T {
    self.state.transport
  }
}

impl<T: AsyncTransport> AsyncBoard<T> {
  /// Initializes subscriptions and enables realtime position updates.
  ///
  /// Initialization subscribes to position notifications, writes
  /// [`Command::enable_realtime_updates`], and then subscribes to command
  /// responses. Completed steps are not rolled back when a later step fails.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// use chessnut_move::transport::{AsyncBoard, AsyncTransport, BoardError};
  ///
  /// # async fn initialize<T: AsyncTransport>(
  /// #     board: &mut AsyncBoard<T>,
  /// # ) -> Result<(), BoardError<T::Error>> {
  /// board.initialize().await?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when subscribing or writing the
  /// realtime-update command fails.
  pub async fn initialize(&mut self) -> Result<(), BoardError<T::Error>> {
    debug_event!(session = "async", "initializing board session");
    self
      .state
      .transport
      .subscribe(NotificationSource::Position)
      .await
      .map_err(|error| {
        warn_event!(
          session = "async",
          stage = "subscribe_position",
          "board initialization failed"
        );
        BoardError::Transport(error)
      })?;
    self.send(&Command::enable_realtime_updates()).await?;
    self
      .state
      .transport
      .subscribe(NotificationSource::CommandResponse)
      .await
      .map_err(|error| {
        warn_event!(
          session = "async",
          stage = "subscribe_command_response",
          "board initialization failed"
        );
        BoardError::Transport(error)
      })?;
    debug_event!(session = "async", "board session initialized");
    Ok(())
  }

  /// Writes a command to the board.
  ///
  /// This method confirms only the transport write. Commands with protocol
  /// responses are received separately through [`AsyncBoard::next_event`].
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when the transport cannot write the
  /// command.
  pub async fn send(&mut self, command: &Command) -> Result<(), BoardError<T::Error>> {
    trace_event!(
      session = "async",
      command_len = command.bytes().len(),
      write_kind = ?command.write_kind(),
      "writing board command"
    );
    self
      .state
      .transport
      .write_command(command)
      .await
      .map_err(|error| {
        warn_event!(session = "async", "board command write failed");
        BoardError::Transport(error)
      })
  }

  /// Waits for and decodes the next observable board event.
  ///
  /// Session-control acknowledgements are consumed internally and are not
  /// returned as events.
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when notification delivery fails, or
  /// [`BoardError::Decode`] when a notification is malformed or unsupported.
  pub async fn next_event(&mut self) -> Result<BoardEvent, BoardError<T::Error>> {
    loop {
      let notification = self
        .state
        .transport
        .next_notification(&mut self.state.notification_buffer)
        .await
        .map_err(|error| {
          warn_event!(session = "async", "notification receive failed");
          BoardError::Transport(error)
        })?;
      trace_event!(
        session = "async",
        source = ?notification.source(),
        notification_len = notification.bytes().len(),
        "received board notification"
      );

      match decode_notification(notification)? {
        DecodedNotification::Event(event) => return Ok(event),
        DecodedNotification::RealtimeUpdatesAcknowledged => {}
      }
    }
  }

  /// Unsubscribes from board notifications and closes the transport.
  ///
  /// Cleanup stops at the first failed operation. Consuming the session with
  /// [`AsyncBoard::into_transport`] remains available after an error.
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when either unsubscribe operation or
  /// [`AsyncTransport::close`] fails.
  pub async fn shutdown(&mut self) -> Result<(), BoardError<T::Error>> {
    debug_event!(session = "async", "shutting down board session");
    self
      .state
      .transport
      .unsubscribe(NotificationSource::CommandResponse)
      .await
      .map_err(|error| {
        warn_event!(
          session = "async",
          stage = "unsubscribe_command_response",
          "board shutdown failed"
        );
        BoardError::Transport(error)
      })?;
    self
      .state
      .transport
      .unsubscribe(NotificationSource::Position)
      .await
      .map_err(|error| {
        warn_event!(
          session = "async",
          stage = "unsubscribe_position",
          "board shutdown failed"
        );
        BoardError::Transport(error)
      })?;
    self.state.transport.close().await.map_err(|error| {
      warn_event!(session = "async", stage = "close", "board shutdown failed");
      BoardError::Transport(error)
    })?;
    debug_event!(session = "async", "board session stopped");
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use core::convert::Infallible;

  use futures_executor::block_on;

  use super::*;
  use crate::protocol::{BatteryStatus, WriteKind};

  struct MockTransport {
    subscriptions: [Option<NotificationSource>; 2],
    subscription_count: usize,
    written: [u8; 35],
    written_len: usize,
    write_kind: Option<WriteKind>,
    notification_count: usize,
  }

  impl MockTransport {
    const fn new() -> Self {
      Self {
        subscriptions: [None; 2],
        subscription_count: 0,
        written: [0; 35],
        written_len: 0,
        write_kind: None,
        notification_count: 0,
      }
    }
  }

  impl AsyncTransport for MockTransport {
    type Error = Infallible;

    async fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
      self.subscriptions[self.subscription_count] = Some(source);
      self.subscription_count += 1;
      Ok(())
    }

    async fn write_command(&mut self, command: &Command) -> Result<(), Self::Error> {
      self.written[..command.bytes().len()].copy_from_slice(command.bytes());
      self.written_len = command.bytes().len();
      self.write_kind = Some(command.write_kind());
      Ok(())
    }

    async fn next_notification<'a>(
      &'a mut self,
      buffer: &'a mut [u8],
    ) -> Result<Notification<'a>, Self::Error> {
      let notification: &[u8] = if self.notification_count == 0 {
        &[0x23, 0x01, 0x00]
      } else {
        &[0x41, 0x03, 0x0c, 0x00, 63]
      };
      self.notification_count += 1;
      buffer[..notification.len()].copy_from_slice(notification);
      Ok(Notification::new(
        NotificationSource::CommandResponse,
        &buffer[..notification.len()],
      ))
    }
  }

  #[test]
  fn initializes_and_decodes_events_without_allocating() {
    block_on(async {
      let mut board = AsyncBoard::new(MockTransport::new());
      board.initialize().await.unwrap();

      assert_eq!(
        board.transport().subscriptions,
        [
          Some(NotificationSource::Position),
          Some(NotificationSource::CommandResponse),
        ]
      );
      assert_eq!(
        &board.transport().written[..board.transport().written_len],
        Command::enable_realtime_updates().bytes()
      );
      assert_eq!(board.transport().write_kind, Some(WriteKind::WithResponse));

      assert_eq!(
        board.next_event().await.unwrap(),
        BoardEvent::BatteryStatus(BatteryStatus {
          charging: false,
          percentage: 63,
        })
      );
    });
  }
}
