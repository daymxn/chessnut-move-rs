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

//! Allocation-free synchronous session built on a blocking transport.

use crate::protocol::{BoardEvent, Command};
use crate::transport::{
  BoardError, BoardState, DecodedNotification, Notification, NotificationSource,
  decode_notification,
};

/// A blocking I/O adapter for a connected Chessnut Move board.
///
/// # Examples
///
/// ```
/// use core::convert::Infallible;
/// use chessnut_move::protocol::Command;
/// use chessnut_move::transport::{
///     BlockingTransport, Notification, NotificationSource,
/// };
///
/// struct MyTransport;
///
/// impl BlockingTransport for MyTransport {
///     type Error = Infallible;
///
///     fn subscribe(
///         &mut self,
///         _source: NotificationSource,
///     ) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn write_command(&mut self, _command: &Command) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn next_notification<'a>(
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
pub trait BlockingTransport {
  /// Error returned by Bluetooth or adapter operations.
  type Error;

  /// Enables notifications for one protocol source.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the adapter cannot enable the corresponding
  /// [GATT characteristic][NotificationSource::characteristic].
  fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error>;

  /// Disables notifications for one protocol source.
  ///
  /// The default implementation performs no operation.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the adapter cannot disable the corresponding
  /// [GATT characteristic][NotificationSource::characteristic].
  fn unsubscribe(&mut self, _source: NotificationSource) -> Result<(), Self::Error> {
    Ok(())
  }

  /// Writes an encoded command using its
  /// [required write kind][Command::write_kind].
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when the command cannot be written.
  fn write_command(&mut self, command: &Command) -> Result<(), Self::Error>;

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
  ) -> Result<Notification<'a>, Self::Error>;

  /// Closes transport-owned resources after subscriptions are disabled.
  ///
  /// The default implementation performs no operation.
  ///
  /// # Errors
  ///
  /// Returns [`Self::Error`] when transport cleanup fails.
  fn close(&mut self) -> Result<(), Self::Error> {
    Ok(())
  }
}

/// An owning, allocation-free blocking session with a Chessnut Move board.
///
/// The session reuses a fixed notification buffer. Dropping a session does not
/// call [`BlockingTransport::unsubscribe`] or [`BlockingTransport::close`];
/// call [`BlockingBoard::shutdown`] when cleanup is required.
///
/// # Examples
///
/// The transport must already represent a connected board:
///
/// ```no_run
/// use chessnut_move::protocol::{BoardEvent, Command, LedPattern};
/// use chessnut_move::transport::{
///     BlockingBoard, BlockingTransport, BoardError,
/// };
///
/// fn run<T: BlockingTransport>(
///     transport: T,
/// ) -> Result<(), BoardError<T::Error>> {
///     let mut board = BlockingBoard::new(transport);
///     board.initialize()?;
///
///     board.send(&Command::set_leds(&LedPattern::default()))?;
///     match board.next_event()? {
///         BoardEvent::PositionChanged(position) => {
///             println!("position: {position:?}");
///         }
///         BoardEvent::BatteryStatus(_) | BoardEvent::PieceStatus(_) => {}
///     }
///
///     board.shutdown()
/// }
/// ```
pub struct BlockingBoard<T> {
  state: BoardState<T>,
}

impl<T> BlockingBoard<T> {
  /// Creates a board session around a connected transport.
  ///
  /// This constructor performs no I/O. Call [`BlockingBoard::initialize`]
  /// before receiving events.
  ///
  /// # Examples
  ///
  /// ```
  /// use chessnut_move::transport::{BlockingBoard, BlockingTransport};
  ///
  /// fn open_session<T: BlockingTransport>(transport: T) -> BlockingBoard<T> {
  ///     BlockingBoard::new(transport)
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

impl<T: BlockingTransport> BlockingBoard<T> {
  /// Initializes subscriptions and enables realtime position updates.
  ///
  /// Initialization subscribes to position notifications, writes
  /// [`Command::enable_realtime_updates`], and then subscribes to command
  /// responses. Completed steps are not rolled back when a later step fails.
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when subscribing or writing the
  /// realtime-update command fails.
  pub fn initialize(&mut self) -> Result<(), BoardError<T::Error>> {
    debug_event!(session = "blocking", "initializing board session");
    self
      .state
      .transport
      .subscribe(NotificationSource::Position)
      .map_err(|error| {
        warn_event!(
          session = "blocking",
          stage = "subscribe_position",
          "board initialization failed"
        );
        BoardError::Transport(error)
      })?;
    self.send(&Command::enable_realtime_updates())?;
    self
      .state
      .transport
      .subscribe(NotificationSource::CommandResponse)
      .map_err(|error| {
        warn_event!(
          session = "blocking",
          stage = "subscribe_command_response",
          "board initialization failed"
        );
        BoardError::Transport(error)
      })?;
    debug_event!(session = "blocking", "board session initialized");
    Ok(())
  }

  /// Writes a command to the board.
  ///
  /// This method confirms only the transport write. Commands with protocol
  /// responses are received separately through [`BlockingBoard::next_event`].
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when the transport cannot write the
  /// command.
  pub fn send(&mut self, command: &Command) -> Result<(), BoardError<T::Error>> {
    trace_event!(
      session = "blocking",
      command_len = command.bytes().len(),
      write_kind = ?command.write_kind(),
      "writing board command"
    );
    self
      .state
      .transport
      .write_command(command)
      .map_err(|error| {
        warn_event!(session = "blocking", "board command write failed");
        BoardError::Transport(error)
      })
  }

  /// Blocks until the next observable board event is decoded.
  ///
  /// Session-control acknowledgements are consumed internally and are not
  /// returned as events.
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when notification delivery fails, or
  /// [`BoardError::Decode`] when a notification is malformed or unsupported.
  pub fn next_event(&mut self) -> Result<BoardEvent, BoardError<T::Error>> {
    loop {
      let notification = self
        .state
        .transport
        .next_notification(&mut self.state.notification_buffer)
        .map_err(|error| {
          warn_event!(session = "blocking", "notification receive failed");
          BoardError::Transport(error)
        })?;
      trace_event!(
        session = "blocking",
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
  /// [`BlockingBoard::into_transport`] remains available after an error.
  ///
  /// # Errors
  ///
  /// Returns [`BoardError::Transport`] when either unsubscribe operation or
  /// [`BlockingTransport::close`] fails.
  pub fn shutdown(&mut self) -> Result<(), BoardError<T::Error>> {
    debug_event!(session = "blocking", "shutting down board session");
    self
      .state
      .transport
      .unsubscribe(NotificationSource::CommandResponse)
      .map_err(|error| {
        warn_event!(
          session = "blocking",
          stage = "unsubscribe_command_response",
          "board shutdown failed"
        );
        BoardError::Transport(error)
      })?;
    self
      .state
      .transport
      .unsubscribe(NotificationSource::Position)
      .map_err(|error| {
        warn_event!(
          session = "blocking",
          stage = "unsubscribe_position",
          "board shutdown failed"
        );
        BoardError::Transport(error)
      })?;
    self.state.transport.close().map_err(|error| {
      warn_event!(
        session = "blocking",
        stage = "close",
        "board shutdown failed"
      );
      BoardError::Transport(error)
    })?;
    debug_event!(session = "blocking", "board session stopped");
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use core::convert::Infallible;

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

  impl BlockingTransport for MockTransport {
    type Error = Infallible;

    fn subscribe(&mut self, source: NotificationSource) -> Result<(), Self::Error> {
      self.subscriptions[self.subscription_count] = Some(source);
      self.subscription_count += 1;
      Ok(())
    }

    fn write_command(&mut self, command: &Command) -> Result<(), Self::Error> {
      self.written[..command.bytes().len()].copy_from_slice(command.bytes());
      self.written_len = command.bytes().len();
      self.write_kind = Some(command.write_kind());
      Ok(())
    }

    fn next_notification<'a>(
      &'a mut self,
      buffer: &'a mut [u8],
    ) -> Result<Notification<'a>, Self::Error> {
      let notification: &[u8] = if self.notification_count == 0 {
        &[0x23, 0x01, 0x00]
      } else {
        &[0x41, 0x03, 0x0c, 0x01, 81]
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
    let mut board = BlockingBoard::new(MockTransport::new());
    board.initialize().unwrap();

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
      board.next_event().unwrap(),
      BoardEvent::BatteryStatus(BatteryStatus {
        charging: true,
        percentage: 81,
      })
    );
  }
}
